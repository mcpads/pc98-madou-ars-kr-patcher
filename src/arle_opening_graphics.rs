//! Compiler for Arle's standalone opening exclamation graphic.
//!
//! `OPA_PT1.CNS` stores a table of 16x16 B/R/G/I tile maps. The opening
//! script selects map 19 for the large `あっ` shown after the first dialogue
//! exchange. The resident `INT 79h / AH=3` consumer reads the map header,
//! treats each non-zero word as a tile code, and draws the four 32-byte tile
//! planes. This compiler replaces only that map and a tile range proven not to
//! be referenced by the preceding maps.

use std::{fs, io::Cursor, path::Path};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::{
    demo_title::area_reduce_rgb,
    media_identity::sha256_hex,
    overlay_lz::{decode_overlay_lz, encode_overlay_lz},
};

const MANIFEST_PATH: &str = "graphics_text/arle_opening_graphics.json";
const SURFACE_WIDTH: usize = 18 * 16;
const SURFACE_HEIGHT: usize = 12 * 16;
const TILE_WIDTH: usize = 16;
const TILE_HEIGHT: usize = 16;
const TILE_PLANE_BYTES: usize = 32;
const TILE_BYTES: usize = TILE_PLANE_BYTES * 4;
const MASKED_TILE_FLAG: u16 = 0x8000;
const TILE_INDEX_MASK: u16 = 0x1fff;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Rect {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArleOpeningGraphicBuild {
    pub packed: Vec<u8>,
    pub decoded: Vec<u8>,
    pub preview_rgb: Vec<u8>,
    pub changed_pixels: usize,
    pub unique_tiles: usize,
    pub allocated_tile_start: usize,
    pub allocated_tile_end: usize,
    pub master_sha256: String,
}

pub fn build_arle_opening_exclamation(
    source_packed: &[u8],
    assets_dir: &Path,
) -> Result<ArleOpeningGraphicBuild> {
    let manifest_path = assets_dir.join(MANIFEST_PATH);
    let manifest_bytes = fs::read(&manifest_path).with_context(|| {
        format!(
            "read Arle opening graphic manifest {}",
            manifest_path.display()
        )
    })?;
    let manifest: Value = serde_json::from_slice(&manifest_bytes).with_context(|| {
        format!(
            "parse Arle opening graphic manifest {}",
            manifest_path.display()
        )
    })?;
    expect_usize(&manifest, "schema_version", 1)?;
    expect_string(&manifest, "id", "PC98-ARLE-OPENING-EXCLAMATION")?;
    expect_string(&manifest, "exact_text", "앗")?;

    let source = object(&manifest, "source")?;
    expect_string(source, "resource", "OPA_PT1.CNS")?;
    expect_usize(source, "packed_bytes", source_packed.len())?;
    expect_string(source, "packed_sha256", &sha256_hex(source_packed))?;

    let source_report = decode_overlay_lz(source_packed).context("decode original OPA_PT1.CNS")?;
    if source_report.bytes_consumed != source_packed.len() {
        bail!(
            "OPA_PT1.CNS has trailing packed bytes after its one proven stream: consumed {} of {}",
            source_report.bytes_consumed,
            source_packed.len()
        );
    }
    expect_usize(source, "decoded_bytes", source_report.output.len())?;
    expect_string(source, "decoded_sha256", &sha256_hex(&source_report.output))?;

    let layout = object(&manifest, "layout")?;
    let graphics_offset = usize_value(layout, "graphics_offset")?;
    let map_index = usize_value(layout, "map_index")?;
    let map_offset = usize_value(layout, "map_offset")?;
    let map_width = usize_value(layout, "map_width")?;
    let map_height = usize_value(layout, "map_height")?;
    let allocated_tile_start = usize_value(layout, "allocated_tile_start")?;
    let allocated_tile_end = usize_value(layout, "allocated_tile_end")?;
    expect_usize(layout, "tile_width", TILE_WIDTH)?;
    expect_usize(layout, "tile_height", TILE_HEIGHT)?;
    expect_string(layout, "tile_layout", "plane-major B/R/G/I")?;
    if (map_width, map_height) != (SURFACE_WIDTH / TILE_WIDTH, SURFACE_HEIGHT / TILE_HEIGHT) {
        bail!(
            "Arle opening map is {map_width}x{map_height} tiles, expected {}x{}",
            SURFACE_WIDTH / TILE_WIDTH,
            SURFACE_HEIGHT / TILE_HEIGHT
        );
    }
    if allocated_tile_start > allocated_tile_end {
        bail!("Arle opening allocated tile range is reversed");
    }

    validate_source_layout(
        &source_report.output,
        graphics_offset,
        map_index,
        map_offset,
        map_width,
        map_height,
        allocated_tile_start,
        allocated_tile_end,
    )?;
    let source_indices = render_map_indices(
        &source_report.output,
        graphics_offset,
        map_offset,
        map_width,
        map_height,
    )?;

    let master = object(&manifest, "master")?;
    let master_asset = string(master, "asset")?;
    let master_path = assets_dir.join(master_asset);
    let master_bytes = fs::read(&master_path)
        .with_context(|| format!("read Arle opening graphic master {}", master_path.display()))?;
    let master_sha256 = sha256_hex(&master_bytes);
    expect_string(master, "sha256", &master_sha256)?;
    let (master_rgb, master_width, master_height) = decode_rgb_png(&master_bytes)?;
    expect_usize(master, "width", master_width)?;
    expect_usize(master, "height", master_height)?;

    let crop = parse_rect(object(master, "content_crop")?, master_width, master_height)?;
    let placement = parse_rect(
        object(&manifest, "placement")?,
        SURFACE_WIDTH,
        SURFACE_HEIGHT,
    )?;
    let cropped = crop_rgb(&master_rgb, master_width, crop)?;
    let reduced = area_reduce_rgb(
        &cropped,
        crop.width,
        crop.height,
        placement.width,
        placement.height,
    )?;

    let palette = parse_palette(&manifest)?;
    let allowed_roles = parse_allowed_palette_roles(&manifest)?;
    let background_index = usize_value(&manifest, "background_palette_index")?;
    if background_index > 15 || !allowed_roles.contains(&(background_index as u8)) {
        bail!("Arle opening background palette index must be one of the allowed 0..15 roles");
    }
    let mut target_indices = vec![background_index as u8; SURFACE_WIDTH * SURFACE_HEIGHT];
    for y in 0..placement.height {
        for x in 0..placement.width {
            let source_pixel = (y * placement.width + x) * 3;
            let rgb = [
                reduced[source_pixel],
                reduced[source_pixel + 1],
                reduced[source_pixel + 2],
            ];
            let index = nearest_allowed_palette_index(rgb, &palette, &allowed_roles);
            target_indices[(placement.y + y) * SURFACE_WIDTH + placement.x + x] = index;
        }
    }
    for required in [1u8, 2, 3, 11] {
        if !target_indices.contains(&required) {
            bail!("Arle opening normalized master does not use required palette role {required}");
        }
    }
    let changed_pixels = source_indices
        .iter()
        .zip(&target_indices)
        .filter(|(source, target)| source != target)
        .count();
    if changed_pixels == 0 {
        bail!("Arle opening graphic compiler produced no indexed-pixel changes");
    }

    let (tiles, tile_slots) = encode_surface_tiles(
        &target_indices,
        map_width,
        map_height,
        allocated_tile_start,
        allocated_tile_end,
    )?;
    let mut decoded = source_report.output;
    let map_codes_offset = map_offset + 2;
    for (slot, tile_index) in tile_slots.iter().copied().enumerate() {
        let code = MASKED_TILE_FLAG | u16::try_from(tile_index)?;
        decoded[map_codes_offset + slot * 2..map_codes_offset + slot * 2 + 2]
            .copy_from_slice(&code.to_le_bytes());
    }
    for (tile_offset, tile) in tiles.iter().enumerate() {
        let tile_index = allocated_tile_start + tile_offset;
        let start = graphics_offset + tile_index * TILE_BYTES;
        decoded[start..start + TILE_BYTES].copy_from_slice(tile);
    }

    let rebuilt_indices =
        render_map_indices(&decoded, graphics_offset, map_offset, map_width, map_height)?;
    if rebuilt_indices != target_indices {
        bail!("rebuilt OPA_PT1.CNS map does not render the normalized Korean master exactly");
    }

    let packed = encode_overlay_lz(&decoded);
    let round_trip = decode_overlay_lz(&packed).context("round-trip rebuilt OPA_PT1.CNS")?;
    if round_trip.bytes_consumed != packed.len() || round_trip.output != decoded {
        bail!("rebuilt OPA_PT1.CNS failed exact LZ round-trip");
    }

    Ok(ArleOpeningGraphicBuild {
        packed,
        decoded,
        preview_rgb: target_indices
            .iter()
            .flat_map(|index| palette[usize::from(*index)])
            .collect(),
        changed_pixels,
        unique_tiles: tiles.len(),
        allocated_tile_start,
        allocated_tile_end: allocated_tile_start + tiles.len() - 1,
        master_sha256,
    })
}

#[allow(clippy::too_many_arguments)]
fn validate_source_layout(
    decoded: &[u8],
    graphics_offset: usize,
    map_index: usize,
    map_offset: usize,
    map_width: usize,
    map_height: usize,
    allocated_tile_start: usize,
    allocated_tile_end: usize,
) -> Result<()> {
    if decoded.get(..2) != Some(&(graphics_offset as u16).to_le_bytes()) {
        bail!("OPA_PT1.CNS graphics offset does not match the pinned layout");
    }
    if graphics_offset >= decoded.len()
        || !(decoded.len() - graphics_offset).is_multiple_of(TILE_BYTES)
    {
        bail!("OPA_PT1.CNS tile bank is not an exact sequence of 128-byte tiles");
    }
    let tile_capacity = (decoded.len() - graphics_offset) / TILE_BYTES;
    if allocated_tile_end >= tile_capacity || allocated_tile_end > usize::from(TILE_INDEX_MASK) {
        bail!(
            "OPA_PT1.CNS allocated tile range ends at {allocated_tile_end:#x}, capacity is {tile_capacity:#x}"
        );
    }
    let table_offset = (map_index + 1) * 4;
    let table_map_offset = read_u16(decoded, table_offset)? as usize;
    if table_map_offset != map_offset {
        bail!(
            "OPA_PT1.CNS map {map_index} points to {table_map_offset:#x}, expected {map_offset:#x}"
        );
    }
    let map_len = 2 + map_width * map_height * 2;
    let map = decoded
        .get(map_offset..map_offset + map_len)
        .context("OPA_PT1.CNS target map exceeds decoded image")?;
    if (usize::from(map[0]), usize::from(map[1])) != (map_width, map_height) {
        bail!(
            "OPA_PT1.CNS target map header is {}x{}, expected {map_width}x{map_height}",
            map[0],
            map[1]
        );
    }

    // Every map before the target must stay outside the tile allocation. This
    // is the fail-closed consumer-ownership check for all non-target scenes.
    for preceding_index in 0..map_index {
        let entry_offset = (preceding_index + 1) * 4;
        let preceding_map_offset = read_u16(decoded, entry_offset)? as usize;
        let width = usize::from(*decoded.get(preceding_map_offset).with_context(|| {
            format!("OPA_PT1.CNS map {preceding_index} width is out of bounds")
        })?);
        let height = usize::from(*decoded.get(preceding_map_offset + 1).with_context(|| {
            format!("OPA_PT1.CNS map {preceding_index} height is out of bounds")
        })?);
        if width == 0 || height == 0 {
            bail!("OPA_PT1.CNS map {preceding_index} has an empty dimension");
        }
        let code_bytes = decoded
            .get(preceding_map_offset + 2..preceding_map_offset + 2 + width * height * 2)
            .with_context(|| format!("OPA_PT1.CNS map {preceding_index} exceeds decoded image"))?;
        for code in code_bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|bytes| u16::from_le_bytes(*bytes))
        {
            let tile_index = usize::from(code & TILE_INDEX_MASK);
            if code != 0 && tile_index >= allocated_tile_start {
                bail!(
                    "OPA_PT1.CNS map {preceding_index} references allocated tile {tile_index:#x}"
                );
            }
        }
    }

    let source_codes = map[2..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|bytes| u16::from_le_bytes(*bytes));
    for code in source_codes {
        let tile_index = usize::from(code & TILE_INDEX_MASK);
        if code & MASKED_TILE_FLAG == 0
            || tile_index < allocated_tile_start
            || tile_index > allocated_tile_end
        {
            bail!("OPA_PT1.CNS target map has unexpected source tile code {code:#06x}");
        }
    }
    Ok(())
}

