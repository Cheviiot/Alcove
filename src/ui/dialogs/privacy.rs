// SPDX-License-Identifier: GPL-3.0-only

use crate::{
    domain::model::{AppConfigV3, AppId, Engine},
    domain::policy::{
        normalize_proxy_uri, AppPolicyV2, ContentFilterRuleSet, Origin, ProxyMode,
        MAX_CONTENT_FILTER_SOURCE_SIZE,
    },
    engines::content_filters,
    system::service::AppService,
    ui::library::window::AlcoveWindow,
};
use adw::prelude::*;
use anyhow::{anyhow, ensure, Context, Result};
use ashpd::WindowIdentifier;
use gettextrs::gettext;
use gtk::{gio, glib};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeSet, VecDeque},
    fs::File,
    io::Read,
    path::Path,
    rc::Rc,
    str::FromStr,
};

#[derive(Clone)]
pub(super) enum Edit {
    Navigation(bool),
    Origins(String),
    Proxy(ProxyMode, String),
    Background(bool, bool),
    FilterEnabled(String, bool),
    FilterRemove(String),
    FilterAdd(ContentFilterRuleSet),
}

struct Editor {
    id: AppId,
    start_url: String,
    dialog: glib::WeakRef<adw::PreferencesDialog>,
    saved: RefCell<AppPolicyV2>,
    populating: Cell<bool>,
    busy: Cell<bool>,
    closing: Cell<bool>,
    pending: RefCell<VecDeque<Edit>>,
    navigation: adw::SwitchRow,
    origins: adw::EntryRow,
    navigation_error: gtk::Label,
    proxy_mode: adw::ComboRow,
    proxy_uri: adw::EntryRow,
    proxy_error: gtk::Label,
    background_group: adw::PreferencesGroup,
    background: adw::SwitchRow,
    autostart: adw::SwitchRow,
    background_error: gtk::Label,
    filter_group: adw::PreferencesGroup,
    filter_rows: RefCell<Vec<gtk::Widget>>,
    filter_error: gtk::Label,
    import_row: adw::ActionRow,
    notice: gtk::Label,
}

pub fn start(parent: &AlcoveWindow, id: AppId) {
    let service = AppService::portal();
    match service
        .load(&id)
        .and_then(|config| Ok((service.load_policy(&id)?, config)))
    {
        Ok((policy, config)) => {
            present_editor(parent, policy, config);
        }
        Err(error) => parent.toast(&error.to_string()),
    }
}

