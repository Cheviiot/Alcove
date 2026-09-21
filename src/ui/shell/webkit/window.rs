// SPDX-License-Identifier: GPL-3.0-only

use std::{
    cell::{Cell, OnceCell, RefCell},
    collections::BTreeSet,
};

use adw::{prelude::*, subclass::prelude::*};
use anyhow::{Context, Result};
use ashpd::WindowIdentifier;
use gettextrs::gettext;
use glib::clone;
use gtk::{gdk, gio, glib};
use webkit::{
    prelude::*, HardwareAccelerationPolicy, PermissionRequest, PolicyDecision, PolicyDecisionType,
    WebContext, WebView,
};

use crate::{
    app::application::AlcoveApplication,
    domain::model::{AppConfigV3, Engine, WindowState},
    domain::policy::{AppPolicyV2, Origin, PermissionDecision, PermissionKind, ProxyMode},
    domain::repository::ProfileLock,
    engines::chromium::{self, EngineAvailability},
    engines::compatibility::{reason_description, CompatibilityCatalogV1},
    engines::content_filters,
    system::background,
    system::service::AppService,
    ui::common,
    ui::dialogs::downloads::DownloadManager,
    ui::shell::web_app_shell::{adjusted_zoom_level, WebAppShell, DEFAULT_ZOOM_LEVEL, ZOOM_STEP},
    ui::shell::webkit::popups::{create_popup, handle_new_window_policy, launch_external_uri},
    ui::shell::webkit::requests::{
        permission_origin, permission_request_details, response_requires_download,
    },
};

fn relative_luminance(color: &gdk::RGBA) -> f32 {
    0.2126 * color.red() + 0.7152 * color.green() + 0.0722 * color.blue()
}

