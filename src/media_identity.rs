//! Exact original-media identities used by the reproducible build.

use anyhow::{Context, Result, ensure};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;

const MANIFEST_FORMAT: &str = "pc98_madou_ars.original_build_media.v1";
const MANIFEST: &str = include_str!("../assets/media/original_build_media.json");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaDisk {
    pub id: String,
    pub label: String,
    pub file_name: String,
    pub sha256: String,
    pub size: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CharacterMedia {
    pub id: String,
    pub game: MediaDisk,
    pub data: MediaDisk,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildMediaManifest {
    pub demo: MediaDisk,
    pub characters: Vec<CharacterMedia>,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn load_build_media() -> Result<BuildMediaManifest> {
    let root: Value =
        serde_json::from_str(MANIFEST).context("parse assets/media/original_build_media.json")?;
    ensure!(
        string_field(&root, "format")? == MANIFEST_FORMAT,
        "original build-media manifest format is not {MANIFEST_FORMAT}"
    );
    let demo = parse_disk(object_field(&root, "demo")?, "demo")?;
    ensure!(demo.id == "demo", "shared Demo media id must be demo");
    let entries = array_field(&root, "characters")?;
    ensure!(
        entries.len() == 3,
        "build-media manifest must contain exactly three characters, got {}",
        entries.len()
    );

    let mut characters = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        ensure!(
            entry.is_object(),
            "character manifest entry {index} is not an object"
        );
        let id = string_field(entry, "id")?.to_owned();
        let game = parse_disk(object_field(entry, "game")?, &format!("{id}.game"))?;
        let data = parse_disk(object_field(entry, "data")?, &format!("{id}.data"))?;
        ensure!(
            game.id == format!("{id}_game"),
            "{id} Game media id must be {id}_game"
        );
        ensure!(
            data.id == format!("{id}_data"),
            "{id} Data media id must be {id}_data"
        );
        characters.push(CharacterMedia { game, data, id });
    }

    validate_manifest_uniqueness(&demo, &characters)?;
    Ok(BuildMediaManifest { demo, characters })
}

pub fn original_game_disks() -> Result<Vec<MediaDisk>> {
    Ok(load_build_media()?
        .characters
        .into_iter()
        .map(|character| character.game)
        .collect())
}

/// Exact original media that may own a distributable BPS target. Character
/// Data disks are included because the shared scenario-title screen lives in
/// `ARS_RX.CS` on each one.
pub fn original_patch_sources() -> Result<Vec<MediaDisk>> {
    let manifest = load_build_media()?;
    Ok(std::iter::once(manifest.demo)
        .chain(
            manifest
                .characters
                .into_iter()
                .flat_map(|character| [character.game, character.data]),
        )
        .collect())
}

pub fn identify_patch_source(bytes: &[u8]) -> Result<MediaDisk> {
    let actual = sha256_hex(bytes);
    let accepted = original_patch_sources()?;
    if let Some(disk) = accepted
        .iter()
        .find(|disk| disk.size == bytes.len() && disk.sha256 == actual)
    {
        return Ok(disk.clone());
    }
    let accepted = accepted
        .iter()
        .map(|disk| format!("{}={}", disk.id, disk.sha256))
        .collect::<Vec<_>>()
        .join(", ");
    anyhow::bail!(
        "source is not a distributable A.R.S Game/Data/Demo HDM (size {}, SHA-256 {actual}); accepted: {accepted}",
        bytes.len()
    )
}

pub fn identify_character(game: &[u8]) -> Result<CharacterMedia> {
    let actual = sha256_hex(game);
    let manifest = load_build_media()?;
    if let Some(character) = manifest
        .characters
        .iter()
        .find(|character| character.game.size == game.len() && character.game.sha256 == actual)
    {
        return Ok(character.clone());
    }
    let accepted = manifest
        .characters
        .iter()
        .map(|character| format!("{}={}", character.game.id, character.game.sha256))
        .collect::<Vec<_>>()
        .join(", ");
    anyhow::bail!(
        "source is not a primary A.R.S Game HDM (size {}, SHA-256 {actual}); accepted: {accepted}",
        game.len()
    )
}

pub fn identify_game_disk(game: &[u8]) -> Result<MediaDisk> {
    Ok(identify_character(game)?.game)
}

pub fn validate_build_inputs(game: &[u8], demo: &[u8], data: &[u8]) -> Result<CharacterMedia> {
    let character = identify_character(game)?;
    let manifest = load_build_media()?;
    validate_disk(demo, &manifest.demo).context("validate Demo disk")?;
    validate_disk(data, &character.data)
        .with_context(|| format!("validate {} Data disk", character.id))?;
    Ok(character)
}

pub fn validate_disk(bytes: &[u8], expected: &MediaDisk) -> Result<()> {
    let actual = sha256_hex(bytes);
    ensure!(
        bytes.len() == expected.size && actual == expected.sha256,
        "{} is not the expected media (size {}, SHA-256 {actual}); expected size {}, SHA-256 {}",
        expected.label,
        bytes.len(),
        expected.size,
        expected.sha256
    );
    Ok(())
}

fn parse_disk(value: &Value, label: &str) -> Result<MediaDisk> {
    ensure!(
        value.is_object(),
        "media manifest field {label} is not an object"
    );
    let disk = MediaDisk {
        id: string_field(value, "id")?.to_owned(),
        label: string_field(value, "label")?.to_owned(),
        file_name: string_field(value, "file_name")?.to_owned(),
        size: usize_field(value, "size")?,
        sha256: string_field(value, "sha256")?.to_owned(),
    };
    ensure!(
        disk.size > 0,
        "media manifest entry {} has size zero",
        disk.id
    );
    ensure!(
        !disk.id.is_empty() && !disk.label.is_empty() && !disk.file_name.is_empty(),
        "media manifest entry {label} has an empty id, label, or file_name"
    );
    ensure!(
        disk.sha256.len() == 64
            && disk
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "media manifest entry {} has an invalid lowercase SHA-256",
        disk.id
    );
    Ok(disk)
}

fn validate_manifest_uniqueness(demo: &MediaDisk, characters: &[CharacterMedia]) -> Result<()> {
    let expected_characters = HashSet::from(["arle", "rulue", "schezo"]);
    let actual_characters: HashSet<_> = characters.iter().map(|entry| entry.id.as_str()).collect();
    ensure!(
        actual_characters == expected_characters,
        "build-media character ids must be arle, rulue, and schezo"
    );

    let mut ids = HashSet::new();
    let mut hashes = HashSet::new();
    for disk in std::iter::once(demo).chain(
        characters
            .iter()
            .flat_map(|character| [&character.game, &character.data]),
    ) {
        ensure!(
            ids.insert(disk.id.as_str()),
            "duplicate media id {}",
            disk.id
        );
        ensure!(
            hashes.insert(disk.sha256.as_str()),
            "duplicate media SHA-256 {}",
            disk.sha256
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

fn array_field<'a>(value: &'a Value, field: &str) -> Result<&'a [Value]> {
    value
        .get(field)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .with_context(|| format!("JSON field {field:?} is missing or not an array"))
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
    fn manifest_has_all_build_media() {
        let manifest = load_build_media().unwrap();
        assert_eq!(manifest.demo.id, "demo");
        assert_eq!(
            manifest
                .characters
                .iter()
                .map(|character| character.id.as_str())
                .collect::<Vec<_>>(),
            ["arle", "rulue", "schezo"]
        );
        assert!(
            manifest
                .characters
                .iter()
                .all(|character| character.game.size == 1_261_568
                    && character.data.size == 1_261_568)
        );
    }

    #[test]
    fn patch_sources_include_demo_games_and_data_disks() {
        let disks = original_patch_sources().unwrap();
        assert_eq!(
            disks
                .iter()
                .map(|disk| disk.id.as_str())
                .collect::<Vec<_>>(),
            [
                "demo",
                "arle_game",
                "arle_data",
                "rulue_game",
                "rulue_data",
                "schezo_game",
                "schezo_data"
            ]
        );
    }

    #[test]
    fn sha256_matches_standard_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
