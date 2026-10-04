//! Source-bound configuration for the three translated scenario builds.
//!
//! The command layer only orchestrates a selected profile. Binary ownership,
//! translation denominators, and renderer selection stay together here so a
//! new command branch cannot silently pair one character's data with another
//! character's executable.

use std::collections::HashSet;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::{
    hook_geometry::RendererOverlay, overlay_lz::decode_overlay_lz, read_fat12_file_from_hdm,
};

pub const RENDERER_IMAGE_FILE: &str = "KFONT.BIN";

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SourceSlotApplication {
    #[default]
    Translate,
    PreserveSource,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StagedSourceSlot {
    pub id: String,
    pub decoded_offset: usize,
    pub byte_budget: usize,
    pub raw: Vec<u8>,
    pub application: SourceSlotApplication,
}

impl StagedSourceSlot {
    /// Parse the immutable source fields carried by one translation row. These
    /// fields are not allocation hints: they are a source-identity contract
    /// that the character builder rechecks against the exact decoded overlay.
    pub fn from_json(entry: &Value, label: &str) -> Result<Self> {
        let id = entry
            .get("id")
            .and_then(Value::as_str)
            .with_context(|| format!("{label}: missing id"))?
            .to_owned();
        let decoded_offset = entry
            .get("string_decoded_offset")
            .and_then(Value::as_str)
            .with_context(|| format!("{label}: missing string_decoded_offset"))
            .and_then(parse_hex_offset)?;
        let byte_budget = entry
            .get("byte_budget")
            .and_then(Value::as_u64)
            .with_context(|| format!("{label}: missing byte_budget"))?
            as usize;
        let raw_hex = entry
            .get("raw_hex")
            .and_then(Value::as_str)
            .with_context(|| format!("{label}: missing raw_hex"))?;
        let raw = decode_hex(raw_hex).with_context(|| format!("{label}: invalid raw_hex"))?;
        let application = match entry.get("application").and_then(Value::as_str) {
            None | Some("translate") => SourceSlotApplication::Translate,
            Some("preserve_source") => SourceSlotApplication::PreserveSource,
            Some(value) => bail!("{label}: unsupported application {value:?}"),
        };
        if byte_budget != raw.len() + 1 {
            bail!(
                "{label}: byte_budget {byte_budget} does not equal raw {} + terminator",
                raw.len()
            );
        }
        Ok(Self {
            id,
            decoded_offset,
            byte_budget,
            raw,
            application,
        })
    }

    pub fn validate_against(&self, decoded: &[u8], overlay: &str) -> Result<()> {
        let raw_end = self
            .decoded_offset
            .checked_add(self.raw.len())
            .context("translation source range overflow")?;
        let slot_end = self
            .decoded_offset
            .checked_add(self.byte_budget)
            .context("translation source slot overflow")?;
        if decoded.get(self.decoded_offset..raw_end) != Some(self.raw.as_slice()) {
            bail!(
                "{overlay}: {} raw_hex does not match exact source bytes at 0x{:04X}",
                self.id,
                self.decoded_offset
            );
        }
        let terminator = decoded.get(raw_end).with_context(|| {
            format!(
                "{overlay}: {} source slot 0x{:04X}..0x{slot_end:04X} is out of range",
                self.id, self.decoded_offset
            )
        })?;
        if !crate::renderer_control::is_terminator(*terminator) {
            bail!(
                "{overlay}: {} source slot at 0x{:04X} has non-terminator 0x{terminator:02X}",
                self.id,
                self.decoded_offset
            );
        }
        Ok(())
    }
}

/// Find source strings that are part of the overlay's disk identity rather
/// than renderer text.
///
/// All three scenario overlays compare the fixed-width identity against a disk
/// header (`mov si, identity; ...; repe cmpsw`) and later pass the exact same
/// pointer to `INT 7Bh / AH=0`. Replacing this byte sequence changes media
/// selection semantics. Requiring both independent consumers prevents a lone
/// instruction-shaped byte sequence from becoming a preservation rule.
pub fn source_identity_slots(decoded: &[u8], load_offset: usize) -> HashSet<usize> {
    const COMPARE_SUFFIX: [u8; 8] = [0x8B, 0xFB, 0xB9, 0x07, 0x00, 0xF3, 0xA7, 0x74];
    const FILE_CALL_SUFFIX: [u8; 4] = [0xB4, 0x00, 0xCD, 0x7B];
    const MAX_SECOND_CONSUMER_DISTANCE: usize = 0x80;

    let mut protected = HashSet::new();
    for site in 0..decoded.len().saturating_sub(12) {
        if decoded[site] != 0xBE
            || decoded.get(site + 3..site + 11) != Some(COMPARE_SUFFIX.as_slice())
        {
            continue;
        }
        let logical = u16::from_le_bytes([decoded[site + 1], decoded[site + 2]]) as usize;
        let Some(offset) = logical.checked_sub(load_offset) else {
            continue;
        };
        let Some(raw) = crate::renderer_control::message_bytes(decoded, offset) else {
            continue;
        };
        if raw.len() != 14 {
            continue;
        }

        let search_end = decoded.len().min(site + MAX_SECOND_CONSUMER_DISTANCE);
        let used_by_file_call = (site + 11..search_end.saturating_sub(6)).any(|candidate| {
            decoded[candidate] == 0xBA
                && decoded.get(candidate + 1..candidate + 3) == Some(&decoded[site + 1..site + 3])
                && decoded.get(candidate + 3..candidate + 7) == Some(FILE_CALL_SUFFIX.as_slice())
        });
        if used_by_file_call {
            protected.insert(offset);
        }
    }
    protected
}

fn parse_hex_offset(text: &str) -> Result<usize> {
    let digits = text.strip_prefix("0x").unwrap_or(text);
    usize::from_str_radix(digits, 16).with_context(|| format!("invalid hex offset {text:?}"))
}

fn decode_hex(text: &str) -> Result<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        bail!("hex string has odd length")
    }
    (0..text.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&text[index..index + 2], 16)
                .with_context(|| format!("invalid hex byte at {index}"))
        })
        .collect()
}

