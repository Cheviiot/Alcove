// SPDX-License-Identifier: GPL-3.0-only
//! Driving the library window and the screens reached from it under the
//! `ui-tests` feature.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;

use super::window::AlcoveWindow;
use crate::domain::model::AppConfigV3;
use crate::engines::chromium::EngineAvailability;
use crate::ui::app_page::AppPage;
use crate::ui::dialogs::addons;
use crate::ui::library::app_row::AppRow;
use crate::ui::library::ui_model::LibraryItem;

pub(crate) fn run_ui_smoke_test<P: IsA<gtk::Application>>(
    application: &P,
    library_only: bool,
) -> anyhow::Result<()> {
    use anyhow::ensure;

    let window = AlcoveWindow::new(application);
    window.set_default_size(360, 640);
    crate::ui::test_support::capture(&window, "library-empty", 800, 640)?;
    crate::ui::test_support::capture(&window, "library-empty-narrow", 360, 640)?;
    window.imp().state.borrow_mut().items = Vec::new();
    window.rebuild_list();
    ensure!(
        window.imp().view_stack.visible_child_name().as_deref() == Some("empty"),
        "empty library state was not shown"
    );

    let first = AppConfigV3::new("Alpha", "https://alpha.example", 0)?;
    let second = AppConfigV3::new("Beta", "https://beta.example", 1)?;
    let mut third = AppConfigV3::new(
        "Очень длинное название приложения для проверки узкого окна",
        "https://long-application-name.example.org",
        2,
    )?;
    third.engine = crate::domain::model::Engine::Chromium;
    window
        .imp()
        .engine_availability
        .replace(Some(EngineAvailability::Missing));
    window.imp().state.borrow_mut().items = vec![
        LibraryItem::from_config(first),
        LibraryItem::from_config(second),
        LibraryItem::from_config(third),
    ];
    window.rebuild_list();
    ensure!(
        window
            .imp()
            .store
            .borrow()
            .as_ref()
            .is_some_and(|store| store.n_items() == 3),
        "filled library did not populate the list"
    );

    crate::ui::test_support::capture(&window, "library", 800, 640)?;
    crate::ui::test_support::capture(&window, "library-narrow", 360, 640)?;
    window.imp().search_bar.set_search_mode(true);
    window.imp().search_entry.set_text("beta");
    window.set_search_text("beta");
    ensure!(
        window
            .imp()
            .store
            .borrow()
            .as_ref()
            .is_some_and(|store| store.n_items() == 1),
        "search did not filter by title"
    );
    crate::ui::test_support::capture(&window, "library-search", 360, 640)?;
    window.imp().search_entry.set_text("missing.example");
    window.set_search_text("missing.example");
    ensure!(
        window.imp().view_stack.visible_child_name().as_deref() == Some("no-results"),
        "empty search state was not shown"
    );
    crate::ui::test_support::capture(&window, "library-no-results", 360, 640)?;
    window.close_search();
    ensure!(
        window.imp().state.borrow().query.is_empty(),
        "Escape did not clear the query"
    );
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
    crate::ui::test_support::capture(&window, "library-dark", 800, 640)?;
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceLight);
    // Verify actual virtualization, including scrolling to a recycled end row.
    // A list widget nested in a non-scrollable container can silently allocate
    // every row, so checking the widget type or model length alone is not enough.
    fn rows(widget: &gtk::Widget) -> Vec<AppRow> {
        let mut result = Vec::new();
        if let Some(row) = widget.downcast_ref::<AppRow>() {
            result.push(row.clone());
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            result.extend(rows(&current));
            child = current.next_sibling();
        }
        result
    }
    let small_library = window.imp().state.borrow().items.clone();
    window.imp().state.borrow_mut().items = (0..10_000)
        .map(|index| {
            AppConfigV3::new(format!("App {index:05}"), "https://example.org", index)
                .map(LibraryItem::from_config)
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    window.rebuild_list();
    crate::ui::test_support::settle();
    let first_count = rows(window.imp().apps_list.upcast_ref()).len();
    ensure!(
        first_count > 0 && first_count < 1_000,
        "10,000 apps allocated {first_count} rows instead of a bounded viewport"
    );
    window
        .imp()
        .apps_list
        .scroll_to(9_999, gtk::ListScrollFlags::FOCUS, None);
    crate::ui::test_support::capture(&window, "library-large-end", 800, 640)?;
    let end_rows = rows(window.imp().apps_list.upcast_ref());
    ensure!(
        end_rows.len() < 1_000,
        "scrolling allocated the whole library"
    );
    ensure!(
        end_rows.iter().any(|row| row
            .tooltip_text()
            .is_some_and(|text| text.starts_with("App 09999\n"))),
        "scrolling did not bind the last application"
    );
    println!(
        "Library virtualization: 10000 apps, {first_count} initial rows, {} end rows",
        end_rows.len()
    );
    window.imp().state.borrow_mut().items = small_library;
    window.rebuild_list();
    if library_only {
        window.destroy();
        return Ok(());
    }

    let page = AppPage::new(
        AppConfigV3::new("GNOME Discourse", "https://discourse.gnome.org", 0)?,
        EngineAvailability::Missing,
    );
    window.imp().navigation_view.push(&page);
    crate::ui::test_support::capture(&window, "application", 800, 760)?;
    crate::ui::test_support::capture(&window, "application-narrow", 360, 640)?;
    page.expand_advanced();
    crate::ui::test_support::capture(&window, "application-edit", 800, 760)?;
    window.set_default_size(360, 640);
    crate::ui::test_support::settle();
    page.expand_advanced();
    crate::ui::test_support::capture(&window, "application-edit-narrow", 360, 640)?;
    crate::ui::common::show_shortcuts(&window, false);
    crate::ui::test_support::capture(&window, "shortcuts-narrow", 360, 640)?;
    if let Some(dialog) = window.visible_dialog() {
        dialog.force_close();
    }
    window.destroy();
    Ok(())
}

pub(crate) fn render_settings<P: IsA<gtk::Application>>(application: &P) -> anyhow::Result<()> {
    let window = AlcoveWindow::new(application);
    let page = AppPage::new(
        AppConfigV3::new("GNOME Discourse", "https://discourse.gnome.org", 0)?,
        EngineAvailability::Missing,
    );
    window.imp().navigation_view.push(&page);
    crate::ui::test_support::capture(&window, "application", 800, 760)?;
    crate::ui::test_support::capture(&window, "application-narrow", 360, 640)?;
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
    crate::ui::test_support::capture(&window, "application-dark", 800, 760)?;
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceLight);
    page.expand_advanced();
    crate::ui::test_support::capture(&window, "application-edit", 800, 760)?;
    window.set_default_size(360, 640);
    crate::ui::test_support::settle();
    page.expand_advanced();
    crate::ui::test_support::capture(&window, "application-edit-narrow", 360, 640)?;
    window.destroy();
    Ok(())
}

