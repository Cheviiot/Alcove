// SPDX-License-Identifier: GPL-3.0-only

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use adw::prelude::*;
use gettextrs::gettext;
use gtk::glib;

use crate::{
    model::AppId,
    policy::{PermissionDecision, PermissionKind},
    service::AppService,
};

pub fn start(parent: &gtk::Window, id: AppId) {
    let policy = match AppService::portal().load_policy(&id) {
        Ok(policy) => policy,
        Err(error) => {
            notify(parent, &error.to_string());
            return;
        }
    };

    present_editor(parent, id, policy);
}

fn present_editor(
    parent: &gtk::Window,
    id: AppId,
    policy: crate::policy::AppPolicyV2,
) -> adw::Dialog {
    let dialog = adw::PreferencesDialog::builder()
        .title(gettext("Permissions"))
        .content_width(540)
        .content_height(580)
        .build();
    let page = adw::PreferencesPage::new();
    let notice_group = adw::PreferencesGroup::new();
    let notice = gtk::Label::builder()
        .label(gettext(
            "Changes take effect the next time you open this application.",
        ))
        .wrap(true)
        .xalign(0.0)
        .visible(false)
        .css_classes(["dim-label", "caption"])
        .build();
    notice_group.add(&notice);
    page.add(&notice_group);
    let reset = adw::ButtonRow::builder()
        .title(gettext("Reset All"))
        .start_icon_name("edit-clear-all-symbolic")
        .sensitive(!policy.permissions.is_empty())
        .build();
    reset.add_css_class("destructive-action");
    let resetting = Rc::new(Cell::new(false));
    let rows = Rc::new(RefCell::new(Vec::<(
        adw::ComboRow,
        Rc<Cell<PermissionDecision>>,
        gtk::Label,
    )>::new()));
    for (origin, permissions) in policy.permissions {
        let group = adw::PreferencesGroup::builder()
            .title(origin.as_str())
            .build();
        for (kind, decision) in permissions {
            let labels = [gettext("Ask"), gettext("Allow"), gettext("Block")];
            let model = gtk::StringList::new(&[&labels[0], &labels[1], &labels[2]]);
            let row = adw::ComboRow::builder()
                .title(permission_label(kind))
                .use_subtitle(true)
                .model(&model)
                .selected(decision_index(decision))
                .build();
            let error = crate::dialogs::inline_error();
            let saved = Rc::new(Cell::new(decision));
            row.connect_selected_notify(glib::clone!(
                #[strong]
                id,
                #[strong]
                origin,
                #[strong]
                saved,
                #[strong]
                resetting,
                #[weak]
                error,
                #[weak]
                notice,
                #[weak]
                reset,
                move |row| {
                    if resetting.get() {
                        return;
                    }
                    let requested = selected_decision(row.selected());
                    if requested == saved.get() {
                        return;
                    }
                    match AppService::portal()
                        .apply_policy_decisions(&id, &[(origin.clone(), kind, requested)])
                    {
                        Ok(_) => {
                            saved.set(requested);
                            error.set_visible(false);
                            notice.set_visible(true);
                            reset.set_sensitive(true);
                        }
                        Err(failure) => {
                            resetting.set(true);
                            row.set_selected(decision_index(saved.get()));
                            resetting.set(false);
                            crate::dialogs::show_inline_error(&error, &failure.to_string());
                        }
                    }
                }
            ));
            rows.borrow_mut().push((row.clone(), saved, error.clone()));
            group.add(&row);
            group.add(&error);
        }
        page.add(&group);
    }
    if rows.borrow().is_empty() {
        let group = adw::PreferencesGroup::new();
        let status = adw::StatusPage::builder()
            .icon_name("system-lock-screen-symbolic")
            .title(gettext("No Saved Permissions"))
            .description(gettext(
                "Websites will ask before using protected capabilities.",
            ))
            .build();
        group.add(&status);
        page.add(&group);
    } else {
        let group = adw::PreferencesGroup::builder()
            .description(gettext("Reset saved choices so websites ask again."))
            .build();
        let error = crate::dialogs::inline_error();
        group.add(&reset);
        group.add(&error);
        page.add(&group);
        reset.connect_activated(glib::clone!(
            #[strong]
            rows,
            #[strong]
            resetting,
            #[weak]
            notice,
            #[weak]
            error,
            move |button| match AppService::portal().reset_policy(&id) {
                Ok(_) => {
                    // Reopen after reset to get a fresh set of origins. Keep
                    // current rows usable so a new choice can be made now.
                    resetting.set(true);
                    for (row, saved, error) in rows.borrow().iter() {
                        saved.set(PermissionDecision::Ask);
                        row.set_selected(0);
                        error.set_visible(false);
                    }
                    resetting.set(false);
                    button.set_sensitive(false);
                    notice.set_visible(true);
                    error.set_visible(false);
                }
                Err(failure) => crate::dialogs::show_inline_error(&error, &failure.to_string()),
            }
        ));
    }
    dialog.add(&page);
    dialog.present(Some(parent));
    dialog.upcast()
}

fn decision_index(decision: PermissionDecision) -> u32 {
    match decision {
        PermissionDecision::Ask => 0,
        PermissionDecision::Allow => 1,
        PermissionDecision::Block => 2,
    }
}

fn selected_decision(selected: u32) -> PermissionDecision {
    match selected {
        1 => PermissionDecision::Allow,
        2 => PermissionDecision::Block,
        _ => PermissionDecision::Ask,
    }
}

fn permission_label(kind: PermissionKind) -> String {
    match kind {
        PermissionKind::Camera => gettext("Camera"),
        PermissionKind::Microphone => gettext("Microphone"),
        PermissionKind::Geolocation => gettext("Location"),
        PermissionKind::Notifications => gettext("Notifications"),
        PermissionKind::Clipboard => gettext("Clipboard"),
        PermissionKind::PointerLock => gettext("Pointer Lock"),
        PermissionKind::ThirdPartyStorage => gettext("Third-Party Storage"),
    }
}

fn notify(parent: &gtk::Window, message: &str) {
    let _ = parent.activate_action("win.notify", Some(&message.to_variant()));
}

#[cfg(feature = "ui-tests")]
pub(crate) fn run_ui_smoke_test<P: IsA<gtk::Application>>(application: &P) -> anyhow::Result<()> {
    let window = crate::window::AlcoveWindow::new(application);
    let mut policy = crate::policy::AppPolicyV2::default();
    let origin: crate::policy::Origin = "https://discourse.gnome.org".parse()?;
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
    let dialog = present_editor(window.upcast_ref(), AppId::generate(), policy);
    crate::ui_test_support::capture(&window, "permissions", 800, 760)?;
    crate::ui_test_support::capture(&window, "permissions-narrow", 360, 640)?;
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceDark);
    crate::ui_test_support::capture(&window, "permissions-dark", 800, 760)?;
    adw::StyleManager::default().set_color_scheme(adw::ColorScheme::ForceLight);
    dialog.force_close();
    crate::ui_test_support::settle();
    let empty = present_editor(window.upcast_ref(), AppId::generate(), Default::default());
    crate::ui_test_support::capture(&window, "permissions-empty-narrow", 360, 640)?;
    empty.force_close();
    window.destroy();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_indices_are_stable() {
        for decision in [
            PermissionDecision::Ask,
            PermissionDecision::Allow,
            PermissionDecision::Block,
        ] {
            assert_eq!(selected_decision(decision_index(decision)), decision);
        }
    }
}
