// SPDX-License-Identifier: GPL-3.0-only
//! What a website's `window.open` is allowed to do: open another window of
//! this application, hand the address to the desktop, or nothing at all.

use adw::prelude::*;
use gettextrs::gettext;
use glib::clone;
use gtk::glib;
use webkit::{prelude::*, NavigationAction, PolicyDecision, WebView};

use super::window::AppWindow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PopupTarget {
    InApp,
    External,
    Blocked,
}

fn classify_popup_target(uri: Option<&str>) -> PopupTarget {
    let Some(uri) = uri else {
        return PopupTarget::InApp;
    };
    let Ok(uri) = url::Url::parse(uri) else {
        return PopupTarget::Blocked;
    };
    match uri.scheme() {
        "http" | "https" | "about" | "blob" => PopupTarget::InApp,
        "mailto" | "tel" => PopupTarget::External,
        _ => PopupTarget::Blocked,
    }
}

fn navigation_uri(decision: &PolicyDecision) -> Option<glib::GString> {
    decision
        .clone()
        .downcast::<webkit::NavigationPolicyDecision>()
        .ok()
        .and_then(|policy| policy.navigation_action())
        .and_then(|action| action.request())
        .and_then(|request| request.uri())
}

/// Hands `uri` to the desktop portal, the only route out of the sandbox, and
/// reports a refusal instead of dropping it. A person dismissing the chooser
/// is not a failure, so that case stays silent.
pub(super) fn launch_external_uri(window: &AppWindow, uri: &str) {
    let window = window.clone();
    let uri = uri.to_owned();
    glib::spawn_future_local(async move {
        if let Err(error) = gtk::UriLauncher::new(&uri)
            .launch_future(Some(&window))
            .await
        {
            if !error.matches(gtk::DialogError::Cancelled) {
                window.toast(&format!(
                    "{}: {error}",
                    gettext("The link could not be opened")
                ));
            }
        }
    });
}

pub(super) fn handle_new_window_policy(
    window: &AppWindow,
    decision: &PolicyDecision,
) -> Option<bool> {
    let uri = navigation_uri(decision);
    match classify_popup_target(uri.as_deref()) {
        PopupTarget::InApp => None,
        PopupTarget::External => {
            if let Some(uri) = uri {
                launch_external_uri(window, uri.as_str());
            }
            decision.ignore();
            Some(true)
        }
        PopupTarget::Blocked => {
            decision.ignore();
            Some(true)
        }
    }
}

pub(super) fn create_popup(
    owner: &AppWindow,
    parent: &gtk::Window,
    parent_view: &WebView,
    action: &NavigationAction,
) -> Option<gtk::Widget> {
    let uri = action.request().and_then(|request| request.uri());
    match classify_popup_target(uri.as_deref()) {
        PopupTarget::InApp => {}
        PopupTarget::External => {
            if let Some(uri) = uri {
                launch_external_uri(owner, uri.as_str());
            }
            return None;
        }
        PopupTarget::Blocked => return None,
    }

    let application = parent.application()?;
    let popup = adw::ApplicationWindow::new(&application);
    popup.set_default_size(720, 640);
    popup.set_title(Some(&gettext("Web App Window")));
    popup.set_transient_for(Some(parent));
    popup.set_destroy_with_parent(true);

    let settings = webkit::prelude::WebViewExt::settings(parent_view)?;
    let content_manager = parent_view.user_content_manager().unwrap_or_default();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    let popup_view = WebView::builder()
        .related_view(parent_view)
        .settings(&settings)
        .user_content_manager(&content_manager)
        .build();
    toolbar.set_content(Some(&popup_view));
    popup.set_content(Some(&toolbar));

    popup_view.connect_title_notify(clone!(
        #[weak]
        popup,
        move |view| {
            popup.set_title(view.title().as_deref().or(Some(&gettext("Web App Window"))));
        }
    ));
    popup_view.connect_ready_to_show(clone!(
        #[weak]
        popup,
        move |view| {
            if let Some(properties) = view.window_properties() {
                let geometry = properties.geometry();
                if geometry.width() > 0 && geometry.height() > 0 {
                    popup.set_default_size(
                        geometry.width().clamp(320, 1600),
                        geometry.height().clamp(240, 1200),
                    );
                }
                popup.set_resizable(properties.is_resizable());
                if properties.is_fullscreen() {
                    popup.fullscreen();
                }
            }
            popup.present();
        }
    ));
    popup_view.connect_close(clone!(
        #[weak]
        popup,
        move |_| popup.close()
    ));
    popup_view.connect_permission_request(clone!(
        #[weak]
        owner,
        #[upgrade_or]
        false,
        move |view, request| owner.handle_permission_request(view, request)
    ));
    popup_view.connect_show_notification(clone!(
        #[weak]
        owner,
        #[upgrade_or]
        false,
        move |_, notification| owner.show_web_notification(notification)
    ));
    popup_view.connect_enter_fullscreen(clone!(
        #[weak]
        popup,
        #[upgrade_or]
        false,
        move |_| {
            popup.fullscreen();
            true
        }
    ));
    popup_view.connect_leave_fullscreen(clone!(
        #[weak]
        popup,
        #[upgrade_or]
        false,
        move |_| {
            popup.unfullscreen();
            true
        }
    ));
    popup_view.connect_decide_policy(clone!(
        #[weak]
        owner,
        #[upgrade_or]
        false,
        move |view, decision, kind| owner.handle_policy_decision(view, decision, kind)
    ));
    popup_view.connect_create(clone!(
        #[weak]
        owner,
        #[weak]
        popup,
        #[upgrade_or]
        None,
        move |view, action| create_popup(&owner, popup.upcast_ref(), view, action)
    ));

    Some(popup_view.upcast())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popup_targets_keep_web_content_in_app() {
        assert_eq!(classify_popup_target(None), PopupTarget::InApp);
        assert_eq!(
            classify_popup_target(Some("https://login.example.org/oauth")),
            PopupTarget::InApp
        );
        assert_eq!(
            classify_popup_target(Some("about:blank")),
            PopupTarget::InApp
        );
        assert_eq!(
            classify_popup_target(Some("mailto:help@example.org")),
            PopupTarget::External
        );
        assert_eq!(
            classify_popup_target(Some("javascript:alert(1)")),
            PopupTarget::Blocked
        );
        assert_eq!(
            classify_popup_target(Some("not a uri")),
            PopupTarget::Blocked
        );
    }
}
