// SPDX-License-Identifier: GPL-3.0-only

use std::{cell::RefCell, collections::HashMap, str::FromStr};

use adw::prelude::*;
use adw::subclass::prelude::*;
use anyhow::{anyhow, Context, Result};
use gettextrs::gettext;
use glib::{OptionArg, OptionFlags};
use gtk::{gio, glib};

use crate::{
    domain::config,
    domain::model::{AppConfigV3, AppId, Engine},
    engines::chromium,
    system::launcher::spawn_app_process,
    system::service::AppService,
    ui::common,
    ui::library::window::AlcoveWindow,
    ui::shell::webkit::AppWindow,
};

pub fn settings() -> gio::Settings {
    gio::Settings::new(config::APP_ID)
}

fn command_app_id(arguments: &[std::ffi::OsString]) -> Option<AppId> {
    arguments
        .iter()
        .skip(1)
        .find_map(|value| AppId::from_str(&value.to_string_lossy()).ok())
}

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct AlcoveApplication {
        pub web_notifications: RefCell<HashMap<String, (String, webkit::Notification)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AlcoveApplication {
        const NAME: &'static str = "AlcoveApplication";
        type Type = super::AlcoveApplication;
        type ParentType = adw::Application;
    }

    impl ObjectImpl for AlcoveApplication {
        fn constructed(&self) {
            self.parent_constructed();
            let app = self.obj();
            app.setup_gactions();
            app.add_main_option(
                "list-applications",
                glib::Char::from(b'l'),
                OptionFlags::NONE,
                OptionArg::None,
                "List known applications",
                None,
            );
            app.add_main_option(
                "background",
                glib::Char::from(0),
                OptionFlags::NONE,
                OptionArg::None,
                "Start opted-in web applications in the background",
                None,
            );
            app.add_main_option(
                "start-background",
                glib::Char::from(0),
                OptionFlags::HIDDEN,
                OptionArg::None,
                "Start one opted-in web application without showing its window",
                None,
            );
            app.add_main_option(
                "save-chromium-window-state",
                glib::Char::from(0),
                OptionFlags::HIDDEN,
                OptionArg::String,
                "Persist Chromium window state for one application",
                Some("APP_ID"),
            );
            app.add_main_option(
                "chromium-window-width",
                glib::Char::from(0),
                OptionFlags::HIDDEN,
                OptionArg::Int,
                "Persisted Chromium window width",
                Some("WIDTH"),
            );
            app.add_main_option(
                "chromium-window-height",
                glib::Char::from(0),
                OptionFlags::HIDDEN,
                OptionArg::Int,
                "Persisted Chromium window height",
                Some("HEIGHT"),
            );
            app.add_main_option(
                "chromium-window-maximized",
                glib::Char::from(0),
                OptionFlags::HIDDEN,
                OptionArg::None,
                "Persist a maximized Chromium window",
                None,
            );
            #[cfg(feature = "ui-tests")]
            crate::ui::test_support::register_options(&*app);
            app.set_accels_for_action("win.shortcuts", &["<primary>question"]);
            app.set_accels_for_action("app.quit", &["<primary>q"]);
        }
    }

    impl ApplicationImpl for AlcoveApplication {
        fn startup(&self) {
            self.parent_startup();
            if let Some(display) = gtk::gdk::Display::default() {
                gtk::IconTheme::for_display(&display)
                    .add_resource_path("/io/github/cheviiot/alcove/icons");
            }
        }

        fn command_line(&self, command_line: &gio::ApplicationCommandLine) -> glib::ExitCode {
            #[cfg(feature = "ui-tests")]
            if let Some(code) = crate::ui::test_support::run(&*self.obj(), command_line) {
                return code;
            }

            let service = AppService::portal();
            if let Some(id) = command_line
                .options_dict()
                .lookup::<String>("save-chromium-window-state")
                .ok()
                .flatten()
            {
                let result = (|| -> Result<()> {
                    let id = AppId::from_str(&id)?;
                    let width = command_line
                        .options_dict()
                        .lookup::<i32>("chromium-window-width")
                        .context("invalid Chromium window width")?
                        .context("missing Chromium window width")?;
                    let height = command_line
                        .options_dict()
                        .lookup::<i32>("chromium-window-height")
                        .context("invalid Chromium window height")?
                        .context("missing Chromium window height")?;
                    let maximized = command_line
                        .options_dict()
                        .lookup::<bool>("chromium-window-maximized")
                        .context("invalid Chromium maximized state")?
                        .unwrap_or(false);
                    service.save_runtime_state(
                        &id,
                        crate::domain::model::WindowState {
                            width,
                            height,
                            maximized,
                        },
                    )
                })();
                return match result {
                    Ok(()) => glib::ExitCode::SUCCESS,
                    Err(error) => {
                        eprintln!("Error: {error:#}");
                        glib::ExitCode::FAILURE
                    }
                };
            }
            if command_line
                .options_dict()
                .lookup::<bool>("background")
                .ok()
                .flatten()
                .unwrap_or(false)
            {
                return match service.list() {
                    Ok(report) => {
                        let mut failed = false;
                        for config in report.apps {
                            match service.load_policy(&config.id) {
                                Ok(policy)
                                    if policy.background.enabled && policy.background.autostart =>
                                {
                                    if let Err(error) = spawn_app_process(&config.id, true) {
                                        failed = true;
                                        eprintln!(
                                            "Failed to start {} in the background: {error:#}",
                                            config.id
                                        );
                                    }
                                }
                                Ok(_) => {}
                                Err(error) => eprintln!(
                                    "Failed to load background policy for {}: {error:#}",
                                    config.id
                                ),
                            }
                        }
                        if failed {
                            glib::ExitCode::FAILURE
                        } else {
                            glib::ExitCode::SUCCESS
                        }
                    }
                    Err(error) => {
                        eprintln!("Error: {error:#}");
                        glib::ExitCode::FAILURE
                    }
                };
            }
            if command_line
                .options_dict()
                .lookup::<bool>("list-applications")
                .ok()
                .flatten()
                .unwrap_or(false)
            {
                match service.list() {
                    Ok(report) => {
                        for app in report.apps {
                            println!("{}\t{}", app.id, app.title);
                        }
                        return glib::ExitCode::SUCCESS;
                    }
                    Err(error) => {
                        eprintln!("Error: {error:#}");
                        return glib::ExitCode::FAILURE;
                    }
                }
            }

            let arguments = command_line.arguments();
            let app_id = command_app_id(&arguments);
            let start_in_background = command_line
                .options_dict()
                .lookup::<bool>("start-background")
                .ok()
                .flatten()
                .unwrap_or(false);
            if start_in_background && app_id.is_none() {
                eprintln!("Error: --start-background requires an application ID");
                return glib::ExitCode::FAILURE;
            }
            if let Some(id) = app_id {
                if let Some(window) = crate::app::chromium::existing_window(&self.obj(), &id) {
                    if !start_in_background {
                        let _ = gtk::prelude::WidgetExt::activate_action(
                            &window,
                            "win.show-background",
                            None,
                        );
                    }
                    return glib::ExitCode::SUCCESS;
                }
                if let Some(window) = self.obj().app_window(&id) {
                    if !start_in_background {
                        window.show_from_background();
                    }
                    return glib::ExitCode::SUCCESS;
                }
                let config = match service.load(&id) {
                    Ok(config) => config,
                    Err(error) => {
                        eprintln!("Error: {error:#}");
                        return glib::ExitCode::FAILURE;
                    }
                };
                if start_in_background {
                    match service.load_policy(&id) {
                        Ok(policy) if policy.background.enabled && policy.background.autostart => {}
                        Ok(_) => return glib::ExitCode::SUCCESS,
                        Err(error) => {
                            eprintln!("Error: {error:#}");
                            return glib::ExitCode::FAILURE;
                        }
                    }
                }
                match config.engine {
                    Engine::WebKit => {
                        let window = AppWindow::new(&*self.obj(), &config);
                        if start_in_background {
                            window.start_in_background();
                        } else {
                            window.present();
                        }
                    }
                    Engine::Chromium => {
                        let result =
                            crate::app::chromium::open(&self.obj(), &config, start_in_background);
                        if let Err(error) = result {
                            if start_in_background {
                                eprintln!("Error: {error:#}");
                                return glib::ExitCode::FAILURE;
                            }
                            self.obj().show_chromium_diagnostic(config, error);
                        }
                    }
                }
                return glib::ExitCode::SUCCESS;
            }

            AlcoveWindow::new(&*self.obj()).present();
            glib::ExitCode::SUCCESS
        }
    }

    impl GtkApplicationImpl for AlcoveApplication {}
    impl AdwApplicationImpl for AlcoveApplication {}
}

