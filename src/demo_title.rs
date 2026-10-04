//! PC-98-source title artwork compiler for the shared Demo disk.
//!
//! `OP20.CNS` is one LZ stream containing four adjacent 320x200 graphics
//! planes. The Demo direct-copy routine consumes the planes in B/R/G/I order.
//! This compiler verifies that exact original source, reduces the reviewed
//! PC-98-specific RGB master to the native surface, maps it to the runtime
//! palette roles, restores protected original pixels, and rebuilds the same
//! one-stream container.

use std::{fs, io::Cursor, path::Path};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::{
    graphics_resource::{DEMO_LINEAR_HEIGHT, DEMO_LINEAR_PLANE_BYTES, DEMO_LINEAR_WIDTH},
    media_identity::sha256_hex,
    overlay_lz::{decode_overlay_lz, encode_overlay_lz},
};

const MANIFEST_PATH: &str = "graphics_text/title_logo.json";
const TITLE_TEXT: &str = "魔導傳記";
const TITLE_SUFFIX: &str = "A.R.S.";
const TITLE_PRONUNCIATION: [&str; 4] = ["마", "도", "전", "기"];

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
pub struct DemoTitleBuild {
    pub packed: Vec<u8>,
    pub decoded: Vec<u8>,
    pub preview_rgb: Vec<u8>,
    pub changed_pixels: usize,
    pub protected_pixels: usize,
    pub source_background_pixels_preserved: usize,
    pub source_logo_pixels_replaced_with_background: usize,
    pub master_sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BackgroundTextureStats {
    pub source_background_pixels_preserved: usize,
    pub source_logo_pixels_replaced_with_background: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReviewedTitleMaster {
    pub manifest: Value,
    pub rgb: Vec<u8>,
    pub width: usize,
    pub height: usize,
    pub palette: [[u8; 3]; 16],
    pub background_palette_roles: [bool; 16],
    pub sha256: String,
}

pub fn build_demo_title(source_packed: &[u8], assets_dir: &Path) -> Result<DemoTitleBuild> {
    let reviewed = load_reviewed_title_master(assets_dir)?;
    let manifest = &reviewed.manifest;
    let master_sha256 = reviewed.sha256.clone();

    let source = object(manifest, "source")?;
    expect_string(source, "resource", "OP20.CNS")?;
    expect_usize(source, "packed_bytes", source_packed.len())?;
    expect_string(source, "packed_sha256", &sha256_hex(source_packed))?;
    expect_usize(source, "decoded_bytes", DEMO_LINEAR_PLANE_BYTES * 4)?;
    expect_usize(source, "width", DEMO_LINEAR_WIDTH)?;
    expect_usize(source, "height", DEMO_LINEAR_HEIGHT)?;

    let source_report = decode_overlay_lz(source_packed).context("decode original OP20.CNS")?;
    if source_report.bytes_consumed != source_packed.len() {
        bail!(
            "OP20.CNS has trailing packed bytes after its one proven stream: consumed {} of {}",
            source_report.bytes_consumed,
            source_packed.len()
        );
    }
    if source_report.output.len() != DEMO_LINEAR_PLANE_BYTES * 4 {
        bail!(
            "OP20.CNS decoded size is {}, expected {}",
            source_report.output.len(),
            DEMO_LINEAR_PLANE_BYTES * 4
        );
    }
    expect_string(source, "decoded_sha256", &sha256_hex(&source_report.output))?;
    let source_indices = decode_indices(&source_report.output)?;

    let reduced = area_reduce_rgb(
        &reviewed.rgb,
        reviewed.width,
        reviewed.height,
        DEMO_LINEAR_WIDTH,
        DEMO_LINEAR_HEIGHT,
    )?;
    let palette = reviewed.palette;
    let mut target_indices = reduced
        .as_chunks::<3>()
        .0
        .iter()
        .map(|pixel| nearest_palette_index([pixel[0], pixel[1], pixel[2]], &palette))
        .collect::<Vec<_>>();

    let protected_rects = parse_protected_rectangles(manifest)?;
    let mut protected_mask = vec![false; target_indices.len()];
    let mut protected_pixels = 0usize;
    for y in 0..DEMO_LINEAR_HEIGHT {
        for x in 0..DEMO_LINEAR_WIDTH {
            if protected_rects.iter().any(|rect| rect.contains(x, y)) {
                let pixel = y * DEMO_LINEAR_WIDTH + x;
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
        bail!("title compiler produced no indexed-pixel changes");
    }
    if protected_pixels == 0 {
        bail!("title manifest protects no original pixels");
    }

    let decoded = encode_indices(&target_indices)?;
    let packed = encode_overlay_lz(&decoded);
    let round_trip = decode_overlay_lz(&packed).context("round-trip rebuilt OP20.CNS")?;
    if round_trip.bytes_consumed != packed.len() || round_trip.output != decoded {
        bail!("rebuilt OP20.CNS failed exact LZ round-trip");
    }

    Ok(DemoTitleBuild {
        packed,
        decoded,
        preview_rgb: render_indices(&target_indices, &palette),
        changed_pixels,
        protected_pixels,
        source_background_pixels_preserved: background_stats.source_background_pixels_preserved,
        source_logo_pixels_replaced_with_background: background_stats
            .source_logo_pixels_replaced_with_background,
        master_sha256,
    })
}

pub(crate) fn load_reviewed_title_master(assets_dir: &Path) -> Result<ReviewedTitleMaster> {
    let manifest_path = assets_dir.join(MANIFEST_PATH);
    let manifest_bytes = fs::read(&manifest_path)
        .with_context(|| format!("read title manifest {}", manifest_path.display()))?;
    let manifest: Value = serde_json::from_slice(&manifest_bytes)
        .with_context(|| format!("parse title manifest {}", manifest_path.display()))?;
    validate_title_identity(&manifest)?;

    let master = object(&manifest, "master")?;
    let master_asset = string(master, "asset")?;
    let master_path = assets_dir.join(master_asset);
    let master_bytes = fs::read(&master_path)
        .with_context(|| format!("read title master {}", master_path.display()))?;
    let master_sha256 = sha256_hex(&master_bytes);
    expect_string(master, "sha256", &master_sha256)?;
    let expected_master_width = usize_value(master, "width")?;
    let expected_master_height = usize_value(master, "height")?;
    let (master_rgb, master_width, master_height) = decode_rgb_png(&master_bytes)?;
    if (master_width, master_height) != (expected_master_width, expected_master_height) {
        bail!(
            "title master dimensions are {master_width}x{master_height}, expected {expected_master_width}x{expected_master_height}"
        );
    }

    let palette = parse_palette(&manifest)?;
    let background_palette_roles = parse_background_palette_roles(&manifest)?;
    Ok(ReviewedTitleMaster {
        manifest,
        rgb: master_rgb,
        width: master_width,
        height: master_height,
        palette,
        background_palette_roles,
        sha256: master_sha256,
    })
}

fn validate_title_identity(manifest: &Value) -> Result<()> {
    expect_usize(manifest, "schema_version", 1)?;
    expect_string(manifest, "id", "PC98-DEMO-TITLE")?;
    expect_string(manifest, "exact_text", TITLE_TEXT)?;
    expect_string(manifest, "suffix", TITLE_SUFFIX)?;
    let pronunciation = manifest
        .get("pronunciation_medallions")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            anyhow::anyhow!("title manifest pronunciation_medallions must be an array")
        })?;
    let actual = pronunciation
        .iter()
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("title pronunciation entry must be a string"))
        })
        .collect::<Result<Vec<_>>>()?;
    if actual != TITLE_PRONUNCIATION {
        bail!("title pronunciation medallions are {actual:?}, expected {TITLE_PRONUNCIATION:?}");
    }
    Ok(())
}

