//! Translation-ready message catalog over a decoded A.R.S overlay.
//!
//! `overlay_text` finds every inline `mov si` renderer message reference, but a
//! single string is often referenced by several call sites (shared/duplicate
//! pointers). This module dedupes those references into one entry per unique
//! string -- the unit a translator works on -- and records the in-place byte
//! budget and every call site whose `mov si` immediate must be rewritten when
//! the message is relocated (`references/strategy/reinsertion.md` 1.2).
//!
//! Completeness (`references/strategy/text-extraction.md` 3.5.2, 6.3): the
//! `mov si` scan cannot see strings that no such call references -- battle
//! fragments assembled at runtime, or any other draw convention. `scan_sjis_runs`
//! heuristically finds every Shift-JIS run so `uncovered_sjis_runs` can surface
//! the Japanese text the catalog missed, turning the "did we find everything"
//! question into a checkable list instead of an assumption.

use std::collections::BTreeMap;

use encoding_rs::SHIFT_JIS;

use crate::overlay_text::collect_all_message_refs;

/// One unique drawn string and everything reinsertion needs about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogedMessage {
    pub string_decoded_offset: usize,
    pub string_logical_offset: usize,
    /// In-place slot size: the string bytes plus the NUL terminator. A Korean
    /// replacement fits in place when its encoded length (with NUL) is <= this.
    pub byte_budget: usize,
    pub raw: Vec<u8>,
    pub text: String,
    /// Decoded offsets of every `mov si` whose immediate points at this string;
    /// all of them are rewritten together when the message is relocated.
    pub call_sites: Vec<usize>,
    /// Distinct renderer draw-mode bytes (`mov al, imm8`) seen at the call sites.
    pub modes: Vec<u8>,
}

/// Dedupe every referenced message by its string offset. Sorted by offset, which
/// is also address order in the decoded overlay.
pub fn catalog_messages(decoded: &[u8], load_offset: usize) -> Vec<CatalogedMessage> {
    let mut by_string = BTreeMap::<usize, CatalogedMessage>::new();
    for r in collect_all_message_refs(decoded, load_offset) {
        let entry = by_string
            .entry(r.string_decoded_offset)
            .or_insert_with(|| CatalogedMessage {
                string_decoded_offset: r.string_decoded_offset,
                string_logical_offset: r.string_logical_offset,
                byte_budget: r.raw.len() + 1,
                raw: r.raw.clone(),
                text: r.text.clone(),
                call_sites: Vec::new(),
                modes: Vec::new(),
            });
        entry.call_sites.push(r.call_decoded_offset);
        if !entry.modes.contains(&r.mode) {
            entry.modes.push(r.mode);
        }
    }
    let mut out: Vec<CatalogedMessage> = by_string.into_values().collect();
    for message in &mut out {
        message.call_sites.sort_unstable();
        message.modes.sort_unstable();
    }
    out
}

/// Decode the NUL-terminated Shift-JIS string starting at `offset`, if the
/// offset is in range and a terminator follows.
pub fn decode_string_at(decoded: &[u8], offset: usize) -> Option<String> {
    let raw = crate::renderer_control::message_bytes(decoded, offset)?;
    Some(SHIFT_JIS.decode(raw).0.into_owned())
}

/// A maximal Shift-JIS run found by the completeness scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SjisRun {
    pub decoded_offset: usize,
    pub raw: Vec<u8>,
    pub text: String,
    /// Hiragana/katakana characters in the run. The decoded overlay interleaves
    /// strings with renderer code, and code bytes form valid SJIS kanji pairs by
    /// chance (`references/strategy/text-extraction.md` 3.5.1); real dialogue
    /// carries kana, so `kana_count > 0` separates likely text from code noise.
    pub kana_count: usize,
}

fn is_sjis_lead(b: u8) -> bool {
    (0x81..=0x9F).contains(&b) || (0xE0..=0xFC).contains(&b)
}

fn is_sjis_trail(b: u8) -> bool {
    (0x40..=0x7E).contains(&b) || (0x80..=0xFC).contains(&b)
}

fn is_sjis_kana(lead: u8, trail: u8) -> bool {
    // Shift-JIS hiragana 0x829F..=0x82F1, katakana 0x8340..=0x8396.
    (lead == 0x82 && (0x9F..=0xF1).contains(&trail))
        || (lead == 0x83 && (0x40..=0x96).contains(&trail))
}

