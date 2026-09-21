// SPDX-License-Identifier: GPL-3.0-only
//! Driving the web app window's background behaviour under the `ui-tests`
//! feature.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;

use super::window::AppWindow;
use crate::domain::model::AppConfigV3;
use crate::domain::policy::AppPolicyV2;

pub(crate) fn run_background_ui_smoke_test<P: IsA<gtk::Application>>(
    application: &P,
) -> anyhow::Result<()> {
    let repository = crate::system::service::user_repository();
    let config = AppConfigV3::new("Background UI smoke test", "http://127.0.0.1:9/", 0)?;
    repository
        .stage_create(&config, b"ui-test-icon")?
        .commit()?;
    let mut policy = AppPolicyV2::default();
    policy.background.enabled = true;
    policy.background.autostart = true;
    repository.merge_policy(&config.id, &AppPolicyV2::default(), &policy)?;

    let window = AppWindow::new(application, &config);
    window.start_in_background();
    anyhow::ensure!(!window.is_visible(), "background window remained visible");
    anyhow::ensure!(
        window.imp().background_hold.borrow().is_some(),
        "background mode did not hold the application"
    );
    window.show_from_background();
    anyhow::ensure!(window.is_visible(), "background window was not revealed");
    anyhow::ensure!(
        window.imp().background_hold.borrow().is_none(),
        "revealing the background window retained the application hold"
    );
    window.start_in_background();
    anyhow::ensure!(
        window.imp().background_hold.borrow().is_some(),
        "background mode could not be entered again"
    );
    window.stop_background();
    anyhow::ensure!(
        window.imp().background_hold.borrow().is_none(),
        "stopping background mode retained the application hold"
    );
    // Release the profile lock and remove the WebView before taking the
    // repository's exclusive cleanup lock. The test runs in-process, so the
    // GTK object may otherwise keep the runtime alive for one main-loop turn.
    window.shell().set_content(None::<&gtk::Widget>);
    window.imp().webview.take();
    window.imp().runtime_lock.take();
    window.destroy();
    drop(window);
    while glib::MainContext::default().pending() {
        glib::MainContext::default().iteration(false);
    }

    let delete_lock = repository.acquire_delete_profile_lock(&config.id)?;
    repository.delete_with_profile_lock(&config.id, delete_lock)?;
    Ok(())
}
