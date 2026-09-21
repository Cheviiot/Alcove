// SPDX-License-Identifier: GPL-3.0-only
mod dialog;
#[cfg(feature = "ui-tests")]
mod ui_test;

pub use dialog::CreateAppDialog;
#[cfg(feature = "ui-tests")]
pub(crate) use ui_test::run_ui_smoke_test;
