// SPDX-License-Identifier: GPL-3.0-only

use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
};

use adw::{prelude::*, subclass::prelude::*};
use ashpd::WindowIdentifier;
use gettextrs::gettext;
use gtk::glib;

use crate::{
    chromium::EngineAvailability,
    compatibility::{reason_description, CompatibilityCatalogV1},
    model::{AppConfigV3, Engine},
    service::{AppService, AppSetting},
    site_icon_provider::{IconHorseProvider, SiteIconProvider},
    util,
};

#[derive(Clone, Copy)]
enum TextField {
    Title,
    Url,
    UserAgent,
}

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/cheviiot/alcove/app_page.ui")]
    pub struct AppPage {
        pub config: RefCell<Option<AppConfigV3>>,
        pub availability: RefCell<Option<EngineAvailability>>,
        pub populating: Cell<bool>,
        pub saving: Cell<bool>,
        pub draining: Cell<bool>,
        pub pending: RefCell<VecDeque<AppSetting>>,
        pub leaving: Cell<bool>,
        pub pending_leave: Cell<Option<bool>>,
        pub icon_generation: Cell<u64>,
        #[template_child]
        pub details_icon: TemplateChild<gtk::Image>,
        #[template_child]
        pub details_domain: TemplateChild<gtk::Label>,
        #[template_child]
        pub engine_status: TemplateChild<gtk::Label>,
        #[template_child]
        pub engine_banner: TemplateChild<adw::Banner>,
        #[template_child]
        pub permissions_button: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub privacy_button: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub repair_button: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub delete_button: TemplateChild<adw::ButtonRow>,
        #[template_child]
        pub settings_content: TemplateChild<gtk::Box>,
        #[template_child]
        pub settings_scroll: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub save_spinner: TemplateChild<gtk::Spinner>,
        #[template_child]
        pub icon_image: TemplateChild<gtk::Image>,
        #[template_child]
        pub icon_expander: TemplateChild<adw::ExpanderRow>,
        #[template_child]
        pub url_entry: TemplateChild<adw::EntryRow>,
        #[template_child]
        pub title_entry: TemplateChild<adw::EntryRow>,
        #[template_child]
        pub title_error: TemplateChild<gtk::Label>,
        #[template_child]
        pub url_error: TemplateChild<gtk::Label>,
        #[template_child]
        pub appearance_error: TemplateChild<gtk::Label>,
        #[template_child]
        pub advanced_error: TemplateChild<gtk::Label>,
        #[template_child]
        pub restart_notice: TemplateChild<gtk::Label>,
        #[template_child]
        pub titlebar_color: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub engine_row: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub engine_note: TemplateChild<gtk::Label>,
        #[template_child]
        pub recommendation_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub advanced_expander: TemplateChild<adw::ExpanderRow>,
        #[template_child]
        pub user_agent_enabled: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub user_agent_entry: TemplateChild<adw::EntryRow>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AppPage {
        const NAME: &'static str = "AlcoveAppPage";
        type Type = super::AppPage;
        type ParentType = adw::NavigationPage;
        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
            klass.bind_template_callbacks();
        }
        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }
    impl ObjectImpl for AppPage {
        fn constructed(&self) {
            self.parent_constructed();
            let page = self.obj();
            // NavigationView disables its own Back binding while can-pop is
            // false. Route Alt+Left through saving before that binding runs.
            let navigation = gtk::EventControllerKey::new();
            navigation.set_propagation_phase(gtk::PropagationPhase::Capture);
            navigation.connect_key_pressed(glib::clone!(
                #[weak]
                page,
                #[upgrade_or]
                glib::Propagation::Proceed,
                move |_, key, _, state| {
                    if key == gtk::gdk::Key::Left
                        && state.contains(gtk::gdk::ModifierType::ALT_MASK)
                    {
                        page.request_back();
                        glib::Propagation::Stop
                    } else {
                        glib::Propagation::Proceed
                    }
                }
            ));
            page.add_controller(navigation);
            for (entry, field) in [
                (self.title_entry.get(), TextField::Title),
                (self.url_entry.get(), TextField::Url),
                (self.user_agent_entry.get(), TextField::UserAgent),
            ] {
                entry.connect_changed(glib::clone!(
                    #[weak]
                    page,
                    move |_| {
                        if !page.imp().populating.get() {
                            page.validate_text(field);
                            page.refresh_navigation();
                            if matches!(field, TextField::Url) {
                                page.refresh_recommendation();
                            }
                        }
                    }
                ));
                entry.connect_entry_activated(glib::clone!(
                    #[weak]
                    page,
                    move |_| page.commit_text(field)
                ));
                let focus = gtk::EventControllerFocus::new();
                focus.connect_leave(glib::clone!(
                    #[weak]
                    page,
                    move |_| page.commit_text(field)
                ));
                entry.add_controller(focus);
            }
            self.titlebar_color.connect_active_notify(glib::clone!(
                #[weak]
                page,
                move |row| {
                    page.queue_setting(AppSetting::ThemeColor(row.is_active()));
                }
            ));
            self.engine_row.connect_selected_notify(glib::clone!(
                #[weak]
                page,
                move |row| {
                    page.queue_setting(AppSetting::Engine(if row.selected() == 1 {
                        Engine::Chromium
                    } else {
                        Engine::WebKit
                    }));
                }
            ));
            self.user_agent_enabled.connect_active_notify(glib::clone!(
                #[weak]
                page,
                move |row| {
                    if page.imp().populating.get() {
                        return;
                    }
                    page.refresh_navigation();
                    if !row.is_active() {
                        page.queue_setting(AppSetting::UserAgent(None));
                    } else {
                        page.imp().user_agent_entry.grab_focus();
                        page.commit_text(TextField::UserAgent);
                    }
                }
            ));
        }
    }
    impl WidgetImpl for AppPage {}
    impl NavigationPageImpl for AppPage {}

    #[gtk::template_callbacks]
    impl AppPage {
        #[template_callback]
        fn on_addons_clicked(&self, _banner: adw::Banner) {
            let _ = self.obj().activate_action("win.addons", None);
        }
        #[template_callback]
        async fn on_icon_clicked(&self, _row: adw::ActionRow) {
            if self.saving.get() {
                return;
            }
            let page = self.obj();
            page.set_busy(true);
            self.icon_generation
                .set(self.icon_generation.get().wrapping_add(1));
            let window = page.root().and_downcast::<gtk::Window>();
            let result = async {
                let file = util::icon_from_dialog(window.as_ref()).await?;
                let (bytes, _) = file.load_contents_future().await?;
                util::normalize_icon(bytes.to_vec()).await
            }
            .await;
            page.set_busy(false);
            match result {
                Ok(icon) => {
                    page.save_setting(AppSetting::Icon(icon)).await;
                }
                Err(error) => {
                    if !error
                        .downcast_ref::<crate::portal::PortalOperationError>()
                        .is_some_and(|error| {
                            error.kind == crate::portal::PortalFailureKind::Cancelled
                        })
                    {
                        page.set_error(&AppSetting::Icon(Vec::new()), &error.to_string());
                    }
                }
            }
            page.continue_leave();
        }
        #[template_callback]
        async fn on_site_icon_clicked(&self, _row: adw::ActionRow) {
            if self.saving.get() || !self.obj().validate_text(TextField::Url) {
                return;
            }
            let page = self.obj();
            let Ok(url) = crate::model::parse_web_url(&self.url_entry.text()) else {
                return;
            };
            let Some(host) = url.host_str() else {
                return;
            };
            page.set_busy(true);
            self.icon_generation
                .set(self.icon_generation.get().wrapping_add(1));
            let result = IconHorseProvider.fetch(host).await;
            page.set_busy(false);
            match result {
                Ok(icon) => {
                    page.save_setting(AppSetting::Icon(icon)).await;
                }
                Err(error) => page.set_error(
                    &AppSetting::Icon(Vec::new()),
                    &format!(
                        "{} {error}",
                        gettext("The site icon provider is unavailable.")
                    ),
                ),
            }
            page.continue_leave();
        }
    }
}

