// SPDX-License-Identifier: GPL-3.0-only

use std::{cell::RefCell, rc::Rc};

use adw::prelude::*;
use gettextrs::gettext;
use gtk::glib;

pub fn inline_error() -> gtk::Label {
    gtk::Label::builder()
        .wrap(true)
        .xalign(0.0)
        .visible(false)
        .css_classes(["error", "caption"])
        .build()
}

pub fn show_inline_error(label: &gtk::Label, message: &str) {
    label.set_label(&format!(
        "{} {message}",
        gettext("Changes could not be saved.")
    ));
    label.set_visible(true);
}

/// Keep a non-cancellable archive operation visible until its atomic write or
/// portal transaction finishes. Call force_close after awaiting the operation.
/// Opens `uri` through the desktop portal, which is the only route out of the
/// sandbox, and reports a failure beside `parent` instead of losing it.
pub fn open_uri(parent: &impl IsA<gtk::Window>, uri: &str, failure_title: &str) {
    let parent = parent.clone().upcast::<gtk::Window>();
    let uri = uri.to_owned();
    let failure_title = failure_title.to_owned();
    glib::spawn_future_local(async move {
        if let Err(error) = gtk::UriLauncher::new(&uri)
            .launch_future(Some(&parent))
            .await
        {
            let alert = adw::AlertDialog::new(Some(&failure_title), Some(&error.to_string()));
            alert.add_response("close", &gettext("Close"));
            alert.present(Some(&parent));
        }
    });
}

pub fn show_progress(parent: &impl IsA<gtk::Widget>, title: &str) -> adw::Dialog {
    let spinner = adw::Spinner::builder()
        .width_request(48)
        .height_request(48)
        .halign(gtk::Align::Center)
        .build();
    let page = adw::StatusPage::builder()
        .title(title)
        .child(&spinner)
        .build();
    let dialog = adw::Dialog::builder()
        .title(title)
        .can_close(false)
        .content_width(420)
        .content_height(280)
        .child(&page)
        .build();
    dialog.present(Some(parent));
    dialog
}

/// A scrollable action dialog with consistent cancel, commit and error feedback.
pub struct ActionDialog {
    pub dialog: adw::Dialog,
    pub toolbar: adw::ToolbarView,
    pub commit: gtk::Button,
}

impl ActionDialog {
    pub fn new(title: &str, action: &str) -> Self {
        let dialog = adw::Dialog::builder()
            .title(title)
            .content_width(540)
            .content_height(580)
            .width_request(320)
            .height_request(220)
            .build();
        let toolbar = adw::ToolbarView::new();
        let header = adw::HeaderBar::builder()
            .show_start_title_buttons(false)
            .show_end_title_buttons(false)
            .build();
        let cancel = gtk::Button::with_mnemonic(&gettext("_Cancel"));
        cancel.connect_clicked(glib::clone!(
            #[weak]
            dialog,
            move |_| {
                dialog.close();
            }
        ));
        dialog
            .bind_property("can-close", &cancel, "sensitive")
            .sync_create()
            .build();
        header.pack_start(&cancel);
        let commit = gtk::Button::with_label(action);
        commit.add_css_class("suggested-action");
        header.pack_end(&commit);
        dialog.set_default_widget(Some(&commit));
        toolbar.add_top_bar(&header);
        let breakpoint = adw::Breakpoint::new(adw::BreakpointCondition::new_length(
            adw::BreakpointConditionLengthType::MaxWidth,
            420.0,
            adw::LengthUnit::Sp,
        ));
        breakpoint.add_setter(&header, "show-title", Some(&false.to_value()));
        // Leave room for the primary action with enlarged text. The icon has
        // the same accessible name and dismissal behavior as the wide button.
        cancel.set_tooltip_text(Some(&gettext("Cancel")));
        cancel.update_property(&[gtk::accessible::Property::Label(&gettext("Cancel"))]);
        breakpoint.connect_apply(glib::clone!(
            #[weak]
            cancel,
            move |_| cancel.set_icon_name("window-close-symbolic")
        ));
        breakpoint.connect_unapply(glib::clone!(
            #[weak]
            cancel,
            move |_| cancel.set_label(&gettext("_Cancel"))
        ));
        dialog.add_breakpoint(breakpoint);
        dialog.set_child(Some(&toolbar));
        Self {
            dialog,
            toolbar,
            commit,
        }
    }

    pub async fn choose(self, parent: &impl IsA<gtk::Widget>) -> bool {
        let (send, receive) = futures::channel::oneshot::channel();
        let send = Rc::new(RefCell::new(Some(send)));
        self.commit.connect_clicked(glib::clone!(
            #[strong]
            send,
            #[weak(rename_to = dialog)]
            self.dialog,
            move |_| {
                if let Some(send) = send.borrow_mut().take() {
                    let _ = send.send(true);
                }
                dialog.close();
            }
        ));
        self.dialog.connect_closed(move |_| {
            if let Some(send) = send.borrow_mut().take() {
                let _ = send.send(false);
            }
        });
        self.dialog.present(Some(parent));
        receive.await.unwrap_or(false)
    }
}

/// Only advertise shortcuts supported by the window that opened this dialog.
pub fn show_shortcuts(parent: &impl IsA<gtk::Widget>, runtime: bool) {
    let dialog = adw::ShortcutsDialog::new();
    let general = adw::ShortcutsSection::new(Some(&gettext("General")));
    general.add(adw::ShortcutsItem::new(&gettext("Main Menu"), "F10"));
    general.add(adw::ShortcutsItem::new(
        &gettext("Keyboard Shortcuts"),
        "<Primary>question",
    ));
    general.add(adw::ShortcutsItem::from_action(
        &gettext("Quit"),
        "app.quit",
    ));
    dialog.add(general);
    let section = adw::ShortcutsSection::new(Some(&if runtime {
        gettext("Navigation")
    } else {
        gettext("Applications")
    }));
    let items = if runtime {
        vec![
            (gettext("Back"), "<Alt>Left"),
            (gettext("Forward"), "<Alt>Right"),
            (gettext("Reload"), "<Primary>r"),
            (gettext("Reload Without Cache"), "<Primary><Shift>r"),
            (gettext("Home"), "<Alt>Home"),
            (gettext("Zoom In"), "<Primary>plus"),
            (gettext("Zoom Out"), "<Primary>minus"),
            (gettext("Reset Zoom"), "<Primary>0"),
            (gettext("Toggle Fullscreen"), "F11"),
        ]
    } else {
        vec![
            (gettext("Add Application"), "<Primary>n"),
            (gettext("Search Applications"), "<Primary>f"),
            (gettext("Close Search or Go Back"), "Escape"),
            (gettext("Back"), "<Alt>Left"),
        ]
    };
    for (title, accelerator) in items {
        section.add(adw::ShortcutsItem::new(&title, accelerator));
    }
    dialog.add(section);
    dialog.present(Some(parent));
}