pub(super) fn present_editor(
    parent: &AlcoveWindow,
    policy: AppPolicyV2,
    config: AppConfigV3,
) -> adw::Dialog {
    let dialog = adw::PreferencesDialog::builder()
        .title(gettext("Privacy and Power"))
        .content_width(540)
        .content_height(640)
        .build();
    let page = adw::PreferencesPage::new();
    let notice = gtk::Label::builder()
        .label(gettext(
            "Changes take effect the next time you open this application.",
        ))
        .wrap(true)
        .xalign(0.0)
        .visible(false)
        .css_classes(["dim-label", "caption"])
        .build();
    let notice_group = adw::PreferencesGroup::new();
    notice_group.add(&notice);
    page.add(&notice_group);
    let navigation_group = adw::PreferencesGroup::builder()
        .title(gettext("Navigation"))
        .description(gettext("Ask before opening an origin outside this list"))
        .build();
    let navigation = adw::SwitchRow::builder()
        .title(gettext("Restrict Navigation"))
        .active(policy.navigation.enabled)
        .build();
    let origins = adw::EntryRow::builder()
        .title(gettext("Allowed Origins"))
        .text(format_origins(&policy))
        .build();
    origins.set_sensitive(policy.navigation.enabled);
    let navigation_error = crate::ui::common::inline_error();
    navigation_group.add(&navigation);
    navigation_group.add(&origins);
    navigation_group.add(&navigation_error);
    page.add(&navigation_group);
    let proxy_group = adw::PreferencesGroup::builder()
        .title(gettext("Proxy"))
        .description(gettext(
            "Applies only to this application's selected engine. Credentials are never stored.",
        ))
        .build();
    let labels = [
        gettext("System Settings"),
        gettext("Direct Connection"),
        gettext("Custom HTTP(S) or SOCKS"),
    ];
    let proxy_mode = adw::ComboRow::builder()
        .title(gettext("Proxy Mode"))
        .use_subtitle(true)
        .model(&gtk::StringList::new(&[&labels[0], &labels[1], &labels[2]]))
        .selected(proxy_mode_index(policy.proxy.mode))
        .build();
    let proxy_uri = adw::EntryRow::builder()
        .title(gettext("Proxy URI"))
        .text(policy.proxy.uri.as_deref().unwrap_or_default())
        .build();
    proxy_uri.set_sensitive(policy.proxy.mode == ProxyMode::Custom);
    let proxy_error = crate::ui::common::inline_error();
    proxy_group.add(&proxy_mode);
    proxy_group.add(&proxy_uri);
    proxy_group.add(&proxy_error);
    page.add(&proxy_group);
    let background_group = adw::PreferencesGroup::builder().title(gettext("Background"))
        .description(gettext("Background access is requested through the desktop portal. Normal launch keeps working if access is denied.")).build();
    let background = adw::SwitchRow::builder()
        .title(gettext("Keep Running in Background"))
        .subtitle(gettext(
            "Closing the window keeps this web application active",
        ))
        .active(policy.background.enabled)
        .build();
    let autostart = adw::SwitchRow::builder()
        .title(gettext("Start at Login"))
        .subtitle(gettext(
            "Start all opted-in Alcove applications without opening windows",
        ))
        .active(policy.background.autostart)
        .build();
    autostart.set_sensitive(policy.background.enabled);
    let background_error = crate::ui::common::inline_error();
    background_group.add(&background);
    background_group.add(&autostart);
    background_group.add(&background_error);
    page.add(&background_group);
    let filter_group = adw::PreferencesGroup::builder()
        .title(gettext("Content Filters"))
        .description(if config.engine == Engine::WebKit {
            gettext("Import WebKit content-extension JSON. Filters affect only this application.")
        } else {
            gettext("Content filters are available only with the WebKitGTK engine.")
        })
        .build();
    filter_group.set_sensitive(config.engine == Engine::WebKit);
    let import_row = adw::ActionRow::builder()
        .use_markup(false)
        .title(gettext("Import Filter List…"))
        .subtitle(gettext(
            "The list is validated by WebKit before it is added",
        ))
        .activatable(true)
        .build();
    import_row.add_suffix(&gtk::Image::from_icon_name("document-open-symbolic"));
    let filter_error = crate::ui::common::inline_error();
    page.add(&filter_group);
    let editor = Rc::new(Editor {
        id: config.id,
        start_url: config.start_url,
        dialog: dialog.downgrade(),
        saved: RefCell::new(policy),
        populating: Cell::new(false),
        busy: Cell::new(false),
        closing: Cell::new(false),
        pending: RefCell::new(VecDeque::new()),
        navigation,
        origins,
        navigation_error,
        proxy_mode,
        proxy_uri,
        proxy_error,
        background_group,
        background,
        autostart,
        background_error,
        filter_group,
        filter_rows: RefCell::new(Vec::new()),
        filter_error,
        import_row,
        notice,
    });
    editor.rebuild_filters();
    editor.connect();
    dialog.add(&page);
    dialog.connect_closed(move |_| {
        let _keep_alive = &editor;
    });
    dialog.present(Some(parent));
    dialog.upcast()
}

