//! Fail-closed validation for gameplay-DAT translation catalogs.
//!
//! A catalog entry must cover the complete text slot selected by the DAT code,
//! not merely a convenient suffix within that slot. Otherwise patching the
//! suffix can leave a Japanese name, particle, speaker label, or first sentence
//! visible before the Korean bytes.

use anyhow::{Context, Result, bail};
use encoding_rs::SHIFT_JIS;
use serde_json::Value;

use crate::enemy_text::find_enemy_attack_names;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DatCatalogAudit {
    pub entries: usize,
}

#[derive(Debug)]
struct Entry {
    id: String,
    start: usize,
    end: usize,
}

pub fn validate_dat_translation_catalog(
    decoded: &[u8],
    catalog: &Value,
) -> Result<DatCatalogAudit> {
    let values = catalog
        .get("entries")
        .and_then(Value::as_array)
        .context("DAT catalog missing entries array")?;
    if let Some(declared) = catalog.get("entry_count") {
        let declared = declared
            .as_u64()
            .context("DAT catalog entry_count must be an integer")? as usize;
        if declared != values.len() {
            bail!(
                "DAT catalog entry_count {declared} does not match entries array length {}",
                values.len(),
            );
        }
    }
    let mut entries = Vec::with_capacity(values.len());
    let mut covered = vec![false; decoded.len()];

    for (index, value) in values.iter().enumerate() {
        let id = value
            .get("id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("entry[{index}]"));
        validate_renderer_controls(value, &id)?;
        let start = value
            .get("string_decoded_offset")
            .and_then(Value::as_str)
            .with_context(|| format!("{id}: missing string_decoded_offset"))
            .and_then(parse_hex_usize)?;
        let raw = value
            .get("raw_hex")
            .and_then(Value::as_str)
            .with_context(|| format!("{id}: missing raw_hex"))
            .and_then(decode_hex)?;
        let end = start
            .checked_add(raw.len())
            .with_context(|| format!("{id}: raw range overflow"))?;
        let actual = decoded.get(start..end).with_context(|| {
            format!("{id}: raw range 0x{start:04X}..0x{end:04X} is out of bounds")
        })?;
        if actual != raw {
            bail!("{id}: protected raw bytes do not match the decoded DAT");
        }
        let budget = value
            .get("byte_budget")
            .and_then(Value::as_u64)
            .with_context(|| format!("{id}: missing byte_budget"))? as usize;
        if budget != raw.len() + 1 {
            bail!(
                "{id}: byte_budget {budget} must cover the {} source bytes plus their NUL",
                raw.len()
            );
        }
        if decoded.get(end) != Some(&0) {
            bail!("{id}: cataloged raw bytes are not followed by their slot NUL");
        }
        covered[start..end].fill(true);
        entries.push(Entry { id, start, end });
    }

    for entry in &entries {
        validate_no_referenced_prefix(decoded, &covered, entry)?;
        validate_no_uncovered_suffix(decoded, &covered, entry)?;
    }
    validate_direct_mov_si_slots_are_covered(decoded, &covered, &entries)?;
    validate_anchored_enemy_names_are_covered(decoded, &covered)?;
    validate_shop_message_table_slots_are_covered(decoded, &covered, catalog)?;

    Ok(DatCatalogAudit {
        entries: entries.len(),
    })
}

/// Require every Japanese slot selected by the shared ten-message shop table.
///
/// Unlike ordinary DAT dialogue, the shop framework indexes these fragments
/// through a compact pointer table instead of a direct `mov si,imm16` call.
/// The final four table entries select one shared `は` fragment twice, followed
/// by `です` and `になります`; that exact source-tail shape anchors the table
/// without treating arbitrary 16-bit words as text pointers.
fn validate_shop_message_table_slots_are_covered(
    decoded: &[u8],
    covered: &[bool],
    catalog: &Value,
) -> Result<()> {
    let Some(dat) = catalog.get("dat").and_then(Value::as_str) else {
        return Ok(());
    };
    if !dat.starts_with("SHOP") || !dat.ends_with(".DAT") {
        return Ok(());
    }

    const TABLE_WORDS: usize = 10;
    const TABLE_BYTES: usize = 2 + TABLE_WORDS * 2;
    const TOPIC_FRAGMENT: &[u8] = b"\x82\xCD\x0A";
    const COPULA_FRAGMENT: &[u8] = b"\x82\xC5\x82\xB7\x0A\x0A";
    const PRICE_FRAGMENT: &[u8] = b"\x82\xC9\x82\xC8\x82\xE8\x82\xDC\x82\xB7\x0A\x0A";

    let mut tables = Vec::new();
    if decoded.len() >= TABLE_BYTES {
        for marker in 0..=decoded.len() - TABLE_BYTES {
            if decoded[marker..marker + 2] != [0xFF, 0xFF] {
                continue;
            }
            let mut targets = [0usize; TABLE_WORDS];
            for (index, target) in targets.iter_mut().enumerate() {
                let site = marker + 2 + index * 2;
                *target = u16::from_le_bytes([decoded[site], decoded[site + 1]]) as usize;
            }
            if targets[6] != targets[8]
                || nul_payload(decoded, targets[6]) != Some(TOPIC_FRAGMENT)
                || nul_payload(decoded, targets[7]) != Some(COPULA_FRAGMENT)
                || nul_payload(decoded, targets[9]) != Some(PRICE_FRAGMENT)
            {
                continue;
            }
            tables.push((marker, targets));
        }
    }

    if tables.len() != 1 {
        bail!(
            "{dat}: expected exactly one anchored ten-message shop table, found {}",
            tables.len(),
        );
    }

    let (marker, targets) = &tables[0];
    let mut missing = Vec::new();
    for &target in targets {
        let Some(raw) = nul_payload(decoded, target) else {
            bail!("{dat}: shop table at 0x{marker:04X} has an invalid pointer 0x{target:04X}");
        };
        if !is_text_with_min_japanese(raw, 1, true) {
            continue;
        }
        let end = target + raw.len();
        if !covered
            .get(target..end)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte))
        {
            missing.push(format!("0x{target:04X} {:?}", SHIFT_JIS.decode(raw).0));
        }
    }
    missing.sort();
    missing.dedup();
    if !missing.is_empty() {
        bail!(
            "{dat}: shop message-table Japanese slots are outside the catalog: {}",
            missing.join(", "),
        );
    }
    Ok(())
}

