// SPDX-License-Identifier: GPL-3.0-only

use std::{cell::RefCell, str::FromStr};

use adw::{prelude::*, subclass::prelude::*};
use ashpd::WindowIdentifier;
use gettextrs::gettext;
use gtk::{gdk, gio, glib};

use crate::{
    app::application::settings,
    domain::model::{AppConfigV3, AppId},
    engines::chromium::EngineAvailability,
    system::background,
    system::portal::{self, PortalFeature},
    system::service::AppService,
    ui::app_page::AppPage,
    ui::creation::CreateAppDialog,
    ui::dialogs::addons,
    ui::dialogs::backup,
    ui::dialogs::permissions,
    ui::dialogs::privacy,
    ui::library::app_row::AppRow,
    ui::library::ui_model::{LibraryItem, LibrarySortMode, LibraryState},
};

pub(super) mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/cheviiot/alcove/window.ui")]
    pub struct AlcoveWindow {
        pub state: RefCell<LibraryState>,
        pub store: RefCell<Option<gio::ListStore>>,
        pub engine_availability: RefCell<Option<EngineAvailability>>,
        #[template_child]
        pub navigation_view: TemplateChild<adw::NavigationView>,
        #[template_child]
        pub apps_list: TemplateChild<gtk::ListView>,
        #[template_child]
        pub view_stack: TemplateChild<adw::ViewStack>,
        #[template_child]
        pub search_entry: TemplateChild<gtk::SearchEntry>,
        #[template_child]
        pub search_bar: TemplateChild<gtk::SearchBar>,
        #[template_child]
        pub diagnostics_banner: TemplateChild<adw::Banner>,
        #[template_child]
        pub toast_overlay: TemplateChild<adw::ToastOverlay>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AlcoveWindow {
        const NAME: &'static str = "AlcoveWindow";
        type Type = super::AlcoveWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
            klass.bind_template_callbacks();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for AlcoveWindow {
        fn constructed(&self) {
            self.parent_constructed();
            let window = self.obj();
            window.setup_list();
            window.setup_search();
            window.setup_gactions();
            window.setup_shortcuts();
            window.load_window_size();
            window.refresh_engine_availability();
            window.refresh();
        }
    }
    impl WidgetImpl for AlcoveWindow {}
    impl WindowImpl for AlcoveWindow {
        fn close_request(&self) -> glib::Propagation {
            if let Some(dialog) = self.obj().visible_dialog() {
                dialog.close();
                return glib::Propagation::Stop;
            }
            if let Some(page) = self
                .navigation_view
                .visible_page()
                .and_downcast::<AppPage>()
            {
                if page.has_pending_changes() {
                    page.request_close();
                    return glib::Propagation::Stop;
                }
            }
            let (width, height) = self.obj().default_size();
            let settings = settings();
            if let Err(error) = settings.set_int("window-width", width) {
                eprintln!("Failed to save main window width: {error}");
            }
            if let Err(error) = settings.set_int("window-height", height) {
                eprintln!("Failed to save main window height: {error}");
            }
            glib::Propagation::Proceed
        }
    }
    impl ApplicationWindowImpl for AlcoveWindow {}
    impl AdwApplicationWindowImpl for AlcoveWindow {}

    #[gtk::template_callbacks]
    impl AlcoveWindow {
        #[template_callback]
        fn on_diagnostics_clicked(&self, _banner: adw::Banner) {
            self.obj().show_repository_warnings();
        }
    }
}

glib::wrapper! {
    pub struct AlcoveWindow(ObjectSubclass<imp::AlcoveWindow>)
        @extends gtk::Widget, gtk::Window, gtk::ApplicationWindow, adw::ApplicationWindow,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
                    gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl AlcoveWindow {
    pub fn new<P: IsA<gtk::Application>>(application: &P) -> Self {
        glib::Object::builder()
            .property("application", application)
            .build()
    }

    fn setup_list(&self) {
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(|_, object| {
            let item = object
                .downcast_ref::<gtk::ListItem>()
                .expect("library list item");
            item.set_child(Some(&AppRow::new()));
            item.set_selectable(false);
            item.set_activatable(true);
        });
        factory.connect_bind(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, object| {
                let item = object
                    .downcast_ref::<gtk::ListItem>()
                    .expect("library list item");
                let value = item
                    .item()
                    .and_downcast::<glib::BoxedAnyObject>()
                    .expect("library model item");
                let config = value.borrow::<AppConfigV3>();
                let row = item.child().and_downcast::<AppRow>().expect("library row");
                row.set_config(config.clone(), window.engine_availability());
                item.set_accessible_label(row.tooltip_text().as_deref().unwrap_or(&config.title));
                item.set_accessible_description(&gettext("Application Settings"));
            }
        ));
        factory.connect_unbind(|_, object| {
            if let Some(row) = object
                .downcast_ref::<gtk::ListItem>()
                .and_then(|item| item.child())
                .and_downcast::<AppRow>()
            {
                row.clear();
            }
        });
        self.imp().apps_list.set_factory(Some(&factory));
        self.imp()
            .apps_list
            .set_model(Some(&gtk::NoSelection::new(Some(store.clone()))));
        self.imp().apps_list.connect_activate(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |list, position| {
                if let Some(value) = list
                    .model()
                    .and_then(|model| model.item(position))
                    .and_downcast::<glib::BoxedAnyObject>()
                {
                    let id = value.borrow::<AppConfigV3>().id.clone();
                    window.show_details(&id);
                }
            }
        ));
        self.imp().store.replace(Some(store));
    }

    fn setup_shortcuts(&self) {
        let controller = gtk::ShortcutController::new();
        controller.set_scope(gtk::ShortcutScope::Managed);
        controller.add_shortcut(gtk::Shortcut::new(
            Some(gtk::KeyvalTrigger::new(
                gdk::Key::f,
                gdk::ModifierType::CONTROL_MASK,
            )),
            Some(gtk::NamedAction::new("win.focus-search")),
        ));
        controller.add_shortcut(gtk::Shortcut::new(
            Some(gtk::KeyvalTrigger::new(
                gdk::Key::n,
                gdk::ModifierType::CONTROL_MASK,
            )),
            Some(gtk::NamedAction::new("win.add")),
        ));
        controller.add_shortcut(gtk::Shortcut::new(
            Some(gtk::KeyvalTrigger::new(
                gdk::Key::Escape,
                gdk::ModifierType::empty(),
            )),
            Some(gtk::NamedAction::new("win.back")),
        ));
        controller.add_shortcut(gtk::Shortcut::new(
            Some(gtk::KeyvalTrigger::new(
                gdk::Key::Left,
                gdk::ModifierType::ALT_MASK,
            )),
            Some(gtk::NamedAction::new("win.back")),
        ));
        self.add_controller(controller);
    }

    fn setup_search(&self) {
        self.imp()
            .search_bar
            .connect_entry(&*self.imp().search_entry);
        self.imp().search_entry.connect_search_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |entry| window.set_search_text(entry.text().as_str())
        ));
        self.imp().search_entry.connect_stop_search(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.close_search()
        ));
    }

    pub(super) fn close_search(&self) {
        self.imp().search_bar.set_search_mode(false);
        self.imp().search_entry.set_text("");
        self.set_search_text("");
        self.imp().apps_list.grab_focus();
    }

    fn setup_gactions(&self) {
        let saved_sort = LibrarySortMode::from_key(&settings().string("library-sort-mode"));
        self.imp().state.borrow_mut().sort_mode = saved_sort;
        self.add_action_entries([
            gio::ActionEntry::builder("shortcuts")
                .activate(|window: &Self, _, _| crate::ui::common::show_shortcuts(window, false))
                .build(),
            gio::ActionEntry::builder("add")
                .activate(|window: &Self, _, _| {
                    CreateAppDialog::new(window.engine_availability()).present(Some(window));
                })
                .build(),
            gio::ActionEntry::builder("focus-search")
                .activate(|window: &Self, _, _| {
                    if window.imp().navigation_view.visible_page_tag().as_deref() == Some("library")
                    {
                        window.imp().search_bar.set_search_mode(true);
                        window.imp().search_entry.grab_focus();
                    }
                })
                .build(),
            gio::ActionEntry::builder("clear-search")
                .activate(|window: &Self, _, _| window.close_search())
                .build(),
            gio::ActionEntry::builder("back")
                .activate(|window: &Self, _, _| {
                    if let Some(dialog) = window.visible_dialog() {
                        dialog.close();
                        return;
                    }
                    if let Some(page) = window
                        .imp()
                        .navigation_view
                        .visible_page()
                        .and_downcast::<AppPage>()
                    {
                        page.request_back();
                        return;
                    }
                    if window.imp().navigation_view.visible_page_tag().as_deref() == Some("library")
                    {
                        window.close_search();
                    } else {
                        window.imp().navigation_view.pop();
                    }
                })
                .build(),
            gio::ActionEntry::builder("show-details")
                .parameter_type(Some(&String::static_variant_type()))
                .activate(|window: &Self, _, parameter| {
                    if let Some(id) = parse_action_id(parameter) {
                        window.show_details(&id);
                    }
                })
                .build(),
            gio::ActionEntry::builder("refresh")
                .activate(|window: &Self, _, _| window.refresh())
                .build(),
            gio::ActionEntry::builder("app-created")
                .parameter_type(Some(&String::static_variant_type()))
                .activate(|window: &Self, _, parameter| {
                    if let Some(id) = parse_action_id(parameter) {
                        window.show_created(&id);
                    }
                })
                .build(),
            gio::ActionEntry::builder("notify")
                .parameter_type(Some(&String::static_variant_type()))
                .activate(|window: &Self, _, parameter| {
                    if let Some(message) = parameter.and_then(|value| value.get::<String>()) {
                        window.toast(&message);
                    }
                })
                .build(),
            gio::ActionEntry::builder("sort")
                .parameter_type(Some(&String::static_variant_type()))
                .state(saved_sort.key().to_variant())
                .change_state(|window: &Self, action, value| {
                    let Some(value) = value.and_then(|value| value.get::<String>()) else {
                        return;
                    };
                    let mode = LibrarySortMode::from_key(&value);
                    action.set_state(&mode.key().to_variant());
                    window.imp().state.borrow_mut().sort_mode = mode;
                    let _ = settings().set_string("library-sort-mode", mode.key());
                    window.rebuild_list();
                })
                .build(),
            gio::ActionEntry::builder("delete")
                .parameter_type(Some(&String::static_variant_type()))
                .activate(|window: &Self, _, parameter| {
                    if let Some(id) = parse_action_id(parameter) {
                        window.confirm_delete(id);
                    }
                })
                .build(),
            gio::ActionEntry::builder("repair")
                .parameter_type(Some(&String::static_variant_type()))
                .activate(|window: &Self, _, parameter| {
                    if let Some(id) = parse_action_id(parameter) {
                        window.repair(id);
                    }
                })
                .build(),
            gio::ActionEntry::builder("permissions")
                .parameter_type(Some(&String::static_variant_type()))
                .activate(|window: &Self, _, parameter| {
                    if let Some(id) = parse_action_id(parameter) {
                        permissions::start(window.upcast_ref(), id);
                    }
                })
                .build(),
            gio::ActionEntry::builder("privacy")
                .parameter_type(Some(&String::static_variant_type()))
                .activate(|window: &Self, _, parameter| {
                    if let Some(id) = parse_action_id(parameter) {
                        privacy::start(window, id);
                    }
                })
                .build(),
            gio::ActionEntry::builder("addons")
                .activate(|window: &Self, _, _| {
                    addons::present(window.upcast_ref(), &window.engine_availability());
                })
                .build(),
            gio::ActionEntry::builder("backup")
                .activate(|window: &Self, _, _| backup::start_backup(window))
                .build(),
            gio::ActionEntry::builder("restore")
                .activate(|window: &Self, _, _| backup::start_restore(window))
                .build(),
            gio::ActionEntry::builder("capabilities")
                .activate(|window: &Self, _, _| window.show_capabilities())
                .build(),
        ]);
    }

    pub(super) fn set_search_text(&self, value: &str) {
        self.imp().state.borrow_mut().query = value.to_owned();
        self.rebuild_list();
    }

    pub(super) fn rebuild_list(&self) {
        let Some(store) = self.imp().store.borrow().clone() else {
            return;
        };
        let state = self.imp().state.borrow();
        let query = state.query.trim().to_lowercase();
        let mut visible = state
            .items
            .iter()
            .filter(|item| matches_search(&item.config, &query))
            .cloned()
            .collect::<Vec<_>>();
        visible.sort_by(|left, right| state.sort_mode.compare(&left.config, &right.config));
        let objects = visible
            .into_iter()
            .map(|item| glib::BoxedAnyObject::new(item.config))
            .collect::<Vec<_>>();
        store.splice(0, store.n_items(), &objects);
        let page = if state.items.is_empty() {
            "empty"
        } else if store.n_items() == 0 {
            "no-results"
        } else {
            "list"
        };
        self.imp().view_stack.set_visible_child_name(page);
    }

    fn refresh_engine_availability(&self) {
        self.imp()
            .engine_availability
            .replace(Some(AppService::portal().chromium_availability()));
    }

    pub(crate) fn engine_availability(&self) -> EngineAvailability {
        self.imp()
            .engine_availability
            .borrow()
            .clone()
            .unwrap_or(EngineAvailability::Missing)
    }

    fn show_details(&self, id: &AppId) {
        match AppService::portal().load(id) {
            Ok(config) => self
                .imp()
                .navigation_view
                .push(&AppPage::new(config, self.engine_availability())),
            Err(error) => self.toast(&error.to_string()),
        }
    }

    fn show_created(&self, id: &AppId) {
        self.imp().navigation_view.pop_to_tag("library");
        self.close_search();
        self.refresh();
        let position = self.imp().store.borrow().as_ref().and_then(|store| {
            (0..store.n_items()).find(|&position| {
                store
                    .item(position)
                    .and_downcast::<glib::BoxedAnyObject>()
                    .is_some_and(|item| item.borrow::<AppConfigV3>().id == *id)
            })
        });
        if let Some(position) = position {
            self.imp()
                .apps_list
                .scroll_to(position, gtk::ListScrollFlags::FOCUS, None);
        }
        self.toast(&gettext("Application Created"));
    }

    pub(super) fn show_repository_warnings(&self) {
        let dialog = adw::PreferencesDialog::builder()
            .title(gettext("Application Data Diagnostics"))
            .content_width(540)
            .content_height(580)
            .build();
        let page = adw::PreferencesPage::new();
        let group = adw::PreferencesGroup::builder()
            .description(gettext("Some application data could not be loaded"))
            .build();
        for warning in &self.imp().state.borrow().warnings {
            let row = adw::ActionRow::builder().use_markup(false).build();
            row.set_title(&warning.path.display().to_string());
            row.set_subtitle(&warning.message);
            row.set_title_lines(2);
            group.add(&row);
        }
        page.add(&group);
        dialog.add(&page);
        dialog.present(Some(self));
    }

    pub(super) fn show_capabilities(&self) {
        let dialog = adw::PreferencesDialog::builder()
            .title(gettext("System Capabilities"))
            .content_width(540)
            .content_height(580)
            .build();
        let page = adw::PreferencesPage::new();
        let group = adw::PreferencesGroup::builder()
            .description(gettext(
                "Alcove uses portals only and never writes launchers directly to the host.",
            ))
            .build();
        let mut rows = Vec::new();
        for title in [
            gettext("Desktop session"),
            gettext("Dynamic Launcher"),
            gettext("Application launchers"),
            gettext("Web application launchers"),
            gettext("File Chooser"),
            gettext("Documents access"),
            gettext("Background activity"),
        ] {
            let row = adw::ActionRow::builder()
                .title(title)
                .use_markup(false)
                .build();
            group.add(&row);
            rows.push(row);
        }
        let refresh = gtk::Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text(gettext("Retry"))
            .css_classes(["flat"])
            .build();
        let spinner = adw::Spinner::builder()
            .width_request(24)
            .height_request(24)
            .build();
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        actions.append(&spinner);
        actions.append(&refresh);
        group.set_header_suffix(Some(&actions));
        let alive = std::rc::Rc::new(std::cell::Cell::new(true));
        dialog.connect_closed(glib::clone!(
            #[strong]
            alive,
            move |_| alive.set(false)
        ));
        refresh.connect_clicked(glib::clone!(
            #[strong]
            rows,
            #[weak]
            spinner,
            #[strong]
            alive,
            move |button| refresh_capabilities(
                rows.clone(),
                button.clone(),
                spinner,
                alive.clone()
            )
        ));
        page.add(&group);
        dialog.add(&page);
        dialog.present(Some(self));
        refresh_capabilities(rows, refresh, spinner, alive);
    }

    fn confirm_delete(&self, id: AppId) {
        let window = self.clone();
        glib::spawn_future_local(async move {
            let dialog = adw::AlertDialog::new(
                Some(&gettext("Delete this application?")),
                Some(&gettext(
                    "Its launcher, settings, and WebKit or Chromium add-on profile—including cookies and caches—will be removed.",
                )),
            );
            dialog.add_responses(&[
                ("cancel", &gettext("Cancel")),
                ("delete", &gettext("Delete")),
            ]);
            dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
            dialog.set_default_response(Some("cancel"));
            dialog.set_close_response("cancel");
            if dialog.choose_future(Some(&window)).await == "delete" {
                match AppService::portal().delete(&id).await {
                    Ok(_) => {
                        if window
                            .imp()
                            .navigation_view
                            .visible_page()
                            .and_then(|page| page.tag())
                            .as_deref()
                            != Some("library")
                        {
                            window.imp().navigation_view.pop();
                        }
                        window.refresh();
                        window.toast(&gettext("Application deleted"));
                    }
                    Err(error) => window.toast(&error.to_string()),
                }
            }
        });
    }

    fn repair(&self, id: AppId) {
        let window = self.clone();
        glib::spawn_future_local(async move {
            let parent = WindowIdentifier::from_native(&window).await;
            match AppService::portal().repair(&id, parent.as_ref()).await {
                Ok(()) => window.toast(&gettext("Launcher repaired")),
                Err(error) => window.toast(&error.to_string()),
            }
        });
    }

    pub(crate) fn refresh(&self) {
        match AppService::portal().list() {
            Ok(report) => {
                let mut state = self.imp().state.borrow_mut();
                state.items = report
                    .apps
                    .into_iter()
                    .map(LibraryItem::from_config)
                    .collect();
                state.warnings = report.warnings;
                self.imp()
                    .diagnostics_banner
                    .set_revealed(!state.warnings.is_empty());
                drop(state);
                self.rebuild_list();
            }
            Err(error) => self.toast(&error.to_string()),
        }
    }

    fn load_window_size(&self) {
        let settings = settings();
        self.set_default_size(settings.int("window-width"), settings.int("window-height"));
    }

    pub(crate) fn toast(&self, message: &str) {
        self.imp().toast_overlay.add_toast(adw::Toast::new(message));
    }
}

