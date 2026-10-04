//! Heuristic Shift-JIS string-region scan for arbitrary A.R.S resources.

use encoding_rs::SHIFT_JIS;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringRegion {
    pub start: usize,
    pub end: usize,
    pub glyph_count: usize,
    pub japanese_count: usize,
}

pub fn scan_string_regions(bytes: &[u8], min_run: usize, min_japanese: usize) -> Vec<StringRegion> {
    let mut regions = Vec::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if let Some((end, glyph_count)) = try_run(bytes, cursor)
            && glyph_count >= min_run
        {
            let japanese_count = count_japanese_sjis(&bytes[cursor..end]);
            if japanese_count >= min_japanese {
                regions.push(StringRegion {
                    start: cursor,
                    end,
                    glyph_count,
                    japanese_count,
                });
            }
            cursor = end;
        } else {
            cursor += 1;
        }
    }
    regions
}

pub fn decode_region(bytes: &[u8]) -> String {
    SHIFT_JIS.decode(bytes).0.into_owned()
}

fn try_run(bytes: &[u8], start: usize) -> Option<(usize, usize)> {
    let mut cursor = start;
    let mut glyphs = 0usize;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if is_sjis_lead(byte)
            && cursor + 1 < bytes.len()
            && is_sjis_trail(bytes[cursor + 1])
            && sjis_decodes(byte, bytes[cursor + 1])
        {
            glyphs += 1;
            cursor += 2;
        } else if byte == b'$' && cursor + 1 < bytes.len() {
            // A.R.S resources use `$X`-shaped command pairs in otherwise
            // printable runs; count the pair as one unit, matching the probe
            // semantics used to establish the documented corpus numbers.
            glyphs += 1;
            cursor += 2;
        } else if (0xA1..=0xDF).contains(&byte) || (0x20..=0x7E).contains(&byte) {
            glyphs += 1;
            cursor += 1;
        } else {
            break;
        }
    }
    (glyphs > 0).then_some((cursor, glyphs))
}

fn count_japanese_sjis(bytes: &[u8]) -> usize {
    let mut count = 0usize;
    let mut cursor = 0usize;
    while cursor + 1 < bytes.len() {
        let lead = bytes[cursor];
        let trail = bytes[cursor + 1];
        if is_sjis_lead(lead) && is_sjis_trail(trail) && sjis_decodes(lead, trail) {
            count += 1;
            cursor += 2;
        } else {
            cursor += 1;
        }
    }
    count
}

fn is_sjis_lead(byte: u8) -> bool {
    (0x81..=0x9F).contains(&byte) || (0xE0..=0xFC).contains(&byte)
}

fn is_sjis_trail(byte: u8) -> bool {
    (0x40..=0x7E).contains(&byte) || (0x80..=0xFC).contains(&byte)
}

fn sjis_decodes(lead: u8, trail: u8) -> bool {
    !SHIFT_JIS.decode(&[lead, trail]).2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_japanese_region_and_filters_ascii_noise() {
        let bytes = [b'X', 0, 0x82, 0xA0, 0x82, 0xA2, b'!', 0];
        let regions = scan_string_regions(&bytes, 3, 2);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].start, 2);
        assert_eq!(regions[0].end, 7);
        assert_eq!(decode_region(&bytes[2..7]), "あい!");
    }
}