fn nul_payload(decoded: &[u8], start: usize) -> Option<&[u8]> {
    let tail = decoded.get(start..)?;
    let end = tail.iter().position(|byte| *byte == 0)?;
    Some(&tail[..end])
}

/// Require every Japanese-bearing NUL slot loaded by the DAT's direct
/// `mov si,imm16; call near` or `mov si,imm16; jmp near` text path to belong
/// to the translation catalog.
///
/// Gameplay DAT executables keep their dispatch code before the first known
/// text slot. Restricting the instruction scan to that proven code prefix,
/// requiring an immediate near call, and accepting only NUL-boundary strings
/// avoids interpreting Shift-JIS trail byte `0xBE` inside text as x86. The
/// stricter consumer shape lets this gate retain one-kana interjections that
/// the broader two-Japanese-character plausibility rule intentionally ignores.
fn validate_direct_mov_si_slots_are_covered(
    decoded: &[u8],
    covered: &[bool],
    entries: &[Entry],
) -> Result<()> {
    let code_end = entries
        .iter()
        .map(|entry| entry.start)
        .min()
        .context("DAT catalog has no entries")?;
    let mut missing = Vec::new();

    for site in 0..code_end.saturating_sub(5) {
        if decoded[site] != 0xBE || !matches!(decoded[site + 3], 0xE8 | 0xE9) {
            continue;
        }
        let target = u16::from_le_bytes([decoded[site + 1], decoded[site + 2]]) as usize;
        if target >= decoded.len() || (target != 0 && decoded[target - 1] != 0) {
            continue;
        }
        let Some(relative_nul) = decoded[target..].iter().position(|byte| *byte == 0) else {
            continue;
        };
        let end = target + relative_nul;
        let raw = &decoded[target..end];
        if raw.is_empty() || !is_text_with_min_japanese(raw, 1, true) {
            continue;
        }
        if covered
            .get(target..end)
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte))
        {
            continue;
        }
        let text = SHIFT_JIS.decode(raw).0;
        missing.push(format!("0x{target:04X} via mov-si 0x{site:04X} {text:?}"));
    }

    missing.sort();
    missing.dedup();
    if !missing.is_empty() {
        bail!(
            "direct mov-si Japanese slots are outside the catalog: {}",
            missing.join(", "),
        );
    }
    Ok(())
}