fn decode_rgb_png(bytes: &[u8]) -> Result<(Vec<u8>, usize, usize)> {
    let decoder = png::Decoder::new(Cursor::new(bytes));
    let mut reader = decoder
        .read_info()
        .context("read title-master PNG header")?;
    let mut buffer = vec![0u8; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buffer)
        .context("decode title-master PNG")?;
    if info.bit_depth != png::BitDepth::Eight || info.color_type != png::ColorType::Rgb {
        bail!(
            "title master must be non-interlaced 8-bit RGB PNG, got {:?} {:?}",
            info.bit_depth,
            info.color_type
        );
    }
    buffer.truncate(info.buffer_size());
    Ok((buffer, info.width as usize, info.height as usize))
}

pub(crate) fn area_reduce_rgb(
    source: &[u8],
    source_width: usize,
    source_height: usize,
    target_width: usize,
    target_height: usize,
) -> Result<Vec<u8>> {
    if source.len() != source_width * source_height * 3 {
        bail!("title-master RGB length does not match its dimensions");
    }
    if source_width < target_width || source_height < target_height {
        bail!("title-master area reduction does not support enlargement");
    }
    let total_weight = (source_width as u64) * (source_height as u64);
    let mut target = vec![0u8; target_width * target_height * 3];
    for target_y in 0..target_height {
        let y0 = target_y * source_height;
        let y1 = (target_y + 1) * source_height;
        let source_y_start = y0 / target_height;
        let source_y_end = y1.div_ceil(target_height);
        for target_x in 0..target_width {
            let x0 = target_x * source_width;
            let x1 = (target_x + 1) * source_width;
            let source_x_start = x0 / target_width;
            let source_x_end = x1.div_ceil(target_width);
            let mut sums = [0u64; 3];
            for source_y in source_y_start..source_y_end {
                let weight_y =
                    ((source_y + 1) * target_height).min(y1) - (source_y * target_height).max(y0);
                for source_x in source_x_start..source_x_end {
                    let weight_x =
                        ((source_x + 1) * target_width).min(x1) - (source_x * target_width).max(x0);
                    let weight = (weight_x as u64) * (weight_y as u64);
                    let source_pixel = (source_y * source_width + source_x) * 3;
                    for channel in 0..3 {
                        sums[channel] += u64::from(source[source_pixel + channel]) * weight;
                    }
                }
            }
            let target_pixel = (target_y * target_width + target_x) * 3;
            for channel in 0..3 {
                target[target_pixel + channel] =
                    ((sums[channel] + total_weight / 2) / total_weight) as u8;
            }
        }
    }
    Ok(target)
}

