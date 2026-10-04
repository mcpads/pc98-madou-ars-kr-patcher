//! Loader for a phase-local gaiji table produced by the Rust font builder.
//!
//! The table maps each Hangul syllable to a PC-98 external-character (gaiji) JIS
//! slot, its Shift-JIS code (the in-text encoding the graphics renderer converts
//! back to JIS), and a 16x16 1bpp glyph bitmap. The build command registers
//! every glyph at boot and encodes translated names with the SJIS codes.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::hangul_probe::{GAIJI_GLYPH_BYTES, GaijiGlyph};

#[derive(Debug, Clone)]
pub struct GaijiEntry {
    pub ch: char,
    pub jis: u16,
    pub sjis: [u8; 2],
    pub bitmap: [u8; GAIJI_GLYPH_BYTES],
}

#[derive(Debug, Clone)]
pub struct GaijiTable {
    pub font: String,
    pub entries: Vec<GaijiEntry>,
    by_char: HashMap<char, usize>,
}

impl GaijiTable {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
        Self::from_json_str(&text).with_context(|| format!("parse {}", path.display()))
    }

    pub fn from_json_str(text: &str) -> Result<Self> {
        let value: Value = serde_json::from_str(text).context("parse gaiji table JSON")?;
        let font = value
            .get("font")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let raw_glyphs = value
            .get("glyphs")
            .and_then(Value::as_array)
            .context("gaiji table missing `glyphs` array")?;

        let mut entries = Vec::with_capacity(raw_glyphs.len());
        let mut by_char = HashMap::new();
        let mut seen_jis = std::collections::HashSet::new();
        let mut seen_sjis = std::collections::HashSet::new();
        for (index, raw) in raw_glyphs.iter().enumerate() {
            let ch_str = raw
                .get("char")
                .and_then(Value::as_str)
                .with_context(|| format!("glyph {index} missing `char`"))?;
            let mut chars = ch_str.chars();
            let (Some(ch), None) = (chars.next(), chars.next()) else {
                bail!("glyph {index} `char` is not a single character: {ch_str:?}");
            };
            let jis = parse_hex_u16(
                raw.get("jis")
                    .and_then(Value::as_str)
                    .with_context(|| format!("glyph {index} missing `jis`"))?,
            )?;
            let sjis_hex = raw
                .get("sjis")
                .and_then(Value::as_str)
                .with_context(|| format!("glyph {index} missing `sjis`"))?;
            let sjis_bytes = decode_hex(sjis_hex)?;
            let sjis: [u8; 2] = sjis_bytes
                .as_slice()
                .try_into()
                .map_err(|_| anyhow::anyhow!("glyph {index} `sjis` is not 2 bytes: {sjis_hex}"))?;
            let expected_sjis = jis_to_sjis(jis)
                .with_context(|| format!("glyph {index} has invalid JIS 0x{jis:04X}"))?;
            if sjis != expected_sjis {
                bail!(
                    "glyph {index} JIS 0x{jis:04X} maps to {:02X}{:02X}, not {sjis_hex}",
                    expected_sjis[0],
                    expected_sjis[1],
                );
            }
            let bitmap_vec = decode_hex(
                raw.get("glyph_hex")
                    .and_then(Value::as_str)
                    .with_context(|| format!("glyph {index} missing `glyph_hex`"))?,
            )?;
            let bitmap: [u8; GAIJI_GLYPH_BYTES] =
                bitmap_vec.as_slice().try_into().map_err(|_| {
                    anyhow::anyhow!(
                        "glyph {index} bitmap is {} bytes, expected {GAIJI_GLYPH_BYTES}",
                        bitmap_vec.len()
                    )
                })?;

            if by_char.insert(ch, index).is_some() {
                bail!("gaiji table has duplicate syllable {ch:?}");
            }
            if !seen_jis.insert(jis) {
                bail!("gaiji table has duplicate JIS slot 0x{jis:04X}");
            }
            if !seen_sjis.insert(sjis) {
                bail!("gaiji table has duplicate Shift-JIS code {sjis_hex}");
            }
            entries.push(GaijiEntry {
                ch,
                jis,
                sjis,
                bitmap,
            });
        }
        if entries.is_empty() {
            bail!("gaiji table is empty");
        }
        Ok(Self {
            font,
            entries,
            by_char,
        })
    }

    pub fn gaiji_glyphs(&self) -> Vec<GaijiGlyph> {
        self.entries
            .iter()
            .map(|entry| GaijiGlyph {
                jis: entry.jis,
                bitmap: entry.bitmap,
            })
            .collect()
    }

    pub fn sjis_for(&self, ch: char) -> Result<[u8; 2]> {
        let index = *self
            .by_char
            .get(&ch)
            .with_context(|| format!("no gaiji glyph registered for {ch:?}"))?;
        Ok(self.entries[index].sjis)
    }

    pub fn contains_sjis(&self, sjis: [u8; 2]) -> bool {
        self.entries.iter().any(|entry| entry.sjis == sjis)
    }

    /// Encode a Hangul string into its gaiji Shift-JIS bytes. Every syllable must
    /// have a registered glyph, mirroring the "encoding miss is a build error"
    /// invariant.
    pub fn encode(&self, text: &str) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(text.chars().count() * 2);
        for ch in text.chars() {
            out.extend_from_slice(&self.sjis_for(ch)?);
        }
        Ok(out)
    }
}

