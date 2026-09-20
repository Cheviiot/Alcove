// SPDX-License-Identifier: GPL-3.0-only

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use adw::prelude::*;
use gettextrs::gettext;
use gtk::glib;

use crate::ui::shell::app_window::AppWindow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DownloadState {
    WaitingForDestination,
    Downloading,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug)]
struct DownloadItem {
    container: gtk::Box,
    row: adw::ActionRow,
    progress: gtk::ProgressBar,
    cancel_button: gtk::Button,
    retry_button: gtk::Button,
    source_uri: String,
    destination: RefCell<Option<String>>,
    download: RefCell<Option<webkit::Download>>,
    state: Cell<DownloadState>,
}

impl DownloadItem {
    fn new(source_uri: String, suggested_name: &str) -> Rc<Self> {
        let progress = gtk::ProgressBar::builder()
            .valign(gtk::Align::Center)
            .show_text(true)
            .text(gettext("Waiting"))
            .build();
        let cancel_button = gtk::Button::builder()
            .icon_name("process-stop-symbolic")
            .tooltip_text(gettext("Cancel Download"))
            .valign(gtk::Align::Center)
            .build();
        let retry_button = gtk::Button::builder()
            .icon_name("view-refresh-symbolic")
            .tooltip_text(gettext("Retry Download"))
            .valign(gtk::Align::Center)
            .visible(false)
            .build();
        let row = adw::ActionRow::builder()
            .title(sanitize_download_name(suggested_name))
            .subtitle(gettext("Choose a destination"))
            .build();
        row.set_use_markup(false);
        row.set_title_lines(1);
        row.set_subtitle_lines(2);
        cancel_button.add_css_class("flat");
        retry_button.add_css_class("flat");
        let container = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .build();
        container.append(&row);
        progress.set_margin_start(12);
        progress.set_margin_end(12);
        progress.set_margin_bottom(12);
        container.append(&progress);
        row.add_suffix(&cancel_button);
        row.add_suffix(&retry_button);

        Rc::new(Self {
            container,
            row,
            progress,
            cancel_button,
            retry_button,
            source_uri,
            destination: RefCell::new(None),
            download: RefCell::new(None),
            state: Cell::new(DownloadState::WaitingForDestination),
        })
    }

    fn set_downloading(&self, destination: String) {
        self.destination.replace(Some(destination.clone()));
        self.state.set(DownloadState::Downloading);
        self.row
            .set_subtitle(&format!("{}: {destination}", gettext("Downloading to")));
        self.progress.set_text(Some("0%"));
    }

    fn update_progress(&self, download: &webkit::Download) {
        if self.state.get() != DownloadState::Downloading {
            return;
        }
        let progress = download.estimated_progress().clamp(0.0, 1.0);
        self.progress.set_fraction(progress);
        self.progress
            .set_text(Some(&format!("{:.0}%", progress * 100.0)));
    }

    fn complete(&self) {
        if self.state.get() != DownloadState::Downloading {
            return;
        }
        self.state.set(DownloadState::Completed);
        self.progress.set_fraction(1.0);
        self.progress.set_text(Some(&gettext("Completed")));
        self.cancel_button.set_visible(false);
        let destination = self.destination.borrow();
        self.row.set_subtitle(&destination.as_deref().map_or_else(
            || gettext("Download completed"),
            |destination| format!("{}: {destination}", gettext("Saved to")),
        ));
    }

    fn cancel(&self) {
        if !matches!(
            self.state.get(),
            DownloadState::WaitingForDestination | DownloadState::Downloading
        ) {
            return;
        }
        self.state.set(DownloadState::Cancelled);
        self.progress.set_text(Some(&gettext("Cancelled")));
        self.row.set_subtitle(&gettext("Download cancelled"));
        self.cancel_button.set_visible(false);
        self.retry_button.set_visible(!self.source_uri.is_empty());
        if let Some(download) = self.download.borrow().as_ref() {
            download.cancel();
        }
    }

