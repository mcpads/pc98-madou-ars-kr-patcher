//! RMS.DAT is a 160-pixel-wide monochrome strip. ENDING_R's credit table
//! selects 16 rows at a time; only declared role/company rows are translated.
use crate::{
    font_build::{FontProfile, rasterize_glyphs},
    media_identity::sha256_hex,
    overlay_lz::{decode_overlay_lz, encode_overlay_lz},
};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::{collections::BTreeSet, path::Path};

const ROLE_OFFSETS: [usize; 17] = [
    0, 0x280, 0x500, 0x780, 0xB40, 0x1540, 0x1900, 0x1CC0, 0x2080, 0x3700, 0x3E80, 0x4100, 0x4380,
    0x4600, 0x4880, 0x5000, 0x5140,
];
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    review_status: String,
    resource: String,
    packed_sha256: String,
    decoded_sha256: String,
    font_profile_id: String,
    surfaces: Vec<Surface>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Surface {
    offset: usize,
    source_text: String,
    text: String,
}
pub struct CreditBuild {
    pub packed: Vec<u8>,
    pub decoded: Vec<u8>,
    pub surfaces: usize,
}

pub fn build(
    source: &[u8],
    assets: &Path,
    profile: &FontProfile,
    allow_needs_review: bool,
) -> Result<CreditBuild> {
    let manifest: Manifest = serde_json::from_slice(&std::fs::read(
        assets.join("graphics_text/rulue_credits.json"),
    )?)?;
    if manifest.review_status != "complete"
        && !(allow_needs_review && manifest.review_status == "needs_review")
    {
        bail!("Rulue credits require approved graphics text");
    }
    if manifest.resource != "RMS.DAT" || manifest.font_profile_id != profile.id {
        bail!("Rulue credits resource/font identity differs");
    }
    if sha256_hex(source) != manifest.packed_sha256 {
        bail!("RMS source packed identity differs");
    }
    let stream = decode_overlay_lz(source)?;
    if stream.bytes_consumed != source.len()
        || stream.output.len() != 32000
        || sha256_hex(&stream.output) != manifest.decoded_sha256
    {
        bail!("RMS decoded identity differs");
    }
    if manifest
        .surfaces
        .iter()
        .map(|s| s.offset)
        .collect::<Vec<_>>()
        != ROLE_OFFSETS
    {
        bail!("RMS surfaces differ from consumer-proven role rows");
    }
    let mut demand = BTreeSet::new();
    for surface in &manifest.surfaces {
        if surface.source_text.is_empty() || surface.text.is_empty() {
            bail!("RMS label is empty");
        }
        demand.extend(surface.text.chars().filter(|c| *c != ' '));
    }
    let glyphs = rasterize_glyphs(profile, &demand)?;
    let mut decoded = stream.output.clone();
    for surface in &manifest.surfaces {
        let width = surface
            .text
            .chars()
            .map(|c| if c == ' ' { 8 } else { 16 })
            .sum::<usize>();
        if width > 160 {
            bail!("RMS label {:?} exceeds 160 pixels", surface.text);
        }
        let slot = &mut decoded[surface.offset..surface.offset + 320];
        slot.fill(0);
        let mut x = (160 - width) / 2;
        for ch in surface.text.chars() {
            if ch == ' ' {
                x += 8;
                continue;
            }
            let glyph = glyphs
                .get(&ch)
                .with_context(|| format!("missing RMS glyph {ch}"))?;
            for y in 0..16 {
                for gx in 0..16 {
                    if glyph[y * 2 + gx / 8] & (0x80 >> (gx % 8)) != 0 {
                        slot[y * 20 + (x + gx) / 8] |= 0x80 >> ((x + gx) % 8);
                    }
                }
            }
            x += 16;
        }
    }
    for (i, (before, after)) in stream.output.iter().zip(&decoded).enumerate() {
        if before != after && !ROLE_OFFSETS.iter().any(|o| (*o..*o + 320).contains(&i)) {
            bail!("RMS protected byte changed at {i:04X}");
        }
    }
    let packed = encode_overlay_lz(&decoded);
    let check = decode_overlay_lz(&packed)?;
    if check.output != decoded || check.bytes_consumed != packed.len() {
        bail!("RMS compression round trip differs");
    }
    Ok(CreditBuild {
        packed,
        decoded,
        surfaces: manifest.surfaces.len(),
    })
}
