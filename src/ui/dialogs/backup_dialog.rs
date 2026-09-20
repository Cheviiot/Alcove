// SPDX-License-Identifier: GPL-3.0-only

use std::collections::HashSet;

use adw::prelude::*;
use age::secrecy::SecretString;
use ashpd::WindowIdentifier;
use gettextrs::gettext;
use gtk::{gio, glib};

use crate::{
    domain::backup::{
        is_encrypted_backup, BackupOptions, BackupService, RestoreDisposition, RestorePlan,
    },
    system::portal,
    ui::library::window::AlcoveWindow,
};

fn backup_options() -> (
    crate::ui::dialogs::common::ActionDialog,
    adw::SwitchRow,
    adw::PasswordEntryRow,
    adw::PasswordEntryRow,
) {
    let options_dialog = crate::ui::dialogs::common::ActionDialog::new(
        &gettext("Back Up Alcove"),
        &gettext("Back Up"),
    );
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::builder()
        .title(gettext("Backup Contents"))
        .description(gettext(
            "Configuration, icons, and permissions are always included.",
        ))
        .build();
    let include_site_data = adw::SwitchRow::builder()
        .title(gettext("Include Website Data"))
        .subtitle(gettext(
            "Back up cookies and storage in an encrypted archive",
        ))
        .build();
    group.add(&include_site_data);
    page.add(&group);
    let encryption = adw::PreferencesGroup::builder()
        .title(gettext("Encryption"))
        .description(gettext("Use a passphrase to protect your website data."))
        .visible(false)
        .build();
    let passphrase = adw::PasswordEntryRow::builder()
        .title(gettext("Passphrase"))
        .build();
    let confirm = adw::PasswordEntryRow::builder()
        .title(gettext("Confirm Passphrase"))
        .build();
    encryption.add(&passphrase);
    encryption.add(&confirm);
    let mismatch = crate::ui::dialogs::common::inline_error();
    mismatch.set_label(&gettext("Passphrases do not match."));
    encryption.add(&mismatch);
    page.add(&encryption);
    let validate = std::rc::Rc::new(glib::clone!(
        #[weak]
        include_site_data,
        #[weak]
        passphrase,
        #[weak]
        confirm,
        #[weak]
        encryption,
        #[weak]
        mismatch,
        #[weak(rename_to = commit)]
        options_dialog.commit,
        move || {
            encryption.set_visible(include_site_data.is_active());
            let invalid = include_site_data.is_active()
                && !confirm.text().is_empty()
                && passphrase.text() != confirm.text();
            mismatch.set_visible(invalid);
            if invalid {
                confirm.add_css_class("error");
            } else {
                confirm.remove_css_class("error");
            }
            commit.set_sensitive(
                !include_site_data.is_active()
                    || (!passphrase.text().is_empty() && passphrase.text() == confirm.text()),
            );
        }
    ));
    include_site_data.connect_active_notify(glib::clone!(
        #[strong]
        validate,
        move |_| validate()
    ));
    passphrase.connect_changed(glib::clone!(
        #[strong]
        validate,
        move |_| validate()
    ));
    confirm.connect_changed(move |_| validate());
    options_dialog.toolbar.set_content(Some(&page));
    (options_dialog, include_site_data, passphrase, confirm)
}

