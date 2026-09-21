// SPDX-License-Identifier: GPL-3.0-only
//! Opening an application on the Chromium engine. The engine renders inside a
//! window this process owns, so the glue that reaches the application object
//! and the stored policy lives here rather than in `engines`.

use adw::prelude::*;
use anyhow::Result;
use gtk::glib;
use std::rc::Rc;

use crate::{
    app::application::AlcoveApplication,
    domain::model::{AppConfigV3, AppId, WindowState},
    domain::policy::{AppPolicyV2, Origin, PermissionDecision, PermissionKind},
    engines::chromium::addon,
    system::service::AppService,
    ui::shell::chromium as engine,
};

pub fn existing_window(app: &AlcoveApplication, id: &AppId) -> Option<gtk::Window> {
    app.windows()
        .into_iter()
        .find(|w| w.widget_name() == format!("cef-{}", id))
}

pub fn background_action(app: &AlcoveApplication, parameter: Option<&glib::Variant>, action: &str) {
    let Some(id) = parameter
        .and_then(|v| v.get::<String>())
        .and_then(|s| s.parse::<AppId>().ok())
    else {
        return;
    };
    if let Some(window) = existing_window(app, &id) {
        let _ = gtk::prelude::WidgetExt::activate_action(&window, action, None);
    }
}

pub fn open(app: &AlcoveApplication, config: &AppConfigV3, background: bool) -> Result<()> {
    let (worker, cef) = addon::resolve()?;
    let service = AppService::portal();
    let mut policy = service.load_policy(&config.id)?;
    // Like the existing Chromium adapter, WebKit content filters do not apply.
    policy.content_filters.clear();
    let lock = service.acquire_runtime_lock(&config.id)?;
    let window = engine::open(
        app.upcast_ref::<adw::Application>(),
        engine::Launch {
            app_id: config.id.clone(),
            start_in_background: background,
            worker,
            cef,
            profile: service.profile_dir(&config.id).join("chromium-cef"),
            url: config.start_url.clone(),
            title: config.title.clone(),
            width: config.window.width,
            height: config.window.height,
            maximized: config.window.maximized,
            user_agent: config.user_agent.clone(),
            lifetime: Rc::new(lock),
            policy,
            policy_store: Rc::new(RepositoryPolicy(config.id.clone())),
        },
    )?;
    window.set_widget_name(&format!("cef-{}", config.id));
    let id = config.id.clone();
    window.connect_visible_notify(move |window| {
        if !window.is_visible() {
            save_window_state(window, &id);
        }
    });
    let id = config.id.clone();
    window.connect_close_request(move |window| {
        save_window_state(window, &id);
        glib::Propagation::Proceed
    });
    // The worker retains the profile lock and IPC through graceful shutdown.
    Ok(())
}

fn save_window_state(window: &adw::ApplicationWindow, id: &AppId) {
    let (width, height) = window.default_size();
    if let Err(error) = AppService::portal().save_runtime_state(
        id,
        WindowState {
            width,
            height,
            maximized: window.is_maximized(),
        },
    ) {
        eprintln!("Failed to save Alcove window state: {error:#}");
    }
}

struct RepositoryPolicy(AppId);
impl engine::PolicyStore for RepositoryPolicy {
    fn permissions(
        &self,
        origin: &Origin,
        kinds: &[PermissionKind],
        decision: PermissionDecision,
    ) -> Result<AppPolicyV2> {
        let changes = kinds
            .iter()
            .map(|kind| (origin.clone(), *kind, decision))
            .collect::<Vec<_>>();
        AppService::portal().apply_policy_decisions(&self.0, &changes)
    }
    fn allow_navigation(&self, origin: Origin) -> Result<AppPolicyV2> {
        AppService::portal().allow_navigation_origin(&self.0, origin)
    }
}
