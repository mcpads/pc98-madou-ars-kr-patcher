//! Korean bitmap choice labels for Rulue's first interlude scene.
//!
//! `RTU1.DAT` is one LZ stream. Its first `0x6C00` decoded bytes are a
//! 288x192 plane-major B/R/G/I screen and its final `0x400` bytes are directly
//! unconsumed by the proven `TYUKAN_R.OVL` routes. The compiler changes only
//! the two source-proven 64x16 button interiors and preserves every other
//! indexed pixel plus the unconsumed tail.

use std::{collections::BTreeSet, fs, path::Path};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::{
    font_build::{FontProfile, GLYPH_HEIGHT, GLYPH_WIDTH, rasterize_glyphs},
    media_identity::sha256_hex,
    overlay_lz::{decode_overlay_lz, encode_overlay_lz},
};

pub const WIDTH: usize = 288;
pub const HEIGHT: usize = 192;
const ROW_BYTES: usize = WIDTH / 8;
const PLANE_STRIDE: usize = ROW_BYTES * HEIGHT;
const IMAGE_BYTES: usize = PLANE_STRIDE * 4;
const DECODED_BYTES: usize = 0x7000;
const MANIFEST_PATH: &str = "graphics_text/rulue_interlude_choices.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RulueInterludeBuild {
    pub packed: Vec<u8>,
    pub decoded: Vec<u8>,
    pub preview_rgb: Vec<u8>,
    pub changed_pixels: usize,
    pub protected_pixels: usize,
    pub surfaces: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: usize,
    id: String,
    review_status: String,
    source: Source,
    font_profile_id: String,
    style: Style,
    surfaces: Vec<Surface>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    resource: String,
    packed_bytes: usize,
    packed_sha256: String,
    decoded_bytes: usize,
    decoded_sha256: String,
    layout: String,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
struct Rect(usize, usize, usize, usize);