fn render_map_indices(
    decoded: &[u8],
    graphics_offset: usize,
    map_offset: usize,
    map_width: usize,
    map_height: usize,
) -> Result<Vec<u8>> {
    let map_len = 2 + map_width * map_height * 2;
    let map = decoded
        .get(map_offset..map_offset + map_len)
        .context("OPA_PT1.CNS map exceeds decoded image")?;
    if (usize::from(map[0]), usize::from(map[1])) != (map_width, map_height) {
        bail!("OPA_PT1.CNS map dimensions changed while rendering");
    }
    let mut indices = vec![0u8; map_width * TILE_WIDTH * map_height * TILE_HEIGHT];
    for (slot, code_bytes) in map[2..].as_chunks::<2>().0.iter().enumerate() {
        let code = u16::from_le_bytes(*code_bytes);
        if code == 0 {
            continue;
        }
        let tile_index = usize::from(code & TILE_INDEX_MASK);
        let tile_offset = graphics_offset + tile_index * TILE_BYTES;
        let tile = decoded
            .get(tile_offset..tile_offset + TILE_BYTES)
            .with_context(|| format!("OPA_PT1.CNS tile {tile_index:#x} exceeds decoded image"))?;
        let tile_x = (slot % map_width) * TILE_WIDTH;
        let tile_y = (slot / map_width) * TILE_HEIGHT;
        for y in 0..TILE_HEIGHT {
            for x in 0..TILE_WIDTH {
                let byte = y * 2 + x / 8;
                let mask = 0x80 >> (x % 8);
                let mut index = 0u8;
                for plane in 0..4 {
                    if tile[plane * TILE_PLANE_BYTES + byte] & mask != 0 {
                        index |= 1 << plane;
                    }
                }
                indices[(tile_y + y) * map_width * TILE_WIDTH + tile_x + x] = index;
            }
        }
    }
    Ok(indices)
}