pub fn start_backup(parent: &AlcoveWindow) {
    let window = parent.clone();
    glib::spawn_future_local(async move {
        let apps = match BackupService::portal().service().list() {
            Ok(report) if !report.apps.is_empty() => report.apps,
            Ok(_) => {
                window.toast(&gettext("There are no applications to back up"));
                return;
            }
            Err(error) => {
                window.toast(&error.to_string());
                return;
            }
        };

        let (options_dialog, include_site_data, passphrase, confirm) = backup_options();
        if !options_dialog.choose(&window).await {
            return;
        }

        let include_site_data = include_site_data.is_active();
        let passphrase = if include_site_data {
            if passphrase.text().is_empty() || passphrase.text() != confirm.text() {
                window.toast(&gettext("Passphrases must be non-empty and match"));
                return;
            }
            Some(SecretString::from(passphrase.text().to_string()))
        } else {
            None
        };
        let file_dialog = gtk::FileDialog::builder()
            .title(gettext("Save Alcove Backup"))
            .accept_label(gettext("Back Up"))
            .initial_name("Alcove.alcove-backup")
            .modal(true)
            .build();
        let file = match file_dialog.save_future(Some(&window)).await {
            Ok(file) => file,
            Err(error) => {
                if let Some(error) =
                    portal::classify_file_dialog_error(gettext("Save backup"), &error)
                {
                    window.toast(&error.to_string());
                }
                return;
            }
        };
        let Some(destination) = file.path() else {
            window.toast(&gettext("The selected destination is not writable"));
            return;
        };
        let ids = apps.into_iter().map(|app| app.id).collect::<Vec<_>>();
        let options = BackupOptions {
            include_site_data,
            passphrase,
        };
        let progress =
            crate::ui::dialogs::common::show_progress(&window, &gettext("Creating Backup…"));
        let result = gio::spawn_blocking(move || {
            BackupService::portal().create_backup(&destination, &ids, &options)
        })
        .await;
        progress.force_close();
        match result {
            Ok(Ok(())) => window.toast(&gettext("Backup completed")),
            Ok(Err(error)) => window.toast(&format!("{}: {error:#}", gettext("Backup failed"))),
            Err(_) => window.toast(&gettext("The backup worker stopped unexpectedly")),
        }
    });
}

pub fn start_restore(parent: &AlcoveWindow) {
    let window = parent.clone();
    glib::spawn_future_local(async move {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(&gettext("Alcove Backups")));
        filter.add_pattern("*.alcove-backup");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let file_dialog = gtk::FileDialog::builder()
            .title(gettext("Open Alcove Backup"))
            .accept_label(gettext("Open"))
            .filters(&filters)
            .modal(true)
            .build();
        let file = match file_dialog.open_future(Some(&window)).await {
            Ok(file) => file,
            Err(error) => {
                if let Some(error) =
                    portal::classify_file_dialog_error(gettext("Restore backup"), &error)
                {
                    window.toast(&error.to_string());
                }
                return;
            }
        };
        let Some(source) = file.path() else {
            window.toast(&gettext("The selected backup cannot be read"));
            return;
        };
        let encrypted = match is_encrypted_backup(&source) {
            Ok(encrypted) => encrypted,
            Err(error) => {
                window.toast(&error.to_string());
                return;
            }
        };
        let mut passphrase_error = None;
        let plan = loop {
            let passphrase = if encrypted {
                match ask_restore_passphrase(&window, passphrase_error.as_deref()).await {
                    Some(passphrase) => Some(passphrase),
                    None => return,
                }
            } else {
                None
            };
            let progress =
                crate::ui::dialogs::common::show_progress(&window, &gettext("Opening Backup…"));
            let source = source.clone();
            let prepared = gio::spawn_blocking(move || {
                BackupService::portal().prepare_restore(&source, passphrase.as_ref())
            })
            .await;
            progress.force_close();
            match prepared {
                Ok(Ok(plan)) => break plan,
                Ok(Err(error))
                    if encrypted
                        && matches!(
                            error.downcast_ref::<age::DecryptError>(),
                            Some(
                                age::DecryptError::DecryptionFailed
                                    | age::DecryptError::NoMatchingKeys
                                    | age::DecryptError::KeyDecryptionFailed
                                    | age::DecryptError::InvalidMac
                            )
                        ) =>
                {
                    passphrase_error = Some(gettext(
                        "Could not unlock this backup. Check the passphrase and try again.",
                    ));
                }
                Ok(Err(error)) => {
                    window.toast(&format!(
                        "{}: {error:#}",
                        gettext("Backup could not be opened")
                    ));
                    return;
                }
                Err(_) => {
                    window.toast(&gettext("The restore worker stopped unexpectedly"));
                    return;
                }
            }
        };
        show_restore_preview(&window, plan).await;
    });
}

