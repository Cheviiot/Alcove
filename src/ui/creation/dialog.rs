// SPDX-License-Identifier: GPL-3.0-only

use std::cell::{Cell, RefCell};

use adw::{prelude::*, subclass::prelude::*};
use ashpd::WindowIdentifier;
use futures::future::{AbortHandle, Abortable};
use gettextrs::gettext;
use gtk::glib;

use crate::{
    domain::config,
    domain::model::{parse_web_url, AppConfigV3, Engine},
    engines::chromium::{self, EngineAvailability},
    engines::compatibility::{reason_description, CompatibilityCatalogV1},
    system::service::AppService,
    ui::common,
    ui::icons,
};

pub(super) mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate, glib::Properties)]
    #[template(resource = "/io/github/cheviiot/alcove/create_app_dialog.ui")]
    #[properties(wrapper_type = super::CreateAppDialog)]
    pub struct CreateAppDialog {
        pub engine_availability: RefCell<Option<EngineAvailability>>,
        pub pending_icon: RefCell<Option<Vec<u8>>>,
        pub review_url: RefCell<Option<String>>,
        pub lookup_abort: RefCell<Option<AbortHandle>>,
        pub generation: Cell<u64>,
        #[template_child]
        pub navigation_view: TemplateChild<adw::NavigationView>,
        #[template_child]
        pub review_page: TemplateChild<adw::NavigationPage>,
        #[template_child]
        pub review_content: TemplateChild<gtk::Box>,
        #[template_child]
        pub next_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub address_error: TemplateChild<gtk::Label>,
        #[template_child]
        pub name_error: TemplateChild<gtk::Label>,
        #[template_child]
        pub address_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub lookup_notice: TemplateChild<gtk::Label>,
        #[template_child]
        pub creation_error: TemplateChild<gtk::Label>,
        #[template_child]
        pub advanced_row: TemplateChild<adw::ExpanderRow>,
        #[template_child]
        pub url_entry: TemplateChild<adw::EntryRow>,
        #[template_child]
        pub button: TemplateChild<gtk::Button>,
        #[template_child]
        pub button_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub button_spinner: TemplateChild<adw::Spinner>,
        #[template_child]
        pub button_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub icon_image: TemplateChild<gtk::Image>,
        #[template_child]
        pub icon_provider_status: TemplateChild<gtk::Label>,
        #[template_child]
        pub title_entry: TemplateChild<adw::EntryRow>,
        #[template_child]
        pub engine_row: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub recommendation_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub addon_button: TemplateChild<gtk::Button>,
        #[property(get, set)]
        pub lookup_loading: Cell<bool>,
        #[property(get, set)]
        pub loading: Cell<bool>,
        #[property(get, set)]
        pub provider_loading: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CreateAppDialog {
        const NAME: &'static str = "AlcoveCreateAppDialog";
        type Type = super::CreateAppDialog;
        type ParentType = adw::Dialog;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
            klass.bind_template_callbacks();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[glib::derived_properties]
    impl ObjectImpl for CreateAppDialog {
        fn constructed(&self) {
            self.parent_constructed();
            self.button
                .update_property(&[gtk::accessible::Property::Label(&gettext("Create"))]);
            self.addon_button.connect_clicked(glib::clone!(
                #[weak(rename_to = dialog)]
                self.obj(),
                move |_| {
                    common::open_uri(
                        &dialog,
                        chromium::ADDON_REF_URL,
                        &gettext("Could Not Open the Add-on Installer"),
                    );
                }
            ));
            self.obj()
                .connect_loading_notify(|dialog| dialog.update_busy());
            self.obj()
                .connect_provider_loading_notify(|dialog| dialog.update_busy());
            self.obj()
                .connect_lookup_loading_notify(|dialog| dialog.update_busy());
            self.obj().connect_closed(|dialog| {
                dialog.cancel_lookup();
                dialog
                    .imp()
                    .generation
                    .set(dialog.imp().generation.get().wrapping_add(1));
            });
            self.navigation_view
                .connect_visible_page_notify(glib::clone!(
                    #[weak(rename_to = dialog)]
                    self.obj(),
                    move |_| {
                        let imp = dialog.imp();
                        if dialog.is_review() {
                            dialog.set_default_widget(Some(&*imp.button));
                        } else {
                            dialog.set_default_widget(Some(&*imp.next_button));
                        }
                        dialog.validate_input();
                    }
                ));
            self.review_page.connect_shown(glib::clone!(
                #[weak(rename_to = dialog)]
                self.obj(),
                move |_| {
                    dialog.imp().title_entry.grab_focus();
                }
            ));
        }
    }
    impl WidgetImpl for CreateAppDialog {}
    impl AdwDialogImpl for CreateAppDialog {}

    #[gtk::template_callbacks]
    impl CreateAppDialog {
        #[template_callback]
        fn on_cancel_clicked(&self, _button: gtk::Button) {
            self.obj().cancel_lookup();
            self.obj().close();
        }

        #[template_callback]
        async fn on_next_clicked(&self, _button: gtk::Button) {
            self.obj().lookup_website().await;
        }

        #[template_callback]
        fn on_skip_lookup_clicked(&self, _button: gtk::Button) {
            self.obj().cancel_lookup();
            self.obj().show_review(Some(&gettext(
                "Website details were not checked. You can edit them below.",
            )));
        }

        #[template_callback]
        fn validate_input_cb(&self, _widget: gtk::Widget) {
            self.obj().validate_input();
        }

        #[template_callback]
        async fn on_icon_clicked(&self, _row: adw::ActionRow) {
            if self.obj().loading() || self.obj().provider_loading() {
                return;
            }
            let generation = self.generation.get();
            self.obj().set_provider_loading(true);
            let window = self.obj().native().and_downcast::<gtk::Window>();
            let result = async {
                let file = icons::icon_from_dialog(window.as_ref()).await?;
                let (bytes, _) = file.load_contents_future().await?;
                let icon = icons::normalize_icon(bytes.to_vec()).await?;
                let texture = icons::load_texture(icon.clone()).await?;
                anyhow::Ok((icon, texture))
            }
            .await;
            self.obj().set_provider_loading(false);
            if self.generation.get() != generation {
                return;
            }
            match result {
                Ok((icon, texture)) => {
                    self.pending_icon.replace(Some(icon));
                    self.icon_image.set_paintable(Some(&texture));
                }
                Err(error) => {
                    if !error
                        .downcast_ref::<crate::system::portal::PortalOperationError>()
                        .is_some_and(|error| {
                            error.kind == crate::system::portal::PortalFailureKind::Cancelled
                        })
                    {
                        self.obj().set_icon_provider_status(&error.to_string());
                    }
                }
            }
        }

        #[template_callback]
        async fn on_create_clicked(&self, _button: gtk::Button) {
            if !self.obj().is_review() || !self.obj().validate_input() {
                return;
            }
            self.creation_error.set_visible(false);
            self.obj().set_loading(true);
            let service = AppService::portal();
            let sort_order = service
                .list()
                .map(|report| {
                    report
                        .apps
                        .iter()
                        .map(|app| app.sort_order)
                        .max()
                        .map(|last| last.saturating_add(1))
                        .unwrap_or_default()
                })
                .unwrap_or_default();
            let result = async {
                let mut app = AppConfigV3::new(
                    self.title_entry.text().as_str(),
                    self.url_entry.text().as_str(),
                    sort_order,
                )?;
                app.engine = if self
                    .engine_availability
                    .borrow()
                    .as_ref()
                    .is_some_and(EngineAvailability::is_available)
                    && self.engine_row.selected() == 1
                {
                    Engine::Chromium
                } else {
                    Engine::WebKit
                };
                while service.contains(&app.id) {
                    app.id = crate::domain::model::AppId::generate();
                }
                let pending_icon = self.pending_icon.borrow().clone();
                let icon = match pending_icon {
                    Some(icon) => icon,
                    None => icons::default_icon().await?,
                };
                let parent = match self.obj().native().and_downcast::<gtk::Window>() {
                    Some(window) => WindowIdentifier::from_native(&window).await,
                    None => None,
                };
                service.create(app, &icon, parent.as_ref()).await
            }
            .await;
            match result {
                Ok(app) => {
                    if let Some(window) = self.obj().native().and_downcast::<gtk::Window>() {
                        // Wait for the modal dialog to release focus before
                        // focusing the newly created row in the library.
                        self.obj().connect_closed(glib::clone!(
                            #[weak]
                            window,
                            move |_| {
                                let _ = window.activate_action(
                                    "win.app-created",
                                    Some(&app.id.as_str().to_variant()),
                                );
                            }
                        ));
                    }
                    self.obj().set_loading(false);
                    self.obj().close();
                }
                Err(error) => {
                    let cancelled = error
                        .downcast_ref::<crate::system::portal::PortalOperationError>()
                        .is_some_and(|error| {
                            error.kind == crate::system::portal::PortalFailureKind::Cancelled
                        });
                    if !cancelled {
                        self.creation_error.set_label(&format!(
                            "{} {error:#}",
                            gettext("The application could not be created.")
                        ));
                        self.creation_error.set_visible(true);
                    }
                }
            }
            self.obj().set_loading(false);
        }
    }
}

glib::wrapper! {
    pub struct CreateAppDialog(ObjectSubclass<imp::CreateAppDialog>)
        @extends adw::Dialog, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl CreateAppDialog {
    pub fn new(availability: EngineAvailability) -> Self {
        let dialog: Self = glib::Object::builder().build();
        dialog.imp().icon_image.set_icon_name(Some(config::APP_ID));
        let chromium_label = gettext("Chromium (add-on)");
        let engines = if availability.is_available() {
            vec!["WebKitGTK", chromium_label.as_str()]
        } else {
            dialog
                .imp()
                .engine_row
                .set_subtitle(&gettext("Chromium · Add-on Required"));
            vec!["WebKitGTK"]
        };
        dialog
            .imp()
            .engine_row
            .set_model(Some(&gtk::StringList::new(&engines)));
        dialog.imp().engine_availability.replace(Some(availability));
        dialog
    }

    pub(super) fn is_review(&self) -> bool {
        self.imp().navigation_view.visible_page_tag().as_deref() == Some("review")
    }

    pub(super) fn cancel_lookup(&self) {
        if let Some(handle) = self.imp().lookup_abort.take() {
            handle.abort();
        }
        self.set_lookup_loading(false);
    }

    pub(super) async fn lookup_website(&self) {
        if self.is_review() || self.lookup_loading() || !self.validate_input() {
            return;
        }
        let Ok(url) = parse_web_url(&self.imp().url_entry.text()) else {
            return;
        };
        let imp = self.imp();
        imp.url_entry.set_text(url.as_str());
        if imp.review_url.borrow().as_deref() == Some(url.as_str()) {
            imp.navigation_view.push_by_tag("review");
            return;
        }
        imp.generation.set(imp.generation.get().wrapping_add(1));
        let generation = imp.generation.get();
        imp.pending_icon.take();
        imp.icon_image.set_icon_name(Some(config::APP_ID));
        imp.title_entry.set_text(url.host_str().unwrap_or_default());
        self.set_icon_provider_status("");
        imp.creation_error.set_visible(false);
        let (abort, registration) = AbortHandle::new_pair();
        imp.lookup_abort.replace(Some(abort));
        self.set_lookup_loading(true);
        // One deadline bounds the complete lookup, including all icon requests.
        // Dropping the future on skip/close prevents a late result changing a draft.
        let result = Abortable::new(
            async {
                glib::future_with_timeout(std::time::Duration::from_secs(15), async {
                    let meta = icons::get_website_meta(url).await?;
                    let texture = match meta.icon.as_ref() {
                        Some(icon) => icons::load_texture(icon.clone()).await.ok(),
                        None => None,
                    };
                    anyhow::Ok((meta, texture))
                })
                .await
            },
            registration,
        )
        .await;
        if imp.generation.get() != generation || !self.lookup_loading() {
            return;
        }
        imp.lookup_abort.take();
        self.set_lookup_loading(false);
        match result {
            Ok(Ok(Ok((meta, texture)))) => {
                if let Some(title) = meta.title {
                    imp.title_entry.set_text(&title);
                }
                if let Some(texture) = texture {
                    imp.icon_image.set_paintable(Some(&texture));
                    imp.pending_icon.replace(meta.icon);
                }
                self.show_review(None);
            }
            Ok(_) => self.show_review(Some(&gettext(
                "The website could not be reached. You can still create this application.",
            ))),
            Err(_) => (),
        }
    }

    pub(super) fn show_review(&self, notice: Option<&str>) {
        let imp = self.imp();
        imp.review_url
            .replace(Some(imp.url_entry.text().to_string()));
        imp.address_row.set_subtitle(&imp.url_entry.text());
        imp.lookup_notice.set_label(notice.unwrap_or_default());
        imp.lookup_notice.set_visible(notice.is_some());
        imp.navigation_view.push_by_tag("review");
        self.refresh_recommendation();
        self.validate_input();
    }

    fn update_busy(&self) {
        let imp = self.imp();
        imp.button_stack.set_visible_child(if self.loading() {
            imp.button_spinner.upcast_ref::<gtk::Widget>()
        } else {
            imp.button_label.upcast_ref::<gtk::Widget>()
        });
        self.set_can_close(!self.loading());
        imp.review_page
            .set_can_pop(!self.loading() && !self.provider_loading());
        imp.review_content
            .set_sensitive(!self.loading() && !self.provider_loading());
        self.validate_input();
    }

    fn validate_input(&self) -> bool {
        let imp = self.imp();
        let valid_url = parse_web_url(&imp.url_entry.text()).is_ok();
        let valid_title = !crate::domain::model::sanitize_title(&imp.title_entry.text()).is_empty();
        let url_error = !valid_url && !imp.url_entry.text().is_empty();
        imp.address_error.set_visible(url_error);
        if url_error {
            imp.url_entry.add_css_class("error");
        } else {
            imp.url_entry.remove_css_class("error");
        }
        let title_error = self.is_review() && !valid_title;
        imp.name_error.set_visible(title_error);
        if title_error {
            imp.title_entry.add_css_class("error");
        } else {
            imp.title_entry.remove_css_class("error");
        }
        let ready = !self.loading() && !self.lookup_loading() && !self.provider_loading();
        imp.next_button.set_sensitive(valid_url && ready);
        imp.button.set_sensitive(valid_url && valid_title && ready);
        valid_url && ready && (!self.is_review() || valid_title)
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
        // Recommending an engine whose add-on is missing is only useful when it
        // also says how to get it.
        let addon_missing = matches!(
            self.imp().engine_availability.borrow().as_ref(),
            Some(EngineAvailability::Missing)
        );
        self.imp()
            .addon_button
            .set_visible(recommendation.is_some() && addon_missing);
        if let Some(reason_code) = recommendation {
            self.imp()
                .recommendation_row
                .set_subtitle(&reason_description(&reason_code));
        }
    }

    fn set_icon_provider_status(&self, message: &str) {
        self.imp().icon_provider_status.set_label(message);
        self.imp()
            .icon_provider_status
            .set_visible(!message.is_empty());
    }
}
