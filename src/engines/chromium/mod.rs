// SPDX-License-Identifier: GPL-3.0-only
//! Whether the Chromium engine can run here, how its separately installed
//! add-on is found, and the wire protocol its worker speaks. The window that
//! renders the engine's output lives in `ui::shell::chromium`.
pub mod addon;
mod availability;
pub mod protocol;

pub use availability::{ChromiumBackend, ChromiumClient, EngineAvailability, ADDON_REF_URL};
pub use protocol::WORKER_PROTOCOL;
