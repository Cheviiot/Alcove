// SPDX-License-Identifier: GPL-3.0-only
//! Driving the preferences dialogs under the `ui-tests` feature.

use adw::prelude::*;
use anyhow::ensure;
use gettextrs::gettext;
use gtk::glib;

use crate::domain::model::{AppConfigV3, AppId};
use crate::domain::policy::{
    AppPolicyV2, ContentFilterRuleSet, PermissionDecision, PermissionKind, ProxyMode,
};
use crate::ui::library::window::AlcoveWindow;
use crate::ui::shell::webkit::AppWindow;

pub(crate) fn backup_smoke_test<P: IsA<gtk::Application>>(application: &P) -> anyhow::Result<()> {
    let window = AlcoveWindow::new(application);
    let (options, include_data, passphrase, confirm) = super::backup::backup_options();
    options.dialog.present(Some(&window));
    ensure!(
        options.commit.is_sensitive(),
        "settings backup should not require a passphrase"
    );
    crate::ui::test_support::capture(&window, "backup-settings", 800, 760)?;
    include_data.set_active(true);
    ensure!(
        !options.commit.is_sensitive(),
        "site data backup allowed without encryption"
    );
    passphrase.set_text("test passphrase");
    confirm.set_text("mismatch");
    ensure!(
        !options.commit.is_sensitive(),
        "mismatching passphrases accepted"
    );
    crate::ui::test_support::capture(&window, "backup-invalid-narrow", 360, 640)?;
    confirm.set_text("test passphrase");
    ensure!(
        options.commit.is_sensitive(),
        "matching passphrases rejected"
    );
    crate::ui::test_support::capture(&window, "backup", 800, 760)?;
    crate::ui::test_support::capture(&window, "backup-narrow", 360, 640)?;
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
    crate::ui::test_support::capture(&window, "backup-dark", 800, 760)?;
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceLight);
    options.dialog.force_close();
    crate::ui::test_support::settle();
    let entries = [
        (
            "GNOME Discourse",
            crate::system::archive::RestoreDisposition::RestoreAsIs,
        ),
        (
            "Очень длинное название приложения для проверки восстановления",
            crate::system::archive::RestoreDisposition::RestoreWithNewId,
        ),
        (
            "Wikipedia",
            crate::system::archive::RestoreDisposition::SkipIdentical,
        ),
    ]
    .into_iter()
    .map(
        |(title, disposition)| crate::system::archive::RestorePreviewEntry {
            source_id: crate::domain::model::AppId::generate(),
            target_id: crate::domain::model::AppId::generate(),
            title: title.into(),
            disposition,
        },
    )
    .collect::<Vec<_>>();
    let (restore, selected) =
        super::backup::restore_options(&entries, &gettext("Select the applications to restore."));
    ensure!(
        selected.borrow().len() == 2,
        "identical app selected for restore"
    );
    restore.dialog.present(Some(&window));
    crate::ui::test_support::capture(&window, "restore", 800, 760)?;
    crate::ui::test_support::capture(&window, "restore-narrow", 360, 640)?;
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
    crate::ui::test_support::capture(&window, "restore-dark", 800, 760)?;
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceLight);
    restore.dialog.force_close();
    crate::ui::test_support::settle();
    let progress = crate::ui::common::show_progress(&window, &gettext("Creating Backup…"));
    crate::ui::test_support::capture(&window, "backup-progress-narrow", 360, 640)?;
    ensure!(!progress.can_close(), "unfinished backup can be dismissed");
    progress.force_close();
    crate::ui::test_support::settle();
    let (password, _) = super::backup::restore_passphrase_dialog(Some(&gettext(
        "Could not unlock this backup. Check the passphrase and try again.",
    )));
    password.present(Some(&window));
    crate::ui::test_support::capture(&window, "restore-passphrase-narrow", 360, 640)?;
    password.force_close();
    window.destroy();
    Ok(())
}

pub(crate) fn downloads_smoke_test<P: IsA<gtk::Application>>(
    application: &P,
) -> anyhow::Result<()> {
    let window: AppWindow = glib::Object::builder()
        .property("application", application)
        .build();
    let manager = super::downloads::DownloadManager::new(&window);
    manager.show();
    ensure!(
        manager.dialog.borrow().is_some(),
        "download manager dialog was not created"
    );
    crate::ui::test_support::capture(&window, "downloads-empty", 800, 640)?;
    crate::ui::test_support::capture(&window, "downloads-empty-narrow", 360, 640)?;
    let dialog = manager.dialog.borrow().clone();
    if let Some(dialog) = dialog {
        dialog.force_close();
    }
    crate::ui::test_support::settle();
    let active = super::downloads::DownloadItem::new(
        "https://example.org/archive".into(),
        "Очень длинное название загружаемого архива с документами.zip",
    );
    active.set_downloading("file:///tmp/Документы/Очень длинное название архива.zip".into());
    active.progress.set_fraction(0.42);
    active.progress.set_text(Some("42%"));
    let complete =
        super::downloads::DownloadItem::new("https://example.org/document".into(), "Document.pdf");
    complete.set_downloading("file:///tmp/Document.pdf".into());
    complete.complete();
    let failed =
        super::downloads::DownloadItem::new("https://example.org/image".into(), "Image.png");
    failed.fail(&"Connection interrupted");
    manager
        .items
        .borrow_mut()
        .extend([active, complete, failed]);
    manager.show();
    crate::ui::test_support::capture(&window, "downloads", 800, 760)?;
    crate::ui::test_support::capture(&window, "downloads-narrow", 360, 640)?;
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
    crate::ui::test_support::capture(&window, "downloads-dark", 800, 760)?;
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceLight);
    let dialog = manager.dialog.borrow().clone();
    if let Some(dialog) = dialog {
        dialog.force_close();
    }
    window.destroy();
    Ok(())
}

