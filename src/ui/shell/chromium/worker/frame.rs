// SPDX-License-Identifier: GPL-3.0-only
//! Decoding a frame the worker wrote to a file, for the software path.

use adw::prelude::*;
use anyhow::Result;
use gtk::{gdk, glib};
use std::{fs::File, io::Read};

use crate::engines::chromium::protocol;

pub fn cpu_texture(path: &std::path::Path) -> Result<gdk::Texture> {
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take(protocol::MAX_FRAME_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    let (w, h) = protocol::frame_dimensions(&bytes)?;
    let data = glib::Bytes::from_owned(bytes[16..].to_vec());
    Ok(gdk::MemoryTexture::new(
        w,
        h,
        gdk::MemoryFormat::B8g8r8a8Premultiplied,
        &data,
        w as usize * 4,
    )
    .upcast())
}
