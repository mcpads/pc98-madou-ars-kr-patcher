//! Font-derived Korean graphics text for the shared Demo-disk scenario menu.
//!
//! `MU.CNS` contains one 128,000-byte AH=5 column-major B/R/G/I screen and a
//! second LZ stream consumed as a three-track BSAMP.COM bank. Only the declared
//! Japanese prompt rectangle may change; the existing English character names
//! remain source-identical. The audio stream is copied in its original packed
//! form and verified byte-for-byte after composition.

use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::{
    bsamp_resource::parse_named_bsamp_companion,
    font_build::{FontProfile, GLYPH_HEIGHT, GLYPH_WIDTH, rasterize_glyphs},
    graphics_resource::{PLANE_BYTES, SCREEN_HEIGHT, SCREEN_WIDTH},
    media_identity::sha256_hex,
    overlay_lz::{decode_overlay_lz, encode_overlay_lz},
};

const MANIFEST_PATH: &str = "graphics_text/menu_graphics.json";

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
struct Surface {
    id: String,
    text: String,
    clear_rect: Rect,
    origin: (usize, usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TextStyle {
    background: u8,
    foreground: u8,
    near_shadow: u8,
    far_shadow: u8,
    near_offset: (usize, usize),
    far_offset: (usize, usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemoMenuBuild {
    pub packed: Vec<u8>,
    pub primary_decoded: Vec<u8>,
    pub companion_packed: Vec<u8>,
    pub changed_pixels: usize,
    pub protected_pixels: usize,
    pub surfaces: usize,
}

pub fn build_demo_menu_text(
    source_packed: &[u8],
    assets_dir: &Path,
    font_profile: &FontProfile,
) -> Result<DemoMenuBuild> {
    let manifest_path = assets_dir.join(MANIFEST_PATH);
    let manifest_bytes = fs::read(&manifest_path)
        .with_context(|| format!("read menu manifest {}", manifest_path.display()))?;
    let manifest: Value = serde_json::from_slice(&manifest_bytes)
        .with_context(|| format!("parse menu manifest {}", manifest_path.display()))?;
    expect_usize(&manifest, "schema_version", 1)?;
    expect_string(&manifest, "id", "PC98-DEMO-MENU-TEXT")?;
    expect_string(&manifest, "font_profile_id", &font_profile.id)?;

    let source = object(&manifest, "source")?;
    expect_string(source, "resource", "MU.CNS")?;
    expect_usize(source, "packed_bytes", source_packed.len())?;
    expect_string(source, "packed_sha256", &sha256_hex(source_packed))?;

    let primary_report = decode_overlay_lz(source_packed).context("decode MU.CNS primary")?;
    expect_usize(
        source,
        "primary_packed_bytes",
        primary_report.bytes_consumed,
    )?;
    expect_usize(source, "primary_decoded_bytes", primary_report.output.len())?;
    expect_string(
        source,
        "primary_decoded_sha256",
        &sha256_hex(&primary_report.output),
    )?;
    if primary_report.output.len() != PLANE_BYTES * 4 {
        bail!(
            "MU.CNS primary decoded size is {}, expected {}",
            primary_report.output.len(),
            PLANE_BYTES * 4
        );
    }

    let companion_packed = source_packed
        .get(primary_report.bytes_consumed..)
        .context("MU.CNS has no companion stream")?;
    expect_usize(source, "companion_packed_bytes", companion_packed.len())?;
    expect_string(
        source,
        "companion_packed_sha256",
        &sha256_hex(companion_packed),
    )?;
    let companion_report =
        decode_overlay_lz(companion_packed).context("decode MU.CNS companion")?;
    if companion_report.bytes_consumed != companion_packed.len() {
        bail!(
            "MU.CNS companion has {} trailing packed bytes",
            companion_packed.len() - companion_report.bytes_consumed
        );
    }
    expect_usize(
        source,
        "companion_decoded_bytes",
        companion_report.output.len(),
    )?;
    expect_string(
        source,
        "companion_decoded_sha256",
        &sha256_hex(&companion_report.output),
    )?;
    parse_named_bsamp_companion("MU.CNS", &companion_report.output)
        .context("validate MU.CNS companion audio bank")?;

    let style = parse_style(&manifest)?;
    let surfaces = parse_surfaces(&manifest, style)?;
    let mut demand = BTreeSet::new();
    for surface in &surfaces {
        demand.extend(surface.text.chars().filter(|ch| !ch.is_whitespace()));
    }
    let glyphs = rasterize_glyphs(font_profile, &demand)?;

    let source_indices = decode_column_major_indices(&primary_report.output)?;
    let mut target_indices = source_indices.clone();
    for surface in &surfaces {
        clear_rect(&mut target_indices, surface.clear_rect, style.background);
    }
    for surface in &surfaces {
        draw_text(
            &mut target_indices,
            surface,
            &glyphs,
            style.far_shadow,
            style.far_offset,
        )?;
    }
    for surface in &surfaces {
        draw_text(
            &mut target_indices,
            surface,
            &glyphs,
            style.near_shadow,
            style.near_offset,
        )?;
    }
    for surface in &surfaces {
        draw_text(
            &mut target_indices,
            surface,
            &glyphs,
            style.foreground,
            (0, 0),
        )?;
    }

    let mut changed_pixels = 0usize;
    let mut protected_pixels = 0usize;
    for y in 0..SCREEN_HEIGHT {
        for x in 0..SCREEN_WIDTH {
            let pixel = y * SCREEN_WIDTH + x;
            let mutable = surfaces
                .iter()
                .any(|surface| surface.clear_rect.contains(x, y));
            if source_indices[pixel] != target_indices[pixel] {
                if !mutable {
                    bail!("MU.CNS compiler changed protected pixel ({x}, {y})");
                }
                changed_pixels += 1;
            } else if !mutable {
                protected_pixels += 1;
            }
        }
    }
    if changed_pixels == 0 {
        bail!("MU.CNS compiler produced no indexed-pixel changes");
    }

    let primary_decoded = encode_column_major_indices(&target_indices)?;
    let mut packed = encode_overlay_lz(&primary_decoded);
    packed.extend_from_slice(companion_packed);
    let primary_round_trip = decode_overlay_lz(&packed).context("round-trip MU.CNS primary")?;
    if primary_round_trip.output != primary_decoded {
        bail!("rebuilt MU.CNS primary failed exact LZ round-trip");
    }
    let companion_readback = packed
        .get(primary_round_trip.bytes_consumed..)
        .context("rebuilt MU.CNS lost its companion stream")?;
    if companion_readback != companion_packed {
        bail!("rebuilt MU.CNS changed the packed companion audio stream");
    }
    let companion_round_trip =
        decode_overlay_lz(companion_readback).context("round-trip MU.CNS companion")?;
    if companion_round_trip.bytes_consumed != companion_readback.len()
        || companion_round_trip.output != companion_report.output
    {
        bail!("rebuilt MU.CNS companion failed exact readback");
    }

    Ok(DemoMenuBuild {
        packed,
        primary_decoded,
        companion_packed: companion_packed.to_vec(),
        changed_pixels,
        protected_pixels,
        surfaces: surfaces.len(),
    })
}

fn parse_style(manifest: &Value) -> Result<TextStyle> {
    let style = object(manifest, "style")?;
    let parsed = TextStyle {
        background: palette_index(style, "background_index")?,
        foreground: palette_index(style, "foreground_index")?,
        near_shadow: palette_index(style, "near_shadow_index")?,
        far_shadow: palette_index(style, "far_shadow_index")?,
        near_offset: pair(style, "near_shadow_offset")?,
        far_offset: pair(style, "far_shadow_offset")?,
    };
    if parsed.background != 0
        || parsed.foreground != 8
        || parsed.near_shadow != 9
        || parsed.far_shadow != 10
    {
        bail!("MU.CNS text palette roles drifted from the source-proven 0/8/9/10 contract");
    }
    Ok(parsed)
}

fn parse_surfaces(manifest: &Value, style: TextStyle) -> Result<Vec<Surface>> {
    let entries = manifest
        .get("surfaces")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("menu manifest surfaces must be an array"))?;
    if entries.len() != 1 {
        bail!("menu manifest has {} surfaces, expected 1", entries.len());
    }
    entries
        .iter()
        .map(|entry| {
            let rect_values = array(entry, "clear_rect", 4)?;
            let origin = pair(entry, "origin")?;
            let surface = Surface {
                id: string(entry, "id")?.to_owned(),
                text: string(entry, "text")?.to_owned(),
                clear_rect: Rect {
                    x: rect_values[0],
                    y: rect_values[1],
                    width: rect_values[2],
                    height: rect_values[3],
                },
                origin,
            };
            if surface.id != "scenario-prompt"
                || string(entry, "source_text")? != "どのシナリオであそぶ?"
            {
                bail!("MU.CNS may replace only the source-proven Japanese scenario prompt");
            }
            if surface.clear_rect.width == 0
                || surface.clear_rect.height == 0
                || surface.clear_rect.x + surface.clear_rect.width > SCREEN_WIDTH
                || surface.clear_rect.y + surface.clear_rect.height > SCREEN_HEIGHT
            {
                bail!(
                    "menu surface {:?} has an invalid clear rectangle",
                    surface.id
                );
            }
            let text_width = surface.text.chars().count() * GLYPH_WIDTH;
            if surface.origin.0 < surface.clear_rect.x
                || surface.origin.1 < surface.clear_rect.y
                || surface.origin.0 + text_width + style.far_offset.0
                    > surface.clear_rect.x + surface.clear_rect.width
                || surface.origin.1 + GLYPH_HEIGHT + style.far_offset.1
                    > surface.clear_rect.y + surface.clear_rect.height
            {
                bail!(
                    "menu surface {:?} text exceeds its clear rectangle",
                    surface.id
                );
            }
            Ok(surface)
        })
        .collect()
}

fn draw_text(
    indices: &mut [u8],
    surface: &Surface,
    glyphs: &std::collections::BTreeMap<char, [u8; 32]>,
    palette_index: u8,
    offset: (usize, usize),
) -> Result<()> {
    for (cell, ch) in surface.text.chars().enumerate() {
        if ch.is_whitespace() {
            continue;
        }
        let glyph = glyphs
            .get(&ch)
            .with_context(|| format!("missing rasterized menu glyph {ch:?}"))?;
        let origin_x = surface.origin.0 + cell * GLYPH_WIDTH + offset.0;
        let origin_y = surface.origin.1 + offset.1;
        for y in 0..GLYPH_HEIGHT {
            for x in 0..GLYPH_WIDTH {
                if glyph[y * 2 + x / 8] & (0x80 >> (x % 8)) != 0 {
                    indices[(origin_y + y) * SCREEN_WIDTH + origin_x + x] = palette_index;
                }
            }
        }
    }
    Ok(())
}

fn clear_rect(indices: &mut [u8], rect: Rect, background: u8) {
    for y in rect.y..rect.y + rect.height {
        indices[y * SCREEN_WIDTH + rect.x..y * SCREEN_WIDTH + rect.x + rect.width].fill(background);
    }
}

fn decode_column_major_indices(decoded: &[u8]) -> Result<Vec<u8>> {
    if decoded.len() != PLANE_BYTES * 4 {
        bail!("AH=5 B/R/G/I payload must be exactly 128000 bytes");
    }
    let mut indices = vec![0u8; SCREEN_WIDTH * SCREEN_HEIGHT];
    for y in 0..SCREEN_HEIGHT {
        for x in 0..SCREEN_WIDTH {
            let mask = 0x80 >> (x % 8);
            let column = x / 8;
            for plane in 0..4 {
                let source = plane * PLANE_BYTES + column * SCREEN_HEIGHT + y;
                if decoded[source] & mask != 0 {
                    indices[y * SCREEN_WIDTH + x] |= 1 << plane;
                }
            }
        }
    }
    Ok(indices)
}

fn encode_column_major_indices(indices: &[u8]) -> Result<Vec<u8>> {
    if indices.len() != SCREEN_WIDTH * SCREEN_HEIGHT {
        bail!("AH=5 indexed surface must be exactly 640x400 pixels");
    }
    if indices.iter().any(|index| *index > 15) {
        bail!("AH=5 indexed surface contains a palette index above 15");
    }
    let mut decoded = vec![0u8; PLANE_BYTES * 4];
    for y in 0..SCREEN_HEIGHT {
        for x in 0..SCREEN_WIDTH {
            let pixel = indices[y * SCREEN_WIDTH + x];
            let mask = 0x80 >> (x % 8);
            let column = x / 8;
            for plane in 0..4 {
                if pixel & (1 << plane) != 0 {
                    decoded[plane * PLANE_BYTES + column * SCREEN_HEIGHT + y] |= mask;
                }
            }
        }
    }
    Ok(decoded)
}

fn object<'a>(value: &'a Value, key: &str) -> Result<&'a Value> {
    value
        .get(key)
        .filter(|value| value.is_object())
        .ok_or_else(|| anyhow::anyhow!("menu manifest {key} must be an object"))
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("menu manifest {key} must be a string"))
}

