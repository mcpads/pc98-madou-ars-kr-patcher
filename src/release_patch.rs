//! Project-specific release wrapper around BPS.
//!
//! BPS supplies source/target/patch CRC32 checks. This layer additionally pins
//! the exact primary A.R.S Game/Data-disk or shared Demo-disk SHA-256 and embeds
//! deterministic metadata, so a draft or release patch cannot silently target
//! an alternate dump.

use crate::{
    bps,
    fat12::Fat12Volume,
    hdm::HdmFile,
    media_identity::{self, identify_patch_source},
};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

pub use crate::media_identity::MediaDisk as OriginalPatchSource;
pub type OriginalGameDisk = OriginalPatchSource;

pub const PROJECT_ID: &str = "pc98_madou_ars";
pub const METADATA_FORMAT: &str = "pc98_madou_ars.bps.v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseMetadata {
    pub source_id: String,
    pub source_sha256: String,
    pub source_size: usize,
    pub target_sha256: String,
    pub target_size: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedReleasePatch {
    pub patch: Vec<u8>,
    pub source: OriginalPatchSource,
    pub target_sha256: String,
    pub info: bps::PatchInfo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedReleasePatch {
    pub target: Vec<u8>,
    pub source: OriginalPatchSource,
    pub target_sha256: String,
    pub info: bps::PatchInfo,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    media_identity::sha256_hex(bytes)
}

pub fn original_game_disks() -> Result<Vec<OriginalGameDisk>> {
    media_identity::original_game_disks()
}

pub fn identify_original_game_disk(bytes: &[u8]) -> Result<OriginalGameDisk> {
    media_identity::identify_game_disk(bytes)
}

pub fn original_patch_sources() -> Result<Vec<OriginalPatchSource>> {
    media_identity::original_patch_sources()
}

pub fn identify_original_patch_source(bytes: &[u8]) -> Result<OriginalPatchSource> {
    identify_patch_source(bytes)
}

/// Create a BPS patch only from a recognized primary Game/Data HDM or shared
/// Demo HDM to another HDM with identical geometry and readable FAT12 files. The
/// freshly encoded patch is immediately applied and byte-compared before it is
/// returned.
pub fn create_release_patch(source: &[u8], target: &[u8]) -> Result<CreatedReleasePatch> {
    let source_identity = identify_original_patch_source(source)?;
    validate_hdm_pair(source, target)?;

    let target_sha256 = sha256_hex(target);
    let metadata = encode_metadata(&source_identity, &target_sha256, target.len())?;
    let patch = bps::create_patch(source, target, &metadata)?;
    let applied = bps::apply_patch(source, &patch).context("self-apply freshly created BPS")?;
    ensure!(
        applied.target == target,
        "freshly created BPS did not reproduce the target byte-for-byte"
    );
    let parsed = parse_metadata(&applied.info.metadata)?;
    validate_metadata(
        &parsed,
        &source_identity,
        Some(&target_sha256),
        target.len(),
    )?;

    Ok(CreatedReleasePatch {
        patch,
        source: source_identity,
        target_sha256,
        info: applied.info,
    })
}

/// Apply a project BPS patch only to the exact source HDM named by its embedded
/// SHA-256 metadata, then verify the resulting HDM, FAT12 files, and target
/// SHA-256.
pub fn apply_release_patch(source: &[u8], patch: &[u8]) -> Result<AppliedReleasePatch> {
    let source_identity = identify_original_patch_source(source)?;
    let inspected = bps::inspect_patch(patch)?;
    let metadata = parse_metadata(&inspected.metadata)?;
    validate_metadata(&metadata, &source_identity, None, inspected.target_size)?;

    let applied = bps::apply_patch(source, patch)?;
    validate_hdm_pair(source, &applied.target)?;
    let target_sha256 = sha256_hex(&applied.target);
    validate_metadata(
        &metadata,
        &source_identity,
        Some(&target_sha256),
        applied.target.len(),
    )?;

    Ok(AppliedReleasePatch {
        target: applied.target,
        source: source_identity,
        target_sha256,
        info: applied.info,
    })
}

fn validate_hdm_pair(source: &[u8], target: &[u8]) -> Result<()> {
    let source = HdmFile::parse(source).context("parse source Game HDM")?;
    let target = HdmFile::parse(target).context("parse target Game HDM")?;
    ensure!(
        source.geometry == target.geometry,
        "source/target HDM geometry differs: {:?} vs {:?}",
        source.geometry,
        target.geometry
    );
    validate_fat12_files(&source, "source")?;
    validate_fat12_files(&target, "target")?;
    Ok(())
}

fn validate_fat12_files(hdm: &HdmFile, label: &str) -> Result<()> {
    let volume =
        Fat12Volume::open(hdm.as_bytes()).with_context(|| format!("open {label} FAT12"))?;
    for entry in volume.list_files() {
        if entry.attr & 0x10 == 0 {
            volume
                .read_entry(&entry)
                .with_context(|| format!("read {label} FAT12 file {}", entry.name))?;
        }
    }
    Ok(())
}

fn encode_metadata(
    source: &OriginalPatchSource,
    target_sha256: &str,
    target_size: usize,
) -> Result<Vec<u8>> {
    serde_json::to_vec(&json!({
        "format": METADATA_FORMAT,
        "project": PROJECT_ID,
        "source": {
            "id": source.id.as_str(),
            "sha256": source.sha256.as_str(),
            "size": source.size,
        },
        "target": {
            "sha256": target_sha256,
            "size": target_size,
        },
    }))
    .context("serialize BPS release metadata")
}

fn parse_metadata(bytes: &[u8]) -> Result<ReleaseMetadata> {
    let root: Value = serde_json::from_slice(bytes).context("parse BPS release metadata JSON")?;
    ensure!(
        string_field(&root, "format")? == METADATA_FORMAT,
        "BPS metadata format is not {METADATA_FORMAT}"
    );
    ensure!(
        string_field(&root, "project")? == PROJECT_ID,
        "BPS metadata project is not {PROJECT_ID}"
    );
    let source = object_field(&root, "source")?;
    let target = object_field(&root, "target")?;
    Ok(ReleaseMetadata {
        source_id: string_field(source, "id")?.to_owned(),
        source_sha256: string_field(source, "sha256")?.to_owned(),
        source_size: usize_field(source, "size")?,
        target_sha256: string_field(target, "sha256")?.to_owned(),
        target_size: usize_field(target, "size")?,
    })
}

fn validate_metadata(
    metadata: &ReleaseMetadata,
    source: &OriginalPatchSource,
    actual_target_sha256: Option<&str>,
    actual_target_size: usize,
) -> Result<()> {
    ensure!(
        metadata.source_id == source.id,
        "BPS expects source {}, but input is {}",
        metadata.source_id,
        source.id
    );
    ensure!(
        metadata.source_sha256 == source.sha256,
        "BPS source SHA-256 metadata does not match {}",
        source.id
    );
    ensure!(
        metadata.source_size == source.size,
        "BPS source size metadata is {}, expected {}",
        metadata.source_size,
        source.size
    );
    ensure!(
        metadata.target_size == actual_target_size,
        "BPS target size metadata is {}, actual/header size is {actual_target_size}",
        metadata.target_size
    );
    if let Some(actual) = actual_target_sha256 {
        ensure!(
            metadata.target_sha256 == actual,
            "BPS target SHA-256 mismatch: metadata {}, actual {actual}",
            metadata.target_sha256
        );
    }
    Ok(())
}

fn object_field<'a>(value: &'a Value, field: &str) -> Result<&'a Value> {
    value
        .get(field)
        .filter(|value| value.is_object())
        .with_context(|| format!("JSON field {field:?} is missing or not an object"))
}