pub(super) mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/cheviiot/alcove/app_window.ui")]
    pub struct AppWindow {
        pub shell: OnceCell<std::rc::Rc<WebAppShell>>,
        #[template_child]
        pub toast_overlay: TemplateChild<adw::ToastOverlay>,
        pub config: RefCell<Option<AppConfigV3>>,
        pub policy: RefCell<AppPolicyV2>,
        pub session_permissions: RefCell<BTreeSet<(Origin, PermissionKind)>>,
        pub download_manager: RefCell<Option<std::rc::Rc<DownloadManager>>>,
        pub webview: RefCell<Option<WebView>>,
        pub provider: RefCell<Option<gtk::CssProvider>>,
        pub runtime_lock: RefCell<Option<ProfileLock>>,
        pub background_hold: RefCell<Option<background::BackgroundSession>>,
        pub background_start_pending: Cell<bool>,
        pub startup_error: RefCell<Option<String>>,
        pub stop_requested: Cell<bool>,
        pub compatibility_prompt_shown: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AppWindow {
        const NAME: &'static str = "AlcoveAppWindow";
        type Type = super::AppWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for AppWindow {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().setup_gestures();
            self.obj().setup_gactions();
            self.obj().setup_runtime_toolbar();
        }
    }
    impl WidgetImpl for AppWindow {}
    impl WindowImpl for AppWindow {
        fn close_request(&self) -> glib::Propagation {
            if let Some(config) = self.config.borrow().as_ref() {
                let (width, height) = self.obj().default_size();
                let window = WindowState {
                    width,
                    height,
                    maximized: self.obj().is_maximized(),
                };
                if let Err(error) = AppService::portal().save_runtime_state(&config.id, window) {
                    eprintln!("Failed to save Alcove window state: {error:#}");
                }
            }
            if self.policy.borrow().background.enabled && !self.stop_requested.get() {
                self.obj().enter_background();
                return glib::Propagation::Stop;
            }
            self.obj().leave_background();
            // Release the profile lock here rather than at finalisation. The
            // whole interface is one process, so a delete started right after
            // this window closes would otherwise find the profile still held.
            self.obj().shell().set_content(None::<&gtk::Widget>);
            self.webview.take();
            self.runtime_lock.take();
            glib::Propagation::Proceed
        }
    }
    impl ApplicationWindowImpl for AppWindow {}
    impl AdwApplicationWindowImpl for AppWindow {}

    impl AppWindow {
        pub(super) fn create_webview(&self, config: &AppConfigV3) -> Result<WebView> {
            let service = AppService::portal();
            let runtime_lock = service.acquire_runtime_lock(&config.id)?;
            let profile = service.profile_dir(&config.id);
            let cache = service.cache_dir(&config.id);
            std::fs::create_dir_all(&profile)
                .with_context(|| format!("failed to create {}", profile.display()))?;
            std::fs::create_dir_all(&cache)
                .with_context(|| format!("failed to create {}", cache.display()))?;

            let mut settings = webkit::Settings::builder()
                .enable_webgl(true)
                .enable_webrtc(true)
                .enable_webaudio(true)
                .enable_media(true)
                .enable_mediasource(true)
                .enable_encrypted_media(true)
                .enable_media_capabilities(true)
                .hardware_acceleration_policy(HardwareAccelerationPolicy::Always)
                .enable_2d_canvas_acceleration(true)
                .enable_html5_local_storage(true)
                .enable_html5_database(true)
                .enable_site_specific_quirks(true);
            if let Some(user_agent) = config.user_agent.as_deref() {
                settings = settings.user_agent(user_agent);
            }
            let settings = settings.build();
            let network_session = webkit::NetworkSession::builder()
                .cache_directory(cache.to_string_lossy().as_ref())
                .data_directory(profile.to_string_lossy().as_ref())
                .build();
            match self.policy.borrow().proxy.mode {
                ProxyMode::System => {
                    network_session.set_proxy_settings(webkit::NetworkProxyMode::Default, None)
                }
                ProxyMode::NoProxy => {
                    network_session.set_proxy_settings(webkit::NetworkProxyMode::NoProxy, None)
                }
                ProxyMode::Custom => {
                    let uri = self
                        .policy
                        .borrow()
                        .proxy
                        .uri
                        .clone()
                        .context("custom proxy URI is missing")?;
                    let settings = webkit::NetworkProxySettings::new(Some(&uri), &[]);
                    network_session
                        .set_proxy_settings(webkit::NetworkProxyMode::Custom, Some(&settings));
                }
            }
            if let Some(cookie_manager) = network_session.cookie_manager() {
                cookie_manager.set_persistent_storage(
                    profile.join("cookies.sqlite").to_string_lossy().as_ref(),
                    webkit::CookiePersistentStorage::Sqlite,
                );
            }

            let download_manager = self.obj().download_manager();
            download_manager.set_session(&network_session);
            let window = self.obj().clone();
            network_session.connect_download_started(move |_, download| {
                window.reveal_runtime_toolbar();
                download_manager.track(download);
            });

            let content_manager = webkit::UserContentManager::new();
            if config.use_theme_color {
                let script = webkit::UserScript::new(
                    include_str!("inject.js"),
                    webkit::UserContentInjectedFrames::TopFrame,
                    webkit::UserScriptInjectionTime::End,
                    &[],
                    &[],
                );
                if content_manager.register_script_message_handler("themeColor", None) {
                    content_manager.connect_script_message_received(
                        Some("themeColor"),
                        clone!(
                            #[weak(rename_to = window)]
                            self.obj(),
                            move |_, value| {
                                let parsed = common::valid_theme_color(value.to_str().as_str());
                                window.load_colors(parsed.as_deref());
                            }
                        ),
                    );
                    content_manager.add_script(&script);
                }
            }

            let context = WebContext::new();
            context.set_spell_checking_enabled(true);
            let view = WebView::builder()
                .network_session(&network_session)
                .settings(&settings)
                .user_content_manager(&content_manager)
                .web_context(&context)
                .build();
            self.runtime_lock.replace(Some(runtime_lock));
            self.connect_webview(&view);
            Ok(view)
        }

        fn connect_webview(&self, view: &WebView) {
            view.connect_show_notification(clone!(
                #[weak(rename_to = window)]
                self.obj(),
                #[upgrade_or]
                false,
                move |_, notification| window.show_web_notification(notification)
            ));
            view.connect_permission_request(clone!(
                #[weak(rename_to = window)]
                self.obj(),
                #[upgrade_or]
                false,
                move |view, request| window.handle_permission_request(view, request)
            ));
            view.connect_decide_policy(clone!(
                #[weak(rename_to = window)]
                self.obj(),
                #[upgrade_or]
                false,
                move |view, decision, kind| window.handle_policy_decision(view, decision, kind)
            ));
            view.connect_create(clone!(
                #[weak(rename_to = window)]
                self.obj(),
                #[upgrade_or]
                None,
                move |view, action| create_popup(&window, window.upcast_ref(), view, action)
            ));
            view.connect_enter_fullscreen(clone!(
                #[weak(rename_to = window)]
                self.obj(),
                #[upgrade_or]
                false,
                move |_| {
                    window.fullscreen();
                    true
                }
            ));
            view.connect_leave_fullscreen(clone!(
                #[weak(rename_to = window)]
                self.obj(),
                #[upgrade_or]
                false,
                move |_| {
                    window.unfullscreen();
                    true
                }
            ));
            view.connect_estimated_load_progress_notify(clone!(
                #[weak(rename_to = window)]
                self.obj(),
                move |view| {
                    let progress = view.estimated_load_progress();
                    window.shell().set_loading(progress < 1.0, progress);
                }
            ));
            view.connect_title_notify(clone!(
                #[weak(rename_to = window)]
                self.obj(),
                move |view| {
                    if let Some(title) = view.title() {
                        window.shell().set_title(&title);
                    }
                }
            ));
            view.connect_load_failed(clone!(
                #[weak(rename_to = window)]
                self.obj(),
                #[upgrade_or]
                false,
                move |_, _, failing_uri, error| {
                    window.shell().set_loading(false, 0.0);
                    window.reveal_runtime_toolbar();
                    window.offer_chromium_after_load_failure(failing_uri, error);
                    false
                }
            ));
            view.connect_notify_local(
                Some("can-go-back"),
                clone!(
                    #[weak(rename_to = window)]
                    self.obj(),
                    move |view, _| window
                        .shell()
                        .set_navigation(view.can_go_back(), view.can_go_forward())
                ),
            );
            view.connect_notify_local(
                Some("can-go-forward"),
                clone!(
                    #[weak(rename_to = window)]
                    self.obj(),
                    move |view, _| window
                        .shell()
                        .set_navigation(view.can_go_back(), view.can_go_forward())
                ),
            );
        }

        pub(super) fn go_back(&self) {
            if let Some(view) = self.webview.borrow().as_ref() {
                view.go_back();
            }
        }

        pub(super) fn go_forward(&self) {
            if let Some(view) = self.webview.borrow().as_ref() {
                view.go_forward();
            }
        }
    }
}