fn restore_passphrase_dialog(error: Option<&str>) -> (adw::AlertDialog, gtk::PasswordEntry) {
    let dialog = adw::AlertDialog::new(
        Some(&gettext("Encrypted Backup")),
        Some(&gettext("Enter the passphrase used to create this backup.")),
    );
    dialog.add_responses(&[("cancel", &gettext("Cancel")), ("open", &gettext("Open"))]);
    dialog.set_response_appearance("open", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("open"));
    dialog.set_close_response("cancel");
    let passphrase = gtk::PasswordEntry::builder()
        .placeholder_text(gettext("Passphrase"))
        .show_peek_icon(true)
        .activates_default(true)
        .build();
    passphrase.update_property(&[gtk::accessible::Property::Label(&gettext("Passphrase"))]);
    let fields = gtk::Box::new(gtk::Orientation::Vertical, 12);
    fields.append(&passphrase);
    let feedback = crate::ui::dialogs::common::inline_error();
    if let Some(error) = error {
        feedback.set_label(error);
        feedback.set_visible(true);
        passphrase.add_css_class("error");
    }
    fields.append(&feedback);
    dialog.set_extra_child(Some(&fields));
    dialog.set_response_enabled("open", false);
    passphrase.connect_changed(glib::clone!(
        #[weak]
        dialog,
        #[weak]
        feedback,
        move |entry| {
            dialog.set_response_enabled("open", !entry.text().is_empty());
            entry.remove_css_class("error");
            feedback.set_visible(false);
        }
    ));
    (dialog, passphrase)
}

async fn ask_restore_passphrase(
    parent: &AlcoveWindow,
    error: Option<&str>,
) -> Option<SecretString> {
    let (dialog, passphrase) = restore_passphrase_dialog(error);
    if dialog.choose_future(Some(parent)).await != "open" || passphrase.text().is_empty() {
        return None;
    }
    Some(SecretString::from(passphrase.text().to_string()))
}

type RestoreSelection = std::rc::Rc<std::cell::RefCell<HashSet<crate::domain::model::AppId>>>;

fn restore_options(
    entries: &[crate::domain::backup::RestorePreviewEntry],
    description: &str,
) -> (crate::ui::dialogs::common::ActionDialog, RestoreSelection) {
    let action_dialog = crate::ui::dialogs::common::ActionDialog::new(
        &gettext("Restore Applications"),
        &gettext("Restore"),
    );
    let page = adw::PreferencesPage::new();
    let group = adw::PreferencesGroup::builder()
        .title(gettext("Applications"))
        .description(description)
        .build();
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    let selected = std::rc::Rc::new(std::cell::RefCell::new(HashSet::new()));
    for entry in entries {
        let subtitle = match entry.disposition {
            RestoreDisposition::RestoreAsIs => gettext("Restore this application"),
            RestoreDisposition::RestoreWithNewId => gettext("Restore as a separate copy"),
            RestoreDisposition::SkipIdentical => gettext("Identical application already exists"),
        };
        let row = adw::ActionRow::builder()
            .use_markup(false)
            .title(&entry.title)
            .subtitle(subtitle)
            .build();
        let check = gtk::CheckButton::builder()
            .active(entry.disposition != RestoreDisposition::SkipIdentical)
            .sensitive(entry.disposition != RestoreDisposition::SkipIdentical)
            .valign(gtk::Align::Center)
            .build();
        check.update_property(&[gtk::accessible::Property::Label(&entry.title)]);
        if check.is_active() {
            selected.borrow_mut().insert(entry.source_id.clone());
        }
        let id = entry.source_id.clone();
        check.connect_toggled(glib::clone!(
            #[strong]
            selected,
            #[weak(rename_to = commit)]
            action_dialog.commit,
            move |check| {
                if check.is_active() {
                    selected.borrow_mut().insert(id.clone());
                } else {
                    selected.borrow_mut().remove(&id);
                }
                commit.set_sensitive(!selected.borrow().is_empty());
            }
        ));
        row.add_prefix(&check);
        row.set_activatable_widget(Some(&check));
        row.set_title_lines(2);
        row.set_subtitle_lines(0);
        list.append(&row);
    }
    group.add(&list);
    page.add(&group);
    action_dialog.toolbar.set_content(Some(&page));
    action_dialog
        .commit
        .set_sensitive(!selected.borrow().is_empty());
    (action_dialog, selected)
}

