// SPDX-License-Identifier: GPL-3.0-only
//! Turning website icons into textures the interface can show, and asking the
//! portal for one the person picked themselves.

use anyhow::{anyhow, bail, Result};
use futures::{stream, StreamExt};
use gettextrs::gettext;
use gtk::{gdk, gio, prelude::*};
use isahc::prelude::*;
use url::Url;

use crate::domain::website::{self, WebsiteMeta, HTML_LIMIT, ICON_SIZE, IMAGE_LIMIT};

const MAX_ICON_REQUESTS: usize = 4;
const MAX_ICON_CANDIDATES: usize = 16;

#[derive(Debug)]
struct IconCandidate {
    bytes: Vec<u8>,
    source_area: u64,
}

pub async fn load_texture(buffer: Vec<u8>) -> Result<gdk::Texture> {
    let mut loader = glycin::Loader::new_vec(buffer);
    loader.sandbox_selector(glycin::SandboxSelector::Auto);
    let image = loader
        .load()
        .await
        .map_err(|error| anyhow!(error.to_string()))?;
    let mime = image.mime_type();
    let frame = if matches!(mime.as_str(), "image/svg+xml" | "image/svg+xml-compressed") {
        image
            .specific_frame(glycin::FrameRequest::new().scale(ICON_SIZE, ICON_SIZE))
            .await
    } else {
        image.next_frame().await
    }
    .map_err(|error| anyhow!(error.to_string()))?;
    Ok(frame.texture())
}

pub async fn normalize_icon(buffer: Vec<u8>) -> Result<Vec<u8>> {
    if buffer.len() > IMAGE_LIMIT {
        bail!("image exceeds the {IMAGE_LIMIT}-byte limit");
    }
    let texture = load_texture(buffer).await?;
    website::normalize_png_bytes(texture.save_to_png_bytes().as_ref())
}

pub async fn default_icon() -> Result<Vec<u8>> {
    normalize_icon(
        include_bytes!("../../data/icons/hicolor/scalable/apps/io.github.cheviiot.alcove.svg")
            .to_vec(),
    )
    .await
}

async fn fetch_icon(url: Url) -> Result<IconCandidate> {
    let mut response = website::http_client()?.get_async(url.to_string()).await?;
    if !response.status().is_success() {
        bail!("{url}: HTTP {}", response.status());
    }
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .map(str::to_ascii_lowercase);
    if content_type
        .as_deref()
        .is_some_and(|value| !value.starts_with("image/"))
    {
        bail!("{url}: response is not an image");
    }
    let bytes = website::read_bounded(&mut response, IMAGE_LIMIT).await?;
    let texture = load_texture(bytes).await?;
    let source_area =
        u64::from(texture.width().max(1) as u32) * u64::from(texture.height().max(1) as u32);
    let normalized = website::normalize_png_bytes(texture.save_to_png_bytes().as_ref())?;
    Ok(IconCandidate {
        bytes: normalized,
        source_area,
    })
}

pub async fn get_website_meta(url: Url) -> Result<WebsiteMeta> {
    let mut response = website::http_client()?.get_async(url.to_string()).await?;
    if !response.status().is_success() {
        bail!("{} returned HTTP {}", url, response.status());
    }
    let effective_url = response
        .effective_uri()
        .and_then(|uri| Url::parse(uri.to_string().as_str()).ok())
        .unwrap_or(url);
    let html = String::from_utf8_lossy(&website::read_bounded(&mut response, HTML_LIMIT).await?)
        .into_owned();
    let (title, urls) = website::parse_document(&html, &effective_url);
    let icon = stream::iter(urls)
        .take(MAX_ICON_CANDIDATES)
        .map(fetch_icon)
        .buffer_unordered(MAX_ICON_REQUESTS)
        .filter_map(|result| async move { result.ok() })
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .max_by_key(|candidate| candidate.source_area)
        .map(|candidate| candidate.bytes);
    Ok(WebsiteMeta { icon, title })
}

pub async fn icon_from_dialog(
    window: Option<&(impl IsA<gtk::Window> + Clone + 'static)>,
) -> Result<gio::File> {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some(&gettext("Images")));
    filter.add_mime_type("image/*");
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    gtk::FileDialog::builder()
        .accept_label(gettext("Select"))
        .modal(true)
        .title(gettext("App Icon"))
        .filters(&filters)
        .build()
        .open_future(window)
        .await
        .map_err(|error| {
            crate::system::portal::classify_file_dialog_error(
                gettext("Select application icon"),
                &error,
            )
            .map(anyhow::Error::from)
            .unwrap_or_else(|| {
                crate::system::portal::PortalOperationError::new(
                    crate::system::portal::PortalFailureKind::Cancelled,
                    gettext("Select application icon"),
                    "icon selection was cancelled",
                )
                .into()
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;

    #[test]
    fn oversized_image_is_rejected_before_decode() {
        let result = futures::executor::block_on(normalize_icon(vec![0; IMAGE_LIMIT + 1]));
        assert!(result.is_err());
    }

    #[test]
    fn non_square_svg_is_normalized() {
        let fixture = include_bytes!("../../tests/fixtures/non-square.svg").to_vec();
        let normalized = futures::executor::block_on(normalize_icon(fixture)).unwrap();
        let result = image::load_from_memory(&normalized).unwrap();
        assert_eq!(result.dimensions(), (ICON_SIZE, ICON_SIZE));
        assert_eq!(result.to_rgba8().get_pixel(0, 0).0[3], 0);
    }
}