fn decode_indices(decoded: &[u8]) -> Result<Vec<u8>> {
    if decoded.len() != DEMO_LINEAR_PLANE_BYTES * 4 {
        bail!("Demo linear B/R/G/I payload must be exactly 32000 bytes");
    }
    let mut indices = vec![0u8; DEMO_LINEAR_WIDTH * DEMO_LINEAR_HEIGHT];
    for (pixel, index) in indices.iter_mut().enumerate() {
        let mask = 0x80 >> (pixel % 8);
        let byte = pixel / 8;
        for plane in 0..4 {
            if decoded[plane * DEMO_LINEAR_PLANE_BYTES + byte] & mask != 0 {
                *index |= 1 << plane;
            }
        }
    }
    Ok(indices)
}

fn encode_indices(indices: &[u8]) -> Result<Vec<u8>> {
    if indices.len() != DEMO_LINEAR_WIDTH * DEMO_LINEAR_HEIGHT {
        bail!("Demo title indexed surface must be exactly 320x200 pixels");
    }
    if indices.iter().any(|index| *index > 15) {
        bail!("Demo title indexed surface contains a palette index above 15");
    }
    let mut decoded = vec![0u8; DEMO_LINEAR_PLANE_BYTES * 4];
    for (pixel, index) in indices.iter().copied().enumerate() {
        let mask = 0x80 >> (pixel % 8);
        let byte = pixel / 8;
        for plane in 0..4 {
            if index & (1 << plane) != 0 {
                decoded[plane * DEMO_LINEAR_PLANE_BYTES + byte] |= mask;
            }
        }
    }
    Ok(decoded)
}

fn render_indices(indices: &[u8], palette: &[[u8; 3]; 16]) -> Vec<u8> {
    indices
        .iter()
        .flat_map(|index| palette[usize::from(*index)])
        .collect()
}

pub(crate) fn nearest_palette_index(pixel: [u8; 3], palette: &[[u8; 3]; 16]) -> u8 {
    palette
        .iter()
        .enumerate()
        .min_by_key(|(_, color)| {
            pixel
                .iter()
                .zip(color.iter())
                .map(|(lhs, rhs)| {
                    let delta = i32::from(*lhs) - i32::from(*rhs);
                    (delta * delta) as u32
                })
                .sum::<u32>()
        })
        .map(|(index, _)| index as u8)
        .expect("fixed nonempty palette")
}

