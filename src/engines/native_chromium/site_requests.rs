// SPDX-License-Identifier: GPL-3.0-only
use super::{
    file_portal,
    policy::{permission_kinds, Policy},
    Chrome, View,
};
use crate::domain::policy::{Origin, PermissionDecision, PermissionKind};
use adw::prelude::*;
use futures::channel::oneshot;
use gettextrs::gettext;
use gtk::glib;
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::{HashMap, VecDeque},
    rc::Rc,
};

enum Surface {
    Dialog(adw::AlertDialog),
    Portal(oneshot::Sender<()>),
}
struct Active {
    id: u64,
    surface: Surface,
}

pub struct Requests {
    window: glib::WeakRef<adw::ApplicationWindow>,
    worker: Rc<View>,
    chrome: Rc<Chrome>,
    queue: RefCell<VecDeque<Value>>,
    active: RefCell<Option<Active>>,
    downloads: gtk::ListBox,
    download_rows: RefCell<HashMap<u64, (adw::ActionRow, gtk::Button)>>,
    toasts: adw::ToastOverlay,
    policy: Rc<Policy>,
}

impl Requests {
    pub fn new(
        window: &adw::ApplicationWindow,
        worker: Rc<View>,
        chrome: Rc<Chrome>,
        toasts: &adw::ToastOverlay,
        policy: Rc<Policy>,
    ) -> Rc<Self> {
        let downloads = gtk::ListBox::new();
        downloads.add_css_class("boxed-list");
        downloads.set_selection_mode(gtk::SelectionMode::None);
        Rc::new(Self {
            window: window.downgrade(),
            worker,
            chrome,
            queue: Default::default(),
            active: Default::default(),
            downloads,
            download_rows: Default::default(),
            toasts: toasts.clone(),
            policy,
        })
    }
    pub fn handle(self: &Rc<Self>, event: &Value) {
        match event["event"].as_str().unwrap_or("") {
            "site-request" => {
                if event["id"].as_u64().is_some() {
                    self.queue.borrow_mut().push_back(event.clone());
                    self.next();
                }
            }
            "request-cancelled" => {
                if let Some(id) = event["id"].as_u64() {
                    self.cancel(id);
                }
            }
            "download" => self.download(event),
            _ => (),
        }
    }
    fn next(self: &Rc<Self>) {
        if self.active.borrow().is_some() {
            return;
        }
        let Some(event) = self.queue.borrow_mut().pop_front() else {
            return;
        };
        let id = event["id"].as_u64().unwrap();
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let kind = event["kind"].as_str().unwrap_or("");
        let navigation = if kind == "navigation" {
            let url = event["url"].as_str().unwrap_or("");
            match url.parse::<Origin>() {
                Ok(origin) if !self.policy.current.borrow().navigation.allows(&origin) => {
                    Some(origin)
                }
                result => {
                    let allow = result.is_ok()
                        || url == "about:blank"
                        || url.starts_with("blob:")
                        || url.starts_with("data:");
                    self.worker
                        .send("request-response", json!({"id":id,"allow":allow}));
                    self.next();
                    return;
                }
            }
        } else {
            None
        };
        let permission = if kind == "permission" {
            let parsed = event["origin"]
                .as_str()
                .unwrap_or("")
                .parse::<Origin>()
                .and_then(|origin| Ok((origin, permission_kinds(&event)?)));
            let (origin, kinds) = match parsed {
                Ok(request) => request,
                Err(_) => {
                    self.worker
                        .send("request-response", json!({"id":id,"allow":false}));
                    self.toasts.add_toast(adw::Toast::new(&gettext(
                        "Unsupported website permission request",
                    )));
                    self.next();
                    return;
                }
            };
            match self.policy.decision(&origin, &kinds) {
                PermissionDecision::Ask => Some((origin, kinds)),
                decision => {
                    self.worker.send(
                        "request-response",
                        json!({"id":id,"allow":decision == PermissionDecision::Allow}),
                    );
                    self.next();
                    return;
                }
            }
        } else {
            None
        };
        if !window.is_visible() {
            let _ = gtk::prelude::WidgetExt::activate_action(&window, "win.show-background", None);
        }
        self.chrome.begin_dialog();
        if matches!(kind, "file-dialog" | "download-request") {
            let (send, cancel) = oneshot::channel();
            *self.active.borrow_mut() = Some(Active {
                id,
                surface: Surface::Portal(send),
            });
            let this = self.clone();
            glib::spawn_future_local(async move {
                let result = file_portal::choose(&window, &event, cancel).await;
                let answer = match result {
                    Ok(paths) => {
                        json!({"allow":!paths.is_empty(), "path":paths.first().map(String::as_str).unwrap_or(""), "paths":paths})
                    }
                    Err(error) => {
                        eprintln!("file portal: {error:#}");
                        this.toasts
                            .add_toast(adw::Toast::new("Не удалось открыть окно выбора файла"));
                        json!({"allow":false})
                    }
                };
                this.finish(id, answer);
            });
            return;
        }
        let dialog = adw::AlertDialog::new(None, None);
        dialog.set_heading_use_markup(false);
        dialog.set_body_use_markup(false);
        let origin = display_origin(event["origin"].as_str().unwrap_or(""));
        let mut entry = None;
        match kind {
            "navigation" => {
                dialog.set_heading(Some(&gettext("Open Another Origin?")));
                dialog.set_body(&format!(
                    "{}\n\n{}: {}",
                    gettext("This origin is outside the application's navigation allowlist."),
                    gettext("Destination"),
                    navigation.as_ref().unwrap()
                ));
                dialog.add_responses(&[
                    ("block", &gettext("Block")),
                    ("external", &gettext("Open Externally")),
                    ("once", &gettext("Open Once")),
                    ("allow", &gettext("Always Allow Origin")),
                ]);
                dialog.set_response_appearance("block", adw::ResponseAppearance::Destructive);
                dialog.set_response_appearance("allow", adw::ResponseAppearance::Suggested);
                dialog.set_default_response(Some("once"));
                dialog.set_close_response("block");
            }
            "js-dialog" => {
                dialog.set_heading(Some(&origin));
                dialog.set_body(event["message"].as_str().unwrap_or(""));
                if event["type"] == 2 {
                    let input = gtk::Entry::new();
                    input.set_text(event["initial"].as_str().unwrap_or(""));
                    input.update_property(&[gtk::accessible::Property::Label("Ответ сайту")]);
                    input.set_activates_default(true);
                    dialog.set_extra_child(Some(&input));
                    entry = Some(input);
                }
                if event["type"] != 0 {
                    dialog.add_response("cancel", "Отмена");
                }
                dialog.add_response("allow", "ОК");
                dialog.set_default_response(Some("allow"));
                dialog.set_close_response(if event["type"] == 0 {
                    "allow"
                } else {
                    "cancel"
                });
            }
            "before-unload" => {
                dialog.set_heading(Some("Покинуть страницу?"));
                dialog.set_body(&format!(
                    "{origin}\nНесохранённые изменения могут быть потеряны."
                ));
                dialog.add_response("cancel", "Остаться");
                dialog.add_response("allow", "Покинуть");
                dialog.set_response_appearance("allow", adw::ResponseAppearance::Destructive);
                dialog.set_default_response(Some("cancel"));
                dialog.set_close_response("cancel");
            }
            "permission" => {
                let (_, kinds) = permission.as_ref().unwrap();
                let labels = kinds
                    .iter()
                    .map(|kind| permission_label(*kind))
                    .collect::<Vec<_>>();
                dialog.set_heading(Some(&gettext("Website Permission")));
                dialog.set_body(&format!("{origin}\n\n{}", labels.join("\n")));
                dialog.add_responses(&[
                    ("cancel", &gettext("Not Now")),
                    ("block", &gettext("Always Block")),
                    ("allow-session", &gettext("Allow for This Session")),
                    ("allow", &gettext("Always Allow")),
                ]);
                dialog.set_response_appearance("block", adw::ResponseAppearance::Destructive);
                dialog.set_response_appearance("allow", adw::ResponseAppearance::Suggested);
                dialog.set_default_response(Some("allow-session"));
                dialog.set_close_response("cancel");
            }
            _ => {
                self.chrome.end_dialog();
                self.worker
                    .send("request-response", json!({"id":id,"allow":false}));
                self.next();
                return;
            }
        }
        *self.active.borrow_mut() = Some(Active {
            id,
            surface: Surface::Dialog(dialog.clone()),
        });
        let this = self.clone();
        glib::spawn_future_local(async move {
            let response = dialog.choose_future(Some(&window)).await;
            if let Some((origin, kinds)) = permission {
                this.permission_response(id, origin, kinds, response.as_str());
                return;
            }
            if let Some(origin) = navigation {
                if this.active.borrow().as_ref().map(|a| a.id) != Some(id) {
                    return;
                }
                let allow = match response.as_str() {
                    "once" => true,
                    "allow" => match this.policy.allow_navigation(origin) {
                        Ok(()) => true,
                        Err(error) => {
                            this.toasts.add_toast(adw::Toast::new(&format!(
                                "{}: {error}",
                                gettext("The origin could not be saved")
                            )));
                            false
                        }
                    },
                    _ => false,
                };
                this.finish(id, json!({"allow":allow}));
                if response == "external" {
                    if let Err(error) = gtk::UriLauncher::new(event["url"].as_str().unwrap_or(""))
                        .launch_future(Some(&window))
                        .await
                    {
                        this.toasts.add_toast(adw::Toast::new(&error.to_string()));
                    }
                }
                return;
            }
            this.finish(id, json!({"allow":response == "allow", "text":entry.map(|e|e.text().to_string()).unwrap_or_default()}));
        });
    }
    fn permission_response(
        self: &Rc<Self>,
        id: u64,
        origin: Origin,
        kinds: Vec<PermissionKind>,
        response: &str,
    ) {
        if self.active.borrow().as_ref().map(|a| a.id) != Some(id) {
            return;
        }
        let allow = match response {
            "allow-session" => {
                self.policy.allow_session(&origin, &kinds);
                true
            }
            "allow" => {
                if let Err(error) = self.policy.save(&origin, &kinds, PermissionDecision::Allow) {
                    self.policy.allow_session(&origin, &kinds);
                    self.toasts.add_toast(adw::Toast::new(&format!(
                        "{}: {error}",
                        gettext("Permission was allowed only for this session")
                    )));
                }
                true
            }
            "block" => {
                if let Err(error) = self.policy.save(&origin, &kinds, PermissionDecision::Block) {
                    self.toasts.add_toast(adw::Toast::new(&format!(
                        "{}: {error}",
                        gettext("Permission block could not be saved")
                    )));
                }
                false
            }
            _ => false,
        };
        self.finish(id, json!({"allow":allow,"dismiss":response == "cancel"}));
    }
    fn finish(self: &Rc<Self>, id: u64, mut answer: Value) {
        if self.active.borrow().as_ref().map(|a| a.id) != Some(id) {
            return;
        }
        self.active.borrow_mut().take();
        self.chrome.end_dialog();
        answer["id"] = json!(id);
        self.worker.send("request-response", answer);
        self.next();
    }
    fn cancel(self: &Rc<Self>, id: u64) {
        self.queue.borrow_mut().retain(|event| event["id"] != id);
        if self.active.borrow().as_ref().map(|a| a.id) != Some(id) {
            return;
        }
        let active = self.active.borrow_mut().take().unwrap();
        match active.surface {
            Surface::Dialog(dialog) => dialog.force_close(),
            Surface::Portal(cancel) => {
                let _ = cancel.send(());
            }
        }
        self.chrome.end_dialog();
        self.next();
    }
    pub fn close(self: &Rc<Self>) {
        self.queue.borrow_mut().clear();
        let id = self.active.borrow().as_ref().map(|a| a.id);
        if let Some(id) = id {
            self.cancel(id);
        }
    }
    pub fn show_downloads(&self) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        if self.downloads.parent().is_some() {
            return;
        }
        let dialog = adw::Dialog::builder()
            .title("Загрузки")
            .content_width(440)
            .content_height(360)
            .build();
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&adw::HeaderBar::new());
        let scroll = gtk::ScrolledWindow::builder()
            .child(&self.downloads)
            .vexpand(true)
            .build();
        self.downloads.set_margin_start(18);
        self.downloads.set_margin_end(18);
        self.downloads.set_margin_top(18);
        self.downloads.set_margin_bottom(18);
        self.downloads.set_placeholder(Some(
            &adw::StatusPage::builder()
                .title("Загрузок пока нет")
                .icon_name("folder-download-symbolic")
                .build(),
        ));
        toolbar.set_content(Some(&scroll));
        dialog.set_child(Some(&toolbar));
        self.chrome.begin_dialog();
        let chrome = self.chrome.clone();
        dialog.connect_closed(move |_| {
            scroll.set_child(gtk::Widget::NONE);
            chrome.end_dialog();
        });
        dialog.present(Some(&window));
    }
    fn download(&self, event: &Value) {
        let Some(id) = event["download"].as_u64() else {
            return;
        };
        let mut rows = self.download_rows.borrow_mut();
        let (row, cancel) = rows.entry(id).or_insert_with(|| {
            let row = adw::ActionRow::new();
            row.set_title_lines(1);
            row.set_use_markup(false);
            let cancel = gtk::Button::builder()
                .icon_name("process-stop-symbolic")
                .tooltip_text("Отменить загрузку")
                .valign(gtk::Align::Center)
                .build();
            cancel.add_css_class("flat");
            let worker = self.worker.clone();
            cancel.connect_clicked(move |_| {
                worker.send("download-control", json!({"download":id,"action":"cancel"}))
            });
            row.add_suffix(&cancel);
            self.downloads.append(&row);
            (row, cancel)
        });
        row.set_title(
            event["name"]
                .as_str()
                .filter(|s| !s.is_empty())
                .unwrap_or("Файл"),
        );
        let received = event["received"].as_i64().unwrap_or(0).max(0);
        let total = event["total"].as_i64().unwrap_or(0);
        let status = if event["complete"] == true {
            "Загрузка завершена".into()
        } else if event["cancelled"] == true {
            "Загрузка отменена".into()
        } else if event["progress"] != true {
            "Не удалось загрузить файл".into()
        } else if total > 0 {
            format!("{} из {} КБ", received / 1024, total / 1024)
        } else {
            format!("{} КБ", received / 1024)
        };
        row.set_subtitle(&status);
        cancel.set_sensitive(event["progress"] == true);
    }
}

fn display_origin(value: &str) -> String {
    url::Url::parse(value)
        .ok()
        .filter(|url| matches!(url.scheme(), "https" | "http"))
        .map(|url| url.origin().ascii_serialization())
        .unwrap_or_else(|| "Сайт".into())
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