    fn fail(&self, error: &impl std::fmt::Display) {
        if matches!(
            self.state.get(),
            DownloadState::Cancelled | DownloadState::Failed
        ) {
            return;
        }
        self.state.set(DownloadState::Failed);
        self.progress.set_text(Some(&gettext("Failed")));
        self.row
            .set_subtitle(&format!("{}: {error}", gettext("Download failed")));
        self.cancel_button.set_visible(false);
        self.retry_button.set_visible(!self.source_uri.is_empty());
    }
}

#[derive(Debug)]
pub struct DownloadManager {
    parent: glib::WeakRef<AppWindow>,
    session: RefCell<Option<webkit::NetworkSession>>,
    items: RefCell<Vec<Rc<DownloadItem>>>,
    dialog: RefCell<Option<adw::Dialog>>,
    visible_group: RefCell<Option<adw::PreferencesGroup>>,
    visible_stack: RefCell<Option<gtk::Stack>>,
}

impl DownloadManager {
    pub fn new(parent: &AppWindow) -> Rc<Self> {
        let weak_parent = glib::WeakRef::new();
        weak_parent.set(Some(parent));
        Rc::new(Self {
            parent: weak_parent,
            session: RefCell::new(None),
            items: RefCell::new(Vec::new()),
            dialog: RefCell::new(None),
            visible_group: RefCell::new(None),
            visible_stack: RefCell::new(None),
        })
    }

    pub fn set_session(&self, session: &webkit::NetworkSession) {
        self.session.replace(Some(session.clone()));
    }

    pub fn track(self: &Rc<Self>, download: &webkit::Download) {
        let source_uri = download
            .request()
            .and_then(|request| request.uri())
            .map(|uri| uri.to_string())
            .unwrap_or_default();
        let item = DownloadItem::new(source_uri, &gettext("Download"));
        item.download.replace(Some(download.clone()));
        item.cancel_button.connect_clicked(glib::clone!(
            #[strong]
            item,
            move |_| item.cancel()
        ));
        item.retry_button.connect_clicked(glib::clone!(
            #[weak(rename_to = manager)]
            self,
            #[strong]
            item,
            move |_| manager.retry(&item.source_uri)
        ));
        download.connect_estimated_progress_notify(glib::clone!(
            #[strong]
            item,
            move |download| item.update_progress(download)
        ));
        download.connect_received_data(glib::clone!(
            #[strong]
            item,
            move |download, _| item.update_progress(download)
        ));
        download.connect_finished(glib::clone!(
            #[strong]
            item,
            move |_| item.complete()
        ));
        download.connect_failed(glib::clone!(
            #[strong]
            item,
            move |_, error| item.fail(error)
        ));
        download.connect_decide_destination(glib::clone!(
            #[weak(rename_to = manager)]
            self,
            #[strong]
            item,
            #[upgrade_or]
            true,
            move |download, suggested_name| {
                let suggested_name = sanitize_download_name(suggested_name);
                item.row.set_title(&suggested_name);
                let download = download.clone();
                glib::spawn_future_local(glib::clone!(
                    #[weak]
                    manager,
                    #[strong]
                    item,
                    async move {
                        let Some(parent) = manager.parent.upgrade() else {
                            item.cancel();
                            return;
                        };
                        let dialog = gtk::FileDialog::builder()
                            .accept_label(gettext("Save"))
                            .title(gettext("Download File"))
                            .modal(true)
                            .initial_name(&suggested_name)
                            .build();
                        match dialog.save_future(Some(&parent)).await {
                            Ok(file) => {
                                let destination = file.parse_name().to_string();
                                item.set_downloading(destination);
                                download.set_destination(file.uri().as_str());
                                manager.show();
                            }
                            Err(error) => {
                                if let Some(error) =
                                    crate::system::portal::classify_file_dialog_error(
                                        gettext("Choose download destination"),
                                        &error,
                                    )
                                {
                                    item.fail(&error);
                                    download.cancel();
                                    manager.show();
                                } else {
                                    item.cancel();
                                }
                            }
                        }
                    }
                ));
                true
            }
        ));

        self.items.borrow_mut().push(item.clone());
        if let Some(group) = self.visible_group.borrow().as_ref() {
            group.add(&item.container);
            if let Some(stack) = self.visible_stack.borrow().as_ref() {
                stack.set_visible_child_name("downloads");
            }
        }
    }