impl Rect {
    fn contains(self, x: usize, y: usize) -> bool {
        x >= self.0 && y >= self.1 && x < self.0 + self.2 && y < self.1 + self.3
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Style {
    background_index: u8,
    foreground_index: u8,
    shadow_index: u8,
    shadow_offset: (usize, usize),
    source_label_indices: Vec<u8>,
    space_width: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Surface {
    id: String,
    source_text: String,
    text: String,
    clear_rect: Rect,
    origin: (usize, usize),
}

pub fn build_rulue_interlude_choices(
    source_packed: &[u8],
    assets_dir: &Path,
    font_profile: &FontProfile,
    allow_needs_review: bool,
) -> Result<RulueInterludeBuild> {
    let manifest_path = assets_dir.join(MANIFEST_PATH);
    let manifest: Manifest = serde_json::from_slice(
        &fs::read(&manifest_path)
            .with_context(|| format!("read RTU1 choice manifest {}", manifest_path.display()))?,
    )
    .with_context(|| format!("parse RTU1 choice manifest {}", manifest_path.display()))?;
    validate_manifest(&manifest, source_packed, font_profile, allow_needs_review)?;

    let report = decode_overlay_lz(source_packed).context("decode exact RTU1.DAT")?;
    if report.bytes_consumed != source_packed.len() {
        bail!(
            "RTU1.DAT has {} trailing packed bytes",
            source_packed.len() - report.bytes_consumed
        );
    }
    if report.output.len() != manifest.source.decoded_bytes
        || sha256_hex(&report.output) != manifest.source.decoded_sha256
    {
        bail!("RTU1.DAT decoded identity does not match the graphics-text manifest");
    }

    let source_indices = decode_plane_major_indices(&report.output)?;
    validate_source_button_pixels(&manifest, &source_indices)?;
    let mut target_indices = source_indices.clone();
    for surface in &manifest.surfaces {
        clear_rect(
            &mut target_indices,
            surface.clear_rect,
            manifest.style.background_index,
        );
    }

    let demand = manifest
        .surfaces
        .iter()
        .flat_map(|surface| surface.text.chars())
        .filter(|ch| !ch.is_whitespace())
        .collect::<BTreeSet<_>>();
    let glyphs = rasterize_glyphs(font_profile, &demand)?;
    for surface in &manifest.surfaces {
        draw_text(
            &mut target_indices,
            surface,
            &glyphs,
            &manifest.style,
            manifest.style.shadow_index,
            manifest.style.shadow_offset,
        )?;
    }
    for surface in &manifest.surfaces {
        draw_text(
            &mut target_indices,
            surface,
            &glyphs,
            &manifest.style,
            manifest.style.foreground_index,
            (0, 0),
        )?;
    }

    let mut changed_pixels = 0usize;
    let mut protected_pixels = 0usize;
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let pixel = y * WIDTH + x;
            let mutable = manifest
                .surfaces
                .iter()
                .any(|surface| surface.clear_rect.contains(x, y));
            if source_indices[pixel] != target_indices[pixel] {
                if !mutable {
                    bail!("RTU1 compiler changed protected pixel ({x}, {y})");
                }
                changed_pixels += 1;
            } else if !mutable {
                protected_pixels += 1;
            }
        }
    }
    if changed_pixels == 0 {
        bail!("RTU1 choice compiler produced no indexed-pixel changes");
    }
    let allowed_output = [
        manifest.style.background_index,
        manifest.style.foreground_index,
        manifest.style.shadow_index,
    ];
    for surface in &manifest.surfaces {
        for y in surface.clear_rect.1..surface.clear_rect.1 + surface.clear_rect.3 {
            for x in surface.clear_rect.0..surface.clear_rect.0 + surface.clear_rect.2 {
                let index = target_indices[y * WIDTH + x];
                if !allowed_output.contains(&index) {
                    bail!(
                        "RTU1 target button {:?} contains undeclared palette index {index}",
                        surface.id
                    );
                }
            }
        }
    }

    let decoded = encode_plane_major_indices(&target_indices, &report.output)?;
    if decoded[IMAGE_BYTES..] != report.output[IMAGE_BYTES..] {
        bail!("RTU1 compiler changed the directly unconsumed 0x400-byte tail");
    }
    let packed = encode_overlay_lz(&decoded);
    let round_trip = decode_overlay_lz(&packed).context("round-trip rebuilt RTU1.DAT")?;
    if round_trip.bytes_consumed != packed.len() || round_trip.output != decoded {
        bail!("rebuilt RTU1.DAT failed exact LZ round-trip");
    }

    Ok(RulueInterludeBuild {
        packed,
        decoded,
        preview_rgb: diagnostic_rgb(&target_indices),
        changed_pixels,
        protected_pixels,
        surfaces: manifest.surfaces.len(),
    })
}

fn validate_manifest(
    manifest: &Manifest,
    source_packed: &[u8],
    font_profile: &FontProfile,
    allow_needs_review: bool,
) -> Result<()> {
    if manifest.schema_version != 1
        || manifest.id != "PC98-RULUE-INTERLUDE-CHOICES"
        || manifest.source.resource != "RTU1.DAT"
        || manifest.source.packed_bytes != source_packed.len()
        || manifest.source.packed_sha256 != sha256_hex(source_packed)
        || manifest.source.decoded_bytes != DECODED_BYTES
        || manifest.source.layout
            != "288x192 plane-major B/R/G/I at 0x0000..0x6C00; directly unconsumed 0x400-byte tail preserved"
        || manifest.font_profile_id != font_profile.id
    {
        bail!("RTU1 choice manifest identity or fixed layout contract drifted");
    }
    if manifest.review_status != "complete"
        && !(allow_needs_review && manifest.review_status == "needs_review")
    {
        bail!(
            "RTU1 choice manifest status {:?} is not allowed; use a complete asset or explicitly permit needs_review for emulator QA",
            manifest.review_status
        );
    }
    if manifest.style.background_index != 2
        || manifest.style.foreground_index != 1
        || manifest.style.shadow_index != 10
        || manifest.style.shadow_offset != (1, 1)
        || manifest.style.source_label_indices != [1, 2, 8, 10]
        || manifest.style.space_width != 8
    {
        bail!("RTU1 choice manifest palette or spacing contract drifted");
    }
    let expected = [
        ("sleep", "ねる", "잔다", Rect(40, 160, 64, 16), (55, 160)),
        (
            "stay-awake",
            "ねない",
            "안 잔다",
            Rect(184, 160, 64, 16),
            (187, 160),
        ),
    ];
    if manifest.surfaces.len() != expected.len() {
        bail!("RTU1 choice manifest must contain exactly two source buttons");
    }
    for (surface, (id, source_text, text, clear_rect, origin)) in
        manifest.surfaces.iter().zip(expected)
    {
        if surface.id != id
            || surface.source_text != source_text
            || surface.text != text
            || surface.clear_rect != clear_rect
            || surface.origin != origin
        {
            bail!(
                "RTU1 choice surface {:?} drifted from its reviewed slot",
                surface.id
            );
        }
        if clear_rect.0 + clear_rect.2 > WIDTH || clear_rect.1 + clear_rect.3 > HEIGHT {
            bail!("RTU1 choice surface {id:?} exceeds the decoded screen");
        }
        let text_width = text_width(text, manifest.style.space_width);
        if origin.0 < clear_rect.0
            || origin.1 < clear_rect.1
            || origin.0 + text_width + manifest.style.shadow_offset.0 > clear_rect.0 + clear_rect.2
            || origin.1 + GLYPH_HEIGHT > clear_rect.1 + clear_rect.3
        {
            bail!("RTU1 Korean choice {id:?} exceeds its button interior");
        }
    }
    Ok(())
}

fn validate_source_button_pixels(manifest: &Manifest, indices: &[u8]) -> Result<()> {
    for surface in &manifest.surfaces {
        let mut background = 0usize;
        let mut label = 0usize;
        for y in surface.clear_rect.1..surface.clear_rect.1 + surface.clear_rect.3 {
            for x in surface.clear_rect.0..surface.clear_rect.0 + surface.clear_rect.2 {
                let index = indices[y * WIDTH + x];
                if !manifest.style.source_label_indices.contains(&index) {
                    bail!(
                        "RTU1 source button {:?} contains unexpected palette index {index}",
                        surface.id
                    );
                }
                if index == manifest.style.background_index {
                    background += 1;
                } else {
                    label += 1;
                }
            }
        }
        if background == 0 || label == 0 {
            bail!(
                "RTU1 source button {:?} lost its background or label",
                surface.id
            );
        }
    }
    Ok(())
}

fn text_width(text: &str, space_width: usize) -> usize {
    text.chars()
        .map(|ch| {
            if ch.is_whitespace() {
                space_width
            } else {
                GLYPH_WIDTH
            }
        })
        .sum()
}

fn draw_text(
    indices: &mut [u8],
    surface: &Surface,
    glyphs: &std::collections::BTreeMap<char, [u8; 32]>,
    style: &Style,
    palette_index: u8,
    offset: (usize, usize),
) -> Result<()> {
    let mut cursor_x = surface.origin.0;
    for ch in surface.text.chars() {
        if ch.is_whitespace() {
            cursor_x += style.space_width;
            continue;
        }
        let glyph = glyphs
            .get(&ch)
            .with_context(|| format!("missing rasterized RTU1 choice glyph {ch:?}"))?;
        for y in 0..GLYPH_HEIGHT {
            for x in 0..GLYPH_WIDTH {
                if glyph[y * 2 + x / 8] & (0x80 >> (x % 8)) == 0 {
                    continue;
                }
                let target_x = cursor_x + x + offset.0;
                let target_y = surface.origin.1 + y + offset.1;
                if !surface.clear_rect.contains(target_x, target_y) {
                    bail!("RTU1 choice glyph {ch:?} escaped button {:?}", surface.id);
                }
                indices[target_y * WIDTH + target_x] = palette_index;
            }
        }
        cursor_x += GLYPH_WIDTH;
    }
    Ok(())
}

fn clear_rect(indices: &mut [u8], rect: Rect, background: u8) {
    for y in rect.1..rect.1 + rect.3 {
        indices[y * WIDTH + rect.0..y * WIDTH + rect.0 + rect.2].fill(background);
    }
}

fn decode_plane_major_indices(decoded: &[u8]) -> Result<Vec<u8>> {
    if decoded.len() != DECODED_BYTES {
        bail!("RTU1 decoded payload must be exactly {DECODED_BYTES} bytes");
    }
    let mut indices = vec![0u8; WIDTH * HEIGHT];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let mask = 0x80 >> (x % 8);
            let source = y * ROW_BYTES + x / 8;
            for plane in 0..4 {
                if decoded[plane * PLANE_STRIDE + source] & mask != 0 {
                    indices[y * WIDTH + x] |= 1 << plane;
                }
            }
        }
    }
    Ok(indices)
}