fn matches_search(app: &AppConfigV3, query: &str) -> bool {
    if query.is_empty() || app.title.to_lowercase().contains(query) {
        return true;
    }
    url::Url::parse(&app.start_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_lowercase))
        .is_some_and(|host| host.contains(query))
}

fn refresh_capabilities(
    rows: Vec<adw::ActionRow>,
    refresh: gtk::Button,
    spinner: adw::Spinner,
    alive: std::rc::Rc<std::cell::Cell<bool>>,
) {
    if !refresh.is_sensitive() {
        return;
    }
    refresh.set_sensitive(false);
    spinner.set_visible(true);
    for row in &rows {
        row.set_subtitle(&gettext("Checking…"));
    }
    glib::spawn_future_local(async move {
        let background = background::capability()
            .await
            .map(|version| format!("{} (v{version})", gettext("Available")))
            .unwrap_or_else(|error| format!("{} ({error})", gettext("Unavailable")));
        let capabilities = portal::probe_capabilities().await;
        if !alive.get() {
            return;
        }
        let values = [
            capabilities.desktop,
            portal_feature(&capabilities.dynamic_launcher.interface),
            optional_availability(capabilities.dynamic_launcher.application_launchers),
            optional_availability(capabilities.dynamic_launcher.web_application_launchers),
            portal_feature(&capabilities.file_chooser),
            portal_feature(&capabilities.documents),
            background,
        ];
        for (row, value) in rows.iter().zip(values) {
            row.set_subtitle(&value);
        }
        spinner.set_visible(false);
        refresh.set_sensitive(true);
    });
}

