// SPDX-License-Identifier: GPL-3.0-only
//! Explicit opt-in while the native add-on is being integrated. The default
//! Chromium route continues to use the installed Electron service.

use adw::prelude::*;
use anyhow::{ensure, Context, Result};
use gtk::glib;
use serde::Deserialize;
use std::{
    path::{Component, Path, PathBuf},
    rc::Rc,
};

use crate::{
    application::AlcoveApplication,
    chromium::{ChromiumCapabilities, RUNTIME_SHELL_FEATURE},
    model::{AppConfigV3, AppId, WindowState},
    policy::{AppPolicyV2, Origin, PermissionDecision, PermissionKind},
    service::AppService,
};

const ADDON_ENV: &str = "ALCOVE_NATIVE_CHROMIUM_ADDON";
const CEF_VERSION: &str = "152.0.7+g83ffcba+chromium-152.0.7977.83";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Addon {
    schema_version: u32,
    worker_protocol: u32,
    cef_version: String,
    worker: PathBuf,
    cef_root: PathBuf,
}

fn addon_path(root: &Path, relative: &Path) -> Result<PathBuf> {
    ensure!(
        !relative.as_os_str().is_empty()
            && relative
                .components()
                .all(|c| matches!(c, Component::Normal(_))),
        "add-on paths must stay inside its directory"
    );
    let path = root.join(relative).canonicalize()?;
    ensure!(path.starts_with(root), "add-on path escapes its directory");
    Ok(path)
}

fn read_addon(path: &Path) -> Result<(PathBuf, PathBuf)> {
    ensure!(
        path.metadata()?.len() <= 64 * 1024,
        "native add-on manifest is too large"
    );
    let addon: Addon = serde_json::from_slice(&std::fs::read(path)?)?;
    ensure!(
        addon.schema_version == 1
            && addon.worker_protocol == alcove::native_chromium::WORKER_PROTOCOL
            && addon.cef_version == CEF_VERSION,
        "incompatible native Chromium add-on"
    );
    let root = path
        .canonicalize()?
        .parent()
        .context("add-on directory")?
        .to_path_buf();
    let worker = addon_path(&root, &addon.worker)?;
    let cef = addon_path(&root, &addon.cef_root)?;
    ensure!(
        worker.is_file() && cef.join("Resources/icudtl.dat").is_file(),
        "native Chromium add-on is incomplete"
    );
    Ok((worker, cef))
}

pub fn enabled() -> bool {
    std::env::var_os(ADDON_ENV).is_some()
}

pub fn installed() -> bool {
    std::env::var_os(ADDON_ENV).is_some_and(|path| Path::new(&path).try_exists().unwrap_or(true))
}

pub fn capabilities() -> Result<ChromiumCapabilities> {
    let manifest =
        std::env::var_os(ADDON_ENV).context("native Chromium add-on is not configured")?;
    read_addon(Path::new(&manifest))?;
    Ok(ChromiumCapabilities {
        protocol_version: alcove::native_chromium::WORKER_PROTOCOL,
        features: [
            "open-app",
            "policy-v2",
            "background",
            "native-cef",
            RUNTIME_SHELL_FEATURE,
        ]
        .into_iter()
        .map(String::from)
        .collect(),
    })
}

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
    let manifest =
        std::env::var_os(ADDON_ENV).context("native Chromium add-on is not configured")?;
    let (worker, cef) = read_addon(Path::new(&manifest))?;
    let service = AppService::portal();
    let mut policy = service.load_policy(&config.id)?;
    // Like the existing Chromium adapter, WebKit content filters do not apply.
    policy.content_filters.clear();
    let lock = service.acquire_runtime_lock(&config.id)?;
    let window = alcove::native_chromium::open(
        app.upcast_ref::<adw::Application>(),
        alcove::native_chromium::Launch {
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
impl alcove::native_chromium::PolicyStore for RepositoryPolicy {
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn addon_cannot_escape_its_directory() {
        let root = tempfile::tempdir().unwrap();
        for path in ["../worker", "/bin/true", ""] {
            assert!(addon_path(root.path(), Path::new(path)).is_err());
        }
        std::os::unix::fs::symlink("/bin/true", root.path().join("escape")).unwrap();
        assert!(addon_path(root.path(), Path::new("escape")).is_err());
    }
    #[test]
    fn rejects_incompatible_addon_before_resolving_binaries() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("addon.json");
        std::fs::write(&path, r#"{"schema_version":99,"worker_protocol":3,"cef_version":"wrong","worker":"worker","cef_root":"cef"}"#).unwrap();
        assert!(read_addon(&path)
            .unwrap_err()
            .to_string()
            .contains("incompatible"));
        std::fs::write(
            &path,
            serde_json::json!({"schema_version":1,"worker_protocol":3,
            "cef_version":CEF_VERSION,"worker":"worker","cef_root":"cef"})
            .to_string(),
        )
        .unwrap();
        assert!(read_addon(&path)
            .unwrap_err()
            .to_string()
            .contains("incompatible"));
    }
}
