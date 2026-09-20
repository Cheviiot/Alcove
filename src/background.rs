// SPDX-License-Identifier: GPL-3.0-only

use anyhow::{ensure, Context, Result};
use ashpd::{desktop::background, WindowIdentifier};
use async_trait::async_trait;
use gettextrs::gettext;
use gtk::{gio, glib, prelude::*};

/// Keeps either engine alive while its window is hidden. Dropping the session
/// withdraws the common notification and releases the application hold.
#[derive(Debug)]
pub struct BackgroundSession {
    application: glib::WeakRef<gtk::Application>,
    notification_id: String,
    _hold: gio::ApplicationHoldGuard,
}

impl BackgroundSession {
    pub fn start(window: &impl IsA<gtk::Window>, id: &str) -> Option<Self> {
        let window = window.as_ref();
        let application = window.application()?;
        let hold = application.hold();
        window.set_visible(false);
        let title = window.title().unwrap_or_else(|| gettext("Bastle").into());
        let notification = gio::Notification::new(&gettext("Web App Running in Background"));
        notification.set_body(Some(&format!(
            "{} {}",
            title,
            gettext("is still active. Use Stop to end its process.")
        )));
        notification
            .set_default_action_and_target_value("app.show-background", Some(&id.to_variant()));
        notification.add_button_with_target_value(
            &gettext("Stop"),
            "app.stop-background",
            Some(&id.to_variant()),
        );
        let notification_id = format!("background-{id}");
        application.send_notification(Some(&notification_id), &notification);
        glib::spawn_future_local(async move {
            if let Err(error) = set_status(&gettext("Bastle web apps are running")).await {
                eprintln!("Failed to update Background Portal status: {error:#}");
            }
        });
        Some(Self {
            application: application.downgrade(),
            notification_id,
            _hold: hold,
        })
    }
}

impl Drop for BackgroundSession {
    fn drop(&mut self) {
        if let Some(application) = self.application.upgrade() {
            application.withdraw_notification(&self.notification_id);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackgroundGrant {
    pub background: bool,
    pub autostart: bool,
}

#[async_trait(?Send)]
pub trait BackgroundBackend: Clone {
    async fn request_access(
        &self,
        parent: Option<&WindowIdentifier>,
        reason: &str,
        autostart: bool,
    ) -> Result<BackgroundGrant>;

    async fn update_autostart(
        &self,
        parent: Option<&WindowIdentifier>,
        enabled: bool,
    ) -> Result<bool>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PortalBackground;

#[async_trait(?Send)]
impl BackgroundBackend for PortalBackground {
    async fn request_access(
        &self,
        parent: Option<&WindowIdentifier>,
        reason: &str,
        autostart: bool,
    ) -> Result<BackgroundGrant> {
        let proxy = background::BackgroundProxy::new()
            .await
            .context("Background Portal is unavailable")?;
        let options = background::BackgroundRequestOptions::default()
            .set_reason(reason)
            .set_auto_start(autostart)
            .set_command(["bastle", "--background"])
            .set_dbus_activatable(false);
        let response = proxy
            .request_background(parent, options)
            .await
            .context("Background Portal request failed")?
            .response()
            .context("background access was cancelled or denied")?;
        ensure!(
            response.run_in_background(),
            "the desktop did not grant background access"
        );
        Ok(BackgroundGrant {
            background: response.run_in_background(),
            autostart: response.auto_start(),
        })
    }

    async fn update_autostart(
        &self,
        parent: Option<&WindowIdentifier>,
        enabled: bool,
    ) -> Result<bool> {
        let granted = self
            .request_access(
                parent,
                &gettext("Keep selected Bastle web applications running in the background"),
                enabled,
            )
            .await?
            .autostart;
        ensure!(
            granted == enabled,
            "the desktop did not apply the requested autostart state"
        );
        Ok(granted)
    }
}

pub async fn set_status(message: &str) -> Result<()> {
    let proxy = background::BackgroundProxy::new()
        .await
        .context("Background Portal is unavailable")?;
    let message = message.chars().take(96).collect::<String>();
    proxy
        .set_status(background::SetStatusOptions::default().set_message(&message))
        .await
        .context("Background Portal could not update the status")
}

pub async fn capability() -> Result<u32> {
    Ok(background::BackgroundProxy::new()
        .await
        .context("Background Portal is unavailable")?
        .version())
}