impl Editor {
    fn connect(self: &Rc<Self>) {
        self.navigation.connect_active_notify(glib::clone!(
            #[weak(rename_to=editor)]
            self,
            move |row| {
                if editor.populating.get() {
                    return;
                }
                if !row.is_active()
                    && editor.origins.is_sensitive()
                    && editor.origins.text().as_str() != format_origins(&editor.saved.borrow())
                    && parse_origins(editor.origins.text().as_str()).is_ok()
                {
                    // A switch can take focus before EntryRow's leave signal.
                    // Commit its valid draft before disabling the entry.
                    editor.queue(editor.text_edit(false));
                }
                editor.origins.set_sensitive(row.is_active());
                editor.queue(Edit::Navigation(row.is_active()));
            }
        ));
        self.proxy_mode.connect_selected_notify(glib::clone!(
            #[weak(rename_to=editor)]
            self,
            move |row| {
                if editor.populating.get() {
                    return;
                }
                editor.proxy_uri.set_sensitive(row.selected() == 2);
                if row.selected() == 2 {
                    editor.proxy_uri.grab_focus();
                }
                editor.commit_proxy();
            }
        ));
        self.background.connect_active_notify(glib::clone!(
            #[weak(rename_to=editor)]
            self,
            move |row| {
                if editor.populating.get() {
                    return;
                }
                editor.autostart.set_sensitive(row.is_active());
                editor.queue(Edit::Background(
                    row.is_active(),
                    row.is_active() && editor.autostart.is_active(),
                ));
            }
        ));
        self.autostart.connect_active_notify(glib::clone!(
            #[weak(rename_to=editor)]
            self,
            move |row| {
                if !editor.populating.get() {
                    editor.queue(Edit::Background(
                        editor.background.is_active(),
                        row.is_active(),
                    ));
                }
            }
        ));
        for (entry, proxy) in [
            (self.origins.clone(), false),
            (self.proxy_uri.clone(), true),
        ] {
            entry.connect_changed(glib::clone!(
                #[weak(rename_to=editor)]
                self,
                move |_| {
                    if !editor.populating.get() {
                        editor.validate_text(proxy);
                        editor.update_close();
                    }
                }
            ));
            entry.connect_entry_activated(glib::clone!(
                #[weak(rename_to=editor)]
                self,
                move |_| editor.commit_text(proxy)
            ));
            let focus = gtk::EventControllerFocus::new();
            focus.connect_leave(glib::clone!(
                #[weak(rename_to=editor)]
                self,
                move |_| editor.commit_text(proxy)
            ));
            entry.add_controller(focus);
        }
        self.import_row.connect_activated(glib::clone!(
            #[weak(rename_to=editor)]
            self,
            move |_| {
                if editor.busy.get() {
                    return;
                }
                editor.busy.set(true);
                editor.update_close();
                editor.import_row.set_sensitive(false);
                glib::spawn_future_local(glib::clone!(
                    #[strong]
                    editor,
                    async move {
                        let parent = editor
                            .dialog
                            .upgrade()
                            .and_then(|dialog| dialog.native().and_downcast::<gtk::Window>());
                        let result = import_filter(parent.as_ref()).await;
                        editor.busy.set(false);
                        editor.import_row.set_sensitive(true);
                        match result {
                            Ok(filter) => editor.queue(Edit::FilterAdd(filter)),
                            Err(error) if !is_cancelled(&error) => {
                                crate::ui::common::show_inline_error(
                                    &editor.filter_error,
                                    &error.to_string(),
                                )
                            }
                            Err(_) => {}
                        }
                        editor.update_close();
                        if !editor.pending.borrow().is_empty() && !editor.busy.get() {
                            editor.drain();
                        }
                        editor.finish_close();
                    }
                ));
            }
        ));
        if let Some(dialog) = self.dialog.upgrade() {
            dialog.connect_close_attempt(glib::clone!(
                #[weak(rename_to=editor)]
                self,
                move |_| editor.request_close()
            ));
        }
    }

    fn text_edit(&self, proxy: bool) -> Edit {
        if proxy {
            Edit::Proxy(
                selected_proxy_mode(self.proxy_mode.selected()),
                self.proxy_uri.text().to_string(),
            )
        } else {
            Edit::Origins(self.origins.text().to_string())
        }
    }

