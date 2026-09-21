// SPDX-License-Identifier: GPL-3.0-only
//! Finding the separately installed add-on that carries the CEF worker, and
//! refusing one whose manifest does not match this build.

use anyhow::{ensure, Context, Result};
use serde::Deserialize;
use std::path::{Component, Path, PathBuf};

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
            && addon.worker_protocol == super::WORKER_PROTOCOL
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

pub fn installed() -> bool {
    std::env::var_os(ADDON_ENV).is_some_and(|path| Path::new(&path).try_exists().unwrap_or(true))
}

/// The worker binary and the CEF root the configured add-on declares.
pub fn resolve() -> Result<(PathBuf, PathBuf)> {
    let manifest =
        std::env::var_os(ADDON_ENV).context("native Chromium add-on is not configured")?;
    read_addon(Path::new(&manifest))
}

/// Confirms the add-on can run, or explains why it cannot.
pub fn probe() -> Result<()> {
    resolve().map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::chromium::ChromiumBackend;
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
    /// A manifest the adapter itself considers valid must also satisfy the
    /// availability layer. A second protocol check anywhere else can disagree
    /// with the manifest and make a working add-on look incompatible.
    #[test]
    fn a_valid_addon_is_accepted_by_the_engine() {
        let root = tempfile::tempdir().expect("temporary add-on root");
        std::fs::create_dir_all(root.path().join("cef/Resources")).expect("cef root");
        std::fs::write(root.path().join("cef/Resources/icudtl.dat"), b"").expect("icu data");
        std::fs::write(
            root.path().join("worker"),
            b"#!/bin/sh
",
        )
        .expect("worker");
        let manifest = root.path().join("addon.json");
        std::fs::write(
            &manifest,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "worker_protocol": super::super::WORKER_PROTOCOL,
                "cef_version": CEF_VERSION,
                "worker": "worker",
                "cef_root": "cef",
            }))
            .expect("manifest"),
        )
        .expect("write manifest");

        std::env::set_var(ADDON_ENV, &manifest);
        let installed = installed();
        let probed = crate::engines::chromium::ChromiumClient.probe();
        std::env::remove_var(ADDON_ENV);

        assert!(installed);
        probed.expect("the engine accepts a manifest the adapter wrote");
    }
}
