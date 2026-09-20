// SPDX-License-Identifier: GPL-3.0-only
use anyhow::{ensure, Context, Result};
use gtk::{gdk, glib};
use rustix::net::{recvmsg, RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, ReturnFlags};
use serde::Deserialize;
use std::{io::IoSliceMut, mem::MaybeUninit, os::fd::AsRawFd, os::unix::net::UnixDatagram};

#[derive(Deserialize)]
struct Frame {
    protocol: u64,
    view: u64,
    surface: Surface,
    generation: u64,
    width: u32,
    height: u32,
    fourcc: u32,
    modifier: u64,
    stride: u32,
    offset: u32,
}

#[derive(Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Surface {
    View,
    Popup,
}

#[derive(Debug)]
pub struct Texture {
    pub view: u64,
    pub surface: Surface,
    pub generation: u64,
    pub texture: gdk::Texture,
}

/// Each packet contains one immutable client-owned GPU allocation, never a
/// descriptor from CEF's reusable source pool. MSG_CMSG_CLOEXEC prevents leaks
/// to subprocesses; the GDK release callback owns the received descriptor.
pub fn receive(socket: &UnixDatagram) -> Result<Option<Texture>> {
    let mut bytes = [0u8; 2048];
    let mut ancillary = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(4))];
    let mut control = RecvAncillaryBuffer::new(&mut ancillary);
    let received = match recvmsg(
        socket,
        &mut [IoSliceMut::new(&mut bytes)],
        &mut control,
        RecvFlags::DONTWAIT | RecvFlags::CMSG_CLOEXEC,
    ) {
        Ok(received) => received,
        Err(rustix::io::Errno::AGAIN) => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut descriptors = Vec::new();
    for message in control.drain() {
        if let RecvAncillaryMessage::ScmRights(fds) = message {
            descriptors.extend(fds);
        }
    }
    ensure!(
        !received
            .flags
            .intersects(ReturnFlags::TRUNC | ReturnFlags::CTRUNC),
        "truncated GPU packet"
    );
    ensure!(descriptors.len() == 1, "expected one GPU descriptor");
    let frame: Frame = serde_json::from_slice(&bytes[..received.bytes])?;
    ensure!(
        frame.protocol == super::protocol::VERSION,
        "GPU protocol mismatch"
    );
    ensure!(
        frame.view > 0
            && ((frame.surface == Surface::View && frame.generation == 0)
                || (frame.surface == Surface::Popup && frame.generation > 0)),
        "invalid GPU surface generation"
    );
    ensure!(
        (1..=super::protocol::MAX_DIMENSION).contains(&frame.width)
            && (1..=super::protocol::MAX_DIMENSION).contains(&frame.height),
        "invalid GPU dimensions"
    );
    ensure!(
        frame.fourcc == u32::from_le_bytes(*b"AR24") && frame.stride >= frame.width * 4,
        "unsupported GPU layout"
    );
    let descriptor = descriptors.pop().context("GPU descriptor")?;
    let builder = gdk::DmabufTextureBuilder::new()
        .set_display(&gdk::Display::default().context("display")?)
        .set_width(frame.width)
        .set_height(frame.height)
        .set_fourcc(frame.fourcc)
        .set_modifier(frame.modifier)
        .set_n_planes(1)
        .set_stride(0, frame.stride)
        .set_offset(0, frame.offset)
        .set_premultiplied(true);
    // SAFETY: descriptor remains owned by this stack until a successful build,
    // then the release closure keeps it alive for the complete texture lifetime.
    // glFinish in the producer completed the copy before sending this buffer;
    // the producer never writes to it again.
    let builder = unsafe { builder.set_fd(0, descriptor.as_raw_fd()) };
    // gtk-rs's build_with_release_func leaks its closure on GDK build failure.
    // Use the C API so that both success and failure have explicit ownership.
    use glib::translate::*;
    type Descriptor = std::os::fd::OwnedFd;
    unsafe extern "C" fn release(data: glib::ffi::gpointer) {
        unsafe {
            drop(Box::from_raw(data.cast::<Descriptor>()));
        }
    }
    let data = Box::into_raw(Box::new(descriptor));
    let mut error = std::ptr::null_mut();
    let texture = unsafe {
        gdk::ffi::gdk_dmabuf_texture_builder_build(
            builder.to_glib_none().0,
            Some(release),
            data.cast(),
            &mut error,
        )
    };
    if texture.is_null() {
        unsafe {
            release(data.cast());
        }
        if !error.is_null() {
            let error: glib::Error = unsafe { from_glib_full(error) };
            return Err(error.into());
        }
        anyhow::bail!("GDK refused DMA-BUF texture");
    }
    Ok(Some(Texture {
        view: frame.view,
        surface: frame.surface,
        generation: frame.generation,
        texture: unsafe { from_glib_full(texture) },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustix::net::{sendmsg, SendAncillaryBuffer, SendAncillaryMessage, SendFlags};
    use std::{fs::File, io::IoSlice, os::fd::AsFd};

    #[test]
    fn malformed_gpu_packets_fail_before_touching_gtk() {
        let (sender, receiver) = UnixDatagram::pair().unwrap();
        assert!(receive(&receiver).unwrap().is_none());
        sender.send(b"{}").unwrap();
        assert!(receive(&receiver)
            .unwrap_err()
            .to_string()
            .contains("descriptor"));
        let descriptor = File::open("/dev/null").unwrap();
        let fds = [descriptor.as_fd()];
        let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
        let mut control = SendAncillaryBuffer::new(&mut space);
        assert!(control.push(SendAncillaryMessage::ScmRights(&fds)));
        let invalid = br#"{"protocol":999,"view":1,"surface":"view","generation":0,"width":10,"height":10,"fourcc":875713089,"modifier":0,"stride":40,"offset":0}"#;
        sendmsg(
            &sender,
            &[IoSlice::new(invalid)],
            &mut control,
            SendFlags::empty(),
        )
        .unwrap();
        assert!(receive(&receiver)
            .unwrap_err()
            .to_string()
            .contains("protocol mismatch"));
        sender.send(&[0u8; 4096]).unwrap();
        assert!(receive(&receiver)
            .unwrap_err()
            .to_string()
            .contains("truncated"));
    }
}