pub(crate) fn render_utilities<P: IsA<gtk::Application>>(application: &P) -> anyhow::Result<()> {
    use anyhow::ensure;
    let window = AlcoveWindow::new(application);
    window.present();
    let capture = |name: &str| -> anyhow::Result<()> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while window.visible_dialog().is_none() && std::time::Instant::now() < deadline {
            crate::ui::test_support::settle();
        }
        ensure!(window.visible_dialog().is_some(), "{name} did not open");
        crate::ui::test_support::capture(&window, name, 800, 760)?;
        crate::ui::test_support::capture(&window, &format!("{name}-narrow"), 360, 640)?;
        adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
        crate::ui::test_support::capture(&window, &format!("{name}-dark"), 800, 760)?;
        adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceLight);
        window.visible_dialog().unwrap().force_close();
        crate::ui::test_support::settle();
        Ok(())
    };
    for (name, availability) in [
        ("missing", EngineAvailability::Missing),
        ("installed", EngineAvailability::Available),
        (
            "incompatible",
            EngineAvailability::Incompatible("Unsupported add-on protocol version".into()),
        ),
        (
            "broken",
            EngineAvailability::Broken("The engine service could not be started".into()),
        ),
    ] {
        addons::present(window.upcast_ref(), &availability);
        capture(&format!("addons-{name}"))?;
    }
    window
        .imp()
        .state
        .borrow_mut()
        .warnings
        .push(crate::domain::repository::RepositoryWarning {
            path: "/temporary-test-data/alcove/apps/invalid-application/app.json".into(),
            message: "Invalid configuration: the application title is missing".into(),
        });
    window.show_repository_warnings();
    capture("diagnostics")?;
    window.show_capabilities();
    fn is_checking(widget: &gtk::Widget) -> bool {
        if widget
            .downcast_ref::<adw::ActionRow>()
            .is_some_and(|row| row.subtitle().as_deref() == Some(gettext("Checking…").as_str()))
        {
            return true;
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            if is_checking(&current) {
                return true;
            }
            child = current.next_sibling();
        }
        false
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while is_checking(window.upcast_ref()) && std::time::Instant::now() < deadline {
        crate::ui::test_support::settle();
    }
    ensure!(
        !is_checking(window.upcast_ref()),
        "capability probe did not finish"
    );
    capture("capabilities")?;
    crate::ui::common::show_shortcuts(&window, false);
    capture("shortcuts-manager")?;
    crate::ui::common::show_shortcuts(&window, true);
    capture("shortcuts-webview")?;
    application.as_ref().activate_action("about", None);
    capture("about")?;
    window.destroy();
    Ok(())
}