fn validate_anchored_enemy_names_are_covered(decoded: &[u8], covered: &[bool]) -> Result<()> {
    let mut missing = Vec::new();
    for name in find_enemy_attack_names(decoded) {
        let range = name.name_offset..name.name_end_offset;
        if !covered
            .get(range.clone())
            .is_some_and(|bytes| bytes.iter().all(|byte| *byte))
        {
            missing.push(format!("0x{:04X} {:?}", name.name_offset, name.name));
        }
    }
    if !missing.is_empty() {
        bail!(
            "anchored enemy name slots are outside the catalog: {}",
            missing.join(", "),
        );
    }
    Ok(())
}

fn validate_renderer_controls(value: &Value, id: &str) -> Result<()> {
    let source = value
        .get("text")
        .and_then(Value::as_str)
        .with_context(|| format!("{id}: missing protected text"))?;
    let korean = value
        .get("ko")
        .and_then(Value::as_str)
        .with_context(|| format!("{id}: missing Korean translation"))?;
    let controls = |text: &str| {
        text.chars()
            .filter(|character| matches!(*character, '\u{0001}'..='\u{000F}'))
            .collect::<Vec<_>>()
    };
    let source_controls = controls(source);
    let korean_controls = controls(korean);
    if source_controls != korean_controls {
        let show = |items: &[char]| {
            items
                .iter()
                .map(|character| format!("{:02X}", *character as u32))
                .collect::<Vec<_>>()
                .join(" ")
        };
        bail!(
            "{id}: renderer control stream changed (source [{}], Korean [{}])",
            show(&source_controls),
            show(&korean_controls)
        );
    }
    Ok(())
}

fn validate_no_referenced_prefix(decoded: &[u8], covered: &[bool], entry: &Entry) -> Result<()> {
    let lower = entry.start.saturating_sub(0x200);
    for candidate in lower..entry.start {
        let prefix = &decoded[candidate..entry.start];
        if prefix.contains(&0)
            || covered[candidate..entry.start].iter().any(|byte| *byte)
            || !is_referenced_before(decoded, candidate)
            || !is_text_with_japanese(prefix)
        {
            continue;
        }
        let text = SHIFT_JIS.decode(prefix).0;
        bail!(
            "{}: referenced Japanese prefix at 0x{candidate:04X} is outside the catalog entry: {text:?}",
            entry.id
        );
    }
    Ok(())
}

fn validate_no_uncovered_suffix(decoded: &[u8], covered: &[bool], entry: &Entry) -> Result<()> {
    let Some(relative_nul) = decoded[entry.end..].iter().position(|byte| *byte == 0) else {
        bail!("{}: decoded DAT ends before the slot NUL", entry.id);
    };
    let nul = entry.end + relative_nul;
    let mut cursor = entry.end;
    while cursor < nul {
        while cursor < nul && covered[cursor] {
            cursor += 1;
        }
        let start = cursor;
        while cursor < nul && !covered[cursor] {
            cursor += 1;
        }
        if start < cursor && is_text_with_japanese(&decoded[start..cursor]) {
            let text = SHIFT_JIS.decode(&decoded[start..cursor]).0;
            bail!(
                "{}: Japanese suffix at 0x{start:04X} is outside the catalog entry: {text:?}",
                entry.id
            );
        }
    }
    Ok(())
}

fn is_referenced_before(decoded: &[u8], offset: usize) -> bool {
    let Ok(word) = u16::try_from(offset) else {
        return false;
    };
    let bytes = word.to_le_bytes();
    decoded[..offset]
        .windows(bytes.len())
        .any(|window| window == bytes)
}