fn string_field<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .with_context(|| format!("JSON field {field:?} is missing or not a string"))
}

fn usize_field(value: &Value, field: &str) -> Result<usize> {
    let raw = value
        .get(field)
        .and_then(Value::as_u64)
        .with_context(|| format!("JSON field {field:?} is missing or not an integer"))?;
    usize::try_from(raw).with_context(|| format!("JSON field {field:?} is too large"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_standard_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn source_manifest_is_complete_and_unique() {
        let disks = original_game_disks().unwrap();
        assert_eq!(
            disks
                .iter()
                .map(|disk| disk.id.as_str())
                .collect::<Vec<_>>(),
            ["arle_game", "rulue_game", "schezo_game"]
        );
        assert!(disks.iter().all(|disk| disk.size == 1_261_568));
    }

    #[test]
    fn release_metadata_round_trips() {
        let source = &original_game_disks().unwrap()[0];
        let target_sha256 = "11".repeat(32);
        let encoded = encode_metadata(source, &target_sha256, source.size).unwrap();
        let parsed = parse_metadata(&encoded).unwrap();
        validate_metadata(&parsed, source, Some(&target_sha256), source.size).unwrap();
    }

    #[test]
    fn release_metadata_rejects_another_game_disk() {
        let disks = original_game_disks().unwrap();
        let source = &disks[0];
        let other = &disks[1];
        let target_sha256 = "22".repeat(32);
        let encoded = encode_metadata(source, &target_sha256, source.size).unwrap();
        let parsed = parse_metadata(&encoded).unwrap();
        let error = validate_metadata(&parsed, other, None, source.size)
            .unwrap_err()
            .to_string();
        assert!(error.contains("expects source arle_game"));
    }

    #[test]
    fn malformed_metadata_fails_closed() {
        let error = parse_metadata(br#"{"project":"pc98_madou_ars"}"#)
            .unwrap_err()
            .to_string();
        assert!(error.contains("format"));
    }
}