pub(crate) fn preserve_source_background_texture(
    source_indices: &[u8],
    target_indices: &mut [u8],
    protected_mask: &[bool],
    background_palette_roles: &[bool; 16],
) -> Result<BackgroundTextureStats> {
    if source_indices.len() != target_indices.len() || source_indices.len() != protected_mask.len()
    {
        bail!(
            "title background preservation inputs differ in length: source {}, target {}, protected mask {}",
            source_indices.len(),
            target_indices.len(),
            protected_mask.len()
        );
    }
    if source_indices.iter().any(|index| *index > 15)
        || target_indices.iter().any(|index| *index > 15)
    {
        bail!("title background preservation received a palette index above 15");
    }

    let mut source_background_pixels_preserved = 0usize;
    let mut source_logo_pixels_replaced_with_background = 0usize;
    for ((source, target), protected) in source_indices
        .iter()
        .copied()
        .zip(target_indices.iter_mut())
        .zip(protected_mask.iter().copied())
    {
        if protected {
            continue;
        }
        let source_is_background = background_palette_roles[usize::from(source)];
        let target_is_background = background_palette_roles[usize::from(*target)];
        if source_is_background && target_is_background {
            *target = source;
            source_background_pixels_preserved += 1;
        } else if !source_is_background && target_is_background {
            source_logo_pixels_replaced_with_background += 1;
        }
    }

    Ok(BackgroundTextureStats {
        source_background_pixels_preserved,
        source_logo_pixels_replaced_with_background,
    })
}

fn parse_palette(manifest: &Value) -> Result<[[u8; 3]; 16]> {
    let entries = manifest
        .get("runtime_palette_rgb")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("title manifest runtime_palette_rgb must be an array"))?;
    if entries.len() != 16 {
        bail!(
            "title runtime palette has {} entries, expected 16",
            entries.len()
        );
    }
    let mut palette = [[0u8; 3]; 16];
    for (index, entry) in entries.iter().enumerate() {
        let channels = entry
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("title palette entry {index} must be an array"))?;
        if channels.len() != 3 {
            bail!(
                "title palette entry {index} has {} channels, expected 3",
                channels.len()
            );
        }
        for (channel, value) in channels.iter().enumerate() {
            palette[index][channel] = value
                .as_u64()
                .and_then(|value| u8::try_from(value).ok())
                .ok_or_else(|| anyhow::anyhow!("title palette channel must be 0..255"))?;
        }
    }
    Ok(palette)
}

fn parse_background_palette_roles(manifest: &Value) -> Result<[bool; 16]> {
    let entries = manifest
        .get("background_palette_indices")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            anyhow::anyhow!("title manifest background_palette_indices must be an array")
        })?;
    if entries.is_empty() {
        bail!("title manifest background_palette_indices must not be empty");
    }
    let mut roles = [false; 16];
    for entry in entries {
        let index = entry
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .filter(|value| *value < roles.len())
            .ok_or_else(|| anyhow::anyhow!("title background palette index must be 0..15"))?;
        if std::mem::replace(&mut roles[index], true) {
            bail!("title background palette index {index} is duplicated");
        }
    }
    Ok(roles)
}

fn parse_protected_rectangles(manifest: &Value) -> Result<Vec<Rect>> {
    let entries = manifest
        .get("protected_rectangles")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("title manifest protected_rectangles must be an array"))?;
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
                || rect.x + rect.width > DEMO_LINEAR_WIDTH
                || rect.y + rect.height > DEMO_LINEAR_HEIGHT
            {
                bail!("title protected rectangle {rect:?} exceeds the 320x200 surface");
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
    fn brgi_index_codec_round_trips_every_role() {
        let mut indices = vec![0u8; DEMO_LINEAR_WIDTH * DEMO_LINEAR_HEIGHT];
        for (index, pixel) in indices.iter_mut().take(16).enumerate() {
            *pixel = index as u8;
        }
        let decoded = encode_indices(&indices).unwrap();
        assert_eq!(decode_indices(&decoded).unwrap(), indices);
    }

    #[test]
    fn exact_area_reduction_preserves_a_flat_color() {
        let source = [10u8, 20, 30].repeat(7 * 5);
        assert_eq!(
            area_reduce_rgb(&source, 7, 5, 3, 2).unwrap(),
            [10u8, 20, 30].repeat(3 * 2)
        );
    }

    #[test]
    fn background_policy_preserves_source_texture_without_restoring_source_logo() {
        let source = [2, 4, 3, 1];
        let mut target = [1, 2, 6, 3];
        let protected = [false, false, false, true];
        let mut background_roles = [false; 16];
        background_roles[..4].fill(true);

        let stats =
            preserve_source_background_texture(&source, &mut target, &protected, &background_roles)
                .unwrap();

        assert_eq!(target, [2, 2, 6, 3]);
        assert_eq!(stats.source_background_pixels_preserved, 1);
        assert_eq!(stats.source_logo_pixels_replaced_with_background, 1);
    }
}
