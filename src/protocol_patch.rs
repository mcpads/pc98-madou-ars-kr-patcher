//! A.R.S distribution packages using the public PC-98 FAT12 patch protocol.
//!
//! The production build first composes private content HDMs. This module turns
//! only their logical FAT12 file changes into independently applicable package
//! ZIPs, then combines the seven required results into one protocol patch set.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, ensure};
use pc98_fat12_patcher_core::{
    PACKAGE_FORMAT, PatchSetPackageInput, apply_patch_package, create_patch_package,
    create_patch_set, inspect_patch_package, inspect_patch_set, materialize_patch_artifact_member,
};
use serde_json::json;

use crate::{fat12::Fat12Volume, hdm::HdmFile, media_identity::sha256_hex};

pub const PATCH_SET_ID: &str = "pc98-madou-ars-kr-complete-set";
pub const PATCH_SET_TITLE: &str = "마도물어 A.R.S 한글패치";

#[derive(Debug, Clone)]
pub struct ProtocolPackage {
    pub key: String,
    pub label: String,
    pub bytes: Vec<u8>,
    pub target_sha256: String,
    pub retained_files: usize,
    pub patched_files: usize,
    source: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
pub struct ProtocolPackageSpec<'a> {
    pub key: &'a str,
    pub label: &'a str,
    pub title: &'a str,
    pub output_filename: &'a str,
    pub source: &'a [u8],
    pub content: &'a [u8],
}

#[derive(Debug, Clone)]
struct LogicalFile {
    name: String,
    bytes: Vec<u8>,
}

pub fn create_protocol_package(spec: ProtocolPackageSpec<'_>) -> Result<ProtocolPackage> {
    let source_files = read_root_files(spec.source, "protocol source")?;
    let content_files = read_root_files(spec.content, "protocol content")?;
    let source_by_name = source_files
        .iter()
        .map(|file| (file.name.as_str(), file))
        .collect::<BTreeMap<_, _>>();

    let mut retained_files = Vec::new();
    let mut placed_files = Vec::new();
    for file in &content_files {
        match source_by_name.get(file.name.as_str()) {
            Some(source) if source.bytes == file.bytes => retained_files.push(json!({
                "name": file.name,
                "size": file.bytes.len(),
                "sha256": sha256_hex(&file.bytes),
            })),
            source => {
                let (source_selector, source_size, source_sha256) = match source {
                    Some(source) => (
                        json!({ "kind": "root_file", "name": source.name }),
                        source.bytes.len(),
                        sha256_hex(&source.bytes),
                    ),
                    None => (json!({ "kind": "empty" }), 0, sha256_hex(&[])),
                };
                placed_files.push(json!({
                    "patch_key": file.name,
                    "name": file.name,
                    "source": source_selector,
                    "source_size": source_size,
                    "source_sha256": source_sha256,
                    "transform": { "kind": "bps" },
                }));
            }
        }
    }

    let hdm = HdmFile::parse(spec.source).context("parse protocol source HDM")?;
    let volume = Fat12Volume::open(hdm.as_bytes()).context("open protocol source FAT12")?;
    let bpb = volume.bpb;
    let plan = json!({
        "format": PACKAGE_FORMAT,
        "id": format!("pc98-madou-ars-kr-{}", spec.key),
        "title": spec.title,
        "output_filename": spec.output_filename,
        "source": {
            "size": spec.source.len(),
            "sha256": sha256_hex(spec.source),
            "geometry": {
                "bytes_per_sector": bpb.bytes_per_sector,
                "sectors_per_cluster": bpb.sectors_per_cluster,
                "reserved_sectors": bpb.reserved_sectors,
                "fat_count": bpb.num_fats,
                "root_entries": bpb.root_dir_entries,
                "total_sectors": bpb.total_sectors,
                "media_descriptor": bpb.media_descriptor,
                "sectors_per_fat": bpb.sectors_per_fat,
                "sectors_per_track": hdm.geometry.sectors_per_track,
                "heads": hdm.geometry.heads,
            },
            "mount_policy": "pc98_dos3",
        },
        "assembly": {
            "retained_files": retained_files,
            "placed_files": placed_files,
        },
    });
    let plan_json = serde_json::to_string_pretty(&plan).context("serialize protocol plan")?;
    let package = create_patch_package(&plan_json, spec.source, spec.content)
        .with_context(|| format!("create protocol package {}", spec.key))?;
    let inspected = inspect_patch_package(&package)
        .with_context(|| format!("inspect protocol package {}", spec.key))?;
    ensure!(
        inspected.recipe.format == PACKAGE_FORMAT,
        "protocol package {} used unexpected format {}",
        spec.key,
        inspected.recipe.format
    );
    let target = apply_patch_package(spec.source, &package)
        .with_context(|| format!("self-apply protocol package {}", spec.key))?;
    require_same_logical_files(spec.content, &target)
        .with_context(|| format!("verify protocol package {} logical target", spec.key))?;
    ensure!(
        sha256_hex(&target) == inspected.recipe.target.sha256,
        "protocol package {} target hash differs after self-apply",
        spec.key
    );

    Ok(ProtocolPackage {
        key: spec.key.to_owned(),
        label: spec.label.to_owned(),
        bytes: package,
        target_sha256: inspected.recipe.target.sha256,
        retained_files: inspected.recipe.assembly.retained_files.len(),
        patched_files: inspected.patches.len(),
        source: spec.source.to_vec(),
    })
}

