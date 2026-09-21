// SPDX-License-Identifier: GPL-3.0-only
//! Driving the two-step creation flow under the `ui-tests` feature.

use adw::prelude::*;
use adw::subclass::prelude::*;

use gettextrs::gettext;
use gtk::glib;

use super::dialog::CreateAppDialog;
use crate::engines::chromium::EngineAvailability;

pub(crate) fn run_ui_smoke_test<P: IsA<gtk::Application>>(application: &P) -> anyhow::Result<()> {
    use anyhow::ensure;
    let window = crate::ui::library::window::AlcoveWindow::new(application);
    window.set_default_size(800, 760);
    window.present();
    let dialog = CreateAppDialog::new(EngineAvailability::Missing);
    dialog.present(Some(&window));
    ensure!(
        dialog.native().and_downcast::<gtk::Window>().is_some(),
        "dialog cannot resolve its parent for portals and library return"
    );
    ensure!(
        !dialog.imp().next_button.is_sensitive(),
        "empty address can advance"
    );
    crate::ui::test_support::capture(&window, "create-address", 800, 760)?;
    crate::ui::test_support::capture(&window, "create-address-narrow", 360, 640)?;
    dialog.imp().url_entry.set_text("file:///tmp/site");
    ensure!(
        !dialog.imp().next_button.is_sensitive() && dialog.imp().address_error.is_visible(),
        "invalid address did not show an inline error"
    );
    crate::ui::test_support::capture(&window, "create-address-invalid", 360, 640)?;
    dialog
        .imp()
        .url_entry
        .set_text("https://discourse.gnome.org/");
    ensure!(
        dialog.imp().next_button.is_sensitive(),
        "address requires a title before review"
    );
    dialog.set_lookup_loading(true);
    ensure!(dialog.can_close(), "metadata lookup cannot be cancelled");
    ensure!(
        !dialog.imp().next_button.is_sensitive(),
        "lookup can be submitted twice"
    );
    crate::ui::test_support::capture(&window, "create-lookup", 360, 640)?;
    dialog.cancel_lookup();
    dialog.imp().title_entry.set_text("GNOME Discourse");
    dialog.show_review(Some(&gettext(
        "The website could not be reached. You can still create this application.",
    )));
    ensure!(
        dialog.is_review() && dialog.imp().button.is_sensitive(),
        "offline review cannot be created"
    );
    ensure!(
        dialog.default_widget().as_ref() == Some(dialog.imp().button.upcast_ref()),
        "review Enter still activates Next"
    );
    dialog.imp().title_entry.set_text("   ");
    ensure!(
        !dialog.imp().button.is_sensitive() && dialog.imp().name_error.is_visible(),
        "empty name can be created"
    );
    dialog.imp().title_entry.set_text("Моё сообщество GNOME");
    dialog.imp().navigation_view.pop();
    glib::MainContext::default().block_on(dialog.lookup_website());
    ensure!(
        dialog.is_review() && dialog.imp().title_entry.text() == "Моё сообщество GNOME",
        "returning to the same address discarded the edited name"
    );
    dialog.set_loading(true);
    ensure!(
        !dialog.can_close()
            && !dialog.imp().button.is_sensitive()
            && !dialog.imp().review_page.can_pop(),
        "creating a launcher can be submitted twice or navigated away from"
    );
    dialog.set_loading(false);
    crate::ui::test_support::capture(&window, "create-review", 800, 760)?;
    crate::ui::test_support::capture(&window, "create-review-narrow", 360, 640)?;
    dialog.imp().advanced_row.set_expanded(true);
    crate::ui::test_support::capture(&window, "create-advanced", 360, 640)?;
    crate::ui::test_support::capture(&window, "create-small", 360, 294)?;
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
    crate::ui::test_support::capture(&window, "create-review-dark", 800, 760)?;
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceLight);
    dialog.force_close();
    crate::ui::test_support::settle();
    let dialog = CreateAppDialog::new(EngineAvailability::Available);
    dialog.present(Some(&window));
    dialog.imp().url_entry.set_text("https://example.org/");
    dialog.imp().title_entry.set_text("Chromium application");
    dialog.show_review(None);
    dialog.imp().advanced_row.set_expanded(true);
    dialog.imp().engine_row.set_selected(1);
    ensure!(
        dialog.imp().engine_row.selected() == 1 && dialog.imp().button.is_sensitive(),
        "available Chromium cannot be selected during creation"
    );
    crate::ui::test_support::capture(&window, "create-engine", 360, 640)?;
    dialog.force_close();
    window.destroy();
    Ok(())
}
