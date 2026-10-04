//! Conservative SJIS pair statistics for A.R.S source binaries.

use encoding_rs::SHIFT_JIS;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphSourceStats {
    pub display: String,
    pub size: usize,
    pub sjis_pairs: usize,
    pub half_kana: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GlyphStats {
    pub sources: Vec<GlyphSourceStats>,
    pub pair_counts: BTreeMap<(u8, u8), usize>,
    pub lead_counts: BTreeMap<u8, usize>,
    pub half_kana_bytes: usize,
}

impl GlyphStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_source(&mut self, display: impl Into<String>, bytes: &[u8]) {
        let mut source_pairs = 0usize;
        let mut source_half_kana = 0usize;
        let mut offset = 0usize;

        while offset < bytes.len() {
            let byte = bytes[offset];
            if is_sjis_lead(byte)
                && offset + 1 < bytes.len()
                && is_sjis_trail(bytes[offset + 1])
                && sjis_decodes(byte, bytes[offset + 1])
            {
                let trail = bytes[offset + 1];
                *self.pair_counts.entry((byte, trail)).or_insert(0) += 1;
                *self.lead_counts.entry(byte).or_insert(0) += 1;
                source_pairs += 1;
                offset += 2;
                continue;
            }

            if is_half_kana(byte) {
                source_half_kana += 1;
            }
            offset += 1;
        }

        self.half_kana_bytes += source_half_kana;
        self.sources.push(GlyphSourceStats {
            display: display.into(),
            size: bytes.len(),
            sjis_pairs: source_pairs,
            half_kana: source_half_kana,
        });
    }

    pub fn total_pair_occurrences(&self) -> usize {
        self.pair_counts.values().sum()
    }

    pub fn unique_pairs(&self) -> usize {
        self.pair_counts.len()
    }

    pub fn free_pairs(&self) -> usize {
        total_decodable_sjis_pairs() - self.unique_pairs()
    }

    pub fn prefix_occurrences(&self, lead: u8) -> usize {
        self.lead_counts.get(&lead).copied().unwrap_or(0)
    }

    pub fn top_pairs(&self, limit: usize) -> Vec<((u8, u8), usize)> {
        let mut pairs: Vec<_> = self
            .pair_counts
            .iter()
            .map(|(&pair, &count)| (pair, count))
            .collect();
        pairs.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        pairs.truncate(limit);
        pairs
    }
}

pub fn total_decodable_sjis_pairs() -> usize {
    let mut count = 0usize;
    for lead in 0x81..=0xFCu16 {
        let lead = lead as u8;
        if !is_sjis_lead(lead) {
            continue;
        }
        for trail in 0x40..=0xFCu16 {
            let trail = trail as u8;
            if is_sjis_trail(trail) && sjis_decodes(lead, trail) {
                count += 1;
            }
        }
    }
    count
}

pub fn decode_sjis_pair(lead: u8, trail: u8) -> Option<char> {
    let bytes = [lead, trail];
    let (decoded, _, had_errors) = SHIFT_JIS.decode(&bytes);
    if had_errors {
        return None;
    }
    let mut chars = decoded.chars();
    let ch = chars.next()?;
    if chars.next().is_none() {
        Some(ch)
    } else {
        None
    }
}

fn is_sjis_lead(byte: u8) -> bool {
    (0x81..=0x9F).contains(&byte) || (0xE0..=0xFC).contains(&byte)
}

fn is_sjis_trail(byte: u8) -> bool {
    (0x40..=0x7E).contains(&byte) || (0x80..=0xFC).contains(&byte)
}

fn is_half_kana(byte: u8) -> bool {
    (0xA1..=0xDF).contains(&byte)
}

fn sjis_decodes(lead: u8, trail: u8) -> bool {
    let (_, _, had_errors) = SHIFT_JIS.decode(&[lead, trail]);
    !had_errors
}

#[cfg(test)]
mod tests {
    use super::GlyphStats;

    #[test]
    fn scans_sjis_pairs_and_half_kana() {
        let mut stats = GlyphStats::new();
        stats.add_source("fixture", &[0x82, 0xA0, 0xA6, 0xE0, 0x80, 0x00]);

        assert_eq!(stats.total_pair_occurrences(), 2);
        assert_eq!(stats.unique_pairs(), 2);
        assert_eq!(stats.half_kana_bytes, 1);
        assert_eq!(stats.prefix_occurrences(0x82), 1);
        assert_eq!(stats.prefix_occurrences(0xE0), 1);
        assert_eq!(stats.sources[0].sjis_pairs, 2);
        assert_eq!(stats.sources[0].half_kana, 1);
    }
}
