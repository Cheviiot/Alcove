// SPDX-License-Identifier: GPL-3.0-only
//! The shared window chrome, wrapped so the host can tell when a dialog is
//! holding the toolbar open.

use std::{
    cell::Cell,
    rc::Rc,
    time::{Duration, Instant},
};

use crate::ui::shell::web_app_shell::WebAppShell;

pub struct Chrome {
    pub shell: Rc<WebAppShell>,
    pub dialogs: Cell<u32>,
    pub dialog_since: Cell<Option<Instant>>,
    pub dialog_hold_observed: Cell<bool>,
    pub dialog_hold_failed: Cell<bool>,
}
impl std::ops::Deref for Chrome {
    type Target = WebAppShell;
    fn deref(&self) -> &Self::Target {
        &self.shell
    }
}
impl Chrome {
    pub fn begin_dialog(&self) {
        if self.dialogs.get() == 0 {
            self.dialog_since.set(Some(Instant::now()));
        }
        self.dialogs.set(self.dialogs.get() + 1);
        self.shell.begin_dialog();
    }
    pub fn end_dialog(&self) {
        self.dialogs.set(self.dialogs.get().saturating_sub(1));
        if self.dialogs.get() == 0 {
            self.dialog_since.set(None);
        }
        self.shell.end_dialog();
    }
    pub fn tick(&self) {
        if self
            .dialog_since
            .get()
            .is_some_and(|t| t.elapsed() > Duration::from_millis(1500))
        {
            self.dialog_hold_observed.set(true);
            if !self.widget().reveals_top_bars() {
                self.dialog_hold_failed.set(true);
            }
        }
    }
}
