//! Small byte-level helpers for length-preserving SJIS marker patches.

use anyhow::{Result, bail};
use encoding_rs::SHIFT_JIS;

pub fn encode_sjis(s: &str) -> Result<Vec<u8>> {
    let (bytes, _, had_errors) = SHIFT_JIS.encode(s);
    if had_errors {
        bail!("string is not fully encodable as CP932/Shift_JIS: {s}");
    }
    Ok(bytes.into_owned())
}

pub fn replace_first_exact(buf: &mut [u8], from: &[u8], to: &[u8]) -> Result<usize> {
    if from.is_empty() {
        bail!("empty search pattern");
    }
    if from.len() != to.len() {
        bail!(
            "replacement must preserve byte length: {} != {}",
            from.len(),
            to.len()
        );
    }
    let Some(offset) = buf.windows(from.len()).position(|window| window == from) else {
        bail!("search pattern not found");
    };
    buf[offset..offset + to.len()].copy_from_slice(to);
    Ok(offset)
}

pub fn replace_all_exact(buf: &mut [u8], from: &[u8], to: &[u8]) -> Result<Vec<usize>> {
    if from.is_empty() {
        bail!("empty search pattern");
    }
    if from.len() != to.len() {
        bail!(
            "replacement must preserve byte length: {} != {}",
            from.len(),
            to.len()
        );
    }

    let mut offsets = Vec::new();
    let mut search_from = 0;
    while search_from + from.len() <= buf.len() {
        let Some(relative) = buf[search_from..]
            .windows(from.len())
            .position(|window| window == from)
        else {
            break;
        };
        let offset = search_from + relative;
        buf[offset..offset + to.len()].copy_from_slice(to);
        offsets.push(offset);
        search_from = offset + to.len();
    }
    if offsets.is_empty() {
        bail!("search pattern not found");
    }
    Ok(offsets)
}