#[derive(Clone, Copy, Debug)]
pub struct CharacterBuildProfile {
    pub id: &'static str,
    pub game_overlay: &'static str,
    pub renderer_overlay: RendererOverlay,
    pub expected_cataloged: usize,
    pub expected_uncovered: usize,
    pub expected_preserved: usize,
    pub expected_translations: usize,
}

pub const CHARACTER_BUILD_PROFILES: [CharacterBuildProfile; 3] = [
    CharacterBuildProfile {
        id: "arle",
        game_overlay: "GAME_A.OVL",
        renderer_overlay: RendererOverlay::Arle,
        expected_cataloged: 367,
        expected_uncovered: 56,
        expected_preserved: 1,
        expected_translations: 424,
    },
    CharacterBuildProfile {
        id: "rulue",
        game_overlay: "GAME_R.OVL",
        renderer_overlay: RendererOverlay::Rulue,
        expected_cataloged: 385,
        expected_uncovered: 58,
        expected_preserved: 1,
        expected_translations: 444,
    },
    CharacterBuildProfile {
        id: "schezo",
        game_overlay: "GAME_S.OVL",
        renderer_overlay: RendererOverlay::Schezo,
        expected_cataloged: 326,
        expected_uncovered: 51,
        expected_preserved: 1,
        expected_translations: 378,
    },
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NormalSelectorOverlayExtent {
    pub source_decoded_bytes: usize,
    pub product_decoded_bytes: usize,
}

pub fn profile_for_id(id: &str) -> Option<&'static CharacterBuildProfile> {
    CHARACTER_BUILD_PROFILES
        .iter()
        .find(|profile| profile.id == id)
}

/// Protect the normal Demo-selector ownership boundary for a GAME overlay.
///
/// The selector hot-swaps the decoded GAME image into a live segment. Bytes
/// immediately after the source overlay extent are therefore not unowned free
/// space: growing or shrinking the decoded product changes that segment's
/// layout. A release product must keep the exact source extent until overflow
/// text/code is moved to storage with a separately proven runtime owner.
pub fn validate_normal_selector_overlay_extent(
    source_disk: &[u8],
    product_disk: &[u8],
    profile: &CharacterBuildProfile,
) -> Result<NormalSelectorOverlayExtent> {
    let source_packed = read_fat12_file_from_hdm(source_disk, profile.game_overlay)
        .with_context(|| format!("read source {}", profile.game_overlay))?;
    let product_packed = read_fat12_file_from_hdm(product_disk, profile.game_overlay)
        .with_context(|| format!("read product {}", profile.game_overlay))?;
    let source_decoded_bytes = decode_overlay_lz(&source_packed)
        .with_context(|| format!("decode source {}", profile.game_overlay))?
        .output
        .len();
    let product_decoded_bytes = decode_overlay_lz(&product_packed)
        .with_context(|| format!("decode product {}", profile.game_overlay))?
        .output
        .len();

    validate_decoded_extent(
        profile.game_overlay,
        source_decoded_bytes,
        product_decoded_bytes,
    )
}