    pub fn show(self: &Rc<Self>) {
        if let Some(dialog) = self.dialog.borrow().as_ref() {
            dialog.present(self.parent.upgrade().as_ref());
            return;
        }
        let Some(parent) = self.parent.upgrade() else {
            return;
        };
        let dialog = adw::Dialog::builder()
            .title(gettext("Downloads"))
            .content_width(620)
            .content_height(500)
            .build();
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&adw::HeaderBar::new());
        let page = adw::PreferencesPage::new();
        let group = adw::PreferencesGroup::new();
        let stack = gtk::Stack::new();
        let empty = adw::StatusPage::builder()
            .icon_name("folder-download-symbolic")
            .title(gettext("No Downloads"))
            .description(gettext("Downloads from this app will appear here."))
            .build();
        stack.add_named(&empty, Some("empty"));
        for item in self.items.borrow().iter() {
            group.add(&item.container);
        }
        page.add(&group);
        stack.add_named(&page, Some("downloads"));
        stack.set_visible_child_name(if self.items.borrow().is_empty() {
            "empty"
        } else {
            "downloads"
        });
        self.visible_stack.replace(Some(stack.clone()));
        toolbar.set_content(Some(&stack));
        dialog.set_child(Some(&toolbar));

        let weak_manager = Rc::downgrade(self);
        dialog.connect_closed(move |_| {
            if let Some(manager) = weak_manager.upgrade() {
                manager.dialog.replace(None);
                manager.visible_group.replace(None);
                manager.visible_stack.replace(None);
            }
        });
        self.visible_group.replace(Some(group));
        self.dialog.replace(Some(dialog.clone()));
        dialog.present(Some(&parent));
    }

    fn retry(&self, source_uri: &str) {
        let Some(parent) = self.parent.upgrade() else {
            return;
        };
        let Some(session) = self.session.borrow().as_ref().cloned() else {
            parent.toast(&gettext("The download session is unavailable"));
            return;
        };
        if source_uri.is_empty() || session.download_uri(source_uri).is_none() {
            parent.toast(&gettext("The download could not be retried"));
        }
    }
}

fn sanitize_download_name(value: &str) -> String {
    let name = value
        .chars()
        .map(|character| {
            if character.is_control() || matches!(character, '/' | '\\') {
                '_'
            } else {
                character
            }
        })
        .collect::<String>();
    let name = name.trim();
    if name.is_empty() {
        "download".to_owned()
    } else {
        name.to_owned()
    }
}

#[cfg(feature = "ui-tests")]
pub(crate) fn run_ui_smoke_test<P: IsA<gtk::Application>>(application: &P) -> anyhow::Result<()> {
    use anyhow::ensure;

    let window: AppWindow = glib::Object::builder()
        .property("application", application)
        .build();
    let manager = DownloadManager::new(&window);
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
    let active = DownloadItem::new(
        "https://example.org/archive".into(),
        "Очень длинное название загружаемого архива с документами.zip",
    );
    active.set_downloading("file:///tmp/Документы/Очень длинное название архива.zip".into());
    active.progress.set_fraction(0.42);
    active.progress.set_text(Some("42%"));
    let complete = DownloadItem::new("https://example.org/document".into(), "Document.pdf");
    complete.set_downloading("file:///tmp/Document.pdf".into());
    complete.complete();
    let failed = DownloadItem::new("https://example.org/image".into(), "Image.png");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggested_download_names_cannot_create_paths() {
        assert_eq!(sanitize_download_name("../report\n.pdf"), ".._report_.pdf");
        assert_eq!(
            sanitize_download_name("folder\\file.txt"),
            "folder_file.txt"
        );
        assert_eq!(sanitize_download_name("\n\r"), "__");
        assert_eq!(sanitize_download_name(""), "download");
    }
}
