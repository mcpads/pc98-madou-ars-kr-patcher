//! Text-reference scanning over decoded A.R.S overlays.

use encoding_rs::SHIFT_JIS;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RendererMessageRef {
    pub renderer_decoded_offset: usize,
    pub renderer_logical_offset: usize,
    pub call_decoded_offset: usize,
    pub call_logical_offset: usize,
    pub string_decoded_offset: usize,
    pub string_logical_offset: usize,
    pub mode: u8,
    pub raw: Vec<u8>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RendererMessageScan {
    pub renderer_decoded_offset: usize,
    pub renderer_logical_offset: usize,
    pub refs: Vec<RendererMessageRef>,
}

pub fn find_renderer_message_refs(
    decoded: &[u8],
    renderer_logical_offset: usize,
    load_offset: usize,
) -> Vec<RendererMessageRef> {
    find_renderer_message_scan(decoded, renderer_logical_offset, load_offset)
        .map(|scan| scan.refs)
        .unwrap_or_default()
}

pub fn find_renderer_message_scan(
    decoded: &[u8],
    renderer_logical_offset: usize,
    load_offset: usize,
) -> Option<RendererMessageScan> {
    let renderer_decoded_offset = renderer_logical_offset.checked_sub(load_offset)?;
    Some(RendererMessageScan {
        renderer_decoded_offset,
        renderer_logical_offset,
        refs: collect_renderer_message_refs(decoded, load_offset, Some(renderer_decoded_offset)),
    })
}

pub fn find_dominant_renderer_message_refs(
    decoded: &[u8],
    load_offset: usize,
) -> Option<RendererMessageScan> {
    let mut scans = collect_renderer_message_scans(decoded, load_offset);
    scans.sort_by(|left, right| {
        right.refs.len().cmp(&left.refs.len()).then_with(|| {
            left.renderer_decoded_offset
                .cmp(&right.renderer_decoded_offset)
        })
    });
    scans.into_iter().next()
}

/// Every inline `mov si` renderer message reference in the overlay, regardless
/// of which renderer call site draws it. This is the complete referenced-message
/// set -- one string is often referenced by several call sites (shared/duplicate
/// pointers), so `overlay_catalog` dedupes these into one entry per string.
pub fn collect_all_message_refs(decoded: &[u8], load_offset: usize) -> Vec<RendererMessageRef> {
    collect_renderer_message_refs(decoded, load_offset, None)
}

fn collect_renderer_message_scans(decoded: &[u8], load_offset: usize) -> Vec<RendererMessageScan> {
    use std::collections::BTreeMap;

    let refs = collect_renderer_message_refs(decoded, load_offset, None);
    let mut by_target = BTreeMap::<usize, Vec<RendererMessageRef>>::new();
    for entry in refs {
        by_target
            .entry(entry.renderer_decoded_offset)
            .or_default()
            .push(entry);
    }

    by_target
        .into_iter()
        .map(|(renderer_decoded_offset, refs)| RendererMessageScan {
            renderer_decoded_offset,
            renderer_logical_offset: renderer_decoded_offset + load_offset,
            refs,
        })
        .collect()
}

fn collect_renderer_message_refs(
    decoded: &[u8],
    load_offset: usize,
    renderer_decoded_filter: Option<usize>,
) -> Vec<RendererMessageRef> {
    let mut refs = Vec::new();
    let mut offset = 0usize;

    while offset + 11 <= decoded.len() {
        if let Some((string_logical_offset, mode, renderer_decoded_offset)) =
            match_immediate_renderer_call(decoded, offset)
            && renderer_decoded_filter.is_none_or(|target| target == renderer_decoded_offset)
            && let Some(string_decoded_offset) = string_logical_offset.checked_sub(load_offset)
            && let Some(raw) = read_nul_terminated(decoded, string_decoded_offset)
        {
            let (text, _, _) = SHIFT_JIS.decode(raw);
            refs.push(RendererMessageRef {
                renderer_decoded_offset,
                renderer_logical_offset: renderer_decoded_offset + load_offset,
                call_decoded_offset: offset,
                call_logical_offset: offset + load_offset,
                string_decoded_offset,
                string_logical_offset,
                mode,
                raw: raw.to_vec(),
                text: text.into_owned(),
            });
        }
        offset += 1;
    }

    refs
}

fn match_immediate_renderer_call(decoded: &[u8], offset: usize) -> Option<(usize, u8, usize)> {
    // mov si,imm16; mov di,FFFFh; mov al,imm8; call rel16
    if decoded[offset] != 0xBE
        || decoded[offset + 3] != 0xBF
        || decoded[offset + 4] != 0xFF
        || decoded[offset + 5] != 0xFF
        || decoded[offset + 6] != 0xB0
        || decoded[offset + 8] != 0xE8
    {
        return None;
    }

    let string_logical_offset =
        u16::from_le_bytes([decoded[offset + 1], decoded[offset + 2]]) as usize;
    let mode = decoded[offset + 7];
    let rel = i16::from_le_bytes([decoded[offset + 9], decoded[offset + 10]]) as isize;
    let next = offset as isize + 11;
    let target = next + rel;
    if target < 0 || target as usize >= decoded.len() {
        return None;
    }

    Some((string_logical_offset, mode, target as usize))
}

fn read_nul_terminated(decoded: &[u8], offset: usize) -> Option<&[u8]> {
    crate::renderer_control::message_bytes(decoded, offset)
}

#[cfg(test)]
mod tests {
    use super::{find_dominant_renderer_message_refs, find_renderer_message_refs};

    fn call_rel(from_call_offset: usize, target: usize) -> [u8; 2] {
        let next = from_call_offset + 3;
        let rel = target as isize - next as isize;
        (rel as i16).to_le_bytes()
    }

    fn write_renderer_call(
        decoded: &mut [u8],
        offset: usize,
        string_logical_offset: u16,
        mode: u8,
        target: usize,
    ) {
        decoded[offset] = 0xBE;
        decoded[offset + 1..offset + 3].copy_from_slice(&string_logical_offset.to_le_bytes());
        decoded[offset + 3] = 0xBF;
        decoded[offset + 4] = 0xFF;
        decoded[offset + 5] = 0xFF;
        decoded[offset + 6] = 0xB0;
        decoded[offset + 7] = mode;
        decoded[offset + 8] = 0xE8;
        decoded[offset + 9..offset + 11].copy_from_slice(&call_rel(offset + 8, target));
    }

    #[test]
    fn finds_immediate_renderer_string_refs() {
        let mut decoded = vec![0; 0x60];
        decoded[0x40..0x44].copy_from_slice(b"abc\0");
        write_renderer_call(&mut decoded, 0, 0x40, 0x02, 0x30);

        let refs = find_renderer_message_refs(&decoded, 0x30, 0);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].renderer_decoded_offset, 0x30);
        assert_eq!(refs[0].renderer_logical_offset, 0x30);
        assert_eq!(refs[0].call_decoded_offset, 0);
        assert_eq!(refs[0].call_logical_offset, 0);
        assert_eq!(refs[0].string_decoded_offset, 0x40);
        assert_eq!(refs[0].string_logical_offset, 0x40);
        assert_eq!(refs[0].mode, 0x02);
        assert_eq!(refs[0].text, "abc");
    }

    #[test]
    fn auto_selects_dominant_renderer_target() {
        let mut decoded = vec![0; 0xC0];
        decoded[0x80..0x84].copy_from_slice(b"one\0");
        decoded[0x88..0x8C].copy_from_slice(b"two\0");
        decoded[0x90..0x96].copy_from_slice(b"three\0");
        write_renderer_call(&mut decoded, 0x00, 0x180, 0x01, 0x30);
        write_renderer_call(&mut decoded, 0x10, 0x188, 0x02, 0x50);
        write_renderer_call(&mut decoded, 0x20, 0x190, 0x03, 0x50);

        let scan = find_dominant_renderer_message_refs(&decoded, 0x100).unwrap();
        assert_eq!(scan.renderer_decoded_offset, 0x50);
        assert_eq!(scan.renderer_logical_offset, 0x150);
        assert_eq!(scan.refs.len(), 2);
        assert_eq!(scan.refs[0].text, "two");
        assert_eq!(scan.refs[1].text, "three");
    }
}