glib::wrapper! {
    pub struct AppPage(ObjectSubclass<imp::AppPage>)
        @extends gtk::Widget, adw::NavigationPage,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl AppPage {
    pub fn new(config: AppConfigV3, availability: EngineAvailability) -> Self {
        let page: Self = glib::Object::builder().build();
        page.imp().availability.replace(Some(availability));
        page.imp().config.replace(Some(config.clone()));
        page.populate(&config);
        let target = config.id.as_str().to_variant();
        for (button, action) in [
            (
                page.imp()
                    .permissions_button
                    .get()
                    .upcast::<gtk::Actionable>(),
                "win.permissions",
            ),
            (
                page.imp().privacy_button.get().upcast::<gtk::Actionable>(),
                "win.privacy",
            ),
            (
                page.imp().repair_button.get().upcast::<gtk::Actionable>(),
                "win.repair",
            ),
            (
                page.imp().delete_button.get().upcast::<gtk::Actionable>(),
                "win.delete",
            ),
        ] {
            button.set_action_name(Some(action));
            button.set_action_target_value(Some(&target));
        }
        glib::spawn_future_local(glib::clone!(
            #[weak]
            page,
            async move {
                if let Ok(bytes) = AppService::portal().read_icon(&config.id) {
                    if let Ok(texture) = util::load_texture(bytes).await {
                        if page.imp().icon_generation.get() == 0 {
                            page.show_icon(&texture);
                        }
                    }
                }
            }
        ));
        page
    }

    fn populate(&self, config: &AppConfigV3) {
        let imp = self.imp();
        imp.populating.set(true);
        imp.title_entry.set_text(&config.title);
        imp.url_entry.set_text(&config.start_url);
        imp.titlebar_color.set_active(config.use_theme_color);
        imp.user_agent_enabled
            .set_active(config.user_agent.is_some());
        imp.user_agent_entry
            .set_text(config.user_agent.as_deref().unwrap_or_default());
        self.refresh_identity(config);
        imp.populating.set(false);
        self.refresh_navigation();
    }

    fn refresh_identity(&self, config: &AppConfigV3) {
        let imp = self.imp();
        let was_populating = imp.populating.replace(true);
        self.set_title(&config.title);
        let domain = url::Url::parse(&config.start_url)
            .ok()
            .and_then(|url| url.host_str().map(ToOwned::to_owned))
            .unwrap_or_else(|| config.start_url.clone());
        imp.details_domain.set_label(&domain);
        imp.details_domain.set_tooltip_text(Some(&config.start_url));
        let available = imp
            .availability
            .borrow()
            .as_ref()
            .is_some_and(EngineAvailability::is_available);
        let chromium_label = gettext("Chromium (add-on)");
        let labels = if available || config.engine == Engine::Chromium {
            vec!["WebKitGTK", chromium_label.as_str()]
        } else {
            vec!["WebKitGTK"]
        };
        imp.engine_row
            .set_model(Some(&gtk::StringList::new(&labels)));
        imp.engine_row
            .set_selected(if config.engine == Engine::Chromium {
                1
            } else {
                0
            });
        imp.engine_row.set_sensitive(labels.len() > 1);
        imp.engine_note.set_visible(labels.len() > 1);
        imp.engine_status.set_label(&match config.engine {
            Engine::WebKit => "WebKitGTK".to_owned(),
            Engine::Chromium if available => gettext("Chromium · Ready"),
            Engine::Chromium => gettext("Chromium · Add-on Required"),
        });
        imp.engine_banner
            .set_revealed(config.engine == Engine::Chromium && !available);
        imp.engine_banner
            .set_visible(config.engine == Engine::Chromium && !available);
        self.refresh_recommendation();
        imp.populating.set(was_populating);
    }

    fn refresh_recommendation(&self) {
        let recommendation = CompatibilityCatalogV1::bundled()
            .and_then(|catalog| {
                catalog
                    .recommendation(&self.imp().url_entry.text())
                    .map(|entry| {
                        entry
                            .filter(|entry| entry.recommended_engine() == Engine::Chromium)
                            .map(|entry| entry.reason_code().to_owned())
                    })
            })
            .ok()
            .flatten();
        self.imp()
            .recommendation_row
            .set_visible(recommendation.is_some());
        if let Some(reason) = recommendation {
            self.imp()
                .recommendation_row
                .set_subtitle(&reason_description(&reason));
        }
    }

    fn text_setting(&self, field: TextField) -> AppSetting {
        match field {
            TextField::Title => AppSetting::Title(self.imp().title_entry.text().to_string()),
            TextField::Url => AppSetting::StartUrl(self.imp().url_entry.text().to_string()),
            TextField::UserAgent => AppSetting::UserAgent(
                self.imp()
                    .user_agent_enabled
                    .is_active()
                    .then(|| self.imp().user_agent_entry.text().to_string()),
            ),
        }
    }

    fn validate_text(&self, field: TextField) -> bool {
        let setting = self.text_setting(field);
        let Some(mut config) = self.imp().config.borrow().clone() else {
            return true;
        };
        setting.apply(&mut config);
        let empty_agent =
            matches!(&setting, AppSetting::UserAgent(Some(value)) if value.trim().is_empty());
        let valid = !empty_agent && config.normalize_and_validate().is_ok();
        let message = if valid {
            String::new()
        } else {
            match field {
                TextField::Title => gettext("Enter a name for the application."),
                TextField::Url => gettext("Enter a valid HTTP or HTTPS address."),
                TextField::UserAgent => {
                    gettext("Enter a user agent without line breaks (up to 4096 bytes).")
                }
            }
        };
        self.set_error(&setting, &message);
        valid
    }

    fn set_error(&self, setting: &AppSetting, message: &str) {
        let imp = self.imp();
        let (label, entry) = match setting {
            AppSetting::Title(_) => (&imp.title_error, Some(&imp.title_entry)),
            AppSetting::StartUrl(_) => (&imp.url_error, Some(&imp.url_entry)),
            AppSetting::UserAgent(_) => (&imp.advanced_error, Some(&imp.user_agent_entry)),
            AppSetting::Engine(_) => (&imp.advanced_error, None),
            AppSetting::ThemeColor(_) | AppSetting::Icon(_) => (&imp.appearance_error, None),
        };
        label.set_label(message);
        label.set_visible(!message.is_empty());
        if let Some(entry) = entry {
            if message.is_empty() {
                entry.remove_css_class("error");
            } else {
                entry.add_css_class("error");
            }
        }
    }

    fn changed_settings(&self) -> Vec<AppSetting> {
        let Some(config) = self.imp().config.borrow().clone() else {
            return Vec::new();
        };
        [
            self.text_setting(TextField::Title),
            self.text_setting(TextField::Url),
            self.text_setting(TextField::UserAgent),
            AppSetting::ThemeColor(self.imp().titlebar_color.is_active()),
            AppSetting::Engine(if self.imp().engine_row.selected() == 1 {
                Engine::Chromium
            } else {
                Engine::WebKit
            }),
        ]
        .into_iter()
        .filter(|setting| {
            let mut draft = config.clone();
            setting.apply(&mut draft);
            draft != config
        })
        .collect()
    }

    pub(crate) fn has_pending_changes(&self) -> bool {
        self.imp().saving.get() || self.imp().draining.get() || !self.changed_settings().is_empty()
    }

    fn refresh_navigation(&self) {
        self.set_can_pop(!self.has_pending_changes());
    }

    fn set_busy(&self, busy: bool) {
        self.imp().saving.set(busy);
        // Keep the focused GtkText alive while the portal maps its dialog.
        // Making the entire form insensitive interrupts its key/focus event.
        for entry in [
            &self.imp().title_entry,
            &self.imp().url_entry,
            &self.imp().user_agent_entry,
        ] {
            entry.set_editable(!busy);
        }
        self.imp().titlebar_color.set_sensitive(!busy);
        self.imp().icon_expander.set_sensitive(!busy);
        self.imp().advanced_expander.set_sensitive(!busy);
        self.imp().permissions_button.set_sensitive(!busy);
        self.imp().privacy_button.set_sensitive(!busy);
        self.imp().delete_button.set_sensitive(!busy);
        self.imp().save_spinner.set_visible(busy);
        self.imp().save_spinner.set_spinning(busy);
        self.refresh_navigation();
    }

    fn commit_text(&self, field: TextField) {
        if self.imp().populating.get() || self.imp().saving.get() || self.imp().leaving.get() {
            return;
        }
        if self.validate_text(field) {
            self.queue_setting(self.text_setting(field));
        }
    }

    fn queue_setting(&self, setting: AppSetting) {
        if self.imp().populating.get() {
            return;
        }
        self.imp()
            .pending
            .borrow_mut()
            .retain(|pending| std::mem::discriminant(pending) != std::mem::discriminant(&setting));
        self.imp().pending.borrow_mut().push_back(setting);
        if self.imp().draining.replace(true) {
            return;
        }
        glib::spawn_future_local(glib::clone!(
            #[strong(rename_to = page)]
            self,
            async move {
                loop {
                    let setting = page.imp().pending.borrow_mut().pop_front();
                    let Some(setting) = setting else {
                        break;
                    };
                    page.save_setting(setting).await;
                }
                page.imp().draining.set(false);
                page.refresh_navigation();
                page.continue_leave();
            }
        ));
    }

    async fn save_setting(&self, setting: AppSetting) -> bool {
        if self.imp().saving.get() {
            return false;
        }
        let Some(previous) = self.imp().config.borrow().clone() else {
            return false;
        };
        let mut desired = previous.clone();
        setting.apply(&mut desired);
        if let Err(error) = desired.normalize_and_validate() {
            self.set_error(&setting, &error.to_string());
            return false;
        }
        if desired == previous && !matches!(setting, AppSetting::Icon(_)) {
            self.imp().populating.set(true);
            match &setting {
                AppSetting::Title(_) => self.imp().title_entry.set_text(&previous.title),
                AppSetting::StartUrl(_) => self.imp().url_entry.set_text(&previous.start_url),
                _ => {}
            }
            self.imp().populating.set(false);
            self.set_error(&setting, "");
            return true;
        }
        if matches!(setting, AppSetting::Engine(Engine::Chromium))
            && previous.engine != Engine::Chromium
            && !self
                .imp()
                .availability
                .borrow()
                .as_ref()
                .is_some_and(EngineAvailability::is_available)
        {
            self.refresh_identity(&previous);
            self.set_error(
                &setting,
                &gettext("This application needs the optional Chromium add-on."),
            );
            return false;
        }
        let blocks_input = matches!(setting, AppSetting::Title(_) | AppSetting::Icon(_));
        if blocks_input {
            self.set_busy(true);
        } else {
            // Local field edits complete without yielding to the main loop.
            // Removing focus during a focus-leave callback confuses GtkText.
            self.imp().saving.set(true);
            self.refresh_navigation();
        }
        self.set_error(&setting, "");
        let parent = if matches!(setting, AppSetting::Title(_) | AppSetting::Icon(_)) {
            match self.root().and_downcast::<gtk::Window>() {
                Some(window) => WindowIdentifier::from_native(&window).await,
                None => None,
            }
        } else {
            None
        };
        let result = AppService::portal()
            .change_setting(&previous.id, &setting, parent.as_ref())
            .await;
        let success = result.is_ok();
        match result {
            Ok(saved) => {
                self.imp().populating.set(true);
                match &setting {
                    AppSetting::Title(_) => self.imp().title_entry.set_text(&saved.title),
                    AppSetting::StartUrl(_) => self.imp().url_entry.set_text(&saved.start_url),
                    AppSetting::UserAgent(_) => {
                        self.imp()
                            .user_agent_enabled
                            .set_active(saved.user_agent.is_some());
                        if let Some(agent) = &saved.user_agent {
                            self.imp().user_agent_entry.set_text(agent);
                        }
                    }
                    AppSetting::Icon(icon) => {
                        self.imp()
                            .icon_generation
                            .set(self.imp().icon_generation.get().wrapping_add(1));
                        if let Ok(texture) = util::load_texture(icon.clone()).await {
                            self.show_icon(&texture);
                        }
                    }
                    _ => {}
                }
                if setting.requires_restart() && saved != previous {
                    self.imp().restart_notice.set_visible(true);
                }
                self.imp().config.replace(Some(saved.clone()));
                self.refresh_identity(&saved);
                self.imp().populating.set(false);
                let _ = self.activate_action("win.refresh", None);
            }
            Err(error) => {
                self.imp().populating.set(true);
                match setting {
                    AppSetting::ThemeColor(_) => self
                        .imp()
                        .titlebar_color
                        .set_active(previous.use_theme_color),
                    AppSetting::Engine(_) => self.refresh_identity(&previous),
                    AppSetting::UserAgent(None) => self
                        .imp()
                        .user_agent_enabled
                        .set_active(previous.user_agent.is_some()),
                    _ => {}
                }
                self.imp().populating.set(false);
                self.set_error(
                    &setting,
                    &format!("{} {error:#}", gettext("Changes could not be saved.")),
                );
            }
        }
        if blocks_input {
            self.set_busy(false);
        } else {
            self.imp().saving.set(false);
            self.refresh_navigation();
        }
        success
    }

    fn show_icon(&self, texture: &gtk::gdk::Texture) {
        self.imp().icon_image.set_paintable(Some(texture));
        self.imp().details_icon.set_paintable(Some(texture));
    }

    pub(crate) fn request_back(&self) {
        self.request_leave(false);
    }
    pub(crate) fn request_close(&self) {
        self.request_leave(true);
    }

    fn continue_leave(&self) {
        if let Some(close) = self.imp().pending_leave.take() {
            self.request_leave(close);
        }
    }

    fn request_leave(&self, close_window: bool) {
        if self.imp().saving.get() || self.imp().draining.get() {
            self.imp().pending_leave.set(Some(close_window));
            return;
        }
        if self.imp().leaving.replace(true) {
            return;
        }
        // A failed (but syntactically valid) rename must not trap the user
        // in an endless portal retry whenever they try to leave the page.
        let unsaved_error = self.changed_settings().iter().any(|setting| match setting {
            AppSetting::Title(_) => self.imp().title_error.is_visible(),
            AppSetting::StartUrl(_) => self.imp().url_error.is_visible(),
            AppSetting::UserAgent(_) | AppSetting::Engine(_) => {
                self.imp().advanced_error.is_visible()
            }
            AppSetting::ThemeColor(_) | AppSetting::Icon(_) => {
                self.imp().appearance_error.is_visible()
            }
        });
        if unsaved_error {
            self.imp().leaving.set(false);
            self.confirm_discard(close_window);
            return;
        }
        let mut valid = true;
        for field in [TextField::Title, TextField::Url, TextField::UserAgent] {
            valid &= self.validate_text(field);
        }
        if !valid {
            self.imp().leaving.set(false);
            self.confirm_discard(close_window);
            return;
        }
        glib::spawn_future_local(glib::clone!(
            #[strong(rename_to = page)]
            self,
            async move {
                for setting in page.changed_settings() {
                    if !page.save_setting(setting).await {
                        page.imp().leaving.set(false);
                        return;
                    }
                }
                page.imp().leaving.set(false);
                page.leave(close_window);
            }
        ));
    }

    fn leave(&self, close_window: bool) {
        if close_window {
            if let Some(window) = self.root().and_downcast::<gtk::Window>() {
                window.close();
            }
        } else {
            let _ = self.activate_action("navigation.pop", None);
        }
    }

    fn confirm_discard(&self, close_window: bool) {
        self.imp().leaving.set(true);
        let dialog = adw::AlertDialog::new(
            Some(&gettext("Discard Unsaved Changes?")),
            Some(&gettext(
                "Your unsaved changes will be lost. Saved settings will be kept.",
            )),
        );
        dialog.add_responses(&[
            ("keep", &gettext("Keep Editing")),
            ("discard", &gettext("Discard")),
        ]);
        dialog.set_close_response("keep");
        dialog.set_default_response(Some("keep"));
        dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
        dialog.connect_response(
            None,
            glib::clone!(
                #[weak(rename_to = page)]
                self,
                move |_, response| {
                    page.imp().leaving.set(false);
                    if response != "discard" {
                        return;
                    }
                    let config = page.imp().config.borrow().clone();
                    if let Some(config) = config {
                        page.populate(&config);
                    }
                    page.leave(close_window);
                }
            ),
        );
        dialog.present(Some(self));
    }

    #[cfg(feature = "ui-tests")]
    pub(crate) fn expand_advanced(&self) {
        self.imp().advanced_expander.set_expanded(true);
        self.imp().icon_expander.set_expanded(true);
        crate::ui_test_support::settle();
        let adjustment = self.imp().settings_scroll.vadjustment();
        adjustment.set_value(adjustment.upper() - adjustment.page_size());
    }
}

#[cfg(feature = "ui-tests")]
pub(crate) fn run_ui_smoke_test() -> anyhow::Result<()> {
    use anyhow::ensure;
    let config = AppConfigV3::new("UI smoke test", "https://example.org", 0)?;
    let page = AppPage::new(config, EngineAvailability::Missing);
    ensure!(!page.has_pending_changes(), "opening settings changed data");
    page.imp().title_entry.set_text("");
    ensure!(
        page.has_pending_changes() && !page.can_pop(),
        "invalid draft can disappear on navigation"
    );
    ensure!(
        page.imp().title_error.is_visible(),
        "name validation is not inline"
    );
    page.imp().url_entry.set_text("file:///tmp/test");
    ensure!(
        page.imp().url_error.is_visible(),
        "invalid address has no inline error"
    );
    let config = page.imp().config.borrow().clone().unwrap();
    page.populate(&config);
    ensure!(
        !page.has_pending_changes(),
        "restoring the saved values stayed dirty"
    );
    ensure!(
        page.imp().engine_row.model().unwrap().n_items() == 1,
        "missing engine offered for selection"
    );
    Ok(())
}