fn encode_surface_tiles(
    indices: &[u8],
    map_width: usize,
    map_height: usize,
    allocated_tile_start: usize,
    allocated_tile_end: usize,
) -> Result<(Vec<[u8; TILE_BYTES]>, Vec<usize>)> {
    if indices.len() != map_width * TILE_WIDTH * map_height * TILE_HEIGHT {
        bail!("Arle opening indexed master does not match the target map dimensions");
    }
    if indices.iter().any(|index| *index > 15) {
        bail!("Arle opening indexed master contains a palette index above 15");
    }
    let mut tiles = Vec::<[u8; TILE_BYTES]>::new();
    let mut slots = Vec::with_capacity(map_width * map_height);
    for tile_y in 0..map_height {
        for tile_x in 0..map_width {
            let mut tile = [0u8; TILE_BYTES];
            for y in 0..TILE_HEIGHT {
                for x in 0..TILE_WIDTH {
                    let index = indices[(tile_y * TILE_HEIGHT + y) * map_width * TILE_WIDTH
                        + tile_x * TILE_WIDTH
                        + x];
                    let byte = y * 2 + x / 8;
                    let mask = 0x80 >> (x % 8);
                    for plane in 0..4 {
                        if index & (1 << plane) != 0 {
                            tile[plane * TILE_PLANE_BYTES + byte] |= mask;
                        }
                    }
                }
            }
            let tile_offset = tiles.iter().position(|candidate| candidate == &tile);
            let unique_offset = if let Some(offset) = tile_offset {
                offset
            } else {
                tiles.push(tile);
                tiles.len() - 1
            };
            slots.push(allocated_tile_start + unique_offset);
        }
    }
    let capacity = allocated_tile_end - allocated_tile_start + 1;
    if tiles.len() > capacity {
        bail!(
            "Arle opening normalized master needs {} unique tiles, but the proven allocation holds {capacity}",
            tiles.len()
        );
    }
    Ok((tiles, slots))
}

