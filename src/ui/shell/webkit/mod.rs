// SPDX-License-Identifier: GPL-3.0-only
//! The WebKitGTK side of the web app window: the view itself, what a website's
//! `window.open` may do, and what it may ask the person for.
mod popups;
mod requests;
#[cfg(feature = "ui-tests")]
pub(crate) mod ui_test;
mod window;

pub use window::AppWindow;