fn jis_to_sjis(jis: u16) -> Result<[u8; 2]> {
    let row = (jis >> 8) as u8;
    let cell = jis as u8;
    if !(0x21..=0x7E).contains(&row) || !(0x21..=0x7E).contains(&cell) {
        bail!("JIS bytes are outside 0x21..0x7E");
    }
    let mut lead = ((row - 0x21) >> 1) + 0x81;
    if lead > 0x9F {
        lead += 0x40;
    }
    let trail = if row % 2 == 1 {
        cell + 0x1F + u8::from(cell > 0x5F)
    } else {
        cell + 0x7E
    };
    Ok([lead, trail])
}

fn parse_hex_u16(text: &str) -> Result<u16> {
    let hex = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    u16::from_str_radix(hex, 16).with_context(|| format!("parse hex u16 {text}"))
}

fn decode_hex(text: &str) -> Result<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        bail!("hex string has odd length: {text}");
    }
    (0..text.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&text[i..i + 2], 16)
                .with_context(|| format!("parse hex byte in {text}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{GaijiTable, jis_to_sjis};

    fn sample_json() -> String {
        let g0 = "0".repeat(64);
        let g1 = "f".repeat(64);
        format!(
            r#"{{"font":"test-font.ttf","glyphs":[
                {{"char":"뿌","jis":"0x7621","sjis":"eb9f","glyph_hex":"{g0}"}},
                {{"char":"요","jis":"0x7622","sjis":"eba0","glyph_hex":"{g1}"}}
            ]}}"#
        )
    }

    #[test]
    fn parses_and_encodes() {
        let table = GaijiTable::from_json_str(&sample_json()).unwrap();
        assert_eq!(table.entries.len(), 2);
        assert_eq!(table.entries[0].ch, '뿌');
        assert_eq!(table.entries[0].jis, 0x7621);
        assert_eq!(table.entries[0].sjis, [0xEB, 0x9F]);
        assert_eq!(table.gaiji_glyphs().len(), 2);
        // "뿌요뿌요" -> two distinct codes, repeated.
        assert_eq!(
            table.encode("뿌요뿌요").unwrap(),
            vec![0xEB, 0x9F, 0xEB, 0xA0, 0xEB, 0x9F, 0xEB, 0xA0]
        );
    }

    #[test]
    fn missing_syllable_is_an_error() {
        let table = GaijiTable::from_json_str(&sample_json()).unwrap();
        assert!(table.encode("나").is_err());
    }

    #[test]
    fn rejects_a_jis_sjis_mapping_mismatch() {
        let bad = sample_json().replacen("eb9f", "eba0", 1);
        assert!(GaijiTable::from_json_str(&bad).is_err());
    }

    #[test]
    fn odd_jis_row_skips_the_illegal_sjis_trail_7f() {
        assert_eq!(jis_to_sjis(0x775E).unwrap(), [0xEC, 0x7D]);
        assert_eq!(jis_to_sjis(0x775F).unwrap(), [0xEC, 0x7E]);
        assert_eq!(jis_to_sjis(0x7760).unwrap(), [0xEC, 0x80]);
        for cell in 0x21..=0x7E {
            assert_ne!(jis_to_sjis(0x7700 | cell).unwrap()[1], 0x7F);
        }
    }
}
