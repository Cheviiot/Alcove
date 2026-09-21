// SPDX-License-Identifier: GPL-3.0-only
//! The entry point Alcove itself uses: validate what the caller asked for,
//! start a worker and hand back the window it renders into.

use anyhow::{Context, Result};
use gtk::glib;
use std::{
    fs::{self},
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    rc::Rc,
};

use crate::engines::chromium::protocol;

use crate::ui::shell::chromium::site::store::PolicyStore;
use crate::ui::shell::chromium::{
    options::Options,
    site::store,
    window::build_view,
    worker::process::{View, Worker},
};

/// Inputs supplied by Alcove after validating the app configuration and policy.
/// `lifetime` retains the repository profile lock until all CEF windows close.
pub struct Launch {
    pub app_id: crate::domain::model::AppId,
    pub start_in_background: bool,
    pub worker: PathBuf,
    pub cef: PathBuf,
    pub profile: PathBuf,
    pub url: String,
    pub title: String,
    pub width: i32,
    pub height: i32,
    pub maximized: bool,
    pub user_agent: Option<String>,
    pub lifetime: Rc<dyn std::any::Any>,
    pub policy: crate::domain::policy::AppPolicyV2,
    pub policy_store: Rc<dyn PolicyStore>,
}

pub fn open(app: &adw::Application, launch: Launch) -> Result<adw::ApplicationWindow> {
    protocol::validate_url(&launch.url)?;
    launch.policy.validate()?;
    let directory = tempfile::Builder::new()
        .prefix("alcove-cef-")
        .tempdir_in(glib::user_runtime_dir())
        .context("create private CEF runtime directory")?;
    fs::create_dir_all(&launch.profile)?;
    fs::set_permissions(&launch.profile, fs::Permissions::from_mode(0o700))?;
    let mut options = Options {
        app_id: Some(launch.app_id),
        start_in_background: launch.start_in_background,
        worker: launch.worker,
        cef: launch.cef,
        output: directory.path().to_path_buf(),
        profile: Some(launch.profile),
        url: launch.url,
        title: launch.title,
        width: launch.width,
        height: launch.height,
        maximized: launch.maximized,
        user_agent: launch.user_agent,
        keep_alive: Some(Rc::new((directory, launch.lifetime))),
        policy: store::Policy::new(launch.policy, Some(launch.policy_store)),
        seconds: 0,
        gpu: true,
        native_accessibility: true,
        diagnostics: false,
        layout: false,
        fake_media: false,
        real_site: false,
    };
    let worker = Worker::start(&options)?;
    // Only the worker owns the profile lock and IPC directory. View options
    // must not extend their lifetime through GTK signal closures.
    options.keep_alive = None;
    build_view(app, options, View::new(worker, 1), None)
}