glib::wrapper! {
    pub struct AlcoveApplication(ObjectSubclass<imp::AlcoveApplication>)
        @extends gio::Application, gtk::Application, adw::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl AlcoveApplication {
    pub fn new(flags: gio::ApplicationFlags) -> Self {
        glib::Object::builder()
            .property("flags", flags)
            .property("application-id", Self::instance_app_id())
            .build()
    }

    fn instance_app_id() -> String {
        let arguments = std::env::args_os().collect::<Vec<_>>();
        command_app_id(&arguments)
            .filter(|id| AppService::portal().contains(id))
            .map(|id| config::managed_app_id(&id))
            .unwrap_or_else(|| config::APP_ID.to_owned())
    }

    fn setup_gactions(&self) {
        self.add_action_entries([
            gio::ActionEntry::builder("quit")
                .activate(|app: &Self, _, _| {
                    if let Some(manager) = app
                        .windows()
                        .into_iter()
                        .find_map(|window| window.downcast::<AlcoveWindow>().ok())
                    {
                        manager.close();
                    } else {
                        app.quit();
                    }
                })
                .build(),
            gio::ActionEntry::builder("about")
                .activate(|app: &Self, _, _| app.show_about())
                .build(),
            gio::ActionEntry::builder("open-app")
                .parameter_type(Some(&String::static_variant_type()))
                .activate(|app: &Self, _, parameter| {
                    let result = parameter
                        .and_then(|value| value.get::<String>())
                        .ok_or_else(|| anyhow!("missing app id"))
                        .and_then(|id| app.open_app(&id));
                    if let Err(error) = result {
                        eprintln!("Failed to open app: {error:#}");
                    }
                })
                .build(),
            gio::ActionEntry::builder("notification-activated")
                .parameter_type(Some(&String::static_variant_type()))
                .activate(|app: &Self, _, parameter| app.activate_web_notification(parameter))
                .build(),
            gio::ActionEntry::builder("show-background")
                .parameter_type(Some(&String::static_variant_type()))
                .activate(|app: &Self, _, parameter| {
                    if let Some(window) = app.background_window(parameter) {
                        window.show_from_background();
                    }
                    crate::app::chromium::background_action(app, parameter, "win.show-background");
                })
                .build(),
            gio::ActionEntry::builder("stop-background")
                .parameter_type(Some(&String::static_variant_type()))
                .activate(|app: &Self, _, parameter| {
                    if let Some(window) = app.background_window(parameter) {
                        window.stop_background();
                    }
                    crate::app::chromium::background_action(app, parameter, "win.stop-background");
                })
                .build(),
        ]);
    }

    fn app_window(&self, id: &AppId) -> Option<AppWindow> {
        self.windows().into_iter().find_map(|window| {
            window
                .downcast::<AppWindow>()
                .ok()
                .filter(|window| window.app_id().as_ref() == Some(id))
        })
    }

    fn background_window(&self, parameter: Option<&glib::Variant>) -> Option<AppWindow> {
        let id = parameter
            .and_then(|value| value.get::<String>())
            .and_then(|value| AppId::from_str(&value).ok())?;
        self.app_window(&id)
    }

    pub fn send_web_notification(&self, id: &AppId, web_notification: &webkit::Notification) {
        let token = format!("{}:{}", id, web_notification.id());
        let notification_id = format!("web-{token}");
        let title = web_notification
            .title()
            .map(|title| title.to_string())
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| gettext("Website Notification"));
        let notification = gio::Notification::new(&title);
        if let Some(body) = web_notification.body().filter(|body| !body.is_empty()) {
            notification.set_body(Some(body.as_str()));
        }
        notification.set_icon(&gio::ThemedIcon::new(config::APP_ID));
        notification.set_default_action_and_target_value(
            "app.notification-activated",
            Some(&token.to_variant()),
        );

        self.imp().web_notifications.borrow_mut().insert(
            token.clone(),
            (notification_id.clone(), web_notification.clone()),
        );
        web_notification.connect_closed(glib::clone!(
            #[weak(rename_to = app)]
            self,
            #[strong]
            token,
            move |_| {
                if let Some((notification_id, _)) =
                    app.imp().web_notifications.borrow_mut().remove(&token)
                {
                    app.withdraw_notification(&notification_id);
                }
            }
        ));
        self.send_notification(Some(&notification_id), &notification);
    }

    fn activate_web_notification(&self, parameter: Option<&glib::Variant>) {
        let Some(token) = parameter.and_then(|value| value.get::<String>()) else {
            return;
        };
        if let Some((notification_id, notification)) =
            self.imp().web_notifications.borrow_mut().remove(&token)
        {
            self.withdraw_notification(&notification_id);
            notification.clicked();
        }
        if let Some((id, _)) = token.split_once(':') {
            if let Ok(id) = AppId::from_str(id) {
                if let Some(window) = self.app_window(&id) {
                    window.show_from_background();
                    return;
                }
            }
            if let Err(error) = self.open_app(id) {
                eprintln!("Failed to open app from notification: {error:#}");
            }
        }
    }

    fn show_about(&self) {
        let dialog = adw::AboutDialog::builder()
            .application_name("Alcove")
            .application_icon(config::APP_ID)
            .developer_name("Cheviiot")
            .version(config::VERSION)
            .developers(vec!["Cheviiot"])
            .copyright("© 2024–2026 Zaedus and Alcove contributors")
            .license_type(gtk::License::Custom)
            .license("GNU General Public License version 3 only (GPL-3.0-only)")
            .website("https://github.com/Cheviiot/Alcove")
            .issue_url("https://github.com/Cheviiot/Alcove/issues")
            .build();
        let original_project = gettext("Original Spider project");
        let zaedus = gettext("Zaedus — original Spider author");
        let cameron = gettext("Cameron Radmore — Spider contributor");
        dialog.add_acknowledgement_section(
            Some(&original_project),
            &[zaedus.as_str(), cameron.as_str()],
        );
        let ux_inspiration = gettext("UX inspiration");
        let cartridges = gettext("Cartridges — interface inspiration");
        dialog.add_acknowledgement_section(Some(&ux_inspiration), &[cartridges.as_str()]);
        dialog.present(self.active_window().as_ref());
    }

    fn open_app(&self, id: &str) -> Result<()> {
        let id = AppId::from_str(id)?;
        if !AppService::portal().contains(&id) {
            return Err(anyhow!("unknown app id {id}"));
        }
        spawn_app_process(&id, false)
    }

    fn show_chromium_diagnostic(&self, config: AppConfigV3, error: anyhow::Error) {
        let manager = AlcoveWindow::new(self);
        manager.present();
        let app = self.clone();
        glib::spawn_future_local(async move {
            let body = format!(
                "{error:#}\n\n{}",
                gettext(
                    "The Chromium add-on could not start. Install or update the add-on, or run this application once with WebKitGTK. Your engine choice and profiles will not be changed."
                )
            );
            let dialog =
                adw::AlertDialog::new(Some(&gettext("Chromium Engine Unavailable")), Some(&body));
            dialog.add_responses(&[
                ("cancel", &gettext("Cancel")),
                ("install", &gettext("Get Chromium Add-on")),
                ("report", &gettext("Report Problem")),
                ("webkit", &gettext("Run Once with WebKit")),
            ]);
            dialog.set_response_appearance("webkit", adw::ResponseAppearance::Suggested);
            dialog.set_default_response(Some("webkit"));
            dialog.set_close_response("cancel");
            match dialog.choose_future(Some(&manager)).await.as_str() {
                "webkit" => {
                    manager.close();
                    AppWindow::new(&app, &config).present();
                }
                "report" => {
                    common::open_uri(
                        &manager,
                        "https://github.com/Cheviiot/Alcove/issues/new",
                        &gettext("Could Not Open the Issue Tracker"),
                    );
                }
                "install" => {
                    common::open_uri(
                        &manager,
                        chromium::ADDON_REF_URL,
                        &gettext("Could Not Open the Add-on Installer"),
                    );
                }
                _ => {}
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_id_is_found_independently_of_internal_options() {
        let cases = [
            vec![
                std::ffi::OsString::from("alcove"),
                std::ffi::OsString::from("--start-background"),
                std::ffi::OsString::from("abcdefghijkl"),
            ],
            vec![
                std::ffi::OsString::from("alcove"),
                std::ffi::OsString::from("--save-chromium-window-state"),
                std::ffi::OsString::from("abcdefghijkl"),
                std::ffi::OsString::from("--chromium-window-width"),
                std::ffi::OsString::from("1440"),
            ],
        ];
        for arguments in cases {
            assert_eq!(
                command_app_id(&arguments).as_ref().map(AppId::as_str),
                Some("abcdefghijkl")
            );
        }
    }
}
