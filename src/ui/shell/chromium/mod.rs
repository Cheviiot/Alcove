// SPDX-License-Identifier: GPL-3.0-only
//! The Chromium engine. The worker runs CEF in a separate process and presents
//! its output inside the application's own GTK window; the add-on carrying it
//! is installed separately and is never required.
mod launch;
mod options;
#[cfg(feature = "native-chromium-probe")]
mod probe;
mod site;
mod widgets;
mod window;
mod worker;

pub use launch::{open, Launch};
#[cfg(feature = "native-chromium-probe")]
pub use probe::run_probe;
pub use site::store::PolicyStore;
