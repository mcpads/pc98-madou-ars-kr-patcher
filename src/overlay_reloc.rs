//! Message relocation for decoded A.R.S overlays.
//!
//! Each drawn message is referenced by an inline `mov si, imm16` immediate (a
//! 16-bit logical offset into the decoded overlay; logical = decoded + load).
//! Korean almost always changes the byte length, so instead of growing a string
//! in place (which would shift every later pointer) the translated string is
//! appended to the end of the decoded overlay and only the `mov si` immediates
//! that point at the original message are rewritten to the new offset. The
//! original bytes stay put as dead space, so any pointer we did not rewrite
//! still renders the original text -- complete relocation per
//! `references/strategy/reinsertion.md` 1.2.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::overlay_text::find_dominant_renderer_message_refs;
use crate::sjis_marker::encode_sjis;

/// Syllable -> in-text Shift-JIS code map from a `kfont.json` glyph sheet.
/// These codes use the free prefixes the renderer fetch hook intercepts; every
/// other character falls back to ordinary Shift-JIS (spaces, punctuation, kanji
/// left untranslated).
pub struct SheetCodes {
    map: HashMap<char, [u8; 2]>,
}

impl SheetCodes {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        Self::from_json_str(&text).with_context(|| format!("parse {}", path.display()))
    }

    pub fn from_json_str(text: &str) -> Result<Self> {
        let value: Value = serde_json::from_str(text).context("parse glyph-sheet JSON")?;
        let glyphs = value
            .get("glyphs")
            .and_then(Value::as_array)
            .context("glyph sheet missing `glyphs` array")?;
        let mut map = HashMap::new();
        for (i, g) in glyphs.iter().enumerate() {
            let ch = g
                .get("char")
                .and_then(Value::as_str)
                .and_then(|s| s.chars().next())
                .with_context(|| format!("glyph {i} missing `char`"))?;
            let sjis = g
                .get("sjis")
                .and_then(Value::as_str)
                .with_context(|| format!("glyph {i} missing `sjis`"))?;
            if sjis.len() != 4 {
                bail!("glyph {i} sjis {sjis:?} must be exactly 4 hex digits");
            }
            let bytes = u16::from_str_radix(sjis, 16)
                .with_context(|| format!("glyph {i} bad sjis {sjis:?}"))?
                .to_be_bytes();
            // The renderer hook only intercepts the gaiji lead bytes EB-EF; a sheet
            // code outside that band would draw the CG font, not our glyph.
            if !(0xEB..=0xEF).contains(&bytes[0]) {
                bail!(
                    "glyph {i} sjis {sjis:?} lead 0x{:02X} is not a hook prefix (EB-EF)",
                    bytes[0]
                );
            }
            if map.insert(ch, bytes).is_some() {
                bail!("glyph {i} duplicate char {ch:?} in the sheet");
            }
        }
        Ok(Self { map })
    }

    /// Encode a line: sheet code for syllables in the sheet, plain Shift-JIS for
    /// everything else. Fails if a character is neither (no silent skip --
    /// `references/strategy/reinsertion.md` 6.6).
    pub fn encode_line(&self, text: &str) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        let mut remaining = text;
        while !remaining.is_empty() {
            if let Some(marker) = remaining.strip_prefix("{josa:") {
                let close = marker
                    .find('}')
                    .context("unterminated runtime particle marker")?;
                let form = &marker[..close];
                let marker_index = match form {
                    "을" | "를" => 0,
                    "이" | "가" => 1,
                    "은" | "는" => 2,
                    "와" | "과" => 3,
                    _ => bail!("unsupported runtime particle form {form:?}"),
                };
                out.extend_from_slice(&crate::josa::particle_marker_sjis(marker_index)?);
                remaining = &marker[close + 1..];
                continue;
            }

            let ch = remaining
                .chars()
                .next()
                .context("renderer input unexpectedly ended")?;
            if let Some(code) = self.map.get(&ch) {
                out.extend_from_slice(code);
            } else {
                let sjis = encode_sjis(&ch.to_string()).with_context(|| {
                    format!("character {ch:?} is not in the sheet or Shift-JIS")
                })?;
                // Reject any plain-SJIS char whose lead byte falls in the hook's
                // gaiji band EB-EF (e.g. CP932 compatibility ideographs 神 U+FA19,
                // fullwidth quotes ＂＇, small roman numerals ⅰ-ⅹ): the renderer
                // would convert it to JIS row 0x75-0x7E and the fetch hook would draw
                // a WRONG sheet glyph instead of the character. Silent, one-play
                // visible -- so it is a hard encode error, not a fallback.
                if matches!(sjis.first(), Some(0xEB..=0xF0)) {
                    bail!(
                        "character {ch:?} encodes to Shift-JIS lead 0x{:02X}, reserved by the \
                         renderer hook for Hangul glyphs or runtime particle markers; it would render as a wrong glyph. \
                         Normalize it (NFC/half-width) or remove it.",
                        sjis[0]
                    );
                }
                out.extend_from_slice(&sjis);
            }
            remaining = &remaining[ch.len_utf8()..];
        }
        Ok(out)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelocReport {
    pub target_decoded_offset: usize,
    pub target_logical_offset: usize,
    pub new_decoded_offset: usize,
    pub new_logical_offset: usize,
    /// Decoded offsets of the `mov si` instructions whose immediate was rewritten.
    pub rewritten_calls: Vec<usize>,
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Relocate the message whose original bytes are `target_jp` to `new_string`
/// (already encoded, no NUL), appended at the end of `decoded`. Rewrites every
/// renderer `mov si` immediate that points at the original message.
pub fn relocate_message(
    decoded: &mut Vec<u8>,
    load_offset: usize,
    target_jp: &[u8],
    new_string: &[u8],
) -> Result<RelocReport> {
    let target_decoded_offset =
        find_bytes(decoded, target_jp).context("target message bytes not found in overlay")?;
    if find_bytes(&decoded[target_decoded_offset + 1..], target_jp).is_some() {
        bail!("target message bytes are not unique; refine the anchor");
    }
    let target_logical_offset = target_decoded_offset + load_offset;

    let scan = find_dominant_renderer_message_refs(decoded, load_offset)
        .context("no renderer message references found")?;
    let rewritten_calls: Vec<usize> = scan
        .refs
        .iter()
        .filter(|r| r.string_decoded_offset == target_decoded_offset)
        .map(|r| r.call_decoded_offset)
        .collect();
    if rewritten_calls.is_empty() {
        bail!("no renderer call site points at the target message");
    }

    let new_decoded_offset = decoded.len();
    let new_logical_offset = new_decoded_offset + load_offset;
    // The whole string + NUL must fit below 0x10000, or `si` wraps mid-string.
    if new_logical_offset + new_string.len() + 1 > 0x10000 {
        bail!(
            "relocated offset 0x{new_logical_offset:X} (+{} bytes) exceeds the 16-bit pointer space; \
             the overlay segment is full",
            new_string.len() + 1
        );
    }
    decoded.extend_from_slice(new_string);
    decoded.push(0x00); // NUL terminator

    let imm = (new_logical_offset as u16).to_le_bytes();
    for &call in &rewritten_calls {
        // mov si, imm16: opcode at `call`, immediate at call+1..call+3.
        decoded[call + 1..call + 3].copy_from_slice(&imm);
    }

    Ok(RelocReport {
        target_decoded_offset,
        target_logical_offset,
        new_decoded_offset,
        new_logical_offset,
        rewritten_calls,
    })
}

/// Overwrite a message in place with a byte-length-preserving replacement
/// (counting the NUL terminator). No pointer is touched and the overlay does not
/// grow, so the decoded image stays exactly where the game expects it -- the
/// safe path when the Korean fits the original byte budget
/// (`references/strategy/reinsertion.md` 1.1).
pub fn patch_message_in_place(
    decoded: &mut [u8],
    target_jp: &[u8],
    new_string: &[u8],
) -> Result<usize> {
    if new_string.len() > target_jp.len() {
        bail!(
            "in-place replacement is {} bytes but the original message is {}; use relocation",
            new_string.len(),
            target_jp.len()
        );
    }
    let at = find_bytes(decoded, target_jp).context("target message bytes not found in overlay")?;
    if find_bytes(&decoded[at + 1..], target_jp).is_some() {
        bail!("target message bytes are not unique; refine the anchor");
    }
    decoded[at..at + new_string.len()].copy_from_slice(new_string);
    // Pad any remaining bytes of the original slot with NUL so the renderer
    // stops at the new (shorter or equal) string.
    for b in decoded[at + new_string.len()..at + target_jp.len()].iter_mut() {
        *b = 0;
    }
    Ok(at)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patches_message_in_place_same_length() {
        let mut decoded = vec![0u8; 0x40];
        let target = b"\x82\xa0\x82\xa2"; // あい
        decoded[0x10..0x14].copy_from_slice(target);
        let at = patch_message_in_place(&mut decoded, target, b"\xeb\xa7\xeb\xaf").unwrap();
        assert_eq!(at, 0x10);
        assert_eq!(&decoded[0x10..0x14], b"\xeb\xa7\xeb\xaf");
    }

    #[test]
    fn in_place_rejects_growth() {
        let mut decoded = vec![0u8; 0x40];
        decoded[0x10..0x12].copy_from_slice(b"\x82\xa0");
        let err =
            patch_message_in_place(&mut decoded, b"\x82\xa0", b"\xeb\xa7\xeb\xaf").unwrap_err();
        assert!(err.to_string().contains("use relocation"));
    }

    #[test]
    fn sheet_encodes_hook_codes_and_sjis_fallback() {
        let json = r#"{"glyphs":[{"char":"상","sjis":"eba7"},{"char":"군","sjis":"ebaf"}]}"#;
        let codes = SheetCodes::from_json_str(json).unwrap();
        // 상 -> sheet code, full-width space -> plain SJIS 0x8140, 군 -> sheet.
        let out = codes.encode_line("상　군").unwrap();
        assert_eq!(out, vec![0xEB, 0xA7, 0x81, 0x40, 0xEB, 0xAF]);
    }

    #[test]
    fn sheet_encodes_object_particle_marker_as_one_renderer_cell() {
        let json = r#"{"glyphs":[{"char":"외","sjis":"eba7"}]}"#;
        let codes = SheetCodes::from_json_str(json).unwrap();
        let out = codes.encode_line("{josa:을}　외").unwrap();
        assert_eq!(out, vec![0xF0, 0x40, 0x81, 0x40, 0xEB, 0xA7]);
        assert_eq!(codes.encode_line("{josa:이}").unwrap(), vec![0xF0, 0x41]);
        assert_eq!(codes.encode_line("{josa:은}").unwrap(), vec![0xF0, 0x42]);
        assert_eq!(codes.encode_line("{josa:와}").unwrap(), vec![0xF0, 0x43]);
        assert!(codes.encode_line("{josa:으로}").is_err());
    }

    fn write_renderer_call(buf: &mut [u8], at: usize, si: u16, target: usize) {
        // mov si,imm16; mov di,FFFF; mov al,01; call rel16
        buf[at] = 0xBE;
        buf[at + 1..at + 3].copy_from_slice(&si.to_le_bytes());
        buf[at + 3] = 0xBF;
        buf[at + 4] = 0xFF;
        buf[at + 5] = 0xFF;
        buf[at + 6] = 0xB0;
        buf[at + 7] = 0x01;
        buf[at + 8] = 0xE8;
        let rel = target as isize - (at as isize + 11);
        buf[at + 9..at + 11].copy_from_slice(&(rel as i16).to_le_bytes());
    }

    #[test]
    fn relocates_message_and_rewrites_pointer() {
        let load = 0x100usize;
        let mut decoded = vec![0u8; 0x400];
        // Renderer body the calls jump to (target of call rel16).
        let renderer = 0x300;
        decoded[renderer] = 0xC3; // ret, arbitrary
        // Target message at decoded 0x200 (logical 0x300).
        let msg = b"\x82\xa0\x82\xa2\x00"; // あい\0
        decoded[0x200..0x200 + msg.len()].copy_from_slice(msg);
        // Two call sites pointing at logical 0x300.
        write_renderer_call(&mut decoded, 0x10, 0x300, renderer);
        write_renderer_call(&mut decoded, 0x40, 0x300, renderer);

        let new = b"\xeb\xa7\xeb\xaf"; // two hook glyphs
        let rep = relocate_message(&mut decoded, load, &msg[..msg.len() - 1], new).unwrap();

        assert_eq!(rep.target_decoded_offset, 0x200);
        assert_eq!(rep.new_decoded_offset, 0x400);
        assert_eq!(rep.new_logical_offset, 0x500);
        assert_eq!(rep.rewritten_calls, vec![0x10, 0x40]);
        // Both pointers now read 0x500.
        assert_eq!(&decoded[0x11..0x13], &0x500u16.to_le_bytes());
        assert_eq!(&decoded[0x41..0x43], &0x500u16.to_le_bytes());
        // New string + NUL appended.
        assert_eq!(&decoded[0x400..0x405], b"\xeb\xa7\xeb\xaf\x00");
        // Original message left intact (dead).
        assert_eq!(&decoded[0x200..0x205], msg);
    }
}