glib::wrapper! {
    pub struct AppWindow(ObjectSubclass<imp::AppWindow>)
        @extends gtk::Widget, gtk::Window, gtk::ApplicationWindow, adw::ApplicationWindow,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
                    gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl AppWindow {
    pub fn new<P: IsA<gtk::Application>>(application: &P, config: &AppConfigV3) -> Self {
        let window: Self = glib::Object::builder()
            .property("application", application)
            .build();
        window.set_config(config);
        window
    }

    fn set_config(&self, config: &AppConfigV3) {
        self.imp().config.replace(Some(config.clone()));
        self.set_widget_name(&format!("b{}", config.id));
        self.set_title(Some(&config.title));
        self.shell().set_title(&config.title);
        self.set_default_size(config.window.width, config.window.height);
        if config.window.maximized {
            self.maximize();
        }
        let policy_loaded = match AppService::portal().load_policy(&config.id) {
            Ok(policy) => {
                self.imp().policy.replace(policy);
                true
            }
            Err(error) => {
                self.imp().policy.replace(AppPolicyV2::default());
                self.show_startup_error(&format!(
                    "{}: {error}",
                    gettext("Privacy policy could not be loaded; web content was not started")
                ));
                false
            }
        };
        if let Some(action) = self
            .lookup_action("stop-background")
            .and_downcast::<gio::SimpleAction>()
        {
            action.set_enabled(self.imp().policy.borrow().background.enabled);
        }
        if !policy_loaded {
            return;
        }
        match self.imp().create_webview(config) {
            Ok(view) => {
                let start_url = config.start_url.clone();
                let profile = AppService::portal().profile_dir(&config.id);
                let policy = self.imp().policy.borrow().clone();
                let has_enabled_filters =
                    policy.content_filters.values().any(|filter| filter.enabled);
                if has_enabled_filters {
                    let Some(manager) = view.user_content_manager() else {
                        self.show_startup_error(&gettext(
                            "Content filters could not be initialized",
                        ));
                        return;
                    };
                    let filter_error_message = gettext("Some content filters could not be enabled");
                    glib::spawn_future_local(glib::clone!(
                        #[weak(rename_to = window)]
                        self,
                        #[strong]
                        view,
                        #[strong]
                        filter_error_message,
                        async move {
                            let failures =
                                content_filters::apply_filters(&profile, &policy, &manager).await;
                            if !failures.is_empty() {
                                window.show_startup_error(&format!(
                                    "{}: {}",
                                    filter_error_message,
                                    failures.join("; ")
                                ));
                            } else {
                                window.publish_webview(&view);
                                view.load_uri(&start_url);
                            }
                        }
                    ));
                } else {
                    self.publish_webview(&view);
                    view.load_uri(&start_url);
                }
            }
            Err(error) => self.show_startup_error(&error.to_string()),
        }
        self.load_colors(None);
    }

    fn load_colors(&self, background: Option<&str>) {
        let Some(display) = gdk::Display::default() else {
            return;
        };
        let provider = self
            .imp()
            .provider
            .borrow_mut()
            .get_or_insert_with(|| {
                let provider = gtk::CssProvider::new();
                gtk::style_context_add_provider_for_display(
                    &display,
                    &provider,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                );
                provider
            })
            .clone();
        let parsed = background.and_then(|value| gdk::RGBA::parse(value).ok());
        let foreground = parsed.as_ref().map_or("@window_fg_color", |color| {
            if relative_luminance(color) > 0.5 {
                "black"
            } else {
                "white"
            }
        });
        let background = parsed
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "@window_bg_color".to_owned());
        provider.load_from_string(&format!(
            "window#{} {{ background: {}; color: {}; }}",
            self.widget_name(),
            background,
            foreground
        ));
    }

    fn setup_gactions(&self) {
        self.add_action_entries([
            gio::ActionEntry::builder("shortcuts")
                .activate(|window: &Self, _, _| crate::ui::common::show_shortcuts(window, true))
                .build(),
            gio::ActionEntry::builder("forward")
                .activate(|window: &Self, _, _| window.imp().go_forward())
                .build(),
            gio::ActionEntry::builder("back")
                .activate(|window: &Self, _, _| window.imp().go_back())
                .build(),
            gio::ActionEntry::builder("reload")
                .activate(|window: &Self, _, _| window.with_webview(|view| view.reload()))
                .build(),
            gio::ActionEntry::builder("reload-bypass-cache")
                .activate(|window: &Self, _, _| {
                    window.with_webview(|view| view.reload_bypass_cache())
                })
                .build(),
            gio::ActionEntry::builder("stop")
                .activate(|window: &Self, _, _| window.with_webview(|view| view.stop_loading()))
                .build(),
            gio::ActionEntry::builder("home")
                .activate(|window: &Self, _, _| window.go_home())
                .build(),
            gio::ActionEntry::builder("zoom-in")
                .activate(|window: &Self, _, _| window.adjust_zoom(ZOOM_STEP))
                .build(),
            gio::ActionEntry::builder("zoom-out")
                .activate(|window: &Self, _, _| window.adjust_zoom(-ZOOM_STEP))
                .build(),
            gio::ActionEntry::builder("zoom-reset")
                .activate(|window: &Self, _, _| {
                    window.with_webview(|view| view.set_zoom_level(DEFAULT_ZOOM_LEVEL))
                })
                .build(),
            gio::ActionEntry::builder("toggle-fullscreen")
                .activate(|window: &Self, _, _| window.toggle_fullscreen())
                .build(),
            gio::ActionEntry::builder("downloads")
                .activate(|window: &Self, _, _| {
                    window.reveal_runtime_toolbar();
                    window.download_manager().show();
                })
                .build(),
            gio::ActionEntry::builder("stop-background")
                .activate(|window: &Self, _, _| window.stop_background())
                .build(),
        ]);
    }

    fn download_manager(&self) -> std::rc::Rc<DownloadManager> {
        if let Some(manager) = self.imp().download_manager.borrow().as_ref() {
            return manager.clone();
        }
        let manager = DownloadManager::new(self);
        self.imp().download_manager.replace(Some(manager.clone()));
        manager
    }

    pub(super) fn handle_policy_decision(
        &self,
        view: &WebView,
        decision: &PolicyDecision,
        kind: PolicyDecisionType,
    ) -> bool {
        if kind == PolicyDecisionType::NewWindowAction {
            return handle_new_window_policy(self, decision).unwrap_or(false);
        }
        if kind == PolicyDecisionType::Response {
            if self.handle_top_level_navigation(view, decision) {
                return true;
            }
            if response_requires_download(view, decision) {
                decision.download();
                return true;
            }
        }
        false
    }

    fn handle_top_level_navigation(&self, view: &WebView, decision: &PolicyDecision) -> bool {
        let Ok(response) = decision
            .clone()
            .downcast::<webkit::ResponsePolicyDecision>()
        else {
            return false;
        };
        if !response.is_main_frame_main_resource() {
            return false;
        }
        let Some(uri) = response.request().and_then(|request| request.uri()) else {
            return false;
        };
        let Ok(url) = url::Url::parse(uri.as_str()) else {
            return false;
        };
        if !matches!(url.scheme(), "http" | "https") {
            return false;
        }
        let Ok(origin) = Origin::from_url(&url) else {
            return false;
        };
        if self.imp().policy.borrow().navigation.allows(&origin) {
            return false;
        }

        let window = self.clone();
        let decision = decision.clone();
        let download = response_requires_download(view, &decision);
        self.set_toolbar_dialog_open(true);
        glib::spawn_future_local(async move {
            let body = format!(
                "{}\n\n{}: {}",
                gettext("This origin is outside the application's navigation allowlist."),
                gettext("Destination"),
                origin
            );
            let dialog = adw::AlertDialog::new(Some(&gettext("Open Another Origin?")), Some(&body));
            dialog.add_responses(&[
                ("block", &gettext("Block")),
                ("external", &gettext("Open Externally")),
                ("once", &gettext("Open Once")),
                ("allow", &gettext("Always Allow Origin")),
            ]);
            dialog.set_response_appearance("block", adw::ResponseAppearance::Destructive);
            dialog.set_response_appearance("allow", adw::ResponseAppearance::Suggested);
            dialog.set_default_response(Some("once"));
            dialog.set_close_response("block");
            let response = dialog.choose_future(Some(&window)).await;
            window.set_toolbar_dialog_open(false);
            match response.as_str() {
                "once" => accept_policy_decision(&decision, download),
                "allow" => match window.persist_navigation_origin(origin) {
                    Ok(()) => accept_policy_decision(&decision, download),
                    Err(error) => {
                        decision.ignore();
                        window.toast(&format!(
                            "{}: {error}",
                            gettext("The origin could not be saved")
                        ));
                    }
                },
                "external" => {
                    decision.ignore();
                    launch_external_uri(&window, url.as_str());
                }
                _ => {
                    decision.ignore();
                    window.toast(&gettext("Navigation blocked"));
                }
            }
        });
        true
    }

    pub(super) fn handle_permission_request(
        &self,
        view: &WebView,
        request: &PermissionRequest,
    ) -> bool {
        let (kinds, description) = match permission_request_details(request) {
            Ok(details) => details,
            Err(error) => {
                request.deny();
                self.toast(&format!(
                    "{}: {error}",
                    gettext("Unsupported website permission request")
                ));
                return true;
            }
        };
        let origin = match permission_origin(view, request) {
            Ok(origin) => origin,
            Err(error) => {
                request.deny();
                self.toast(&format!(
                    "{}: {error}",
                    gettext("Website permission origin is unavailable")
                ));
                return true;
            }
        };

        let policy = self.imp().policy.borrow();
        if kinds
            .iter()
            .any(|kind| policy.decision(&origin, *kind) == PermissionDecision::Block)
        {
            request.deny();
            return true;
        }
        let session_permissions = self.imp().session_permissions.borrow();
        if kinds.iter().all(|kind| {
            policy.decision(&origin, *kind) == PermissionDecision::Allow
                || session_permissions.contains(&(origin.clone(), *kind))
        }) {
            request.allow();
            return true;
        }
        drop(session_permissions);
        drop(policy);

        let window = self.clone();
        let request = request.clone();
        self.set_toolbar_dialog_open(true);
        glib::spawn_future_local(async move {
            let body = format!("{}\n\n{}: {}", description, gettext("Website"), origin);
            let dialog = adw::AlertDialog::new(Some(&gettext("Website Permission")), Some(&body));
            dialog.add_responses(&[
                ("cancel", &gettext("Not Now")),
                ("block", &gettext("Always Block")),
                ("allow-session", &gettext("Allow for This Session")),
                ("allow", &gettext("Always Allow")),
            ]);
            dialog.set_response_appearance("block", adw::ResponseAppearance::Destructive);
            dialog.set_response_appearance("allow", adw::ResponseAppearance::Suggested);
            dialog.set_default_response(Some("allow-session"));
            dialog.set_close_response("cancel");

            let response = dialog.choose_future(Some(&window)).await;
            window.set_toolbar_dialog_open(false);
            match response.as_str() {
                "allow-session" => {
                    let mut session = window.imp().session_permissions.borrow_mut();
                    for kind in &kinds {
                        session.insert((origin.clone(), *kind));
                    }
                    request.allow();
                }
                "allow" => {
                    if let Err(error) = window.persist_permission_decision(
                        &origin,
                        &kinds,
                        PermissionDecision::Allow,
                    ) {
                        let mut session = window.imp().session_permissions.borrow_mut();
                        for kind in &kinds {
                            session.insert((origin.clone(), *kind));
                        }
                        window.toast(&format!(
                            "{}: {error}",
                            gettext("Permission was allowed only for this session")
                        ));
                    }
                    request.allow();
                }
                "block" => {
                    if let Err(error) = window.persist_permission_decision(
                        &origin,
                        &kinds,
                        PermissionDecision::Block,
                    ) {
                        window.toast(&format!(
                            "{}: {error}",
                            gettext("Permission block could not be saved")
                        ));
                    }
                    request.deny();
                }
                _ => request.deny(),
            }
        });
        true
    }

    pub(super) fn show_web_notification(&self, notification: &webkit::Notification) -> bool {
        let Some(id) = self
            .imp()
            .config
            .borrow()
            .as_ref()
            .map(|config| config.id.clone())
        else {
            notification.close();
            return true;
        };
        let Some(application) = self.application().and_downcast::<AlcoveApplication>() else {
            notification.close();
            self.toast(&gettext("System notifications are unavailable"));
            return true;
        };
        application.send_web_notification(&id, notification);
        true
    }

    fn persist_permission_decision(
        &self,
        origin: &Origin,
        kinds: &[PermissionKind],
        decision: PermissionDecision,
    ) -> Result<()> {
        let id = self
            .imp()
            .config
            .borrow()
            .as_ref()
            .map(|config| config.id.clone())
            .context("application configuration is unavailable")?;
        let changes = kinds
            .iter()
            .map(|kind| (origin.clone(), *kind, decision))
            .collect::<Vec<_>>();
        let updated = AppService::portal().apply_policy_decisions(&id, &changes)?;
        self.imp().policy.replace(updated);
        Ok(())
    }

    fn persist_navigation_origin(&self, origin: Origin) -> Result<()> {
        let id = self
            .imp()
            .config
            .borrow()
            .as_ref()
            .map(|config| config.id.clone())
            .context("application configuration is unavailable")?;
        let updated = AppService::portal().allow_navigation_origin(&id, origin)?;
        self.imp().policy.replace(updated);
        Ok(())
    }

    pub(crate) fn app_id(&self) -> Option<crate::domain::model::AppId> {
        self.imp()
            .config
            .borrow()
            .as_ref()
            .map(|config| config.id.clone())
    }

    pub(crate) fn start_in_background(&self) {
        if self.imp().startup_error.borrow().is_some() {
            self.imp().background_start_pending.set(false);
            self.present();
        } else if self.imp().webview.borrow().is_some() {
            self.enter_background();
        } else {
            self.imp().background_start_pending.set(true);
        }
    }

    pub(crate) fn show_from_background(&self) {
        self.imp().background_start_pending.set(false);
        self.leave_background();
        self.present();
    }

    pub(crate) fn stop_background(&self) {
        self.imp().stop_requested.set(true);
        self.imp().background_start_pending.set(false);
        self.leave_background();
        self.close();
    }

    fn enter_background(&self) {
        if self.imp().background_hold.borrow().is_some() {
            return;
        }
        if let Some(id) = self.app_id() {
            self.imp()
                .background_hold
                .replace(background::BackgroundSession::start(self, id.as_str()));
        }
    }

    fn leave_background(&self) {
        self.imp().background_hold.borrow_mut().take();
    }

    fn with_webview(&self, operation: impl FnOnce(&WebView)) {
        if let Some(view) = self.imp().webview.borrow().as_ref() {
            operation(view);
        }
    }

    fn publish_webview(&self, view: &WebView) {
        self.shell().set_content(Some(view));
        self.imp().webview.replace(Some(view.clone()));
        if self.imp().background_start_pending.replace(false) {
            self.enter_background();
        }
    }

    fn show_startup_error(&self, message: &str) {
        self.imp().startup_error.replace(Some(message.to_owned()));
        if self.imp().background_start_pending.replace(false) {
            self.present();
        }
        self.reveal_runtime_toolbar();
        self.toast(message);
    }

    fn offer_chromium_after_load_failure(&self, failing_uri: &str, error: &glib::Error) {
        if self.imp().compatibility_prompt_shown.get() {
            return;
        }
        let recommendation = CompatibilityCatalogV1::bundled()
            .ok()
            .and_then(|catalog| catalog.recommendation(failing_uri).ok().flatten().cloned())
            .filter(|entry| entry.recommended_engine() == Engine::Chromium);
        let Some(recommendation) = recommendation else {
            return;
        };
        self.imp().compatibility_prompt_shown.set(true);
        self.set_toolbar_dialog_open(true);

        let window = self.clone();
        let failing_uri = failing_uri.to_owned();
        let failure = error.to_string();
        let reason = reason_description(recommendation.reason_code());
        glib::spawn_future_local(async move {
            let body = format!(
                "{}\n\n{}: {}\n{}: {}",
                reason,
                gettext("Address"),
                failing_uri,
                gettext("WebKitGTK error"),
                failure
            );
            // Offering the engine is pointless while its add-on is missing: the
            // launch would fail and leave the person without a way forward.
            let addon_missing = matches!(
                AppService::portal().chromium_availability(),
                EngineAvailability::Missing
            );
            let body = if addon_missing {
                format!(
                    "{body}\n\n{}",
                    gettext("The Chromium add-on is not installed yet.")
                )
            } else {
                body
            };
            let dialog = adw::AlertDialog::new(
                Some(&gettext("Try This Application with Chromium?")),
                Some(&body),
            );
            let offer = if addon_missing { "install" } else { "chromium" };
            let offer_label = if addon_missing {
                gettext("Get Chromium Add-on")
            } else {
                gettext("Use Chromium")
            };
            dialog.add_responses(&[
                ("cancel", &gettext("Keep WebKitGTK")),
                (offer, &offer_label),
            ]);
            dialog.set_response_appearance(offer, adw::ResponseAppearance::Suggested);
            dialog.set_default_response(Some("cancel"));
            dialog.set_close_response("cancel");
            let response = dialog.choose_future(Some(&window)).await;
            window.set_toolbar_dialog_open(false);
            if response == "install" {
                // The prompt may return once the add-on is in place.
                window.imp().compatibility_prompt_shown.set(false);
                common::open_uri(
                    &window,
                    chromium::ADDON_REF_URL,
                    &gettext("Could Not Open the Add-on Installer"),
                );
                return;
            }
            if response != "chromium" {
                return;
            }

            let Some(id) = window.app_id() else {
                return;
            };
            let service = AppService::portal();
            let result = async {
                let mut config = service.load(&id)?;
                config.engine = Engine::Chromium;
                let parent = WindowIdentifier::from_native(&window).await;
                let saved = service.update(config, None, parent.as_ref()).await?;
                service.open_chromium(&saved, false)
            }
            .await;
            match result {
                Ok(()) => window.stop_background(),
                Err(error) => {
                    window.imp().compatibility_prompt_shown.set(false);
                    window.toast(&format!(
                        "{}: {error:#}",
                        gettext("Chromium could not be started")
                    ));
                }
            }
        });
    }

    fn go_home(&self) {
        let start_url = self
            .imp()
            .config
            .borrow()
            .as_ref()
            .map(|config| config.start_url.clone());
        if let Some(start_url) = start_url {
            self.with_webview(|view| view.load_uri(&start_url));
        }
    }

    fn adjust_zoom(&self, delta: f64) {
        self.with_webview(|view| {
            view.set_zoom_level(adjusted_zoom_level(view.zoom_level(), delta));
        });
    }

    fn toggle_fullscreen(&self) {
        if self.is_fullscreen() {
            self.unfullscreen();
        } else {
            self.fullscreen();
        }
    }

    fn setup_gestures(&self) {
        let gesture = gtk::GestureClick::new();
        gesture.set_button(0);
        gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
        gesture.connect_pressed(clone!(
            #[weak(rename_to = window)]
            self,
            move |gesture, _, _, _| {
                if window.shell().is_modal() {
                    return;
                }
                match gesture.current_button() {
                    8 => {
                        gesture.set_state(gtk::EventSequenceState::Claimed);
                        window.imp().go_back();
                    }
                    9 => {
                        gesture.set_state(gtk::EventSequenceState::Claimed);
                        window.imp().go_forward();
                    }
                    _ => {}
                }
            }
        ));
        self.add_controller(gesture);
    }

    pub(super) fn shell(&self) -> &std::rc::Rc<WebAppShell> {
        self.imp().shell.get().expect("window shell initialized")
    }

    fn setup_runtime_toolbar(&self) {
        let menu = gio::Menu::new();
        for (label, action) in [
            (gettext("_Downloads"), "win.downloads"),
            (gettext("Stop Background Activity"), "win.stop-background"),
            (gettext("_Keyboard Shortcuts"), "win.shortcuts"),
        ] {
            menu.append(Some(&label), Some(action));
        }
        let shell = WebAppShell::new(self, &menu);
        self.imp().toast_overlay.set_child(Some(shell.widget()));
        self.imp().shell.set(shell).expect("shell initialized once");
    }

    fn reveal_runtime_toolbar(&self) {
        self.shell().reveal();
    }

    fn set_toolbar_dialog_open(&self, open: bool) {
        if open {
            self.shell().begin_dialog();
        } else {
            self.shell().end_dialog();
        }
    }

    pub(crate) fn toast(&self, message: &str) {
        self.imp().toast_overlay.add_toast(adw::Toast::new(message));
    }
}

fn accept_policy_decision(decision: &PolicyDecision, download: bool) {
    if download {
        decision.download();
    } else {
        decision.use_();
    }
}
