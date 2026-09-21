// SPDX-License-Identifier: GPL-3.0-only
//! Everything one Chromium window needs before it is built: the worker and
//! CEF locations, the page to open and the policy that governs it.

use std::{path::PathBuf, rc::Rc};

use crate::ui::shell::chromium::site::store;

#[derive(Clone)]
pub(super) struct Options {
    pub(super) worker: PathBuf,
    pub(super) cef: PathBuf,
    pub(super) output: PathBuf,
    pub(super) url: String,
    pub(super) seconds: u64,
    pub(super) gpu: bool,
    pub(super) layout: bool,
    pub(super) native_accessibility: bool,
    pub(super) fake_media: bool,
    pub(super) real_site: bool,
    pub(super) diagnostics: bool,
    pub(super) profile: Option<PathBuf>,
    pub(super) title: String,
    pub(super) width: i32,
    pub(super) height: i32,
    pub(super) maximized: bool,
    pub(super) user_agent: Option<String>,
    pub(super) keep_alive: Option<Rc<dyn std::any::Any>>,
    pub(super) policy: Rc<store::Policy>,
    pub(super) app_id: Option<crate::domain::model::AppId>,
    pub(super) start_in_background: bool,
}
