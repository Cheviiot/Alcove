// SPDX-License-Identifier: GPL-3.0-only

mod addons_dialog;
mod app_page;
mod app_row;
mod app_window;
mod application;
use alcove::background;
mod backup;
mod backup_dialog;
mod chromium;
mod compatibility;
mod config;
mod content_filters;
mod create_app_dialog;
mod dialogs;
mod download_manager;
mod launcher;
use alcove::model;
#[cfg(feature = "native-chromium")]
mod native_chromium_launch;
mod permissions_dialog;
use alcove::policy;
mod portal;
mod privacy_dialog;
mod repository;
mod service;
mod site_icon_provider;
mod ui_model;
#[cfg(feature = "ui-tests")]
mod ui_test_support;
mod util;
use alcove::web_app_shell;
mod window;

use application::AlcoveApplication;
use config::{GETTEXT_PACKAGE, LOCALEDIR, PKGDATADIR};
use gettextrs::{bind_textdomain_codeset, bindtextdomain, textdomain};
use gtk::{gio, glib, prelude::*};
use window::AlcoveWindow;

fn main() -> glib::ExitCode {
    // SAFETY: process startup, before GTK or any worker threads are initialized.
    unsafe {
        gettextrs::setlocale(gettextrs::LocaleCategory::LcAll, "");
    }
    #[cfg(feature = "ui-tests")]
    let test_locale = std::env::var("ALCOVE_TEST_LOCALEDIR").ok();
    #[cfg(feature = "ui-tests")]
    let locale_dir = test_locale.as_deref().unwrap_or(LOCALEDIR);
    #[cfg(not(feature = "ui-tests"))]
    let locale_dir = LOCALEDIR;
    let gettext_result = bindtextdomain(GETTEXT_PACKAGE, locale_dir)
        .and_then(|_| bind_textdomain_codeset(GETTEXT_PACKAGE, "UTF-8"))
        .and_then(|_| textdomain(GETTEXT_PACKAGE));
    if let Err(error) = gettext_result {
        eprintln!("Failed to initialize translations: {error}");
        return glib::ExitCode::FAILURE;
    }

    #[cfg(feature = "ui-tests")]
    let resource_path = std::env::var("ALCOVE_TEST_RESOURCE")
        .unwrap_or_else(|_| format!("{PKGDATADIR}/alcove.gresource"));
    #[cfg(not(feature = "ui-tests"))]
    let resource_path = format!("{PKGDATADIR}/alcove.gresource");
    let resources = match gio::Resource::load(&resource_path) {
        Ok(resources) => resources,
        Err(error) => {
            eprintln!("Failed to load {resource_path}: {error}");
            return glib::ExitCode::FAILURE;
        }
    };
    gio::resources_register(&resources);

    AlcoveApplication::new(gio::ApplicationFlags::HANDLES_COMMAND_LINE).run()
}
