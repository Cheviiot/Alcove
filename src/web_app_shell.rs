// SPDX-License-Identifier: GPL-3.0-only
//! The window chrome shared by WebKit and the optional Chromium host.
//! Engines supply a widget and implement the same `win.*` actions.

use adw::prelude::*;
use gettextrs::gettext;
use gtk::{gdk, gio, glib};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
    time::{Duration, Instant},
};

pub const DEFAULT_ZOOM_LEVEL: f64 = 1.0;
pub const MIN_ZOOM_LEVEL: f64 = 0.5;
pub const MAX_ZOOM_LEVEL: f64 = 3.0;
pub const ZOOM_STEP: f64 = 0.1;
pub fn adjusted_zoom_level(current: f64, delta: f64) -> f64 {
    let stepped = ((current + delta) * 10.0).round() / 10.0;
    stepped.clamp(MIN_ZOOM_LEVEL, MAX_ZOOM_LEVEL)
}

const HIDE_DELAY: Duration = Duration::from_millis(1500);

#[derive(Debug, Default, Clone, Copy)]
struct ToolbarState {
    loading: bool,
    pointer: bool,
    focus: bool,
    menu: bool,
    dialogs: u32,
}

impl ToolbarState {
    fn can_hide(self) -> bool {
        !self.loading && !self.pointer && !self.focus && !self.menu && self.dialogs == 0
    }
}

#[derive(Debug)]
pub struct WebAppShell {
    toolbar: adw::ToolbarView,
    content: adw::Bin,
    title: gtk::Label,
    back: gtk::Button,
    forward: gtk::Button,
    progress: gtk::ProgressBar,
    keys: gtk::EventControllerKey,
    css: gtk::CssProvider,
    state: Cell<ToolbarState>,
    hide_source: RefCell<Option<glib::SourceId>>,
    weak: Weak<Self>,
}

