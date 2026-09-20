// SPDX-License-Identifier: GPL-3.0-only

use std::collections::BTreeSet;

use anyhow::{bail, Context, Result};

use crate::{
    domain::model::{AppConfigV3, AppId},
    domain::policy::AppPolicyV2,
    domain::repository::AppRepository,
};

pub const PROTOCOL_VERSION: u32 = 1;
/// Where the native engine keeps its per-application storage.
pub const CEF_PROFILE_DIR: &str = "chromium-cef";
pub const RUNTIME_SHELL_FEATURE: &str = "runtime-shell-v1";
pub const ADDON_REF_URL: &str =
    "https://cheviiot.github.io/Alcove/alcove-chromium-native.flatpakref";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChromiumCapabilities {
    pub protocol_version: u32,
    pub features: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineAvailability {
    Missing,
    Available(ChromiumCapabilities),
    Incompatible(String),
    Broken(String),
}

impl EngineAvailability {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available(_))
    }

    pub fn diagnostic(&self) -> Option<&str> {
        match self {
            Self::Incompatible(message) | Self::Broken(message) => Some(message),
            Self::Missing | Self::Available(_) => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ChromiumClient;

pub trait ChromiumBackend: Clone {
    fn installed(&self) -> bool;
    fn capabilities(&self) -> Result<ChromiumCapabilities>;
    fn open_app(
        &self,
        app: &AppConfigV3,
        policy: &AppPolicyV2,
        token: &str,
        start_in_background: bool,
    ) -> Result<()>;
    fn delete_profile(&self, id: &AppId, token: &str) -> Result<()>;
}

impl ChromiumBackend for ChromiumClient {
    fn installed(&self) -> bool {
        crate::engines::native_chromium_launch::installed()
    }

    fn capabilities(&self) -> Result<ChromiumCapabilities> {
        let capabilities = crate::engines::native_chromium_launch::capabilities()?;
        validate_capabilities(&capabilities)?;
        Ok(capabilities)
    }

    fn open_app(
        &self,
        app: &AppConfigV3,
        _policy: &AppPolicyV2,
        _token: &str,
        start_in_background: bool,
    ) -> Result<()> {
        // The engine runs inside the application's own process, so switching to
        // it means starting that process; it then opens the window natively.
        crate::app::application::spawn_app_process(&app.id, start_in_background)
    }

    fn delete_profile(&self, id: &AppId, _token: &str) -> Result<()> {
        let profile = AppRepository::for_current_user()
            .profile_dir(id)
            .join(CEF_PROFILE_DIR);
        match std::fs::remove_dir_all(&profile) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => {
                Err(error).with_context(|| format!("failed to remove {}", profile.display()))
            }
        }
    }
}

impl ChromiumCapabilities {
    pub fn require(&self, feature: &str) -> Result<()> {
        if !self.features.contains(feature) {
            bail!("the Chromium add-on does not support required feature {feature}");
        }
        Ok(())
    }
}

fn validate_capabilities(capabilities: &ChromiumCapabilities) -> Result<()> {
    if capabilities.protocol_version != PROTOCOL_VERSION {
        bail!(
            "incompatible Chromium add-on protocol {}; Alcove requires {}",
            capabilities.protocol_version,
            PROTOCOL_VERSION
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_mismatch_is_rejected_before_opening_an_app() {
        let capabilities = ChromiumCapabilities {
            protocol_version: PROTOCOL_VERSION + 1,
            features: BTreeSet::new(),
        };
        assert!(validate_capabilities(&capabilities)
            .unwrap_err()
            .to_string()
            .contains("incompatible"));
    }

    #[test]
    fn availability_distinguishes_user_visible_states() {
        assert!(!EngineAvailability::Missing.is_available());
        assert_eq!(EngineAvailability::Missing.diagnostic(), None);
        let broken = EngineAvailability::Broken("failed".to_owned());
        assert_eq!(broken.diagnostic(), Some("failed"));
    }
}
