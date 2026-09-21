// SPDX-License-Identifier: GPL-3.0-only

use adw::prelude::*;
use anyhow::{ensure, Context, Result};
use glib::{OptionArg, OptionFlags};
use gtk::{gdk, gio, glib};

pub fn settle() {
    let context = glib::MainContext::default();
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(250);
    while std::time::Instant::now() < deadline {
        while context.pending() {
            context.iteration(false);
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Render actual GTK widgets under Xvfb, and catch minimum-width regressions.
pub fn capture(window: &impl IsA<gtk::Window>, name: &str, width: i32, height: i32) -> Result<()> {
    let window = window.as_ref();
    window.set_default_size(width, height);
    window.present();
    settle();
    let dialog_width = window
        .downcast_ref::<adw::ApplicationWindow>()
        .and_then(|window| window.visible_dialog())
        .map(|dialog| {
            dialog.width().max(
                dialog
                    .child()
                    .map(|child| child.measure(gtk::Orientation::Horizontal, -1).0)
                    .unwrap_or(0),
            )
        })
        .unwrap_or(0);
    if window.width() > width || dialog_width > width {
        fn oversized(widget: &gtk::Widget, width: i32) {
            let (minimum, natural, _, _) = widget.measure(gtk::Orientation::Horizontal, -1);
            if minimum > width {
                eprintln!(
                    "Oversized {}: min={minimum} natural={natural}",
                    widget.type_().name()
                );
            }
            let mut child = widget.first_child();
            while let Some(current) = child {
                oversized(&current, width);
                child = current.next_sibling();
            }
        }
        oversized(window.upcast_ref(), width);
    }
    ensure!(
        window.width() <= width,
        "{name}: minimum window width {} exceeds {width}",
        window.width()
    );
    ensure!(
        dialog_width <= width,
        "{name}: dialog width {dialog_width} exceeds {width}"
    );
    let Some(directory) = std::env::var_os("ALCOVE_UI_SCREENSHOTS") else {
        return Ok(());
    };
    std::fs::create_dir_all(&directory)?;
    let snapshot = gtk::Snapshot::new();
    let paintable = gtk::WidgetPaintable::new(Some(window));
    paintable.snapshot(&snapshot, window.width() as f64, window.height() as f64);
    let node = snapshot.to_node().context("window has no render node")?;
    let texture = window
        .renderer()
        .context("window has no renderer")?
        .render_texture(&node, None);
    texture.save_to_png(std::path::Path::new(&directory).join(format!("{name}.png")))?;
    Ok(())
}

pub fn prepare() {
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceLight);
    if std::env::var("ADW_DEBUG_HIGH_CONTRAST").as_deref() == Ok("1") {
        assert!(
            adw::StyleManager::default().is_high_contrast(),
            "high contrast was not enabled"
        );
    }
    println!(
        "UI appearance: high_contrast={}, font={:?}",
        adw::StyleManager::default().is_high_contrast(),
        gtk::Settings::default().map(|settings| settings.gtk_font_name())
    );
    if let Some(display) = gdk::Display::default() {
        gtk::IconTheme::for_display(&display)
            .add_search_path(concat!(env!("CARGO_MANIFEST_DIR"), "/data/icons"));
    }
}

/// The hidden `--ui-test-*` options the checks under `tests/ui` drive.
pub(crate) fn register_options(app: &impl IsA<gio::Application>) {
    app.add_main_option(
        "ui-test-app-page",
        glib::Char::from(0),
        OptionFlags::HIDDEN,
        OptionArg::None,
        "Run the app-page UI regression test",
        None,
    );
    app.add_main_option(
        "ui-test-library",
        glib::Char::from(0),
        OptionFlags::HIDDEN,
        OptionArg::None,
        "Run the library UI regression test",
        None,
    );
    app.add_main_option(
        "ui-test-creation",
        glib::Char::from(0),
        OptionFlags::HIDDEN,
        OptionArg::None,
        "Run the creation UI regression test",
        None,
    );
    app.add_main_option(
        "ui-test-utilities",
        glib::Char::from(0),
        OptionFlags::HIDDEN,
        OptionArg::None,
        "Render supporting application dialogs",
        None,
    );
    app.add_main_option(
        "ui-test-policy",
        glib::Char::from(0),
        OptionFlags::HIDDEN,
        OptionArg::None,
        "Render permission and behavior settings",
        None,
    );
    app.add_main_option(
        "ui-test-settings",
        glib::Char::from(0),
        OptionFlags::HIDDEN,
        OptionArg::None,
        "Render the application settings page",
        None,
    );
}

/// Runs the screen the command line asked for, or returns `None` so the
/// application starts normally.
pub(crate) fn run(
    app: &impl IsA<gtk::Application>,
    command_line: &gio::ApplicationCommandLine,
) -> Option<glib::ExitCode> {
    let library_only = command_line
        .options_dict()
        .lookup::<bool>("ui-test-library")
        .ok()
        .flatten()
        .unwrap_or(false);
    let creation_only = command_line
        .options_dict()
        .lookup::<bool>("ui-test-creation")
        .ok()
        .flatten()
        .unwrap_or(false);
    let settings_only = command_line
        .options_dict()
        .lookup::<bool>("ui-test-settings")
        .ok()
        .flatten()
        .unwrap_or(false);
    let policy_only = command_line
        .options_dict()
        .lookup::<bool>("ui-test-policy")
        .ok()
        .flatten()
        .unwrap_or(false);
    let utilities_only = command_line
        .options_dict()
        .lookup::<bool>("ui-test-utilities")
        .ok()
        .flatten()
        .unwrap_or(false);
    if library_only
        || creation_only
        || settings_only
        || policy_only
        || utilities_only
        || command_line
            .options_dict()
            .lookup::<bool>("ui-test-app-page")
            .ok()
            .flatten()
            .unwrap_or(false)
    {
        prepare();
        let result = if library_only {
            crate::ui::library::ui_test::run_ui_smoke_test(app, true)
        } else if creation_only {
            crate::ui::creation::run_ui_smoke_test(app)
        } else if policy_only {
            crate::ui::dialogs::ui_test::permissions_smoke_test(app)
                .and_then(|()| crate::ui::dialogs::ui_test::privacy_smoke_test(app))
        } else if settings_only {
            crate::ui::app_page::run_ui_smoke_test()
                .and_then(|()| crate::ui::library::ui_test::render_settings(app))
        } else if utilities_only {
            crate::ui::dialogs::ui_test::backup_smoke_test(app)
                .and_then(|()| crate::ui::library::ui_test::render_utilities(app))
                .and_then(|()| crate::ui::dialogs::ui_test::downloads_smoke_test(app))
        } else {
            crate::ui::app_page::run_ui_smoke_test()
                .and_then(|()| crate::ui::dialogs::ui_test::downloads_smoke_test(app))
                .and_then(|()| crate::ui::dialogs::ui_test::backup_smoke_test(app))
                .and_then(|()| crate::ui::dialogs::ui_test::privacy_smoke_test(app))
                .and_then(|()| crate::ui::dialogs::ui_test::permissions_smoke_test(app))
                .and_then(|()| crate::ui::library::ui_test::run_ui_smoke_test(app, false))
                .and_then(|()| crate::ui::creation::run_ui_smoke_test(app))
                .and_then(|()| crate::ui::shell::webkit::ui_test::run_background_ui_smoke_test(app))
        };
        // A failed assertion may leave a test window registered. Close
        // it as well, so the diagnostic exits with the failure status.
        for window in app.windows() {
            window.destroy();
        }
        return Some(match result {
            Ok(()) => glib::ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("UI smoke test failed: {error:#}");
                glib::ExitCode::FAILURE
            }
        });
    }
    None
}