    fn validate_text(&self, proxy: bool) -> bool {
        let relevant = if proxy {
            self.proxy_mode.selected() == 2
        } else {
            self.navigation.is_active()
        };
        let error = if proxy {
            &self.proxy_error
        } else {
            &self.navigation_error
        };
        let entry = if proxy {
            &self.proxy_uri
        } else {
            &self.origins
        };
        let mut policy = self.saved.borrow().clone();
        let valid =
            !relevant || apply_edit(&mut policy, &self.text_edit(proxy), &self.start_url).is_ok();
        if valid {
            error.set_visible(false);
            entry.remove_css_class("error");
        } else {
            error.set_label(&if proxy {
                gettext("Enter an HTTP(S) or SOCKS proxy address without credentials.")
            } else {
                gettext("Enter valid HTTP or HTTPS origins, separated by commas.")
            });
            error.set_visible(true);
            entry.add_css_class("error");
        }
        valid
    }

    fn commit_proxy(self: &Rc<Self>) {
        self.update_close();
        if self.validate_text(true) {
            self.queue(self.text_edit(true));
        }
    }
    fn commit_text(self: &Rc<Self>, proxy: bool) {
        if self.populating.get() || self.closing.get() {
            return;
        }
        if proxy {
            self.commit_proxy();
        } else if self.navigation.is_active() && self.validate_text(false) {
            self.queue(self.text_edit(false));
        }
    }
    fn dirty(&self) -> bool {
        let saved = self.saved.borrow();
        (self.navigation.is_active() && self.origins.text().as_str() != format_origins(&saved))
            || self.proxy_mode.selected() != proxy_mode_index(saved.proxy.mode)
            || (self.proxy_mode.selected() == 2
                && self.proxy_uri.text().as_str() != saved.proxy.uri.as_deref().unwrap_or_default())
    }
    fn update_close(&self) {
        if let Some(dialog) = self.dialog.upgrade() {
            dialog.set_can_close(!self.busy.get() && !self.dirty());
        }
    }
    fn queue(self: &Rc<Self>, edit: Edit) {
        if self.populating.get() {
            return;
        }
        self.pending.borrow_mut().push_back(edit);
        if !self.busy.get() {
            self.drain();
        }
    }
    fn drain(self: &Rc<Self>) {
        if self.busy.replace(true) {
            return;
        }
        self.update_close();
        glib::spawn_future_local(glib::clone!(
            #[strong(rename_to=editor)]
            self,
            async move {
                loop {
                    let edit = editor.pending.borrow_mut().pop_front();
                    let Some(edit) = edit else {
                        break;
                    };
                    let result = editor.save(&edit).await;
                    match result {
                        Ok(saved) => editor.saved_edit(&edit, saved),
                        Err(error) => {
                            editor.restore_switches(&edit);
                            if matches!(
                                edit,
                                Edit::FilterEnabled(_, _)
                                    | Edit::FilterRemove(_)
                                    | Edit::FilterAdd(_)
                            ) {
                                editor.rebuild_filters();
                            }
                            crate::ui::common::show_inline_error(
                                editor.error_for(&edit),
                                &error.to_string(),
                            );
                        }
                    }
                }
                editor.busy.set(false);
                editor.update_close();
                editor.finish_close();
            }
        ));
    }
    fn error_for(&self, edit: &Edit) -> &gtk::Label {
        match edit {
            Edit::Navigation(_) | Edit::Origins(_) => &self.navigation_error,
            Edit::Proxy(_, _) => &self.proxy_error,
            Edit::Background(_, _) => &self.background_error,
            _ => &self.filter_error,
        }
    }
    async fn save(&self, edit: &Edit) -> Result<AppPolicyV2> {
        let service = AppService::portal();
        let original = service.load_policy(&self.id)?;
        let mut desired = original.clone();
        apply_edit(&mut desired, edit, &self.start_url)?;
        if desired == original {
            return Ok(original);
        }
        if matches!(edit, Edit::Background(_, _)) {
            self.background_group.set_sensitive(false);
            let parent = self
                .dialog
                .upgrade()
                .and_then(|dialog| dialog.native().and_downcast::<gtk::Window>());
            let identifier = match parent {
                Some(parent) => WindowIdentifier::from_native(&parent).await,
                None => None,
            };
            let result = service
                .merge_policy_with_background(
                    &self.id,
                    &original,
                    &desired,
                    identifier.as_ref(),
                    &gettext("Keep this Alcove application running in the background"),
                )
                .await;
            self.background_group.set_sensitive(true);
            result?;
            let saved = service.load_policy(&self.id)?;
            if saved.background != desired.background {
                self.saved.replace(saved);
                return Err(anyhow!(gettext("Background access was not enabled")));
            }
            Ok(saved)
        } else {
            service.merge_policy(&self.id, &original, &desired)
        }
    }
    fn saved_edit(self: &Rc<Self>, edit: &Edit, saved: AppPolicyV2) {
        let changed = *self.saved.borrow() != saved;
        let old_origins = format_origins(&self.saved.borrow());
        self.saved.replace(saved.clone());
        self.populating.set(true);
        match edit {
            Edit::Navigation(_) => {
                self.navigation.set_active(saved.navigation.enabled);
                self.origins.set_sensitive(saved.navigation.enabled);
                // Preserve an existing unsaved list when only its switch changed.
                if self.origins.text().as_str() == old_origins {
                    self.origins.set_text(&format_origins(&saved));
                }
            }
            Edit::Origins(value) if self.origins.text().as_str() == value => {
                self.origins.set_text(&format_origins(&saved))
            }
            Edit::Proxy(mode, value)
                if self.proxy_mode.selected() == proxy_mode_index(*mode)
                    && self.proxy_uri.text().as_str() == value =>
            {
                if let Some(uri) = &saved.proxy.uri {
                    self.proxy_uri.set_text(uri);
                }
            }
            Edit::Background(_, _) => self.restore_switches(edit),
            Edit::FilterEnabled(_, _) | Edit::FilterRemove(_) | Edit::FilterAdd(_) => {
                self.rebuild_filters()
            }
            _ => {}
        }
        self.populating.set(false);
        self.error_for(edit).set_visible(false);
        if changed {
            self.notice.set_visible(true);
        }
    }
    fn restore_switches(&self, edit: &Edit) {
        let saved = self.saved.borrow();
        let previous = self.populating.replace(true);
        match edit {
            Edit::Navigation(_) => {
                self.navigation.set_active(saved.navigation.enabled);
                self.origins.set_sensitive(saved.navigation.enabled);
            }
            Edit::Background(_, _) => {
                self.background.set_active(saved.background.enabled);
                self.autostart.set_active(saved.background.autostart);
                self.autostart.set_sensitive(saved.background.enabled);
            }
            Edit::Proxy(_, _) => {
                self.proxy_mode
                    .set_selected(proxy_mode_index(saved.proxy.mode));
                self.proxy_uri
                    .set_sensitive(saved.proxy.mode == ProxyMode::Custom);
            }
            _ => {}
        }
        self.populating.set(previous);
    }
    fn rebuild_filters(self: &Rc<Self>) {
        for row in self.filter_rows.borrow_mut().drain(..) {
            self.filter_group.remove(&row);
        }
        let mut rows = self.filter_rows.borrow_mut();
        for (id, filter) in &self.saved.borrow().content_filters {
            let row = adw::SwitchRow::builder()
                .use_markup(false)
                .title(&filter.name)
                .subtitle(gettext("WebKit content-extension rules"))
                .active(filter.enabled)
                .build();
            let remove = gtk::Button::builder()
                .icon_name("edit-delete-symbolic")
                .tooltip_text(gettext("Remove Filter"))
                .valign(gtk::Align::Center)
                .css_classes(["flat"])
                .build();
            row.add_suffix(&remove);
            row.connect_active_notify(glib::clone!(
                #[weak(rename_to=editor)]
                self,
                #[strong]
                id,
                move |row| {
                    if !editor.populating.get() {
                        editor.queue(Edit::FilterEnabled(id.clone(), row.is_active()));
                    }
                }
            ));
            remove.connect_clicked(glib::clone!(
                #[weak(rename_to=editor)]
                self,
                #[strong]
                id,
                move |_| editor.queue(Edit::FilterRemove(id.clone()))
            ));
            self.filter_group.add(&row);
            rows.push(row.upcast());
        }
        self.filter_group.add(&self.import_row);
        rows.push(self.import_row.clone().upcast());
        self.filter_group.add(&self.filter_error);
        rows.push(self.filter_error.clone().upcast());
    }
    fn request_close(self: &Rc<Self>) {
        if self.closing.replace(true) {
            return;
        }
        let valid_origins = self.validate_text(false);
        let valid_proxy = self.validate_text(true);
        // Queue valid drafts even while a portal is pending. Completion will
        // close the dialog or offer to discard only invalid/failed fields.
        if valid_origins && self.navigation.is_active() {
            self.queue(self.text_edit(false));
        }
        if valid_proxy {
            self.queue(self.text_edit(true));
        }
        self.finish_close();
    }
    fn finish_close(self: &Rc<Self>) {
        if !self.closing.get() || self.busy.get() {
            return;
        }
        if self.dirty() {
            self.confirm_discard();
        } else if let Some(dialog) = self.dialog.upgrade() {
            dialog.force_close();
        }
    }
    fn confirm_discard(self: &Rc<Self>) {
        let Some(parent) = self.dialog.upgrade() else {
            return;
        };
        let alert = adw::AlertDialog::new(
            Some(&gettext("Discard Unsaved Changes?")),
            Some(&gettext(
                "Your unsaved changes will be lost. Saved settings will be kept.",
            )),
        );
        alert.add_responses(&[
            ("keep", &gettext("Keep Editing")),
            ("discard", &gettext("Discard")),
        ]);
        alert.set_close_response("keep");
        alert.set_default_response(Some("keep"));
        alert.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
        alert.connect_response(
            None,
            glib::clone!(
                #[weak(rename_to=editor)]
                self,
                move |_, response| {
                    if response == "discard" {
                        if let Some(dialog) = editor.dialog.upgrade() {
                            dialog.force_close();
                        }
                    } else {
                        editor.closing.set(false);
                    }
                }
            ),
        );
        alert.present(Some(&parent));
    }
}

