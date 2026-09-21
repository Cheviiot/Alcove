// SPDX-License-Identifier: GPL-3.0-only
//! Reading a website's own metadata: the page title and the icons it declares.
//! Nothing here touches GTK; turning bytes into a texture belongs to `ui::icons`.

use std::{collections::HashSet, io::Cursor, sync::OnceLock, time::Duration};

use anyhow::{anyhow, bail, Context, Result};
use futures::io::AsyncReadExt;
use image::{imageops, DynamicImage, ImageBuffer, ImageFormat, Rgba};
use isahc::{config, prelude::*, HttpClient, Response};
use scraper::{Html, Selector};
use url::Url;

pub(crate) const HTML_LIMIT: usize = 2 * 1024 * 1024;
pub(crate) const IMAGE_LIMIT: usize = 10 * 1024 * 1024;
pub(crate) const ICON_SIZE: u32 = 256;
const ICON_CONTENT_SIZE: u32 = 224;

#[derive(Debug, Default)]
pub struct WebsiteMeta {
    pub icon: Option<Vec<u8>>,
    pub title: Option<String>,
}

pub(crate) fn http_client() -> Result<&'static HttpClient> {
    static HTTP: OnceLock<Result<HttpClient, String>> = OnceLock::new();
    HTTP.get_or_init(|| {
        HttpClient::builder()
            .redirect_policy(config::RedirectPolicy::Limit(5))
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|error| error.to_string())
    })
    .as_ref()
    .map_err(|error| anyhow!(error.clone()))
}

fn icon_selector() -> &'static Selector {
    static SELECTOR: OnceLock<Selector> = OnceLock::new();
    SELECTOR.get_or_init(|| {
        Selector::parse(
            "link[rel='icon'], link[rel='shortcut icon'], link[rel^='apple-touch-icon']",
        )
        .expect("the built-in icon selector is valid")
    })
}

fn title_selector() -> &'static Selector {
    static SELECTOR: OnceLock<Selector> = OnceLock::new();
    SELECTOR.get_or_init(|| Selector::parse("title").expect("the built-in title selector is valid"))
}

pub(crate) async fn read_bounded(
    response: &mut Response<isahc::AsyncBody>,
    limit: usize,
) -> Result<Vec<u8>> {
    if response
        .headers()
        .get("content-length")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|length| length > limit)
    {
        bail!("response exceeds the {limit}-byte limit");
    }
    let mut output = Vec::with_capacity(limit.min(64 * 1024));
    response
        .body_mut()
        .take((limit + 1) as u64)
        .read_to_end(&mut output)
        .await?;
    if output.len() > limit {
        bail!("response exceeds the {limit}-byte limit");
    }
    Ok(output)
}

/// The page title and every icon address the document offers, including the
/// conventional `/favicon.*` locations the markup usually leaves out.
pub(crate) fn parse_document(html: &str, base: &Url) -> (Option<String>, HashSet<Url>) {
    let document = Html::parse_document(html);
    let title = document
        .select(title_selector())
        .next()
        .map(|element| element.text().collect::<String>())
        .map(|title| crate::domain::model::sanitize_title(&title))
        .filter(|title| !title.is_empty());

    let mut urls = document
        .select(icon_selector())
        .filter_map(|element| element.attr("href"))
        .filter_map(|path| base.join(path).ok())
        .collect::<HashSet<_>>();
    for path in ["/favicon.ico", "/favicon.png", "favicon.ico", "favicon.png"] {
        if let Ok(url) = base.join(path) {
            urls.insert(url);
        }
    }
    (title, urls)
}

/// Centres the rendered icon on a transparent square canvas of `ICON_SIZE`.
pub(crate) fn normalize_png_bytes(bytes: &[u8]) -> Result<Vec<u8>> {
    let source = image::load_from_memory(bytes).context("failed to decode rendered icon")?;
    let resized = source
        .thumbnail(ICON_CONTENT_SIZE, ICON_CONTENT_SIZE)
        .to_rgba8();
    let mut canvas = ImageBuffer::from_pixel(ICON_SIZE, ICON_SIZE, Rgba([0, 0, 0, 0]));
    let x = (ICON_SIZE - resized.width()) / 2;
    let y = (ICON_SIZE - resized.height()) / 2;
    imageops::overlay(&mut canvas, &resized, i64::from(x), i64::from(y));
    let mut output = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(canvas)
        .write_to(&mut output, ImageFormat::Png)
        .context("failed to encode normalized icon")?;
    Ok(output.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;

    #[test]
    fn non_square_image_gets_transparent_padding() {
        let image = DynamicImage::new_rgba8(400, 100);
        let mut input = Cursor::new(Vec::new());
        image.write_to(&mut input, ImageFormat::Png).unwrap();
        let normalized = normalize_png_bytes(input.get_ref()).unwrap();
        let result = image::load_from_memory(&normalized).unwrap();
        assert_eq!(result.dimensions(), (ICON_SIZE, ICON_SIZE));
        assert_eq!(result.to_rgba8().get_pixel(0, 0).0[3], 0);
    }

    #[test]
    fn favicon_locations_are_offered_even_without_markup() {
        let base = Url::parse("https://example.org/start").unwrap();
        let (title, urls) =
            parse_document("<html><head><title> Example </title></head></html>", &base);
        assert_eq!(title.as_deref(), Some("Example"));
        assert!(urls.contains(&Url::parse("https://example.org/favicon.ico").unwrap()));
    }
}