async fn show_restore_preview(parent: &AlcoveWindow, plan: RestorePlan) {
    let description = if plan.manifest.includes_site_data {
        gettext(
            "This encrypted backup includes cookies and site storage. Select the applications to restore.",
        )
    } else if plan.encrypted {
        gettext("This encrypted backup contains settings only. Select the applications to restore.")
    } else {
        gettext("Select the applications to restore.")
    };
    let description = format!(
        "{}\n\n{}",
        description,
        gettext("Background activity and autostart must be enabled again after restore."),
    );
    let (action_dialog, selected) = restore_options(&plan.entries, &description);
    if !action_dialog.choose(parent).await {
        return;
    }
    let selected = selected.borrow().clone();
    if selected.is_empty() {
        parent.toast(&gettext("No applications were selected"));
        return;
    }
    let parent_identifier = WindowIdentifier::from_native(parent).await;
    let progress =
        crate::ui::dialogs::common::show_progress(parent, &gettext("Restoring Applications…"));
    let report = BackupService::portal()
        .restore(plan, &selected, parent_identifier.as_ref())
        .await;
    progress.force_close();
    parent.refresh();
    let summary = format!(
        "{}: {}; {}: {}",
        gettext("Restored"),
        report.restored,
        gettext("Skipped"),
        report.skipped
    );
    if report.failed.is_empty() {
        parent.toast(&summary);
    } else {
        let body = format!("{summary}; {}: {}", gettext("Failed"), report.failed.len());
        let details = report
            .failed
            .iter()
            .map(|failure| format!("{}: {}", failure.source_id, failure.message))
            .collect::<Vec<_>>()
            .join("\n\n");
        let label = gtk::Label::builder()
            .label(details)
            .selectable(true)
            .wrap(true)
            .xalign(0.0)
            .build();
        let scroll = gtk::ScrolledWindow::builder()
            .min_content_height(120)
            .max_content_height(360)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&label)
            .build();
        let result_dialog =
            adw::AlertDialog::new(Some(&gettext("Restore Finished with Errors")), Some(&body));
        result_dialog.set_extra_child(Some(&scroll));
        result_dialog.add_response("close", &gettext("Close"));
        result_dialog.set_default_response(Some("close"));
        result_dialog.set_close_response("close");
        result_dialog.choose_future(Some(parent)).await;
    }
}

#[cfg(feature = "ui-tests")]
pub(crate) fn run_ui_smoke_test<P: IsA<gtk::Application>>(application: &P) -> anyhow::Result<()> {
    use anyhow::ensure;

    let window = AlcoveWindow::new(application);
    let (options, include_data, passphrase, confirm) = backup_options();
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
        ("GNOME Discourse", RestoreDisposition::RestoreAsIs),
        (
            "Очень длинное название приложения для проверки восстановления",
            RestoreDisposition::RestoreWithNewId,
        ),
        ("Wikipedia", RestoreDisposition::SkipIdentical),
    ]
    .into_iter()
    .map(
        |(title, disposition)| crate::domain::backup::RestorePreviewEntry {
            source_id: crate::domain::model::AppId::generate(),
            target_id: crate::domain::model::AppId::generate(),
            title: title.into(),
            disposition,
        },
    )
    .collect::<Vec<_>>();
    let (restore, selected) =
        restore_options(&entries, &gettext("Select the applications to restore."));
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
    let progress = crate::ui::dialogs::common::show_progress(&window, &gettext("Creating Backup…"));
    crate::ui::test_support::capture(&window, "backup-progress-narrow", 360, 640)?;
    ensure!(!progress.can_close(), "unfinished backup can be dismissed");
    progress.force_close();
    crate::ui::test_support::settle();
    let (password, _) = restore_passphrase_dialog(Some(&gettext(
        "Could not unlock this backup. Check the passphrase and try again.",
    )));
    password.present(Some(&window));
    crate::ui::test_support::capture(&window, "restore-passphrase-narrow", 360, 640)?;
    password.force_close();
    window.destroy();
    Ok(())
}