/// Heuristic scan for maximal runs of Shift-JIS text: sequences of ASCII
/// printables, newlines, and valid double-byte SJIS pairs, kept only when they
/// hold at least `min_double` double-byte (i.e. Japanese) characters so ASCII
/// noise and stray pairs are filtered out. Each run also carries its kana count
/// so callers can separate real text from code that decodes as kanji by chance.
/// Oriented toward false-negative discovery, not precise extraction.
pub fn scan_sjis_runs(decoded: &[u8], min_double: usize) -> Vec<SjisRun> {
    let mut runs = Vec::new();
    let mut i = 0usize;
    while i < decoded.len() {
        let start = i;
        let mut doubles = 0usize;
        let mut kana = 0usize;
        while i < decoded.len() {
            let b = decoded[i];
            if is_sjis_lead(b) && i + 1 < decoded.len() && is_sjis_trail(decoded[i + 1]) {
                if is_sjis_kana(b, decoded[i + 1]) {
                    kana += 1;
                }
                doubles += 1;
                i += 2;
            } else if (0x20..=0x7E).contains(&b) || b == 0x0A {
                i += 1;
            } else {
                break;
            }
        }
        if doubles >= min_double {
            let raw = decoded[start..i].to_vec();
            let text = SHIFT_JIS.decode(&raw).0.into_owned();
            runs.push(SjisRun {
                decoded_offset: start,
                raw,
                text,
                kana_count: kana,
            });
        }
        if i == start {
            i += 1;
        }
    }
    runs
}

/// Offsets that begin a NUL-delimited Shift-JIS string carrying at least
/// `min_kana` kana. A message is stored NUL-terminated, so its true start is the
/// byte after a NUL (or offset 0); the run scan can instead start mid-message at
/// an internal `…`/control byte, so a table pointing at the true start is missed.
/// This gives those true starts directly.
pub fn nul_delimited_kana_starts(
    decoded: &[u8],
    min_kana: usize,
) -> std::collections::HashSet<usize> {
    let mut starts = std::collections::HashSet::new();
    let mut i = 0usize;
    while i < decoded.len() {
        let follows_real_nul = i > 0
            && decoded[i - 1] == 0
            && !crate::renderer_control::is_control_parameter_at(decoded, i - 1);
        if i == 0 || follows_real_nul {
            let mut j = i;
            let mut kana = 0usize;
            while j < decoded.len() {
                let b = decoded[j];
                if is_sjis_lead(b) && j + 1 < decoded.len() && is_sjis_trail(decoded[j + 1]) {
                    if is_sjis_kana(b, decoded[j + 1]) {
                        kana += 1;
                    }
                    j += 2;
                } else if (0x20..=0x7E).contains(&b) || b == 0x0A {
                    j += 1;
                } else {
                    break;
                }
            }
            if kana >= min_kana && j > i {
                starts.insert(i);
            }
        }
        i += 1;
    }
    starts
}

/// Shift-JIS runs whose byte span overlaps no cataloged message -- Japanese text
/// the `mov si` catalog did not reach, so the false-negative candidates to
/// investigate (runtime-assembled fragments, other draw conventions).
pub fn uncovered_sjis_runs(catalog: &[CatalogedMessage], runs: &[SjisRun]) -> Vec<SjisRun> {
    let mut spans: Vec<(usize, usize)> = catalog
        .iter()
        .map(|m| {
            (
                m.string_decoded_offset,
                m.string_decoded_offset + m.raw.len(),
            )
        })
        .collect();
    spans.sort_unstable();
    runs.iter()
        .filter(|run| {
            let run_start = run.decoded_offset;
            let run_end = run_start + run.raw.len();
            !spans
                .iter()
                .any(|&(span_start, span_end)| run_start < span_end && span_start < run_end)
        })
        .cloned()
        .collect()
}

