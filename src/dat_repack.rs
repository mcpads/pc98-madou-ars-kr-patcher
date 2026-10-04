//! Transactional repacking for adjacent gameplay-DAT string slots.
//!
//! Most DAT translations stay inside their individual source slots. A short
//! source slot can still be localized without inventing an abbreviation when
//! it belongs to a proven contiguous group whose translated strings fit in the
//! group's total source capacity. Direct references to entries that move are
//! verified before any bytes are committed and then rewritten atomically.

use std::collections::HashSet;

use anyhow::{Context, Result, bail};

#[derive(Debug)]
pub struct RepackEntry<'a> {
    pub id: &'a str,
    pub source_offset: usize,
    pub source_budget: usize,
    pub encoded: &'a [u8],
    pub rewrite_sites: &'a [usize],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepackReport {
    pub capacity: usize,
    pub used: usize,
    pub destinations: Vec<(String, usize)>,
}

/// Repack a complete contiguous source-slot group at its original start.
///
/// Every source budget must match the live NUL-terminated slot and the entries
/// must exactly touch in the declared order. The destination group never grows:
/// translated payloads plus their NULs must fit in the original total capacity.
/// Each declared rewrite site must contain the entry's original 16-bit offset.
/// Validation and staging finish before the image is changed.
pub fn repack_contiguous_group(
    decoded: &mut [u8],
    entries: &[RepackEntry<'_>],
) -> Result<RepackReport> {
    let first = entries.first().context("DAT repack group is empty")?;
    let start = first.source_offset;
    let mut cursor = start;
    let mut seen_ids = HashSet::new();
    let mut seen_sites = HashSet::new();

    for entry in entries {
        if !seen_ids.insert(entry.id) {
            bail!("DAT repack group repeats entry {}", entry.id);
        }
        if entry.source_offset != cursor {
            bail!(
                "{}: source offset 0x{:04X} is not contiguous at 0x{cursor:04X}",
                entry.id,
                entry.source_offset,
            );
        }
        if entry.source_budget == 0 {
            bail!("{}: source budget is zero", entry.id);
        }
        let end = entry
            .source_offset
            .checked_add(entry.source_budget)
            .with_context(|| format!("{}: source range overflow", entry.id))?;
        if end > decoded.len() {
            bail!(
                "{}: source range ends at 0x{end:04X}, past decoded length 0x{:04X}",
                entry.id,
                decoded.len(),
            );
        }
        let live_budget = decoded[entry.source_offset..]
            .iter()
            .position(|byte| *byte == 0)
            .map(|payload| payload + 1)
            .with_context(|| format!("{}: source slot has no NUL", entry.id))?;
        if live_budget != entry.source_budget {
            bail!(
                "{}: declared source budget {} does not match live slot {}",
                entry.id,
                entry.source_budget,
                live_budget,
            );
        }
        cursor = end;
    }

    // Every direct renderer load of a group entry must be declared. The DAT
    // dispatch code precedes its text, so scan the prefix before the group for
    // `mov si,imm16` immediately followed by a near call or jump.
    for entry in entries {
        let declared = entry.rewrite_sites.iter().copied().collect::<HashSet<_>>();
        let pointer = u16::try_from(entry.source_offset)
            .with_context(|| format!("{}: source offset exceeds 16 bits", entry.id))?
            .to_le_bytes();
        for site in 1..start.saturating_sub(2) {
            if decoded[site - 1] == 0xBE
                && decoded[site..site + 2] == pointer
                && matches!(decoded[site + 2], 0xE8 | 0xE9)
                && !declared.contains(&site)
            {
                bail!(
                    "{}: direct mov-si consumer 0x{site:04X} is not a declared rewrite site",
                    entry.id,
                );
            }
        }
    }

    let capacity = cursor - start;
    let used = entries.iter().try_fold(0usize, |total, entry| {
        total
            .checked_add(entry.encoded.len() + 1)
            .with_context(|| format!("{}: translated size overflow", entry.id))
    })?;
    if used > capacity {
        bail!("DAT repack group needs {used} bytes but has {capacity}");
    }

    let mut staged = vec![0u8; capacity];
    let mut destinations = Vec::with_capacity(entries.len());
    let mut write_at = 0usize;
    for entry in entries {
        let destination = start + write_at;
        if destination > u16::MAX as usize {
            bail!(
                "{}: destination 0x{destination:04X} does not fit a 16-bit pointer",
                entry.id,
            );
        }
        staged[write_at..write_at + entry.encoded.len()].copy_from_slice(entry.encoded);
        destinations.push((entry.id.to_owned(), destination));
        write_at += entry.encoded.len() + 1;
    }

    for (entry, (_, destination)) in entries.iter().zip(&destinations) {
        for &site in entry.rewrite_sites {
            if !seen_sites.insert(site) {
                bail!("DAT repack group repeats rewrite site 0x{site:04X}");
            }
            if site >= start && site < cursor {
                bail!(
                    "{}: rewrite site 0x{site:04X} overlaps the repacked source band",
                    entry.id,
                );
            }
            let site_end = site
                .checked_add(2)
                .with_context(|| format!("{}: rewrite site overflow", entry.id))?;
            let raw = decoded.get(site..site_end).with_context(|| {
                format!("{}: rewrite site 0x{site:04X} is out of range", entry.id)
            })?;
            let actual = u16::from_le_bytes([raw[0], raw[1]]) as usize;
            if actual != entry.source_offset {
                bail!(
                    "{}: rewrite site 0x{site:04X} points to 0x{actual:04X}, expected 0x{:04X}",
                    entry.id,
                    entry.source_offset,
                );
            }
            if *destination > u16::MAX as usize {
                bail!("{}: rewritten pointer does not fit u16", entry.id);
            }
        }
    }

    decoded[start..cursor].copy_from_slice(&staged);
    for (entry, (_, destination)) in entries.iter().zip(&destinations) {
        let pointer = (*destination as u16).to_le_bytes();
        for &site in entry.rewrite_sites {
            decoded[site..site + 2].copy_from_slice(&pointer);
        }
    }

    Ok(RepackReport {
        capacity,
        used,
        destinations,
    })
}

#[cfg(test)]
mod tests {
    use super::{RepackEntry, repack_contiguous_group};

    #[test]
    fn repacks_adjacent_slots_and_rewrites_moved_pointer() {
        let mut decoded = vec![0u8; 0x80];
        decoded[0x10..0x13].copy_from_slice(b"AA\0");
        decoded[0x13..0x18].copy_from_slice(b"BBBB\0");
        decoded[0x18..0x20].copy_from_slice(b"CCCCCCC\0");
        decoded[0x04..0x06].copy_from_slice(&0x0010u16.to_le_bytes());
        decoded[0x06..0x08].copy_from_slice(&0x0018u16.to_le_bytes());

        let first = b"XY";
        let second = b"B";
        let third = b"CC";
        let entries = [
            RepackEntry {
                id: "FIRST",
                source_offset: 0x10,
                source_budget: 3,
                encoded: first,
                rewrite_sites: &[0x04],
            },
            RepackEntry {
                id: "SECOND",
                source_offset: 0x13,
                source_budget: 5,
                encoded: second,
                rewrite_sites: &[],
            },
            RepackEntry {
                id: "THIRD",
                source_offset: 0x18,
                source_budget: 8,
                encoded: third,
                rewrite_sites: &[0x06],
            },
        ];

        let report = repack_contiguous_group(&mut decoded, &entries).unwrap();
        assert_eq!(report.capacity, 16);
        assert_eq!(report.used, 8);
        assert_eq!(report.destinations[2], ("THIRD".to_owned(), 0x15));
        assert_eq!(&decoded[0x10..0x20], b"XY\0B\0CC\0\0\0\0\0\0\0\0\0");
        assert_eq!(&decoded[0x04..0x06], &0x0010u16.to_le_bytes());
        assert_eq!(&decoded[0x06..0x08], &0x0015u16.to_le_bytes());
    }

    #[test]
    fn undeclared_direct_mov_si_consumer_is_rejected() {
        // ENEMY209 regression: the moved `の　攻撃！` suffix kept a stale
        // `mov si` pointer because only a sibling's consumer was declared.
        let mut decoded = vec![0u8; 0x80];
        decoded[0x10..0x13].copy_from_slice(b"AA\0");
        decoded[0x13..0x18].copy_from_slice(b"BBBB\0");
        decoded[0x04] = 0xBE;
        decoded[0x05..0x07].copy_from_slice(&0x0013u16.to_le_bytes());
        decoded[0x07] = 0xE8;
        let before = decoded.clone();
        let entries = [
            RepackEntry {
                id: "NAME",
                source_offset: 0x10,
                source_budget: 3,
                encoded: b"XYZ",
                rewrite_sites: &[],
            },
            RepackEntry {
                id: "SUFFIX",
                source_offset: 0x13,
                source_budget: 5,
                encoded: b"B",
                rewrite_sites: &[],
            },
        ];

        let error = repack_contiguous_group(&mut decoded, &entries).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("SUFFIX: direct mov-si consumer 0x0005")
        );
        assert_eq!(decoded, before);
    }

    #[test]
    fn validation_failure_is_transactional() {
        let mut decoded = vec![0u8; 0x30];
        decoded[0x10..0x13].copy_from_slice(b"AA\0");
        decoded[0x04..0x06].copy_from_slice(&0x0020u16.to_le_bytes());
        let original = decoded.clone();
        let encoded = b"B";
        let entries = [RepackEntry {
            id: "ENTRY",
            source_offset: 0x10,
            source_budget: 3,
            encoded,
            rewrite_sites: &[0x04],
        }];

        let error = repack_contiguous_group(&mut decoded, &entries)
            .unwrap_err()
            .to_string();
        assert!(error.contains("points to 0x0020"), "{error}");
        assert_eq!(decoded, original);
    }

    #[test]
    fn one_byte_capacity_overflow_is_transactional() {
        let mut decoded = vec![0u8; 0x30];
        decoded[0x10..0x13].copy_from_slice(b"AA\0");
        decoded[0x04..0x06].copy_from_slice(&0x0010u16.to_le_bytes());
        let original = decoded.clone();
        let encoded = b"AAA";
        let entries = [RepackEntry {
            id: "ENTRY",
            source_offset: 0x10,
            source_budget: 3,
            encoded,
            rewrite_sites: &[0x04],
        }];

        let error = repack_contiguous_group(&mut decoded, &entries)
            .unwrap_err()
            .to_string();

        assert!(error.contains("needs 4 bytes but has 3"), "{error}");
        assert_eq!(decoded, original);
    }
}
