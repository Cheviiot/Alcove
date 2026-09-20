// SPDX-License-Identifier: GPL-3.0-only
use crate::{domain::model::AppId, system::background::BackgroundSession};
use adw::prelude::*;
use gtk::glib;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

pub(super) struct Background {
    window: glib::WeakRef<adw::ApplicationWindow>,
    id: Option<AppId>,
    enabled: bool,
    stopping: Cell<bool>,
    session: RefCell<Option<BackgroundSession>>,
}

impl Background {
    pub fn new(window: &adw::ApplicationWindow, id: Option<AppId>, enabled: bool) -> Rc<Self> {
        Rc::new(Self {
            window: window.downgrade(),
            id,
            enabled,
            stopping: Cell::new(false),
            session: RefCell::new(None),
        })
    }
    pub fn enter(&self) -> bool {
        if !self.enabled || self.stopping.get() {
            return false;
        }
        let (Some(window), Some(id)) = (self.window.upgrade(), self.id.as_ref()) else {
            return false;
        };
        if self.session.borrow().is_none() {
            self.session
                .replace(BackgroundSession::start(&window, id.as_str()));
        }
        self.session.borrow().is_some()
    }
    pub fn finish(&self) {
        self.session.borrow_mut().take();
    }
    pub fn show(&self) {
        self.finish();
        if let Some(window) = self.window.upgrade() {
            window.present();
        }
    }
    pub fn stop(&self) {
        self.stopping.set(true);
        self.finish();
        if let Some(window) = self.window.upgrade() {
            window.close();
        }
    }
}
