// SPDX-License-Identifier: GPL-3.0-only
use anyhow::{bail, ensure, Result};
use serde_json::{json, Value};

pub const VERSION: u64 = 4;
pub const MAX_DIMENSION: u32 = 4096;
pub const MAX_FRAME_BYTES: usize = MAX_DIMENSION as usize * MAX_DIMENSION as usize * 4 + 16;

pub fn command(name: &str, parameters: Value) -> Value {
    let mut value = parameters;
    value["protocol"] = json!(VERSION);
    value["command"] = json!(name);
    value
}

pub fn frame_dimensions(bytes: &[u8]) -> Result<(i32, i32)> {
    ensure!(bytes.len() >= 16, "truncated frame header");
    let words: Vec<u32> = bytes[..16]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|word| u32::from_le_bytes(*word))
        .collect();
    ensure!(
        words[0] == 0x42535446 && u64::from(words[1]) == VERSION,
        "invalid frame protocol"
    );
    let (width, height) = (words[2], words[3]);
    ensure!(
        (1..=MAX_DIMENSION).contains(&width) && (1..=MAX_DIMENSION).contains(&height),
        "invalid frame size"
    );
    ensure!(
        bytes.len() == 16 + width as usize * height as usize * 4,
        "invalid frame length"
    );
    Ok((width as i32, height as i32))
}

pub fn validate_url(input: &str) -> Result<()> {
    let url = url::Url::parse(input)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        bail!("Chromium accepts HTTP(S) URLs without credentials only");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(w: u32, h: u32) -> Vec<u8> {
        [0x42535446u32, VERSION as u32, w, h]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect()
    }
    #[test]
    fn rejects_truncated_overflowing_and_wrong_version_frames() {
        assert!(frame_dimensions(&[]).is_err());
        assert!(frame_dimensions(&frame(u32::MAX, 1)).is_err());
        assert!(frame_dimensions(&frame(0, 1)).is_err());
        let mut valid = frame(2, 3);
        valid.resize(40, 0);
        assert_eq!(frame_dimensions(&valid).unwrap(), (2, 3));
        valid.push(0);
        assert!(frame_dimensions(&valid).is_err());
        valid.pop();
        valid[4] = 99;
        assert!(frame_dimensions(&valid).is_err());
    }
    #[test]
    fn probe_does_not_accept_local_files_or_credentials() {
        assert!(validate_url("https://example.org/").is_ok());
        for invalid in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "https://user:pass@example.org/",
        ] {
            assert!(validate_url(invalid).is_err());
        }
    }
}