fn crop_rgb(source: &[u8], source_width: usize, crop: Rect) -> Result<Vec<u8>> {
    let source_height = source.len() / 3 / source_width;
    if source.len() != source_width * source_height * 3 {
        bail!("Arle opening master RGB length does not match its dimensions");
    }
    let mut cropped = Vec::with_capacity(crop.width * crop.height * 3);
    for y in crop.y..crop.y + crop.height {
        let start = (y * source_width + crop.x) * 3;
        let end = start + crop.width * 3;
        cropped.extend_from_slice(&source[start..end]);
    }
    Ok(cropped)
}

fn decode_rgb_png(bytes: &[u8]) -> Result<(Vec<u8>, usize, usize)> {
    let decoder = png::Decoder::new(Cursor::new(bytes));
    let mut reader = decoder
        .read_info()
        .context("read Arle opening master PNG header")?;
    let mut buffer = vec![0u8; reader.output_buffer_size()];
    let info = reader
        .next_frame(&mut buffer)
        .context("decode Arle opening master PNG")?;
    if info.bit_depth != png::BitDepth::Eight || info.color_type != png::ColorType::Rgb {
        bail!(
            "Arle opening master must be non-interlaced 8-bit RGB PNG, got {:?} {:?}",
            info.bit_depth,
            info.color_type
        );
    }
    buffer.truncate(info.buffer_size());
    Ok((buffer, info.width as usize, info.height as usize))
}

