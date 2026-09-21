// SPDX-License-Identifier: GPL-3.0-only
//! Naming what a website asked for and which origin asked, so the window can
//! answer from the stored policy or ask the person.

use std::str::FromStr;

use anyhow::{bail, Context, Result};
use gettextrs::gettext;
use webkit::{prelude::*, PermissionRequest, PolicyDecision, WebView};

use crate::domain::policy::{Origin, PermissionKind};

pub(super) fn response_requires_download(view: &WebView, decision: &PolicyDecision) -> bool {
    decision
        .clone()
        .downcast::<webkit::ResponsePolicyDecision>()
        .ok()
        .and_then(|policy| policy.response())
        .and_then(|response| response.http_headers())
        .and_then(|headers| headers.one("Content-Type"))
        .is_some_and(|mime| !view.can_show_mime_type(mime.as_str()))
}

pub(super) fn permission_request_details(
    request: &PermissionRequest,
) -> Result<(Vec<PermissionKind>, String)> {
    if let Ok(media) = request
        .clone()
        .downcast::<webkit::UserMediaPermissionRequest>()
    {
        if webkit::functions::user_media_permission_is_for_display_device(&media) {
            bail!("{}", gettext("Screen sharing is not available"));
        }
        let mut kinds = Vec::new();
        if media.is_for_video_device() {
            kinds.push(PermissionKind::Camera);
        }
        if media.is_for_audio_device() {
            kinds.push(PermissionKind::Microphone);
        }
        let description = match kinds.as_slice() {
            [PermissionKind::Camera] => gettext("Allow this website to use the camera?"),
            [PermissionKind::Microphone] => gettext("Allow this website to use the microphone?"),
            [PermissionKind::Camera, PermissionKind::Microphone] => {
                gettext("Allow this website to use the camera and microphone?")
            }
            _ => bail!("{}", gettext("Unknown media device request")),
        };
        return Ok((kinds, description));
    }
    if request.is::<webkit::GeolocationPermissionRequest>() {
        return Ok((
            vec![PermissionKind::Geolocation],
            gettext("Allow this website to access your location?"),
        ));
    }
    if request.is::<webkit::NotificationPermissionRequest>() {
        return Ok((
            vec![PermissionKind::Notifications],
            gettext("Allow this website to send notifications?"),
        ));
    }
    if request.is::<webkit::ClipboardPermissionRequest>() {
        return Ok((
            vec![PermissionKind::Clipboard],
            gettext("Allow this website to read the clipboard?"),
        ));
    }
    if request.is::<webkit::PointerLockPermissionRequest>() {
        return Ok((
            vec![PermissionKind::PointerLock],
            gettext("Allow this website to lock the pointer?"),
        ));
    }
    if request.is::<webkit::WebsiteDataAccessPermissionRequest>() {
        return Ok((
            vec![PermissionKind::ThirdPartyStorage],
            gettext("Allow this website to access third-party storage?"),
        ));
    }
    bail!(
        "{}",
        gettext("This WebKit permission type is not supported")
    )
}

pub(super) fn permission_origin(view: &WebView, request: &PermissionRequest) -> Result<Origin> {
    let current_uri = view.uri().context("the web view has no current URL")?;
    let current_url = url::Url::parse(current_uri.as_str())?;

    if let Ok(storage) = request
        .clone()
        .downcast::<webkit::WebsiteDataAccessPermissionRequest>()
    {
        let domain = storage
            .requesting_domain()
            .context("the storage request has no requesting domain")?;
        let origin = format!("{}://{}", current_url.scheme(), domain);
        return Origin::from_str(&origin);
    }

    Origin::from_url(&current_url)
}