fn encode_plane_major_indices(indices: &[u8], source_decoded: &[u8]) -> Result<Vec<u8>> {
    if indices.len() != WIDTH * HEIGHT || source_decoded.len() != DECODED_BYTES {
        bail!("RTU1 indexed or decoded geometry is invalid");
    }
    if indices.iter().any(|index| *index > 15) {
        bail!("RTU1 indexed surface contains a palette index above 15");
    }
    let mut decoded = source_decoded.to_vec();
    decoded[..IMAGE_BYTES].fill(0);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let pixel = indices[y * WIDTH + x];
            let mask = 0x80 >> (x % 8);
            let destination = y * ROW_BYTES + x / 8;
            for plane in 0..4 {
                if pixel & (1 << plane) != 0 {
                    decoded[plane * PLANE_STRIDE + destination] |= mask;
                }
            }
        }
    }
    Ok(decoded)
}

fn diagnostic_rgb(indices: &[u8]) -> Vec<u8> {
    const PALETTE: [[u8; 3]; 16] = [
        [0x00, 0x00, 0x00],
        [0x00, 0x00, 0xAA],
        [0xAA, 0x00, 0x00],
        [0xAA, 0x00, 0xAA],
        [0x00, 0xAA, 0x00],
        [0x00, 0xAA, 0xAA],
        [0xAA, 0x55, 0x00],
        [0xAA, 0xAA, 0xAA],
        [0x55, 0x55, 0x55],
        [0x55, 0x55, 0xFF],
        [0xFF, 0x55, 0x55],
        [0xFF, 0x55, 0xFF],
        [0x55, 0xFF, 0x55],
        [0x55, 0xFF, 0xFF],
        [0xFF, 0xFF, 0x55],
        [0xFF, 0xFF, 0xFF],
    ];
    indices
        .iter()
        .flat_map(|index| PALETTE[usize::from(*index)])
        .collect()
}