impl WebAppShell {
    pub fn new(window: &impl IsA<gtk::Window>, extra_menu: &gio::Menu) -> Rc<Self> {
        if let Some(app) = window.as_ref().application() {
            for (action, keys) in [
                ("win.back", vec!["<alt>Left", "Back"]),
                ("win.forward", vec!["<alt>Right", "Forward"]),
                ("win.reload", vec!["<primary>r", "F5"]),
                ("win.reload-bypass-cache", vec!["<primary><shift>r"]),
                ("win.home", vec!["<alt>Home"]),
                ("win.zoom-in", vec!["<primary>plus", "<primary>equal"]),
                ("win.zoom-out", vec!["<primary>minus"]),
                ("win.zoom-reset", vec!["<primary>0"]),
                ("win.toggle-fullscreen", vec!["F11"]),
            ] {
                app.set_accels_for_action(action, &keys);
            }
        }
        let toolbar = adw::ToolbarView::new();
        toolbar.set_extend_content_to_top_edge(true);
        let header = adw::HeaderBar::new();
        // Use an opaque native toolbar even when it overlays web content.
        header.add_css_class("bastle-web-header");
        let css = gtk::CssProvider::new();
        css.load_from_string(".bastle-web-header { background-color: var(--window-bg-color); }");
        gtk::style_context_add_provider_for_display(
            &header.display(),
            &css,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        let title = gtk::Label::builder()
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(32)
            .css_classes(["heading"])
            .build();
        header.set_title_widget(Some(&title));
        let navigation = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let back = gtk::Button::builder()
            .icon_name("go-previous-symbolic")
            .tooltip_text(gettext("Back"))
            .action_name("win.back")
            .sensitive(false)
            .build();
        let forward = gtk::Button::builder()
            .icon_name("go-next-symbolic")
            .tooltip_text(gettext("Forward"))
            .action_name("win.forward")
            .sensitive(false)
            .build();
        navigation.append(&back);
        navigation.append(&forward);
        header.pack_start(&navigation);
        let model = gio::Menu::new();
        for entries in [
            vec![
                (gettext("_Reload"), "win.reload"),
                (gettext("Reload Without Cache"), "win.reload-bypass-cache"),
                (gettext("_Stop Loading"), "win.stop"),
                (gettext("_Home"), "win.home"),
            ],
            vec![
                (gettext("Zoom _In"), "win.zoom-in"),
                (gettext("Zoom _Out"), "win.zoom-out"),
                (gettext("_Reset Zoom"), "win.zoom-reset"),
                (gettext("_Fullscreen"), "win.toggle-fullscreen"),
            ],
        ] {
            let section = gio::Menu::new();
            for (label, action) in entries {
                section.append(Some(&label), Some(action));
            }
            model.append_section(None, &section);
        }
        model.append_section(None, extra_menu);
        let menu = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .tooltip_text(gettext("Menu"))
            .popover(&native_menu(&model.upcast()))
            .build();
        header.pack_end(&menu);
        toolbar.add_top_bar(&header);
        let content = adw::Bin::new();
        let overlay = gtk::Overlay::new();
        overlay.set_child(Some(&content));
        let progress = gtk::ProgressBar::builder()
            .valign(gtk::Align::Start)
            .height_request(2)
            .can_target(false)
            .visible(false)
            .css_classes(["osd"])
            .build();
        overlay.add_overlay(&progress);
        let hot_zone = gtk::Box::builder()
            .valign(gtk::Align::Start)
            .height_request(6)
            .hexpand(true)
            .build();
        overlay.add_overlay(&hot_zone);
        toolbar.set_content(Some(&overlay));
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let shell = Rc::new_cyclic(|weak| Self {
            toolbar,
            content,
            title,
            back,
            forward,
            progress,
            keys,
            css,
            state: Cell::new(ToolbarState::default()),
            hide_source: RefCell::new(None),
            weak: weak.clone(),
        });

        let motion = gtk::EventControllerMotion::new();
        let weak = Rc::downgrade(&shell);
        motion.connect_enter(move |_, _, _| {
            if let Some(shell) = weak.upgrade() {
                shell.update(|s| s.pointer = true);
            }
        });
        let weak = Rc::downgrade(&shell);
        motion.connect_leave(move |_| {
            if let Some(shell) = weak.upgrade() {
                shell.update(|s| s.pointer = false);
            }
        });
        header.add_controller(motion);
        let focus = gtk::EventControllerFocus::new();
        let weak = Rc::downgrade(&shell);
        focus.connect_contains_focus_notify(move |focus| {
            if let Some(shell) = weak.upgrade() {
                shell.update(|s| s.focus = focus.contains_focus());
            }
        });
        header.add_controller(focus);
        let weak = Rc::downgrade(&shell);
        menu.connect_active_notify(move |menu| {
            if let Some(shell) = weak.upgrade() {
                shell.update(|s| s.menu = menu.is_active());
            }
        });
        let motion = gtk::EventControllerMotion::new();
        let weak = Rc::downgrade(&shell);
        motion.connect_enter(move |_, _, _| {
            if let Some(shell) = weak.upgrade() {
                shell.reveal();
            }
        });
        hot_zone.add_controller(motion);
        let tap = gtk::GestureClick::new();
        let weak = Rc::downgrade(&shell);
        tap.connect_pressed(move |_, _, _, _| {
            if let Some(shell) = weak.upgrade() {
                shell.reveal();
            }
        });
        hot_zone.add_controller(tap);
        let weak = Rc::downgrade(&shell);
        shell.keys.connect_key_pressed(move |_, key, _, _| {
            if key != gdk::Key::F10 {
                return glib::Propagation::Proceed;
            }
            if let Some(shell) = weak.upgrade() {
                shell.reveal();
                // The hidden header must be mapped before focus can move to it.
                // Stop requesting frames once focus is obtained (or unmapped).
                let deadline = Instant::now() + Duration::from_millis(500);
                menu.add_tick_callback(move |menu, _| {
                    if menu.grab_focus() || Instant::now() >= deadline {
                        glib::ControlFlow::Break
                    } else {
                        glib::ControlFlow::Continue
                    }
                });
            }
            glib::Propagation::Stop
        });
        window.as_ref().add_controller(shell.keys.clone());
        shell.reveal();
        shell
    }

    pub fn widget(&self) -> &adw::ToolbarView {
        &self.toolbar
    }
    pub fn set_content(&self, content: Option<&impl IsA<gtk::Widget>>) {
        self.content.set_child(content);
    }
    pub fn set_title(&self, title: &str) {
        self.title.set_label(title);
    }
    pub fn set_navigation(&self, back: bool, forward: bool) {
        self.back.set_sensitive(back);
        self.forward.set_sensitive(forward);
    }
    pub fn set_loading(&self, loading: bool, progress: f64) {
        self.progress.set_fraction(progress.clamp(0.0, 1.0));
        self.progress.set_visible(loading);
        if self.state.get().loading != loading {
            self.update(|s| s.loading = loading);
        }
    }
    pub fn begin_dialog(&self) {
        self.update(|s| s.dialogs += 1);
    }
    pub fn end_dialog(&self) {
        self.update(|s| s.dialogs = s.dialogs.saturating_sub(1));
    }
    pub fn is_modal(&self) -> bool {
        self.state.get().menu || self.state.get().dialogs > 0
    }
    pub fn reveal(&self) {
        self.toolbar.set_reveal_top_bars(true);
        self.schedule_hide();
    }
    fn update(&self, f: impl FnOnce(&mut ToolbarState)) {
        let mut state = self.state.get();
        f(&mut state);
        self.state.set(state);
        if state.can_hide() {
            self.schedule_hide();
        } else {
            self.reveal();
        }
    }
    fn schedule_hide(&self) {
        if let Some(source) = self.hide_source.borrow_mut().take() {
            source.remove();
        }
        if !self.state.get().can_hide() {
            return;
        }
        let weak = self.weak.clone();
        let source = glib::timeout_add_local_once(HIDE_DELAY, move || {
            if let Some(shell) = weak.upgrade() {
                shell.hide_source.borrow_mut().take();
                if shell.state.get().can_hide() {
                    shell.toolbar.set_reveal_top_bars(false);
                }
            }
        });
        self.hide_source.replace(Some(source));
    }
}

impl Drop for WebAppShell {
    fn drop(&mut self) {
        gtk::style_context_remove_provider_for_display(&self.toolbar.display(), &self.css);
        if let Some(source) = self.hide_source.get_mut().take() {
            source.remove();
        }
    }
}

fn native_menu(model: &gio::MenuModel) -> gtk::PopoverMenu {
    // Explicit labels also work around unnamed generated AT-SPI menu items
    // in GTK 4.22. Keep the native popover's sections and keyboard navigation.
    fn slots(model: &gio::MenuModel, commands: &mut Vec<(String, String, String)>) -> gio::Menu {
        let output = gio::Menu::new();
        for index in 0..model.n_items() {
            if let Some(section) = model.item_link(index, "section") {
                output.append_section(None, &slots(&section, commands));
                continue;
            }
            let Some(action) = model
                .item_attribute_value(index, "action", None)
                .and_then(|v| v.get::<String>())
            else {
                continue;
            };
            let Some(label) = model
                .item_attribute_value(index, "label", None)
                .and_then(|v| v.get::<String>())
            else {
                continue;
            };
            let id = format!("command-{}", commands.len());
            let item = gio::MenuItem::new(None, None);
            item.set_attribute_value("custom", Some(&id.to_variant()));
            output.append_item(&item);
            commands.push((id, action, label));
        }
        output
    }
    let mut commands = Vec::new();
    let popover = gtk::PopoverMenu::from_model(Some(&slots(model, &mut commands)));
    for (id, action, label) in commands {
        let button = gtk::Button::builder()
            .label(&label)
            .use_underline(true)
            .action_name(&action)
            .accessible_role(gtk::AccessibleRole::MenuItem)
            .hexpand(true)
            .css_classes(["flat"])
            .build();
        button.update_property(&[gtk::accessible::Property::Label(&label.replace('_', ""))]);
        if let Some(label) = button.child().and_downcast::<gtk::Label>() {
            label.set_xalign(0.0);
        }
        button.connect_clicked(glib::clone!(
            #[weak]
            popover,
            move |_| popover.popdown()
        ));
        assert!(popover.add_child(&button, &id));
    }
    popover
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn toolbar_stays_visible_during_loading_or_interaction() {
        assert!(ToolbarState::default().can_hide());
        for state in [
            ToolbarState {
                loading: true,
                ..Default::default()
            },
            ToolbarState {
                pointer: true,
                ..Default::default()
            },
            ToolbarState {
                focus: true,
                ..Default::default()
            },
            ToolbarState {
                menu: true,
                ..Default::default()
            },
            ToolbarState {
                dialogs: 2,
                ..Default::default()
            },
        ] {
            assert!(!state.can_hide());
        }
    }
    #[test]
    fn zoom_levels_are_stepped_and_bounded() {
        assert_eq!(adjusted_zoom_level(1.0, ZOOM_STEP), 1.1);
        assert_eq!(adjusted_zoom_level(1.1, -ZOOM_STEP), 1.0);
        assert_eq!(adjusted_zoom_level(MIN_ZOOM_LEVEL, -ZOOM_STEP), 0.5);
        assert_eq!(adjusted_zoom_level(MAX_ZOOM_LEVEL, ZOOM_STEP), 3.0);
    }
}
