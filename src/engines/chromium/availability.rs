// SPDX-License-Identifier: GPL-3.0-only

use anyhow::Result;

use crate::{domain::model::AppConfigV3, domain::policy::AppPolicyV2};

pub const ADDON_REF_URL: &str =
    "https://cheviiot.github.io/Alcove/alcove-chromium-native.flatpakref";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineAvailability {
    Missing,
    Available,
    Incompatible(String),
    Broken(String),
}

impl EngineAvailability {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }

    pub fn diagnostic(&self) -> Option<&str> {
        match self {
            Self::Incompatible(message) | Self::Broken(message) => Some(message),
            Self::Missing | Self::Available => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ChromiumClient;

pub trait ChromiumBackend: Clone {
    fn installed(&self) -> bool;
    /// Confirms the engine can run, or explains why it cannot.
    fn probe(&self) -> Result<()>;
    fn open_app(
        &self,
        app: &AppConfigV3,
        policy: &AppPolicyV2,
        start_in_background: bool,
    ) -> Result<()>;
}

impl ChromiumBackend for ChromiumClient {
    fn installed(&self) -> bool {
        crate::engines::chromium::addon::installed()
    }

    fn probe(&self) -> Result<()> {
        // The worker protocol and CEF version are checked against the add-on
        // manifest in `chromium::addon::read_addon`.
        crate::engines::chromium::addon::probe()
    }

    fn open_app(
        &self,
        app: &AppConfigV3,
        _policy: &AppPolicyV2,
        start_in_background: bool,
    ) -> Result<()> {
        // The engine runs inside the application's own process, so switching to
        // it means starting that process; it then opens the window natively.
        crate::system::launcher::spawn_app_process(&app.id, start_in_background)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn availability_distinguishes_user_visible_states() {
        assert!(!EngineAvailability::Missing.is_available());
        assert_eq!(EngineAvailability::Missing.diagnostic(), None);
        let broken = EngineAvailability::Broken("failed".to_owned());
        assert_eq!(broken.diagnostic(), Some("failed"));
    }
}