fn optional_availability(available: Option<bool>) -> String {
    match available {
        Some(true) => gettext("Available"),
        Some(false) => gettext("Unsupported"),
        None => gettext("Unknown (interface unavailable)"),
    }
}

fn portal_feature(feature: &PortalFeature) -> String {
    match feature {
        PortalFeature::Available { version } => format!("{} (v{version})", gettext("Available")),
        PortalFeature::Problem(error) => error.to_string(),
    }
}

fn parse_action_id(parameter: Option<&glib::Variant>) -> Option<AppId> {
    parameter
        .and_then(|value| value.get::<String>())
        .and_then(|value| AppId::from_str(&value).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(title: &str, url: &str, order: u32) -> AppConfigV3 {
        AppConfigV3::new(title, url, order).unwrap()
    }

    #[test]
    fn library_sort_modes_are_stable() {
        let alpha = app("Alpha", "https://alpha.example", 1);
        let zulu = app("Zulu", "https://zulu.example", 2);
        assert_eq!(
            LibrarySortMode::TitleAscending.compare(&alpha, &zulu),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            LibrarySortMode::TitleDescending.compare(&alpha, &zulu),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            LibrarySortMode::Newest.compare(&alpha, &zulu),
            std::cmp::Ordering::Greater
        );
        assert_eq!(
            LibrarySortMode::Oldest.compare(&alpha, &zulu),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn search_matches_titles_and_domains() {
        let app = app("Example Workspace", "https://office.example.org/path", 0);
        assert!(matches_search(&app, "workspace"));
        assert!(matches_search(&app, "office.example"));
        assert!(!matches_search(&app, "missing"));
    }
}