fn is_text_with_japanese(bytes: &[u8]) -> bool {
    is_text_with_min_japanese(bytes, 2, false)
}

fn is_text_with_min_japanese(bytes: &[u8], minimum: usize, allow_halfwidth: bool) -> bool {
    if bytes.is_empty() {
        return false;
    }
    let mut cursor = 0usize;
    let mut japanese = 0usize;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        match byte {
            0x01..=0x0F | 0x20..=0x7E => cursor += 1,
            0xA1..=0xDF => {
                if !allow_halfwidth {
                    return false;
                }
                let (text, _, errors) = SHIFT_JIS.decode(&bytes[cursor..cursor + 1]);
                if errors || text.chars().count() != 1 {
                    return false;
                }
                japanese += 1;
                cursor += 1;
            }
            _ => {
                let Some(pair) = bytes.get(cursor..cursor + 2) else {
                    return false;
                };
                let (text, _, errors) = SHIFT_JIS.decode(pair);
                if errors {
                    return false;
                }
                let mut chars = text.chars();
                let Some(ch) = chars.next() else {
                    return false;
                };
                if chars.next().is_some() {
                    return false;
                }
                japanese += usize::from(matches!(
                    ch,
                    '\u{3040}'..='\u{30FF}' | '\u{3400}'..='\u{9FFF}'
                ));
                cursor += 2;
            }
        }
    }
    // One accidental CJK decode is common in code/graphics bytes. Every
    // actionable omitted prefix in the current corpus has at least two source
    // characters (including the shared `妖精` speaker label).
    japanese >= minimum
}

fn parse_hex_usize(text: &str) -> Result<usize> {
    let digits = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    usize::from_str_radix(digits, 16).with_context(|| format!("invalid hex offset {text:?}"))
}

