//! PC-98-source title artwork compiler for the shared `ARS_RX.CS` Data-disk
//! screen.
//!
//! `ARS_RX.CS` is one LZ stream containing four adjacent 80x400 column-major
//! planes in B/R/G/I order. All three character Data disks carry the same exact
//! file and their opening consumers transpose it into the 640x400 title screen.

use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::{
    demo_title::{
        area_reduce_rgb, load_reviewed_title_master, nearest_palette_index,
        preserve_source_background_texture,
    },
    media_identity::sha256_hex,
    overlay_lz::{decode_overlay_lz, encode_overlay_lz},
};

pub const SCENARIO_TITLE_WIDTH: usize = 640;
pub const SCENARIO_TITLE_HEIGHT: usize = 400;
pub const SCENARIO_TITLE_PLANE_BYTES: usize = 80 * SCENARIO_TITLE_HEIGHT;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rect {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

impl Rect {
    fn contains(self, x: usize, y: usize) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioTitleBuild {
    pub packed: Vec<u8>,
    pub decoded: Vec<u8>,
    pub preview_rgb: Vec<u8>,
    pub changed_pixels: usize,
    pub protected_pixels: usize,
    pub source_background_pixels_preserved: usize,
    pub source_logo_pixels_replaced_with_background: usize,
    pub master_sha256: String,
}

pub fn build_scenario_title(source_packed: &[u8], assets_dir: &Path) -> Result<ScenarioTitleBuild> {
    let reviewed = load_reviewed_title_master(assets_dir)?;
    let source = object(&reviewed.manifest, "scenario_source")?;
    expect_string(source, "resource", "ARS_RX.CS")?;
    expect_usize(source, "packed_bytes", source_packed.len())?;
    expect_string(source, "packed_sha256", &sha256_hex(source_packed))?;
    expect_usize(source, "decoded_bytes", SCENARIO_TITLE_PLANE_BYTES * 4)?;
    expect_usize(source, "width", SCENARIO_TITLE_WIDTH)?;
    expect_usize(source, "height", SCENARIO_TITLE_HEIGHT)?;
    expect_string(source, "layout", "column-major B/R/G/I planes")?;

    let source_report = decode_overlay_lz(source_packed).context("decode original ARS_RX.CS")?;
    if source_report.bytes_consumed != source_packed.len() {
        bail!(
            "ARS_RX.CS has trailing packed bytes after its one proven stream: consumed {} of {}",
            source_report.bytes_consumed,
            source_packed.len()
        );
    }
    if source_report.output.len() != SCENARIO_TITLE_PLANE_BYTES * 4 {
        bail!(
            "ARS_RX.CS decoded size is {}, expected {}",
            source_report.output.len(),
            SCENARIO_TITLE_PLANE_BYTES * 4
        );
    }
    expect_string(source, "decoded_sha256", &sha256_hex(&source_report.output))?;
    let source_indices = decode_indices(&source_report.output)?;

    let reduced = area_reduce_rgb(
        &reviewed.rgb,
        reviewed.width,
        reviewed.height,
        SCENARIO_TITLE_WIDTH,
        SCENARIO_TITLE_HEIGHT,
    )?;
    let mut target_indices = reduced
        .as_chunks::<3>()
        .0
        .iter()
        .map(|pixel| nearest_palette_index([pixel[0], pixel[1], pixel[2]], &reviewed.palette))
        .collect::<Vec<_>>();

    let protected_rects = parse_protected_rectangles(source)?;
    let mut protected_mask = vec![false; target_indices.len()];
    let mut protected_pixels = 0usize;
    for y in 0..SCENARIO_TITLE_HEIGHT {
        for x in 0..SCENARIO_TITLE_WIDTH {
            if protected_rects.iter().any(|rect| rect.contains(x, y)) {
                let pixel = y * SCENARIO_TITLE_WIDTH + x;
                target_indices[pixel] = source_indices[pixel];
                protected_mask[pixel] = true;
                protected_pixels += 1;
            }
        }
    }
    let background_stats = preserve_source_background_texture(
        &source_indices,
        &mut target_indices,
        &protected_mask,
        &reviewed.background_palette_roles,
    )?;

    let changed_pixels = source_indices
        .iter()
        .zip(&target_indices)
        .filter(|(source, target)| source != target)
        .count();
    if changed_pixels == 0 {
        bail!("scenario-title compiler produced no indexed-pixel changes");
    }
    if protected_pixels == 0 {
        bail!("scenario-title manifest protects no original pixels");
    }

    let decoded = encode_indices(&target_indices)?;
    let packed = encode_overlay_lz(&decoded);
    let round_trip = decode_overlay_lz(&packed).context("round-trip rebuilt ARS_RX.CS")?;
    if round_trip.bytes_consumed != packed.len() || round_trip.output != decoded {
        bail!("rebuilt ARS_RX.CS failed exact LZ round-trip");
    }

    Ok(ScenarioTitleBuild {
        packed,
        decoded,
        preview_rgb: target_indices
            .iter()
            .flat_map(|index| reviewed.palette[usize::from(*index)])
            .collect(),
        changed_pixels,
        protected_pixels,
        source_background_pixels_preserved: background_stats.source_background_pixels_preserved,
        source_logo_pixels_replaced_with_background: background_stats
            .source_logo_pixels_replaced_with_background,
        master_sha256: reviewed.sha256,
    })
}

fn decode_indices(decoded: &[u8]) -> Result<Vec<u8>> {
    if decoded.len() != SCENARIO_TITLE_PLANE_BYTES * 4 {
        bail!("scenario title B/R/G/I payload must be exactly 128000 bytes");
    }
    let mut indices = vec![0u8; SCENARIO_TITLE_WIDTH * SCENARIO_TITLE_HEIGHT];
    for y in 0..SCENARIO_TITLE_HEIGHT {
        for x in 0..SCENARIO_TITLE_WIDTH {
            let mask = 0x80 >> (x % 8);
            let byte = (x / 8) * SCENARIO_TITLE_HEIGHT + y;
            let pixel = y * SCENARIO_TITLE_WIDTH + x;
            for plane in 0..4 {
                if decoded[plane * SCENARIO_TITLE_PLANE_BYTES + byte] & mask != 0 {
                    indices[pixel] |= 1 << plane;
                }
            }
        }
    }
    Ok(indices)
}

fn encode_indices(indices: &[u8]) -> Result<Vec<u8>> {
    if indices.len() != SCENARIO_TITLE_WIDTH * SCENARIO_TITLE_HEIGHT {
        bail!("scenario title indexed surface must be exactly 640x400 pixels");
    }
    if indices.iter().any(|index| *index > 15) {
        bail!("scenario title indexed surface contains a palette index above 15");
    }
    let mut decoded = vec![0u8; SCENARIO_TITLE_PLANE_BYTES * 4];
    for y in 0..SCENARIO_TITLE_HEIGHT {
        for x in 0..SCENARIO_TITLE_WIDTH {
            let pixel = y * SCENARIO_TITLE_WIDTH + x;
            let mask = 0x80 >> (x % 8);
            let byte = (x / 8) * SCENARIO_TITLE_HEIGHT + y;
            for plane in 0..4 {
                if indices[pixel] & (1 << plane) != 0 {
                    decoded[plane * SCENARIO_TITLE_PLANE_BYTES + byte] |= mask;
                }
            }
        }
    }
    Ok(decoded)
}

fn parse_protected_rectangles(source: &Value) -> Result<Vec<Rect>> {
    let entries = source
        .get("protected_rectangles")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("scenario_source protected_rectangles must be an array"))?;
    entries
        .iter()
        .map(|entry| {
            let rect = Rect {
                x: usize_value(entry, "x")?,
                y: usize_value(entry, "y")?,
                width: usize_value(entry, "width")?,
                height: usize_value(entry, "height")?,
            };
            if rect.width == 0
                || rect.height == 0
                || rect.x + rect.width > SCENARIO_TITLE_WIDTH
                || rect.y + rect.height > SCENARIO_TITLE_HEIGHT
            {
                bail!("scenario-title protected rectangle {rect:?} exceeds the 640x400 surface");
            }
            Ok(rect)
        })
        .collect()
}

fn object<'a>(value: &'a Value, key: &str) -> Result<&'a Value> {
    value
        .get(key)
        .filter(|value| value.is_object())
        .ok_or_else(|| anyhow::anyhow!("title manifest {key} must be an object"))
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("title manifest {key} must be a string"))
}

fn usize_value(value: &Value, key: &str) -> Result<usize> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| anyhow::anyhow!("title manifest {key} must be a non-negative integer"))
}

fn expect_string(value: &Value, key: &str, expected: &str) -> Result<()> {
    let actual = string(value, key)?;
    if actual != expected {
        bail!("title manifest {key} is {actual:?}, expected {expected:?}");
    }
    Ok(())
}

fn expect_usize(value: &Value, key: &str, expected: usize) -> Result<()> {
    let actual = usize_value(value, key)?;
    if actual != expected {
        bail!("title manifest {key} is {actual}, expected {expected}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_major_brgi_codec_round_trips_every_role() {
        let mut indices = vec![0u8; SCENARIO_TITLE_WIDTH * SCENARIO_TITLE_HEIGHT];
        for (index, pixel) in indices.iter_mut().take(16).enumerate() {
            *pixel = index as u8;
        }
        let decoded = encode_indices(&indices).unwrap();
        assert_eq!(decode_indices(&decoded).unwrap(), indices);
    }
}
