//! Minimal, standards-compatible BPS1 encoder and decoder.
//!
//! The deterministic encoder emits `SourceRead`, `TargetRead`, and `SourceCopy`;
//! the decoder accepts all four standard action types so patches remain
//! interoperable with other BPS tooling.

use anyhow::{Context, Result, ensure};
use std::collections::HashMap;

const MAGIC: &[u8; 4] = b"BPS1";
const FOOTER_LEN: usize = 12;

const SOURCE_READ: u64 = 0;
const TARGET_READ: u64 = 1;
const SOURCE_COPY: u64 = 2;
const TARGET_COPY: u64 = 3;
const MIN_COPY_MATCH: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchInfo {
    pub source_size: usize,
    pub target_size: usize,
    pub metadata: Vec<u8>,
    pub source_crc32: u32,
    pub target_crc32: u32,
    pub patch_crc32: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedPatch {
    pub target: Vec<u8>,
    pub info: PatchInfo,
}

/// Create a deterministic BPS1 patch and append the three standard CRC32
/// footer fields. The target is described completely and can have a different
/// length from the source.
pub fn create_patch(source: &[u8], target: &[u8], metadata: &[u8]) -> Result<Vec<u8>> {
    let mut patch = Vec::new();
    patch.extend_from_slice(MAGIC);
    encode_number(
        u64::try_from(source.len()).context("source is too large for BPS")?,
        &mut patch,
    );
    encode_number(
        u64::try_from(target.len()).context("target is too large for BPS")?,
        &mut patch,
    );
    encode_number(
        u64::try_from(metadata.len()).context("metadata is too large for BPS")?,
        &mut patch,
    );
    patch.extend_from_slice(metadata);

    let source_index = build_source_index(source);
    let mut source_relative = 0i64;
    let mut offset = 0usize;
    while offset < target.len() {
        match best_source_action(source, target, &source_index, offset) {
            Some(SourceAction::Read { length }) => {
                encode_action(SOURCE_READ, length, &mut patch)?;
                offset += length;
            }
            Some(SourceAction::Copy {
                source_offset,
                length,
            }) => {
                encode_action(SOURCE_COPY, length, &mut patch)?;
                source_relative =
                    encode_relative_offset(source_relative, source_offset, length, &mut patch)?;
                offset += length;
            }
            None => {
                let start = offset;
                offset += 1;
                while offset < target.len()
                    && best_source_action(source, target, &source_index, offset).is_none()
                {
                    offset += 1;
                }
                encode_action(TARGET_READ, offset - start, &mut patch)?;
                patch.extend_from_slice(&target[start..offset]);
            }
        }
    }

    patch.extend_from_slice(&crc32(source).to_le_bytes());
    patch.extend_from_slice(&crc32(target).to_le_bytes());
    let patch_crc32 = crc32(&patch);
    patch.extend_from_slice(&patch_crc32.to_le_bytes());
    Ok(patch)
}

/// Read and checksum-validate the BPS header, metadata, and footer without
/// applying any actions.
pub fn inspect_patch(patch: &[u8]) -> Result<PatchInfo> {
    ensure!(
        patch.len() >= MAGIC.len() + 3 + FOOTER_LEN,
        "BPS patch is too short: {} bytes",
        patch.len()
    );
    ensure!(&patch[..MAGIC.len()] == MAGIC, "BPS magic is not BPS1");

    let body_end = patch.len() - FOOTER_LEN;
    let source_crc32 = read_u32_le(&patch[body_end..body_end + 4]);
    let target_crc32 = read_u32_le(&patch[body_end + 4..body_end + 8]);
    let patch_crc32 = read_u32_le(&patch[body_end + 8..]);
    let actual_patch_crc32 = crc32(&patch[..patch.len() - 4]);
    ensure!(
        actual_patch_crc32 == patch_crc32,
        "BPS patch CRC32 mismatch: expected {patch_crc32:08x}, got {actual_patch_crc32:08x}"
    );

    let mut reader = Reader::new(patch, MAGIC.len(), body_end);
    let source_size = to_usize(reader.read_number()?, "BPS source size")?;
    let target_size = to_usize(reader.read_number()?, "BPS target size")?;
    let metadata_size = to_usize(reader.read_number()?, "BPS metadata size")?;
    let metadata = reader.read_bytes(metadata_size)?.to_vec();

    Ok(PatchInfo {
        source_size,
        target_size,
        metadata,
        source_crc32,
        target_crc32,
        patch_crc32,
    })
}

/// Apply a BPS1 patch, checking the source, target, and patch CRC32 fields.
pub fn apply_patch(source: &[u8], patch: &[u8]) -> Result<AppliedPatch> {
    let info = inspect_patch(patch)?;
    ensure!(
        source.len() == info.source_size,
        "BPS source size mismatch: patch expects {}, got {}",
        info.source_size,
        source.len()
    );
    let actual_source_crc32 = crc32(source);
    ensure!(
        actual_source_crc32 == info.source_crc32,
        "BPS source CRC32 mismatch: patch expects {:08x}, got {actual_source_crc32:08x}",
        info.source_crc32
    );

    let body_end = patch.len() - FOOTER_LEN;
    let mut reader = Reader::new(patch, MAGIC.len(), body_end);
    let source_size = to_usize(reader.read_number()?, "BPS source size")?;
    let target_size = to_usize(reader.read_number()?, "BPS target size")?;
    let metadata_size = to_usize(reader.read_number()?, "BPS metadata size")?;
    reader.read_bytes(metadata_size)?;
    ensure!(
        source_size == info.source_size,
        "BPS header changed while applying"
    );
    ensure!(
        target_size == info.target_size,
        "BPS header changed while applying"
    );

    let mut target = Vec::new();
    target
        .try_reserve_exact(target_size)
        .context("reserve BPS target buffer")?;
    let mut source_relative = 0i64;
    let mut target_relative = 0i64;

    while target.len() < target_size {
        let action_data = reader.read_number()?;
        let action = action_data & 3;
        let length = to_usize((action_data >> 2) + 1, "BPS action length")?;
        ensure!(
            length <= target_size - target.len(),
            "BPS action writes {length} bytes past target size {target_size}"
        );

        match action {
            SOURCE_READ => {
                let start = target.len();
                let end = start
                    .checked_add(length)
                    .context("BPS SourceRead range overflow")?;
                let bytes = source
                    .get(start..end)
                    .with_context(|| format!("BPS SourceRead {start}..{end} exceeds source"))?;
                target.extend_from_slice(bytes);
            }
            TARGET_READ => {
                target.extend_from_slice(reader.read_bytes(length)?);
            }
            SOURCE_COPY => {
                source_relative = add_relative_offset(source_relative, reader.read_number()?)
                    .context("BPS SourceCopy relative offset")?;
                let start = relative_to_usize(source_relative, "BPS SourceCopy offset")?;
                let end = start
                    .checked_add(length)
                    .context("BPS SourceCopy range overflow")?;
                let bytes = source
                    .get(start..end)
                    .with_context(|| format!("BPS SourceCopy {start}..{end} exceeds source"))?;
                target.extend_from_slice(bytes);
                source_relative = i64::try_from(end).context("BPS SourceCopy cursor overflow")?;
            }
            TARGET_COPY => {
                target_relative = add_relative_offset(target_relative, reader.read_number()?)
                    .context("BPS TargetCopy relative offset")?;
                for _ in 0..length {
                    let index = relative_to_usize(target_relative, "BPS TargetCopy offset")?;
                    let byte = *target.get(index).with_context(|| {
                        format!(
                            "BPS TargetCopy offset {index} is not before output position {}",
                            target.len()
                        )
                    })?;
                    target.push(byte);
                    target_relative = target_relative
                        .checked_add(1)
                        .context("BPS TargetCopy cursor overflow")?;
                }
            }
            _ => unreachable!("BPS actions use two bits"),
        }
    }

    ensure!(
        reader.position() == body_end,
        "BPS action stream has {} trailing byte(s)",
        body_end - reader.position()
    );
    let actual_target_crc32 = crc32(&target);
    ensure!(
        actual_target_crc32 == info.target_crc32,
        "BPS target CRC32 mismatch: patch expects {:08x}, got {actual_target_crc32:08x}",
        info.target_crc32
    );

    Ok(AppliedPatch { target, info })
}

fn encode_action(action: u64, length: usize, output: &mut Vec<u8>) -> Result<()> {
    ensure!(length > 0, "BPS actions cannot be empty");
    let length = u64::try_from(length).context("BPS action is too long")?;
    let encoded = (length - 1)
        .checked_shl(2)
        .and_then(|value| value.checked_add(action))
        .context("BPS action encoding overflow")?;
    encode_number(encoded, output);
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceAction {
    Read { length: usize },
    Copy { source_offset: usize, length: usize },
}

fn build_source_index(source: &[u8]) -> HashMap<u64, usize> {
    let mut index = HashMap::new();
    if source.len() < MIN_COPY_MATCH {
        return index;
    }
    index.reserve(source.len() / 2);
    for offset in 0..=source.len() - MIN_COPY_MATCH {
        let key = match_key(&source[offset..offset + MIN_COPY_MATCH]);
        index.entry(key).or_insert(offset);
    }
    index
}

fn best_source_action(
    source: &[u8],
    target: &[u8],
    source_index: &HashMap<u64, usize>,
    target_offset: usize,
) -> Option<SourceAction> {
    let read_length = same_position_length(source, target, target_offset);
    let copy = source_copy_match(source, target, source_index, target_offset);

    if let Some((source_offset, copy_length)) = copy
        && copy_length >= MIN_COPY_MATCH
        && copy_length > read_length.saturating_add(4)
    {
        return Some(SourceAction::Copy {
            source_offset,
            length: copy_length,
        });
    }
    if read_length >= 4 {
        return Some(SourceAction::Read {
            length: read_length,
        });
    }
    copy.filter(|(_, length)| *length >= MIN_COPY_MATCH)
        .map(|(source_offset, length)| SourceAction::Copy {
            source_offset,
            length,
        })
}

fn same_position_length(source: &[u8], target: &[u8], offset: usize) -> usize {
    let limit = source.len().min(target.len());
    if offset >= limit {
        return 0;
    }
    let mut length = 0usize;
    while offset + length < limit && source[offset + length] == target[offset + length] {
        length += 1;
    }
    length
}

fn source_copy_match(
    source: &[u8],
    target: &[u8],
    source_index: &HashMap<u64, usize>,
    target_offset: usize,
) -> Option<(usize, usize)> {
    let key_end = target_offset.checked_add(MIN_COPY_MATCH)?;
    let key_bytes = target.get(target_offset..key_end)?;
    let source_offset = *source_index.get(&match_key(key_bytes))?;
    let max_length = (source.len() - source_offset).min(target.len() - target_offset);
    let mut length = MIN_COPY_MATCH;
    while length < max_length && source[source_offset + length] == target[target_offset + length] {
        length += 1;
    }
    Some((source_offset, length))
}

fn match_key(bytes: &[u8]) -> u64 {
    u64::from_le_bytes(
        bytes[..MIN_COPY_MATCH]
            .try_into()
            .expect("eight-byte BPS match key"),
    )
}

fn encode_relative_offset(
    current: i64,
    source_offset: usize,
    length: usize,
    output: &mut Vec<u8>,
) -> Result<i64> {
    let source_offset = i64::try_from(source_offset).context("BPS SourceCopy offset overflow")?;
    let delta = source_offset
        .checked_sub(current)
        .context("BPS SourceCopy delta overflow")?;
    let magnitude = delta.unsigned_abs();
    let encoded = magnitude
        .checked_shl(1)
        .and_then(|value| value.checked_add(u64::from(delta.is_negative())))
        .context("BPS SourceCopy relative encoding overflow")?;
    encode_number(encoded, output);
    source_offset
        .checked_add(i64::try_from(length).context("BPS SourceCopy length overflow")?)
        .context("BPS SourceCopy cursor overflow")
}

fn encode_number(mut value: u64, output: &mut Vec<u8>) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            output.push(byte | 0x80);
            return;
        }
        output.push(byte);
        value -= 1;
    }
}