/// Sanity check for the catalog: no two messages' `[offset, offset+len)` spans
/// overlap (distinct NUL-terminated strings never share bytes). Returns the
/// first overlapping pair, if any.
pub fn first_overlapping_pair(catalog: &[CatalogedMessage]) -> Option<(usize, usize)> {
    let mut spans: Vec<(usize, usize)> = catalog
        .iter()
        .map(|m| {
            (
                m.string_decoded_offset,
                m.string_decoded_offset + m.raw.len(),
            )
        })
        .collect();
    spans.sort_unstable();
    for pair in spans.windows(2) {
        let (_, prev_end) = pair[0];
        let (next_start, _) = pair[1];
        if next_start < prev_end {
            return Some((pair[0].0, pair[1].0));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // mov si,imm16; mov di,FFFF; mov al,mode; call rel16 (target = renderer body).
    fn write_call(buf: &mut [u8], at: usize, si: u16, mode: u8, target: usize) {
        buf[at] = 0xBE;
        buf[at + 1..at + 3].copy_from_slice(&si.to_le_bytes());
        buf[at + 3] = 0xBF;
        buf[at + 4] = 0xFF;
        buf[at + 5] = 0xFF;
        buf[at + 6] = 0xB0;
        buf[at + 7] = mode;
        buf[at + 8] = 0xE8;
        let rel = target as isize - (at as isize + 11);
        buf[at + 9..at + 11].copy_from_slice(&(rel as i16).to_le_bytes());
    }

    #[test]
    fn dedupes_shared_pointer_into_one_message_with_all_call_sites() {
        let load = 0x100usize;
        let mut decoded = vec![0u8; 0x400];
        decoded[0x300] = 0xC3; // renderer body (ret)
        // String at decoded 0x200 (logical 0x300): "hi\0".
        decoded[0x200..0x203].copy_from_slice(b"hi\0");
        // Two call sites, different modes, pointing at the same string.
        write_call(&mut decoded, 0x10, 0x300, 0x01, 0x300);
        write_call(&mut decoded, 0x40, 0x300, 0x02, 0x300);

        let catalog = catalog_messages(&decoded, load);
        assert_eq!(catalog.len(), 1);
        let m = &catalog[0];
        assert_eq!(m.string_decoded_offset, 0x200);
        assert_eq!(m.string_logical_offset, 0x300);
        assert_eq!(m.text, "hi");
        assert_eq!(m.byte_budget, 3); // "hi" + NUL
        assert_eq!(m.call_sites, vec![0x10, 0x40]);
        assert_eq!(m.modes, vec![0x01, 0x02]);
        assert!(first_overlapping_pair(&catalog).is_none());
    }

    #[test]
    fn scan_finds_sjis_run_and_cross_check_flags_the_uncovered_one() {
        let load = 0x100usize;
        let mut decoded = vec![0u8; 0x400];
        decoded[0x300] = 0xC3;
        // Referenced message at 0x200: full-width あ (0x82 0xA0) + NUL.
        decoded[0x200..0x203].copy_from_slice(&[0x82, 0xA0, 0x00]);
        write_call(&mut decoded, 0x10, 0x300, 0x01, 0x300);
        // Unreferenced SJIS run at 0x260: full-width いう, no mov si points here.
        decoded[0x260..0x265].copy_from_slice(&[0x82, 0xA2, 0x82, 0xA4, 0x00]);

        let catalog = catalog_messages(&decoded, load);
        let runs = scan_sjis_runs(&decoded, 1);
        let uncovered = uncovered_sjis_runs(&catalog, &runs);

        // The referenced string is covered; the standalone run is flagged.
        assert!(uncovered.iter().any(|r| r.decoded_offset == 0x260));
        assert!(!uncovered.iter().any(|r| r.decoded_offset == 0x200));
    }

    #[test]
    fn scan_ignores_ascii_only_runs() {
        let mut decoded = vec![0u8; 0x40];
        decoded[0x10..0x15].copy_from_slice(b"HELLO");
        // min_double=1 -> a run with no double-byte char is not reported.
        assert!(scan_sjis_runs(&decoded, 1).is_empty());
    }

    #[test]
    fn nul_start_scan_skips_a_nul_control_parameter() {
        let mut decoded = vec![0u8; 0x30];
        decoded[0x10..0x1A]
            .copy_from_slice(&[0x01, 0x0E, 0x00, 0x04, 0x07, 0x82, 0xA0, 0x04, 0x02, 0x00]);

        assert!(!nul_delimited_kana_starts(&decoded, 1).contains(&0x13));
        assert_eq!(
            crate::renderer_control::message_bytes(&decoded, 0x10),
            Some(&decoded[0x10..0x19])
        );
    }
}