fn nearest_allowed_palette_index(pixel: [u8; 3], palette: &[[u8; 3]; 16], allowed: &[u8]) -> u8 {
    *allowed
        .iter()
        .min_by_key(|index| {
            pixel
                .iter()
                .zip(palette[usize::from(**index)].iter())
                .map(|(lhs, rhs)| {
                    let delta = i32::from(*lhs) - i32::from(*rhs);
                    (delta * delta) as u32
                })
                .sum::<u32>()
        })
        .expect("validated nonempty palette role set")
}

fn parse_palette(manifest: &Value) -> Result<[[u8; 3]; 16]> {
    let entries = manifest
        .get("runtime_palette_rgb")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("Arle opening runtime_palette_rgb must be an array"))?;
    if entries.len() != 16 {
        bail!("Arle opening runtime palette must have exactly 16 entries");
    }
    let mut palette = [[0u8; 3]; 16];
    for (index, entry) in entries.iter().enumerate() {
        let channels = entry
            .as_array()
            .with_context(|| format!("Arle opening palette entry {index} must be an array"))?;
        if channels.len() != 3 {
            bail!("Arle opening palette entry {index} must have exactly three channels");
        }
        for (channel, value) in channels.iter().enumerate() {
            palette[index][channel] = value
                .as_u64()
                .and_then(|value| u8::try_from(value).ok())
                .context("Arle opening palette channel must be 0..255")?;
        }
    }
    Ok(palette)
}