fn add_relative_offset(current: i64, encoded: u64) -> Result<i64> {
    let magnitude = i64::try_from(encoded >> 1).context("BPS relative offset is too large")?;
    let delta = if encoded & 1 == 1 {
        magnitude
            .checked_neg()
            .context("BPS negative relative offset overflow")?
    } else {
        magnitude
    };
    current
        .checked_add(delta)
        .context("BPS relative cursor overflow")
}

fn relative_to_usize(value: i64, label: &str) -> Result<usize> {
    usize::try_from(value).with_context(|| format!("{label} is negative or too large: {value}"))
}

fn to_usize(value: u64, label: &str) -> Result<usize> {
    usize::try_from(value).with_context(|| format!("{label} is too large: {value}"))
}

fn read_u32_le(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes.try_into().expect("four-byte CRC32 field"))
}

fn crc32(bytes: &[u8]) -> u32 {
    crc32fast::hash(bytes)
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
    end: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8], position: usize, end: usize) -> Self {
        Self {
            bytes,
            position,
            end,
        }
    }

    fn position(&self) -> usize {
        self.position
    }

    fn read_bytes(&mut self, length: usize) -> Result<&'a [u8]> {
        let start = self.position;
        let end = start
            .checked_add(length)
            .context("BPS read range overflow")?;
        ensure!(
            end <= self.end,
            "BPS stream ended at {}, need {length} byte(s) from {start}",
            self.end
        );
        self.position = end;
        Ok(&self.bytes[start..end])
    }

    fn read_number(&mut self) -> Result<u64> {
        let start = self.position;
        let mut value = 0u64;
        let mut shift = 1u64;
        loop {
            let byte = *self
                .read_bytes(1)?
                .first()
                .context("BPS number byte missing")?;
            let digit = u64::from(byte & 0x7f);
            value = value
                .checked_add(
                    digit
                        .checked_mul(shift)
                        .context("BPS number multiplication overflow")?,
                )
                .context("BPS number addition overflow")?;
            if byte & 0x80 != 0 {
                break;
            }
            shift = shift.checked_shl(7).context("BPS number shift overflow")?;
            value = value
                .checked_add(shift)
                .context("BPS number continuation overflow")?;
        }

        let mut canonical = Vec::new();
        encode_number(value, &mut canonical);
        ensure!(
            canonical == self.bytes[start..self.position],
            "BPS number at offset {start} is not canonically encoded"
        );
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finalize_patch(mut body: Vec<u8>, source: &[u8], target: &[u8]) -> Vec<u8> {
        body.extend_from_slice(&crc32(source).to_le_bytes());
        body.extend_from_slice(&crc32(target).to_le_bytes());
        let checksum = crc32(&body);
        body.extend_from_slice(&checksum.to_le_bytes());
        body
    }

    fn patch_prefix(source_len: usize, target_len: usize) -> Vec<u8> {
        let mut patch = MAGIC.to_vec();
        encode_number(source_len as u64, &mut patch);
        encode_number(target_len as u64, &mut patch);
        encode_number(0, &mut patch);
        patch
    }

    #[test]
    fn bps_numbers_round_trip_at_boundaries() {
        for value in [
            0,
            1,
            126,
            127,
            128,
            129,
            255,
            256,
            16_383,
            16_384,
            u32::MAX as u64,
        ] {
            let mut encoded = Vec::new();
            encode_number(value, &mut encoded);
            let mut reader = Reader::new(&encoded, 0, encoded.len());
            assert_eq!(reader.read_number().unwrap(), value);
            assert_eq!(reader.position(), encoded.len());
        }
    }

    #[test]
    fn creates_and_applies_sparse_patch() {
        let source = b"0123456789abcdefghij";
        let target = b"012XX56789abc---hij!";
        let metadata = br#"{"format":"test"}"#;
        let patch = create_patch(source, target, metadata).unwrap();
        let applied = apply_patch(source, &patch).unwrap();
        assert_eq!(applied.target, target);
        assert_eq!(applied.info.metadata, metadata);
    }

    #[test]
    fn encoder_reuses_data_moved_from_another_source_offset() {
        let source = b"abcdefgh----";
        let target = b"xxxxabcdefgh";
        let patch = create_patch(source, target, b"").unwrap();
        inspect_patch(&patch).unwrap();
        let body_end = patch.len() - FOOTER_LEN;
        let mut reader = Reader::new(&patch, MAGIC.len(), body_end);
        reader.read_number().unwrap();
        reader.read_number().unwrap();
        let metadata_size = reader.read_number().unwrap() as usize;
        reader.read_bytes(metadata_size).unwrap();

        let first = reader.read_number().unwrap();
        assert_eq!(first & 3, TARGET_READ);
        reader.read_bytes((first >> 2) as usize + 1).unwrap();
        let second = reader.read_number().unwrap();
        assert_eq!(second & 3, SOURCE_COPY);
        assert_eq!((second >> 2) + 1, 8);
        assert_eq!(apply_patch(source, &patch).unwrap().target, target);
    }

    #[test]
    fn applies_source_copy_action() {
        let source = b"abcd";
        let target = b"bcd";
        let mut patch = patch_prefix(source.len(), target.len());
        encode_action(SOURCE_COPY, target.len(), &mut patch).unwrap();
        encode_number(2, &mut patch); // positive relative offset +1
        let patch = finalize_patch(patch, source, target);
        assert_eq!(apply_patch(source, &patch).unwrap().target, target);
    }

    #[test]
    fn applies_a_negative_source_copy_offset() {
        let source = b"abcdef";
        let target = b"efab";
        let mut patch = patch_prefix(source.len(), target.len());
        encode_action(SOURCE_COPY, 2, &mut patch).unwrap();
        encode_number(8, &mut patch); // positive relative offset +4
        encode_action(SOURCE_COPY, 2, &mut patch).unwrap();
        encode_number(13, &mut patch); // negative relative offset -6
        let patch = finalize_patch(patch, source, target);
        assert_eq!(apply_patch(source, &patch).unwrap().target, target);
    }

    #[test]
    fn applies_overlapping_target_copy_action() {
        let source = b"";
        let target = b"abcabcabc";
        let mut patch = patch_prefix(source.len(), target.len());
        encode_action(TARGET_READ, 3, &mut patch).unwrap();
        patch.extend_from_slice(b"abc");
        encode_action(TARGET_COPY, 6, &mut patch).unwrap();
        encode_number(0, &mut patch); // target-relative cursor remains at zero
        let patch = finalize_patch(patch, source, target);
        assert_eq!(apply_patch(source, &patch).unwrap().target, target);
    }

    #[test]
    fn rejects_wrong_source_and_corrupt_patch() {
        let source = b"source";
        let target = b"target";
        let patch = create_patch(source, target, b"").unwrap();
        let error = apply_patch(b"sourcf", &patch).unwrap_err().to_string();
        assert!(error.contains("source CRC32 mismatch"));

        let mut corrupt = patch;
        corrupt[4] ^= 1;
        let error = inspect_patch(&corrupt).unwrap_err().to_string();
        assert!(error.contains("patch CRC32 mismatch"));
    }
}
