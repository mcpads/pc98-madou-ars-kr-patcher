//! Localize the four-row ending caption in ED_A's own picture-gaiji bank.
//! DEMO 07CB/121F register 38 consecutive 32-byte bitmaps; the apparent +34
//! stride is offset by the callee's -2 adjustment. Registration is unchanged.
use crate::{
    font_build::{FontProfile, rasterize_glyphs},
    media_identity::sha256_hex,
};
use anyhow::{Context, Result, bail};
use std::collections::BTreeSet;
const BANK: std::ops::Range<usize> = 0x504A..0x550A;
const ROWS: [usize; 4] = [0x1565, 0x1594, 0x15C3, 0x15F2];

pub fn install(
    decoded: &mut [u8],
    label: &serde_json::Value,
    profile: &FontProfile,
    allow_needs_review: bool,
) -> Result<()> {
    if label["source_text"] != "おしまい" {
        bail!("Arle end-card source label differs");
    }
    let status = label["status"].as_str().unwrap_or("");
    if status != "complete" && !(allow_needs_review && status == "needs_review") {
        bail!("Arle end-card wording is not approved");
    }
    let text = label["ko"].as_str().context("Arle end card lacks ko")?;
    if text.chars().count() != 1 || !text.chars().all(|c| ('가'..='힣').contains(&c)) {
        bail!("Arle end card needs one Hangul syllable for its 64x64 picture");
    }
    if sha256_hex(decoded.get(BANK).context("Arle picture bank absent")?)
        != "882673b52b79bee00dc15144a028626e2fa61651168b16ab8cd33b92eb7c51c3"
    {
        bail!("Arle picture bank differs from source");
    }
    let expected: [&[u8]; 4] = [
        &[
            0xEB, 0x9F, 0xEB, 0xA0, 0xEB, 0xA1, 0xEB, 0xA2, 0x81, 0x40, 0xEB, 0xA3, 0xEB, 0xA4,
            0xEB, 0xA5, 0xEB, 0xA6, 0xEB, 0xA7, 0x81, 0x40,
        ],
        &[
            0xEB, 0xA8, 0xEB, 0xA9, 0xEB, 0xAA, 0xEB, 0xAB, 0x81, 0x40, 0xEB, 0xAC, 0xEB, 0xAD,
            0xEB, 0xAE, 0xEB, 0xAF, 0xEB, 0xB0, 0xEB, 0xB1,
        ],
        &[
            0xEB, 0xB2, 0xEB, 0xB3, 0xEB, 0xB4, 0xEB, 0xB5, 0x81, 0x40, 0xEB, 0xB6, 0xEB, 0xB7,
            0xEB, 0xB8, 0xEB, 0xB9, 0xEB, 0xBA, 0xEB, 0xBB,
        ],
        &[
            0xEB, 0xBC, 0xEB, 0xBD, 0xEB, 0xBE, 0xEB, 0xBF, 0xEB, 0xC0, 0xEB, 0xC1, 0xEB, 0xC2,
            0xEB, 0xC3, 0xEB, 0xC4, 0x81, 0x40, 0x81, 0x40,
        ],
    ];
    for (offset, cells) in ROWS.iter().zip(expected) {
        let row = decoded
            .get(*offset..*offset + 45)
            .context("Arle picture row absent")?;
        if row[..20] != [b' '; 20] || &row[20..42] != cells || row[42..] != [b' ', b' ', 0] {
            bail!("Arle picture row differs at {offset:04X}");
        }
    }
    let demand = text.chars().collect::<BTreeSet<_>>();
    let glyphs = rasterize_glyphs(profile, &demand)?;
    let bitmap = glyphs
        .values()
        .next()
        .context("Arle end-card bitmap absent")?;
    let mut bank = [0u8; 38 * 32];
    // Nearest-neighbour 4x scaling keeps the approved raster font's geometry.
    for y in 0..64 {
        for x in 0..64 {
            if bitmap[(y / 4) * 2 + (x / 4) / 8] & (0x80 >> ((x / 4) % 8)) != 0 {
                let cell = (y / 16) * 4 + x / 16;
                bank[cell * 32 + (y % 16) * 2 + (x % 16) / 8] |= 0x80 >> (x % 8);
            }
        }
    }
    decoded[BANK].copy_from_slice(&bank);
    for (y, offset) in ROWS.iter().enumerate() {
        let row = &mut decoded[*offset..*offset + 44];
        row.fill(b' ');
        for x in 0..4 {
            row[18 + x * 2] = 0xEB;
            row[19 + x * 2] = 0x9F + (y * 4 + x) as u8;
        }
    }
    Ok(())
}