fn parse_allowed_palette_roles(manifest: &Value) -> Result<Vec<u8>> {
    let entries = manifest
        .get("allowed_palette_indices")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("Arle opening allowed_palette_indices must be an array"))?;
    let mut roles = Vec::new();
    for entry in entries {
        let role = entry
            .as_u64()
            .and_then(|value| u8::try_from(value).ok())
            .filter(|value| *value < 16)
            .context("Arle opening palette role must be 0..15")?;
        if roles.contains(&role) {
            bail!("Arle opening palette role {role} is duplicated");
        }
        roles.push(role);
    }
    if roles.is_empty() {
        bail!("Arle opening allowed palette role set must not be empty");
    }
    Ok(roles)
}

fn parse_rect(value: &Value, bound_width: usize, bound_height: usize) -> Result<Rect> {
    let rect = Rect {
        x: usize_value(value, "x")?,
        y: usize_value(value, "y")?,
        width: usize_value(value, "width")?,
        height: usize_value(value, "height")?,
    };
    if rect.width == 0
        || rect.height == 0
        || rect.x + rect.width > bound_width
        || rect.y + rect.height > bound_height
    {
        bail!("Arle opening rectangle {rect:?} exceeds {bound_width}x{bound_height}");
    }
    Ok(rect)
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16> {
    let raw = bytes
        .get(offset..offset + 2)
        .with_context(|| format!("read OPA_PT1.CNS word at {offset:#x}"))?;
    Ok(u16::from_le_bytes([raw[0], raw[1]]))
}

fn object<'a>(value: &'a Value, key: &str) -> Result<&'a Value> {
    value
        .get(key)
        .filter(|value| value.is_object())
        .ok_or_else(|| anyhow::anyhow!("Arle opening manifest {key} must be an object"))
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("Arle opening manifest {key} must be a string"))
}

fn usize_value(value: &Value, key: &str) -> Result<usize> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .ok_or_else(|| {
            anyhow::anyhow!("Arle opening manifest {key} must be a non-negative integer")
        })
}

fn expect_string(value: &Value, key: &str, expected: &str) -> Result<()> {
    let actual = string(value, key)?;
    if actual != expected {
        bail!("Arle opening manifest {key} is {actual:?}, expected {expected:?}");
    }
    Ok(())
}

fn expect_usize(value: &Value, key: &str, expected: usize) -> Result<()> {
    let actual = usize_value(value, key)?;
    if actual != expected {
        bail!("Arle opening manifest {key} is {actual}, expected {expected}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_codec_round_trips_four_runtime_palette_roles() {
        let mut indices = vec![1u8; SURFACE_WIDTH * SURFACE_HEIGHT];
        for (x, role) in [1u8, 2, 3, 11].into_iter().enumerate() {
            indices[x] = role;
        }
        let (tiles, slots) = encode_surface_tiles(&indices, 18, 12, 0x223, 0x2a7).unwrap();
        assert_eq!(slots.len(), 216);
        assert!(!tiles.is_empty());

        let mut decoded = vec![0u8; 0x8d0 + 680 * TILE_BYTES];
        decoded[..2].copy_from_slice(&0x08d0u16.to_le_bytes());
        decoded[0x794] = 18;
        decoded[0x795] = 12;
        for (slot, tile_index) in slots.into_iter().enumerate() {
            let code = MASKED_TILE_FLAG | tile_index as u16;
            decoded[0x796 + slot * 2..0x798 + slot * 2].copy_from_slice(&code.to_le_bytes());
        }
        for (offset, tile) in tiles.iter().enumerate() {
            let start = 0x8d0 + (0x223 + offset) * TILE_BYTES;
            decoded[start..start + TILE_BYTES].copy_from_slice(tile);
        }
        assert_eq!(
            render_map_indices(&decoded, 0x8d0, 0x794, 18, 12).unwrap(),
            indices
        );
    }
}