fn usize_value(value: &Value, key: &str) -> Result<usize> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| anyhow::anyhow!("menu manifest {key} must be a non-negative integer"))
}

fn expect_string(value: &Value, key: &str, expected: &str) -> Result<()> {
    let actual = string(value, key)?;
    if actual != expected {
        bail!("menu manifest {key} is {actual:?}, expected {expected:?}");
    }
    Ok(())
}

fn expect_usize(value: &Value, key: &str, expected: usize) -> Result<()> {
    let actual = usize_value(value, key)?;
    if actual != expected {
        bail!("menu manifest {key} is {actual}, expected {expected}");
    }
    Ok(())
}

fn palette_index(value: &Value, key: &str) -> Result<u8> {
    let index = usize_value(value, key)?;
    if index > 15 {
        bail!("menu manifest {key} must be in 0..=15");
    }
    Ok(index as u8)
}

fn pair(value: &Value, key: &str) -> Result<(usize, usize)> {
    let values = array(value, key, 2)?;
    Ok((values[0], values[1]))
}

fn array(value: &Value, key: &str, expected_len: usize) -> Result<Vec<usize>> {
    let entries = value
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("menu manifest {key} must be an array"))?;
    if entries.len() != expected_len {
        bail!(
            "menu manifest {key} has {} entries, expected {expected_len}",
            entries.len()
        );
    }
    entries
        .iter()
        .map(|entry| {
            entry
                .as_u64()
                .and_then(|value| usize::try_from(value).ok())
                .ok_or_else(|| anyhow::anyhow!("menu manifest {key} entry must be an integer"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn column_major_index_codec_round_trips_every_role() {
        let mut indices = vec![0u8; SCREEN_WIDTH * SCREEN_HEIGHT];
        for (index, pixel) in indices.iter_mut().take(16).enumerate() {
            *pixel = index as u8;
        }
        let decoded = encode_column_major_indices(&indices).unwrap();
        assert_eq!(decode_column_major_indices(&decoded).unwrap(), indices);
    }

    #[test]
    fn menu_scope_accepts_only_the_japanese_prompt() {
        let manifest = json!({
            "surfaces": [{
                "id": "scenario-prompt",
                "source_text": "どのシナリオであそぶ?",
                "text": "어느 시나리오로 할까?",
                "clear_rect": [0, 368, 208, 24],
                "origin": [8, 371]
            }]
        });
        let surfaces = parse_surfaces(&manifest, source_style()).unwrap();
        assert_eq!(surfaces.len(), 1);
        assert_eq!(surfaces[0].id, "scenario-prompt");
    }

    #[test]
    fn menu_scope_rejects_an_english_name_replacement() {
        let manifest = json!({
            "surfaces": [
                {
                    "id": "scenario-prompt",
                    "source_text": "どのシナリオであそぶ?",
                    "text": "어느 시나리오로 할까?",
                    "clear_rect": [0, 368, 208, 24],
                    "origin": [8, 371]
                },
                {
                    "id": "arle-name",
                    "source_text": "ARLE",
                    "text": "아르르",
                    "clear_rect": [0, 342, 96, 26],
                    "origin": [24, 346]
                }
            ]
        });
        let error = parse_surfaces(&manifest, source_style()).unwrap_err();
        assert!(error.to_string().contains("expected 1"));
    }

    fn source_style() -> TextStyle {
        TextStyle {
            background: 0,
            foreground: 8,
            near_shadow: 9,
            far_shadow: 10,
            near_offset: (1, 1),
            far_offset: (2, 2),
        }
    }
}