pub(crate) fn permissions_smoke_test<P: IsA<gtk::Application>>(
    application: &P,
) -> anyhow::Result<()> {
    let window = crate::ui::library::window::AlcoveWindow::new(application);
    let mut policy = crate::domain::policy::AppPolicyV2::default();
    let origin: crate::domain::policy::Origin = "https://discourse.gnome.org".parse()?;
    policy.set_decision(
        origin.clone(),
        PermissionKind::Camera,
        PermissionDecision::Block,
    );
    policy.set_decision(
        origin,
        PermissionKind::Notifications,
        PermissionDecision::Allow,
    );
    let dialog = super::permissions::present_editor(window.upcast_ref(), AppId::generate(), policy);
    crate::ui::test_support::capture(&window, "permissions", 800, 760)?;
    crate::ui::test_support::capture(&window, "permissions-narrow", 360, 640)?;
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
    crate::ui::test_support::capture(&window, "permissions-dark", 800, 760)?;
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceLight);
    dialog.force_close();
    crate::ui::test_support::settle();
    let empty = super::permissions::present_editor(
        window.upcast_ref(),
        AppId::generate(),
        Default::default(),
    );
    crate::ui::test_support::capture(&window, "permissions-empty-narrow", 360, 640)?;
    empty.force_close();
    window.destroy();
    Ok(())
}

pub(crate) fn privacy_smoke_test<P: IsA<gtk::Application>>(application: &P) -> anyhow::Result<()> {
    let parent = AlcoveWindow::new(application);
    let config = AppConfigV3::new("Privacy UI smoke test", "https://example.org/", 0)?;
    let mut policy = AppPolicyV2::default();
    super::privacy::apply_edit(
        &mut policy,
        &super::privacy::Edit::Navigation(true),
        &config.start_url,
    )?;
    super::privacy::apply_edit(
        &mut policy,
        &super::privacy::Edit::Proxy(ProxyMode::Custom, "socks5://127.0.0.1:1080".into()),
        &config.start_url,
    )?;
    policy.add_content_filter(ContentFilterRuleSet::new(
        "Example content filter",
        serde_json::json!([{"trigger":{"url-filter":".*tracker.*"},"action":{"type":"block"}}]),
    )?)?;
    let dialog = super::privacy::present_editor(&parent, policy, config);
    crate::ui::test_support::capture(&parent, "privacy", 800, 760)?;
    crate::ui::test_support::capture(&parent, "privacy-narrow", 360, 640)?;
    fn scroll_to_group(widget: &gtk::Widget, title: &str) -> bool {
        if let Some(group) = widget.downcast_ref::<adw::PreferencesGroup>() {
            if group.title().as_str() == title {
                let mut parent = widget.parent();
                while let Some(ancestor) = parent {
                    if let Some(scroll) = ancestor.downcast_ref::<gtk::ScrolledWindow>() {
                        if let Some(bounds) = widget.compute_bounds(scroll) {
                            let adjustment = scroll.vadjustment();
                            adjustment.set_value(adjustment.value() + f64::from(bounds.y()) - 16.0);
                        }
                        return true;
                    }
                    parent = ancestor.parent();
                }
            }
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            if scroll_to_group(&current, title) {
                return true;
            }
            child = current.next_sibling();
        }
        false
    }
    for (name, title) in [
        ("proxy", "Proxy"),
        ("background", "Background"),
        ("filters", "Content Filters"),
    ] {
        for (width, suffix) in [(800, ""), (360, "-narrow")] {
            parent.set_default_size(width, 760);
            crate::ui::test_support::settle();
            ensure!(scroll_to_group(dialog.upcast_ref(), &gettext(title)));
            crate::ui::test_support::capture(
                &parent,
                &format!("privacy-{name}{suffix}"),
                width,
                760,
            )?;
        }
    }
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
    crate::ui::test_support::capture(&parent, "privacy-dark", 800, 760)?;
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceLight);
    dialog.force_close();
    parent.destroy();
    Ok(())
}