fn decode_hex(text: &str) -> Result<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        bail!("hex string has odd length");
    }
    (0..text.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&text[index..index + 2], 16)
                .with_context(|| format!("invalid hex byte at index {index}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejects_stale_declared_entry_count() {
        let decoded = b"A\0";
        let catalog = json!({
            "entry_count": 2,
            "entries": [{
                "id": "ONLY",
                "string_decoded_offset": "0x0000",
                "byte_budget": 2,
                "raw_hex": "41",
                "text": "A",
                "ko": "가"
            }]
        });

        let error = validate_dat_translation_catalog(decoded, &catalog)
            .unwrap_err()
            .to_string();
        assert!(error.contains("entry_count 2"), "{error}");
        assert!(error.contains("length 1"), "{error}");
    }

    #[test]
    fn rejects_a_referenced_japanese_prefix_outside_the_entry() {
        let mut decoded = vec![0u8; 0x40];
        decoded[0..2].copy_from_slice(&0x0008u16.to_le_bytes());
        let source = hex("82b582a982b5208ea995aa8ea9906782aa945282a682bd8149");
        decoded[0x08..0x08 + source.len()].copy_from_slice(&source);
        let suffix = hex("8ea995aa8ea9906782aa945282a682bd8149");
        let suffix_start = 0x08 + (source.len() - suffix.len());
        let catalog = json!({
            "entries": [{
                "id": "PARTIAL",
                "string_decoded_offset": format!("0x{suffix_start:04X}"),
                "byte_budget": suffix.len() + 1,
                "raw_hex": suffix.iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
                "text": "会社食堂主任が現れた！",
                "ko": "회사 식당 주임이 나타났다！"
            }]
        });

        let error = validate_dat_translation_catalog(&decoded, &catalog)
            .unwrap_err()
            .to_string();
        assert!(error.contains("referenced Japanese prefix"), "{error}");
    }

    #[test]
    fn rejects_an_anchored_enemy_name_in_a_separate_nul_slot() {
        let mut decoded = vec![0xC3];
        decoded.extend_from_slice(&hex("906c9854"));
        decoded.push(0);
        let attack_start = decoded.len();
        let attack = hex("82cc81408d558c8281490a0a");
        decoded.extend_from_slice(&attack);
        decoded.push(0);
        let catalog = json!({
            "entries": [{
                "id": "ATTACK_ONLY",
                "string_decoded_offset": format!("0x{attack_start:04X}"),
                "byte_budget": attack.len() + 1,
                "raw_hex": attack.iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
                "text": "の　攻撃！\n\n",
                "ko": "의　공격！\n\n"
            }]
        });

        let error = validate_dat_translation_catalog(&decoded, &catalog)
            .unwrap_err()
            .to_string();
        assert!(error.contains("anchored enemy name"), "{error}");
        assert!(error.contains("人狼"), "{error}");
    }

    #[test]
    fn rejects_a_dropped_renderer_control() {
        let decoded = b"A\n\n\0";
        let catalog = json!({
            "entries": [{
                "id": "DROPPED_NEWLINE",
                "string_decoded_offset": "0x0000",
                "byte_budget": 4,
                "raw_hex": "410a0a",
                "text": "A\n\n",
                "ko": "가\n"
            }]
        });

        let error = validate_dat_translation_catalog(decoded, &catalog)
            .unwrap_err()
            .to_string();
        assert!(error.contains("renderer control stream changed"), "{error}");
        assert!(error.contains("source [0A 0A], Korean [0A]"), "{error}");
    }

    #[test]
    fn rejects_a_one_kana_slot_loaded_by_direct_mov_si_call() {
        let mut decoded = vec![0u8; 0x60];
        decoded[0x08..0x0E].copy_from_slice(&[0xBE, 0x20, 0x00, 0xE8, 0x10, 0x00]);
        let missed = hex("817582aea5a5a581490a");
        decoded[0x20..0x20 + missed.len()].copy_from_slice(&missed);
        let known = hex("82a082a2");
        decoded[0x40..0x40 + known.len()].copy_from_slice(&known);
        let catalog = json!({
            "entries": [{
                "id": "KNOWN",
                "string_decoded_offset": "0x0040",
                "byte_budget": known.len() + 1,
                "raw_hex": known.iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
                "text": "あい",
                "ko": "아이"
            }]
        });

        let error = validate_dat_translation_catalog(&decoded, &catalog)
            .unwrap_err()
            .to_string();
        assert!(error.contains("direct mov-si Japanese slots"), "{error}");
        assert!(error.contains("0x0020"), "{error}");
        assert!(error.contains("0x0008"), "{error}");
        assert!(error.contains("ぐ"), "{error}");
    }

    #[test]
    fn shop_table_requires_its_shared_one_kana_fragment() {
        let mut decoded = vec![0u8; 0x80];
        decoded[0x10..0x12].copy_from_slice(&[0xFF, 0xFF]);
        let targets = [
            0x40u16, 0x40, 0x40, 0x40, 0x40, 0x40, 0x40, 0x44, 0x40, 0x4B,
        ];
        for (index, target) in targets.into_iter().enumerate() {
            let site = 0x12 + index * 2;
            decoded[site..site + 2].copy_from_slice(&target.to_le_bytes());
        }
        decoded[0x40..0x44].copy_from_slice(b"\x82\xCD\x0A\0");
        decoded[0x44..0x4B].copy_from_slice(b"\x82\xC5\x82\xB7\x0A\x0A\0");
        decoded[0x4B..0x58].copy_from_slice(b"\x82\xC9\x82\xC8\x82\xE8\x82\xDC\x82\xB7\x0A\x0A\0");
        let catalog = json!({"dat": "SHOP999.DAT"});
        let mut covered = vec![false; decoded.len()];

        let error = validate_shop_message_table_slots_are_covered(&decoded, &covered, &catalog)
            .unwrap_err()
            .to_string();
        assert!(error.contains("0x0040"), "{error}");
        assert!(error.contains("は"), "{error}");

        covered[0x40..0x43].fill(true);
        covered[0x44..0x4A].fill(true);
        covered[0x4B..0x57].fill(true);
        validate_shop_message_table_slots_are_covered(&decoded, &covered, &catalog).unwrap();
    }

    fn hex(text: &str) -> Vec<u8> {
        decode_hex(text).unwrap()
    }
}
