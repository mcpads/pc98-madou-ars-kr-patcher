//! Pointer-table detection for table-indexed overlay messages.
//!
//! The `mov si` scan (`overlay_text`) finds messages whose pointer is an inline
//! immediate. The rest are reached through data pointer tables: a run of 16-bit
//! little-endian entries, each the logical offset of a string, laid out at a
//! fixed stride (2 bytes for a pure table, larger when the pointer is one field
//! of a record). This module finds those tables so their entries become
//! translation units with a reinsertion path -- relocation rewrites the table
//! entry, not a `mov si`.
//!
//! Safety (`references/strategy/text-extraction.md` 3.5.1): a bare 16-bit value
//! equal to a string offset appears in code by chance, and rewriting such a
//! false pointer would corrupt code (violating the zero-defect rule). A table is
//! trusted only when several consecutive entries at one stride all point at
//! confirmed string starts; that run requirement makes a coincidental table
//! astronomically unlikely.

use std::collections::HashSet;

use crate::overlay_catalog::{CatalogedMessage, SjisRun};

/// One entry of a detected pointer table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointerEntry {
    /// Decoded offset of the 16-bit pointer (the byte to rewrite on relocation).
    pub site: usize,
    pub target_decoded: usize,
    pub target_logical: usize,
}

/// A detected pointer table: `entries.len()` pointers at `stride` bytes apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointerTable {
    pub start: usize,
    pub stride: usize,
    pub entries: Vec<PointerEntry>,
}

/// Confirmed string-start offsets a table entry may point at: cataloged
/// messages, kana-bearing SJIS runs, and NUL-delimited kana string starts (the
/// true message starts a run scan can miss when a message begins after an
/// internal split rather than a NUL).
pub fn string_start_set(
    decoded: &[u8],
    catalog: &[CatalogedMessage],
    runs: &[SjisRun],
) -> HashSet<usize> {
    let mut starts = crate::overlay_catalog::nul_delimited_kana_starts(decoded, 1);
    for message in catalog {
        starts.insert(message.string_decoded_offset);
    }
    for run in runs {
        if run.kana_count > 0 {
            starts.insert(run.decoded_offset);
        }
    }
    starts
}

fn entry_target(
    decoded: &[u8],
    load_offset: usize,
    site: usize,
    starts: &HashSet<usize>,
) -> Option<usize> {
    if site + 2 > decoded.len() {
        return None;
    }
    let value = u16::from_le_bytes([decoded[site], decoded[site + 1]]) as usize;
    let target = value.checked_sub(load_offset)?;
    starts.contains(&target).then_some(target)
}

/// Find every pointer table: for each stride, each maximal run of >= `min_entries`
/// consecutive entries whose values all resolve to a confirmed string start.
/// Tables whose target set is already fully covered by a longer table are
/// dropped, so a sub-sampled run at a multiple stride does not double-report.
pub fn find_pointer_tables(
    decoded: &[u8],
    load_offset: usize,
    starts: &HashSet<usize>,
    min_entries: usize,
    strides: &[usize],
) -> Vec<PointerTable> {
    let mut candidates = Vec::new();
    for &stride in strides {
        if stride < 2 {
            continue;
        }
        let mut site = 0usize;
        while site + 2 <= decoded.len() {
            if let Some(target) = entry_target(decoded, load_offset, site, starts) {
                let is_run_start = site < stride
                    || entry_target(decoded, load_offset, site - stride, starts).is_none();
                if is_run_start {
                    let mut entries = vec![PointerEntry {
                        site,
                        target_decoded: target,
                        target_logical: target + load_offset,
                    }];
                    let mut next = site + stride;
                    while let Some(target) = entry_target(decoded, load_offset, next, starts) {
                        entries.push(PointerEntry {
                            site: next,
                            target_decoded: target,
                            target_logical: target + load_offset,
                        });
                        next += stride;
                    }
                    if entries.len() >= min_entries {
                        candidates.push(PointerTable {
                            start: site,
                            stride,
                            entries,
                        });
                    }
                }
            }
            site += 1;
        }
    }

    // Prefer the longest tables; drop any whose targets a kept table already covers.
    candidates.sort_by(|left, right| {
        right
            .entries
            .len()
            .cmp(&left.entries.len())
            .then(left.stride.cmp(&right.stride))
            .then(left.start.cmp(&right.start))
    });
    let mut covered_targets = HashSet::new();
    let mut accepted = Vec::new();
    for table in candidates {
        if table
            .entries
            .iter()
            .all(|entry| covered_targets.contains(&entry.target_decoded))
        {
            continue;
        }
        for entry in &table.entries {
            covered_targets.insert(entry.target_decoded);
        }
        accepted.push(table);
    }
    accepted.sort_by_key(|table| table.start);
    accepted
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put_le16(buf: &mut [u8], at: usize, value: u16) {
        buf[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }

    #[test]
    fn finds_a_stride_2_table_of_confirmed_string_starts() {
        let load = 0x100usize;
        let mut decoded = vec![0u8; 0x200];
        // Four string starts at decoded 0x80, 0x90, 0xA0, 0xB0 (logical +0x100).
        let starts: HashSet<usize> = [0x80, 0x90, 0xA0, 0xB0].into_iter().collect();
        // Pure 2-byte table at 0x10 pointing at them.
        put_le16(&mut decoded, 0x10, 0x180);
        put_le16(&mut decoded, 0x12, 0x190);
        put_le16(&mut decoded, 0x14, 0x1A0);
        put_le16(&mut decoded, 0x16, 0x1B0);

        let tables = find_pointer_tables(&decoded, load, &starts, 4, &[2, 4, 8]);
        assert_eq!(tables.len(), 1);
        let table = &tables[0];
        assert_eq!(table.start, 0x10);
        assert_eq!(table.stride, 2);
        assert_eq!(table.entries.len(), 4);
        assert_eq!(table.entries[0].site, 0x10);
        assert_eq!(table.entries[0].target_decoded, 0x80);
        assert_eq!(table.entries[3].target_logical, 0x1B0);
    }

    #[test]
    fn finds_a_record_table_where_the_pointer_is_one_field() {
        let load = 0x100usize;
        let mut decoded = vec![0u8; 0x200];
        let starts: HashSet<usize> = [0x80, 0x90, 0xA0, 0xB0].into_iter().collect();
        // 8-byte records; the pointer field is at record+2.
        for (i, target) in [0x180u16, 0x190, 0x1A0, 0x1B0].into_iter().enumerate() {
            put_le16(&mut decoded, 0x20 + i * 8 + 2, target);
        }
        let tables = find_pointer_tables(&decoded, load, &starts, 4, &[2, 4, 6, 8]);
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].stride, 8);
        assert_eq!(tables[0].start, 0x22);
        assert_eq!(tables[0].entries.len(), 4);
    }

    #[test]
    fn a_run_shorter_than_min_entries_is_not_a_table() {
        let load = 0x100usize;
        let mut decoded = vec![0u8; 0x200];
        let starts: HashSet<usize> = [0x80, 0x90].into_iter().collect();
        put_le16(&mut decoded, 0x10, 0x180);
        put_le16(&mut decoded, 0x12, 0x190);
        // Only two entries; a coincidental pair must not be trusted as a table.
        assert!(find_pointer_tables(&decoded, load, &starts, 4, &[2]).is_empty());
    }
}
