//! Parser for the length-prefixed sample banks consumed by `BSAMP.COM`.
//!
//! `TC.CNS` and `MU.CNS` pair their primary graphics stream with a second LZ
//! stream. Their overlays decode that stream separately and pass a selected
//! offset to the resident sound wrapper, which invokes `INT 7Eh` with the
//! `BSAMP.COM` length-prefixed form. The character `S0`/`S1`/`S2` resources use
//! the same track form behind a four-byte indexed entry table. Each selected
//! track starts with a little-endian 16-bit payload length followed by that many
//! sample bytes.

use anyhow::{Result, bail};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BsampTrack {
    pub offset: usize,
    pub payload_offset: usize,
    pub payload_bytes: usize,
}

impl BsampTrack {
    pub fn end_offset(&self) -> usize {
        self.payload_offset + self.payload_bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BsampBank {
    pub tracks: Vec<BsampTrack>,
    /// Bytes after the final track's declared payload. These stay explicit;
    /// they are not silently treated as sample data.
    pub trailing_bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedBsampEntry {
    pub track_offset: Option<usize>,
    /// Raw word consumed by the GAME overlay. Values below 0x8000 are passed
    /// to the sound wrapper; high-bit values branch to another table entry.
    pub control: u16,
    /// Target derived by the consumer's two left shifts and table-relative add.
    pub link_target: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedBsampBank {
    pub table_bytes: usize,
    pub entries: Vec<IndexedBsampEntry>,
    /// Unique tracks in ascending byte-offset order.
    pub tracks: Vec<BsampTrack>,
}

/// Parse selected track offsets from a decoded `BSAMP.COM` sample bank.
///
/// Every track except the last must end exactly at the next selected offset.
/// The last track may have an explicit trailing region, as `TC.CNS` does.
pub fn parse_bsamp_bank(bytes: &[u8], track_offsets: &[usize]) -> Result<BsampBank> {
    if track_offsets.is_empty() {
        bail!("BSAMP bank has no selected track offsets");
    }

    let mut tracks = Vec::with_capacity(track_offsets.len());
    for (index, &offset) in track_offsets.iter().enumerate() {
        if index > 0 && offset <= track_offsets[index - 1] {
            bail!("BSAMP track offsets are not strictly increasing at 0x{offset:X}");
        }
        let length = bytes
            .get(offset..offset + 2)
            .ok_or_else(|| anyhow::anyhow!("BSAMP track header at 0x{offset:X} is truncated"))?;
        let payload_bytes = usize::from(u16::from_le_bytes([length[0], length[1]]));
        let payload_offset = offset + 2;
        let end_offset = payload_offset
            .checked_add(payload_bytes)
            .ok_or_else(|| anyhow::anyhow!("BSAMP track at 0x{offset:X} overflows"))?;
        if end_offset > bytes.len() {
            bail!(
                "BSAMP track at 0x{offset:X} declares {payload_bytes} payload bytes past bank size {}",
                bytes.len()
            );
        }
        if let Some(&next_offset) = track_offsets.get(index + 1)
            && end_offset != next_offset
        {
            bail!(
                "BSAMP track at 0x{offset:X} ends at 0x{end_offset:X}, not next selected offset 0x{next_offset:X}"
            );
        }
        tracks.push(BsampTrack {
            offset,
            payload_offset,
            payload_bytes,
        });
    }

    let final_end = tracks
        .last()
        .expect("non-empty offsets checked above")
        .end_offset();
    Ok(BsampBank {
        tracks,
        trailing_bytes: bytes.len() - final_end,
    })
}

/// Parse the indexed length-prefixed bank used by character S0/S1/S2 files.
///
/// The first nonzero entry pointer is the table size and the first track. Track
/// blocks then cover the remainder of the decoded stream contiguously. Duplicate
/// pointers are allowed because several indices intentionally reuse one sample.
pub fn parse_indexed_bsamp_bank(bytes: &[u8]) -> Result<IndexedBsampBank> {
    if bytes.len() < 4 {
        bail!("indexed BSAMP bank is shorter than one table entry");
    }

    let mut first_pointer = None;
    for entry_offset in (0..bytes.len().saturating_sub(3)).step_by(4) {
        let pointer = usize::from(u16::from_le_bytes([
            bytes[entry_offset],
            bytes[entry_offset + 1],
        ]));
        if pointer != 0 {
            first_pointer = Some((entry_offset, pointer));
            break;
        }
    }
    let (first_pointer_entry, table_bytes) = first_pointer
        .ok_or_else(|| anyhow::anyhow!("indexed BSAMP table has no nonzero track pointer"))?;
    if !table_bytes.is_multiple_of(4)
        || table_bytes < first_pointer_entry + 4
        || table_bytes > bytes.len()
    {
        bail!(
            "indexed BSAMP first pointer 0x{table_bytes:X} is not a valid table boundary after entry 0x{first_pointer_entry:X}"
        );
    }

    let mut entries = Vec::with_capacity(table_bytes / 4);
    let mut track_offsets = Vec::new();
    for (index, entry) in bytes[..table_bytes].as_chunks::<4>().0.iter().enumerate() {
        let pointer = usize::from(u16::from_le_bytes([entry[0], entry[1]]));
        let control = u16::from_le_bytes([entry[2], entry[3]]);
        if pointer == 0 {
            if control != 0 {
                bail!(
                    "indexed BSAMP entry {index} has control 0x{control:04X} without a track pointer"
                );
            }
            entries.push(IndexedBsampEntry {
                track_offset: None,
                control,
                link_target: None,
            });
            continue;
        }
        if pointer < table_bytes || pointer >= bytes.len() {
            bail!("indexed BSAMP entry {index} points outside the track region at 0x{pointer:X}");
        }
        track_offsets.push(pointer);

        let link_target = if control & 0x8000 != 0 {
            let byte_delta = control.wrapping_shl(2) as i16;
            let entry_delta = byte_delta / 4;
            let target = isize::try_from(index)? + isize::from(entry_delta);
            if target < 0 || usize::try_from(target)? >= table_bytes / 4 {
                bail!("indexed BSAMP entry {index} link 0x{control:04X} leaves the table");
            }
            Some(usize::try_from(target)?)
        } else {
            None
        };
        entries.push(IndexedBsampEntry {
            track_offset: Some(pointer),
            control,
            link_target,
        });
    }

    track_offsets.sort_unstable();
    track_offsets.dedup();
    if track_offsets.first().copied() != Some(table_bytes) {
        bail!(
            "indexed BSAMP first track is {:?}, expected table end 0x{table_bytes:X}",
            track_offsets.first().map(|offset| format!("0x{offset:X}"))
        );
    }
    let bank = parse_bsamp_bank(bytes, &track_offsets)?;
    if bank.trailing_bytes != 0 {
        bail!(
            "indexed BSAMP tracks leave {} trailing byte(s)",
            bank.trailing_bytes
        );
    }
    Ok(IndexedBsampBank {
        table_bytes,
        entries,
        tracks: bank.tracks,
    })
}

/// Parse the exact indexed-bank shape proven for a named character S resource.
pub fn parse_named_indexed_bsamp_bank(
    name: &str,
    bytes: &[u8],
) -> Result<Option<IndexedBsampBank>> {
    let expected = match name.to_ascii_uppercase().as_str() {
        "ARURU_S0.CNS" => (0xFCFC, 0xA0, 40, 22),
        "ARURU_S1.CNS" => (0xDC98, 0x40, 16, 15),
        "ARURU_S2.CNS" => (0xB28A, 0x70, 28, 15),
        "RURUU_S0.CNS" => (0xEF5E, 0x90, 36, 20),
        "RURUU_S1.CNS" => (0xB90B, 0x40, 16, 11),
        "RURUU_S2.CNS" => (0x68C1, 0x20, 8, 8),
        "SHEZO_S0.CNS" => (0xFD7A, 0x90, 36, 23),
        "SHEZO_S1.CNS" => (0xFE56, 0x60, 24, 20),
        _ => return Ok(None),
    };
    let bank = parse_indexed_bsamp_bank(bytes)?;
    let actual = (
        bytes.len(),
        bank.table_bytes,
        bank.entries.len(),
        bank.tracks.len(),
    );
    if actual != expected {
        bail!("{name} indexed BSAMP shape {actual:?}; expected {expected:?}");
    }
    Ok(Some(bank))
}

/// Parse the exact companion-bank contract proven for the named Demo resource.
pub fn parse_named_bsamp_companion(name: &str, bytes: &[u8]) -> Result<Option<BsampBank>> {
    let (track_offsets, expected_payloads, expected_trailing): (&[usize], &[usize], usize) =
        match name.to_ascii_uppercase().as_str() {
            "TC.CNS" => (&[0], &[0x127E], 4),
            "MU.CNS" => (&[0, 0x0D80, 0x1840], &[0x0D7E, 0x0ABE, 0x1741], 0),
            _ => return Ok(None),
        };
    let bank = parse_bsamp_bank(bytes, track_offsets)?;
    let payloads = bank
        .tracks
        .iter()
        .map(|track| track.payload_bytes)
        .collect::<Vec<_>>();
    if payloads != expected_payloads || bank.trailing_bytes != expected_trailing {
        bail!(
            "{name} BSAMP companion drift: payloads {payloads:?}, trailing {}; expected {expected_payloads:?}, trailing {expected_trailing}",
            bank.trailing_bytes
        );
    }
    Ok(Some(bank))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_contiguous_tracks_and_explicit_trailing_bytes() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&3u16.to_le_bytes());
        bytes.extend_from_slice(&[1, 2, 3]);
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&[4, 5]);
        bytes.push(0x80);

        let bank = parse_bsamp_bank(&bytes, &[0, 5]).unwrap();
        assert_eq!(bank.tracks[0].payload_offset, 2);
        assert_eq!(bank.tracks[0].payload_bytes, 3);
        assert_eq!(bank.tracks[1].payload_offset, 7);
        assert_eq!(bank.tracks[1].payload_bytes, 2);
        assert_eq!(bank.trailing_bytes, 1);
    }

    #[test]
    fn rejects_a_track_that_does_not_end_at_the_next_selected_offset() {
        let mut bytes = vec![0u8; 12];
        bytes[0..2].copy_from_slice(&3u16.to_le_bytes());
        bytes[6..8].copy_from_slice(&2u16.to_le_bytes());
        let error = parse_bsamp_bank(&bytes, &[0, 6]).unwrap_err().to_string();
        assert!(error.contains("ends at 0x5, not next selected offset 0x6"));
    }

    #[test]
    fn named_contract_rejects_a_declared_length_drift() {
        let mut bytes = vec![0u8; 4_740];
        bytes[0..2].copy_from_slice(&0x127Du16.to_le_bytes());
        let error = parse_named_bsamp_companion("TC.CNS", &bytes)
            .unwrap_err()
            .to_string();
        assert!(error.contains("payloads [4733], trailing 5"));
    }

    #[test]
    fn parses_an_indexed_bank_with_reused_tracks_and_relative_links() {
        let mut bytes = vec![0u8; 12];
        bytes[0..4].copy_from_slice(&[12, 0, 0xF4, 0x01]);
        bytes[4..8].copy_from_slice(&[17, 0, 0x01, 0x80]);
        bytes[8..12].copy_from_slice(&[17, 0, 0xFF, 0xFF]);
        bytes.extend_from_slice(&3u16.to_le_bytes());
        bytes.extend_from_slice(&[1, 2, 3]);
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&[4, 5]);

        let bank = parse_indexed_bsamp_bank(&bytes).unwrap();
        assert_eq!(bank.table_bytes, 12);
        assert_eq!(bank.entries.len(), 3);
        assert_eq!(bank.tracks.len(), 2);
        assert_eq!(bank.entries[0].link_target, None);
        assert_eq!(bank.entries[1].link_target, Some(2));
        assert_eq!(bank.entries[2].link_target, Some(1));
        assert_eq!(bank.tracks[0].payload_bytes, 3);
        assert_eq!(bank.tracks[1].payload_bytes, 2);
    }

    #[test]
    fn indexed_bank_rejects_a_gap_between_tracks() {
        let mut bytes = vec![0u8; 16];
        bytes[0..4].copy_from_slice(&[8, 0, 0, 0]);
        bytes[4..8].copy_from_slice(&[13, 0, 0, 0]);
        bytes[8..10].copy_from_slice(&2u16.to_le_bytes());
        bytes[13..15].copy_from_slice(&1u16.to_le_bytes());

        let error = parse_indexed_bsamp_bank(&bytes).unwrap_err().to_string();
        assert!(error.contains("ends at 0xC, not next selected offset 0xD"));
    }
}