pub(super) fn apply_edit(policy: &mut AppPolicyV2, edit: &Edit, start_url: &str) -> Result<()> {
    match edit {
        Edit::Navigation(enabled) => {
            policy.navigation.enabled = *enabled;
            if *enabled {
                policy
                    .navigation
                    .allowed_origins
                    .insert(Origin::from_str(start_url)?);
            }
        }
        Edit::Origins(value) => {
            policy.navigation.allowed_origins = parse_origins(value)?;
            if policy.navigation.enabled {
                policy
                    .navigation
                    .allowed_origins
                    .insert(Origin::from_str(start_url)?);
            }
        }
        Edit::Proxy(mode, value) => {
            policy.proxy.mode = *mode;
            policy.proxy.uri = if *mode == ProxyMode::Custom {
                Some(normalize_proxy_uri(value)?)
            } else {
                None
            };
        }
        Edit::Background(enabled, autostart) => {
            policy.background.enabled = *enabled;
            policy.background.autostart = *enabled && *autostart;
        }
        Edit::FilterEnabled(id, enabled) => {
            policy
                .content_filters
                .get_mut(id)
                .context("content filter no longer exists")?
                .enabled = *enabled;
        }
        Edit::FilterRemove(id) => {
            policy.content_filters.remove(id);
        }
        Edit::FilterAdd(filter) => {
            policy.add_content_filter(filter.clone())?;
        }
    }
    policy.validate()
}
fn parse_origins(value: &str) -> Result<BTreeSet<Origin>> {
    value
        .split(|character: char| character.is_whitespace() || character == ',' || character == ';')
        .filter(|origin| !origin.is_empty())
        .map(Origin::from_str)
        .collect()
}
fn format_origins(policy: &AppPolicyV2) -> String {
    policy
        .navigation
        .allowed_origins
        .iter()
        .map(Origin::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}
fn proxy_mode_index(mode: ProxyMode) -> u32 {
    match mode {
        ProxyMode::System => 0,
        ProxyMode::NoProxy => 1,
        ProxyMode::Custom => 2,
    }
}
fn selected_proxy_mode(index: u32) -> ProxyMode {
    match index {
        1 => ProxyMode::NoProxy,
        2 => ProxyMode::Custom,
        _ => ProxyMode::System,
    }
}
async fn import_filter(parent: Option<&gtk::Window>) -> Result<ContentFilterRuleSet> {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some(&gettext("WebKit Content Filters")));
    filter.add_mime_type("application/json");
    filter.add_pattern("*.json");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let file = gtk::FileDialog::builder()
        .title(gettext("Import Content Filter"))
        .accept_label(gettext("Import"))
        .filters(&filters)
        .modal(true)
        .build()
        .open_future(parent)
        .await
        .map_err(|error| {
            crate::system::portal::classify_file_dialog_error(
                gettext("Import content filter"),
                &error,
            )
            .map(anyhow::Error::from)
            .unwrap_or_else(|| {
                crate::system::portal::PortalOperationError::new(
                    crate::system::portal::PortalFailureKind::Cancelled,
                    gettext("Import content filter"),
                    "content filter selection was cancelled",
                )
                .into()
            })
        })?;
    let path = file
        .path()
        .context("the selected content filter is not a local file")?;
    let name = file
        .basename()
        .and_then(|name| Path::new(&name).file_stem().map(|stem| stem.to_owned()))
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| gettext("Imported Filter"));
    let bytes = gio::spawn_blocking(move || read_limited(&path))
        .await
        .map_err(|_| anyhow!("the content filter reader stopped unexpectedly"))??;
    let source = serde_json::from_slice(&bytes).context("invalid content filter JSON")?;
    let filter = ContentFilterRuleSet::new(name, source)?;
    content_filters::validate_filter(&filter).await?;
    Ok(filter)
}