fn validate_decoded_extent(
    overlay: &str,
    source_decoded_bytes: usize,
    product_decoded_bytes: usize,
) -> Result<NormalSelectorOverlayExtent> {
    if product_decoded_bytes != source_decoded_bytes {
        let delta = product_decoded_bytes as isize - source_decoded_bytes as isize;
        bail!(
            "normal Demo selector requires source-exact decoded {overlay} extent: source \
             0x{source_decoded_bytes:X} ({source_decoded_bytes} bytes), product \
             0x{product_decoded_bytes:X} ({product_decoded_bytes} bytes), delta {delta:+}; the \
             selector hot-swaps GAME into a live segment, so changing the source-tail boundary \
             can overwrite or expose live state"
        );
    }

    Ok(NormalSelectorOverlayExtent {
        source_decoded_bytes,
        product_decoded_bytes,
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn staged_source_slot_binds_raw_bytes_budget_and_terminator() {
        let entry = json!({
            "id": "GAME_R_TEST",
            "string_decoded_offset": "0x0010",
            "byte_budget": 5,
            "raw_hex": "82a082a2"
        });
        let slot = StagedSourceSlot::from_json(&entry, "test entry").unwrap();
        let mut decoded = vec![0u8; 0x20];
        decoded[0x10..0x14].copy_from_slice(&[0x82, 0xA0, 0x82, 0xA2]);
        slot.validate_against(&decoded, "GAME_R.OVL").unwrap();

        decoded[0x12] ^= 1;
        let error = slot
            .validate_against(&decoded, "GAME_R.OVL")
            .unwrap_err()
            .to_string();
        assert!(error.contains("raw_hex does not match exact source"));
    }

    #[test]
    fn staged_source_slot_rejects_a_claimed_budget_beyond_raw_and_terminator() {
        let entry = json!({
            "id": "GAME_R_TEST",
            "string_decoded_offset": "0x0010",
            "byte_budget": 6,
            "raw_hex": "82a082a2"
        });
        let error = StagedSourceSlot::from_json(&entry, "test entry")
            .unwrap_err()
            .to_string();
        assert!(error.contains("does not equal raw 4 + terminator"));
    }

    #[test]
    fn classifies_identity_only_when_compare_and_file_consumers_agree() {
        let mut decoded = vec![0u8; 0x300];
        decoded[0x80..0x8F].copy_from_slice(b"12345678901234\0");
        decoded[0x10..0x1B].copy_from_slice(&[
            0xBE, 0x80, 0x01, 0x8B, 0xFB, 0xB9, 0x07, 0x00, 0xF3, 0xA7, 0x74,
        ]);
        assert!(source_identity_slots(&decoded, 0x100).is_empty());

        decoded[0x40..0x47].copy_from_slice(&[0xBA, 0x80, 0x01, 0xB4, 0x00, 0xCD, 0x7B]);
        assert_eq!(
            source_identity_slots(&decoded, 0x100),
            HashSet::from([0x80])
        );
    }

    #[test]
    fn normal_selector_accepts_a_source_exact_game_extent() {
        assert_eq!(
            validate_decoded_extent("GAME_R.OVL", 0xBE56, 0xBE56).unwrap(),
            NormalSelectorOverlayExtent {
                source_decoded_bytes: 0xBE56,
                product_decoded_bytes: 0xBE56,
            }
        );
    }

    #[test]
    fn normal_selector_rejects_a_grown_game_extent() {
        let error = validate_decoded_extent("GAME_R.OVL", 0xBE56, 0xC77C)
            .unwrap_err()
            .to_string();
        assert!(error.contains("normal Demo selector"));
        assert!(error.contains("delta +2342"));
        assert!(error.contains("live state"));
    }

    #[test]
    fn normal_selector_rejects_a_shrunk_game_extent() {
        let error = validate_decoded_extent("GAME_S.OVL", 0xC000, 0xBFF0)
            .unwrap_err()
            .to_string();
        assert!(error.contains("delta -16"));
    }
}