pub fn create_protocol_patch_set(packages: Vec<ProtocolPackage>) -> Result<Vec<u8>> {
    let members = packages
        .iter()
        .map(|package| PatchSetPackageInput {
            key: package.key.clone(),
            label: package.label.clone(),
            package: package.bytes.clone(),
        })
        .collect();
    let patch_set = create_patch_set(PATCH_SET_ID, PATCH_SET_TITLE, members)
        .context("create A.R.S protocol patch set")?;
    let inspected = inspect_patch_set(&patch_set).context("inspect A.R.S protocol patch set")?;
    ensure!(
        inspected.manifest.id == PATCH_SET_ID,
        "protocol patch set changed its id"
    );
    ensure!(
        inspected.manifest.members.len() == packages.len(),
        "protocol patch set member count changed"
    );
    for package in &packages {
        ensure!(
            inspected.packages.get(&package.key) == Some(&package.bytes),
            "protocol patch set changed nested package {}",
            package.key
        );
        let target = materialize_patch_artifact_member(&package.source, &patch_set, &package.key)
            .with_context(|| format!("materialize patch-set member {}", package.key))?;
        ensure!(
            sha256_hex(&target) == package.target_sha256,
            "patch-set member {} materialized a different target",
            package.key
        );
    }
    Ok(patch_set)
}

fn require_same_logical_files(expected: &[u8], actual: &[u8]) -> Result<()> {
    let expected = logical_file_map(read_root_files(expected, "expected content")?)?;
    let actual = logical_file_map(read_root_files(actual, "applied target")?)?;
    ensure!(
        expected.keys().eq(actual.keys()),
        "logical target file names differ: expected {:?}, got {:?}",
        expected.keys().collect::<Vec<_>>(),
        actual.keys().collect::<Vec<_>>()
    );
    for (name, expected_bytes) in expected {
        let actual_bytes = actual
            .get(&name)
            .with_context(|| format!("applied target is missing {name}"))?;
        ensure!(
            actual_bytes == &expected_bytes,
            "applied target file differs: {name}"
        );
    }
    Ok(())
}

fn logical_file_map(files: Vec<LogicalFile>) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut mapped = BTreeMap::new();
    for file in files {
        ensure!(
            mapped.insert(file.name.clone(), file.bytes).is_none(),
            "duplicate FAT12 root file {}",
            file.name
        );
    }
    Ok(mapped)
}

fn read_root_files(disk: &[u8], label: &str) -> Result<Vec<LogicalFile>> {
    let hdm = HdmFile::parse(disk).with_context(|| format!("parse {label} HDM"))?;
    let volume =
        Fat12Volume::open(hdm.as_bytes()).with_context(|| format!("open {label} FAT12"))?;
    let mut names = BTreeSet::new();
    let mut files = Vec::new();
    for entry in volume.list_files() {
        ensure!(
            entry.attr & 0x10 == 0,
            "{label} contains an unsupported root directory: {}",
            entry.name
        );
        ensure!(
            entry.name.is_ascii() && entry.name == entry.name.to_ascii_uppercase(),
            "{label} contains a non-ASCII or non-canonical SFN: {:?}",
            entry.name
        );
        ensure!(
            names.insert(entry.name.clone()),
            "{label} contains a duplicate root name: {}",
            entry.name
        );
        let bytes = volume
            .read_entry(&entry)
            .with_context(|| format!("read {label} file {}", entry.name))?;
        files.push(LogicalFile {
            name: entry.name,
            bytes,
        });
    }
    Ok(files)
}