fn read_limited(path: &Path) -> Result<Vec<u8>> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut bytes = Vec::new();
    file.take((MAX_CONTENT_FILTER_SOURCE_SIZE + 1) as u64)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= MAX_CONTENT_FILTER_SOURCE_SIZE,
        "content filter exceeds the 8 MiB limit"
    );
    Ok(bytes)
}

fn is_cancelled(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<crate::system::portal::PortalOperationError>()
        .is_some_and(|error| error.kind == crate::system::portal::PortalFailureKind::Cancelled)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_is_not_inferred_from_a_file_error_message() {
        assert!(!is_cancelled(&anyhow!("failed to open cancelled.json")));
        let cancelled = crate::system::portal::PortalOperationError::new(
            crate::system::portal::PortalFailureKind::Cancelled,
            "Import content filter",
            "selection dismissed",
        );
        assert!(is_cancelled(&anyhow::Error::from(cancelled)));
    }
    #[test]
    fn origin_editor_normalizes_and_deduplicates() {
        let origins = parse_origins(
            "https://example.org/path, HTTPS://EXAMPLE.ORG:443; http://other.example:8080/x",
        )
        .unwrap();
        assert_eq!(origins.len(), 2);
        assert!(origins.contains(&Origin::from_str("https://example.org").unwrap()));
    }
    #[test]
    fn field_edits_preserve_unrelated_policy_and_keep_background_opt_in() {
        let mut policy = AppPolicyV2::default();
        let origin = Origin::from_str("https://example.org").unwrap();
        policy.set_decision(
            origin.clone(),
            crate::domain::policy::PermissionKind::Camera,
            crate::domain::policy::PermissionDecision::Block,
        );
        apply_edit(
            &mut policy,
            &Edit::Navigation(true),
            "https://example.org/page",
        )
        .unwrap();
        assert!(policy.navigation.allowed_origins.contains(&origin));
        apply_edit(
            &mut policy,
            &Edit::Proxy(ProxyMode::NoProxy, String::new()),
            "https://example.org",
        )
        .unwrap();
        assert_eq!(
            policy.decision(&origin, crate::domain::policy::PermissionKind::Camera),
            crate::domain::policy::PermissionDecision::Block
        );
        assert!(!policy.background.enabled);
        assert!(!policy.background.autostart);
        assert!(policy.navigation.enabled);
        apply_edit(
            &mut policy,
            &Edit::Background(false, true),
            "https://example.org",
        )
        .unwrap();
        assert!(!policy.background.autostart);
    }
}
