//! Validation for staged cutscene translations against the immutable raw catalog.

use anyhow::{Context, Result, bail};
use encoding_rs::SHIFT_JIS;
use serde_json::Value;
use std::collections::BTreeMap;

use crate::gaiji_table::GaijiTable;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CutsceneOverflow {
    pub id: String,
    pub required: usize,
    pub budget: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CutsceneTranslationReport {
    pub entries: usize,
    pub targets: usize,
    pub structural: usize,
    pub over_budget: Vec<CutsceneOverflow>,
    pub max_growth: usize,
    pub pointer_grid_repack: Option<CutsceneRepackUsage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CutsceneRepackUsage {
    pub region: String,
    pub required: usize,
    pub capacity: usize,
}

/// Refresh protected catalog metadata from a newly emitted raw catalog while
/// preserving only the declared translator-editable fields. Entry IDs are the
/// stable join key; missing, duplicate, or extra IDs fail before any caller
/// writes the result.
pub fn refresh_catalog_metadata(raw: &Value, translation: &Value) -> Result<Value> {
    let raw_resource = raw
        .get("resource")
        .and_then(Value::as_str)
        .context("raw catalog missing `resource`")?;
    let translated_resource = translation
        .get("resource")
        .and_then(Value::as_str)
        .context("translation catalog missing `resource`")?;
    if raw_resource != translated_resource {
        bail!("resource mismatch: raw {raw_resource:?}, translation {translated_resource:?}");
    }

    let editable_fields = raw
        .get("editable_fields")
        .and_then(Value::as_array)
        .context("raw catalog missing `editable_fields`")?
        .iter()
        .map(|field| {
            field
                .as_str()
                .context("editable field name is not a string")
        })
        .collect::<Result<Vec<_>>>()?;
    let mut translated_by_id = BTreeMap::<&str, &Value>::new();
    for entry in entries(translation)? {
        let id = string_field(entry, "id", "entry")?;
        if translated_by_id.insert(id, entry).is_some() {
            bail!("{raw_resource}: duplicate translated entry id {id}");
        }
    }

    let mut refreshed = raw.clone();
    let refreshed_entries = refreshed
        .get_mut("entries")
        .and_then(Value::as_array_mut)
        .context("refreshed catalog missing `entries` array")?;
    for entry in refreshed_entries {
        let id = string_field(entry, "id", "entry")?.to_owned();
        let translated_entry = translated_by_id
            .remove(id.as_str())
            .with_context(|| format!("{raw_resource}: missing translated entry {id}"))?;
        let output = entry
            .as_object_mut()
            .with_context(|| format!("{id}: raw entry is not an object"))?;
        for &name in &editable_fields {
            output.insert(name.to_owned(), field(translated_entry, name, &id)?.clone());
        }
    }
    if let Some((extra, _)) = translated_by_id.into_iter().next() {
        bail!("{raw_resource}: translated catalog has extra entry {extra}");
    }
    Ok(refreshed)
}

fn entries(value: &Value) -> Result<&Vec<Value>> {
    value
        .get("entries")
        .and_then(Value::as_array)
        .context("catalog missing `entries` array")
}

fn field<'a>(entry: &'a Value, name: &str, id: &str) -> Result<&'a Value> {
    entry
        .get(name)
        .with_context(|| format!("{id}: missing `{name}`"))
}

fn string_field<'a>(entry: &'a Value, name: &str, id: &str) -> Result<&'a str> {
    field(entry, name, id)?
        .as_str()
        .with_context(|| format!("{id}: `{name}` is not a string"))
}

fn is_structural(entry: &Value) -> bool {
    entry
        .get("flags")
        .and_then(Value::as_array)
        .is_some_and(|flags| flags.iter().any(|flag| flag.as_str() == Some("structural")))
}

fn raw_token_payloads(text: &str) -> Result<Vec<String>> {
    let mut tokens = Vec::new();
    let mut rest = text;
    loop {
        let raw_start = rest.find("{raw:");
        let omit_start = rest.find("{omit_raw:");
        let (start, prefix) = match (raw_start, omit_start) {
            (Some(raw), Some(omit)) if raw <= omit => (raw, "{raw:"),
            (Some(_), Some(omit)) => (omit, "{omit_raw:"),
            (Some(raw), None) => (raw, "{raw:"),
            (None, Some(omit)) => (omit, "{omit_raw:"),
            (None, None) => break,
        };
        rest = &rest[start..];
        let end = rest
            .find('}')
            .with_context(|| format!("unclosed `{prefix}...}}` token"))?;
        let token = &rest[..=end];
        let hex = &token[prefix.len()..token.len() - 1];
        if hex.is_empty()
            || !hex.len().is_multiple_of(2)
            || !hex.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            bail!("malformed raw token {token:?}");
        }
        tokens.push(hex.to_ascii_uppercase());
        rest = &rest[end + 1..];
    }
    Ok(tokens)
}

fn contains_source_japanese(text: &str) -> bool {
    text.chars().any(|ch| {
        ('\u{3040}'..='\u{309F}').contains(&ch)
            || (('\u{30A0}'..='\u{30FF}').contains(&ch) && !matches!(ch, '・' | 'ー'))
            || ('\u{4E00}'..='\u{9FFF}').contains(&ch)
    })
}

fn wide_cell_char(ch: char) -> char {
    match ch {
        ' ' => '\u{3000}',
        '!'..='~' => char::from_u32(ch as u32 + 0xFEE0).expect("full-width ASCII is valid"),
        _ => ch,
    }
}

fn encode_text_with(
    text: &str,
    wide_cells: bool,
    newline_controls: &[u8],
    mut hangul_code: impl FnMut(char) -> Result<[u8; 2]>,
    mut validate_fallback: impl FnMut(char, &[u8]) -> Result<()>,
) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut rest = text;
    let mut newline_index = 0usize;
    while !rest.is_empty() {
        if rest.starts_with("{raw:") {
            let end = rest.find('}').context("unclosed `{raw:...}` token")?;
            let token = &rest[..=end];
            let hex = &token[5..token.len() - 1];
            if hex.is_empty()
                || !hex.len().is_multiple_of(2)
                || !hex.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                bail!("malformed raw token {token:?}");
            }
            for offset in (0..hex.len()).step_by(2) {
                output.push(
                    u8::from_str_radix(&hex[offset..offset + 2], 16)
                        .with_context(|| format!("parse raw token {token:?}"))?,
                );
            }
            rest = &rest[end + 1..];
            continue;
        }
        if rest.starts_with("{omit_raw:") {
            let end = rest.find('}').context("unclosed `{omit_raw:...}` token")?;
            let token = &rest[..=end];
            let hex = &token[10..token.len() - 1];
            if hex.is_empty()
                || !hex.len().is_multiple_of(2)
                || !hex.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                bail!("malformed omitted raw token {token:?}");
            }
            rest = &rest[end + 1..];
            continue;
        }

        let ch = rest.chars().next().expect("rest is not empty");
        rest = &rest[ch.len_utf8()..];
        if ('가'..='힣').contains(&ch) {
            output.extend_from_slice(&hangul_code(ch)?);
            continue;
        }
        if matches!(ch, '\n' | '\r') {
            let control = *newline_controls.get(newline_index).with_context(|| {
                format!(
                    "translation has more newlines than the protected source control sequence ({})",
                    newline_controls.len()
                )
            })?;
            if !matches!(control, b'\n' | b'\r') {
                bail!("unsupported protected newline control 0x{control:02X}");
            }
            output.push(control);
            newline_index += 1;
            continue;
        }
        let ch = if wide_cells { wide_cell_char(ch) } else { ch };
        let mut buffer = [0u8; 4];
        let encoded_source = ch.encode_utf8(&mut buffer);
        let (encoded, _, errors) = SHIFT_JIS.encode(encoded_source);
        if errors {
            bail!("character {ch:?} is neither Hangul nor Shift-JIS encodable");
        }
        if wide_cells && encoded.len() != 2 {
            bail!("character {ch:?} is not encodable as one two-byte credit cell");
        }
        validate_fallback(ch, &encoded)?;
        output.extend_from_slice(&encoded);
    }
    if newline_index != newline_controls.len() {
        bail!(
            "translation has {newline_index} newline(s), protected source has {}",
            newline_controls.len()
        );
    }
    Ok(output)
}

fn encoded_len(text: &str, wide_cells: bool) -> Result<usize> {
    let newline_controls = vec![b'\r'; text.chars().filter(|ch| matches!(ch, '\n' | '\r')).count()];
    Ok(encode_text_with(
        text,
        wide_cells,
        &newline_controls,
        |_| Ok([0, 0]),
        |_, _| Ok(()),
    )?
    .len())
}

/// Encode one staged cutscene translation into the resource byte stream.
/// Hangul uses the phase's BIOS-gaiji table, each catalog newline reuses the
/// corresponding protected source control byte (`0x0A` or `0x0D`), and
/// `{raw:...}` tokens reproduce their protected bytes exactly. An explicitly
/// reviewed `{omit_raw:...}` directive accounts for the same protected source
/// token without emitting it. Schezo credit strings set `wide_cells` so ASCII
/// is normalized to full-width Shift-JIS before the word-at-a-time consumer
/// sees it.
pub fn encode_cutscene_text(
    text: &str,
    wide_cells: bool,
    gaiji: &GaijiTable,
    newline_controls: &[u8],
) -> Result<Vec<u8>> {
    encode_text_with(
        text,
        wide_cells,
        newline_controls,
        |ch| gaiji.sjis_for(ch),
        |ch, bytes| {
            if let Ok(code) = <[u8; 2]>::try_from(bytes)
                && gaiji.contains_sjis(code)
            {
                bail!(
                    "plain Shift-JIS character {ch:?} collides with registered gaiji code {:02X}{:02X}",
                    code[0],
                    code[1],
                );
            }
            Ok(())
        },
    )
}

pub fn validate_catalog_pair(
    raw: &Value,
    translation: &Value,
) -> Result<CutsceneTranslationReport> {
    let raw_resource = raw
        .get("resource")
        .and_then(Value::as_str)
        .context("raw catalog missing `resource`")?;
    let translated_resource = translation
        .get("resource")
        .and_then(Value::as_str)
        .context("translation catalog missing `resource`")?;
    if raw_resource != translated_resource {
        bail!("resource mismatch: raw {raw_resource:?}, translation {translated_resource:?}");
    }

    let mut raw_header = raw.clone();
    let mut translated_header = translation.clone();
    raw_header
        .as_object_mut()
        .context("raw catalog is not an object")?
        .remove("entries");
    translated_header
        .as_object_mut()
        .context("translation catalog is not an object")?
        .remove("entries");
    if raw_header != translated_header {
        bail!("{raw_resource}: protected catalog metadata changed");
    }

    let protected_fields = raw
        .get("protected_fields")
        .and_then(Value::as_array)
        .context("raw catalog missing `protected_fields`")?;
    let raw_entries = entries(raw)?;
    let translated_entries = entries(translation)?;
    if raw_entries.len() != translated_entries.len() {
        bail!(
            "{raw_resource}: entry count changed from {} to {}",
            raw_entries.len(),
            translated_entries.len()
        );
    }

    let mut targets = 0usize;
    let mut structural = 0usize;
    let mut over_budget = Vec::new();
    let mut max_growth = 0usize;
    let mut pointer_grid_required = 0usize;
    let mut pointer_grid_capacity = 0usize;
    for (raw_entry, translated_entry) in raw_entries.iter().zip(translated_entries) {
        let id = string_field(raw_entry, "id", "entry")?;
        if string_field(translated_entry, "id", id)? != id {
            bail!("{raw_resource}: entry order or id changed at {id}");
        }
        for protected in protected_fields {
            let name = protected
                .as_str()
                .context("protected field name is not a string")?;
            if field(raw_entry, name, id)? != field(translated_entry, name, id)? {
                bail!("{id}: protected `{name}` changed");
            }
        }

        let ko = string_field(translated_entry, "ko", id)?;
        let status = string_field(translated_entry, "status", id)?;
        let notes = string_field(translated_entry, "notes", id)?;
        if is_structural(raw_entry) {
            structural += 1;
            if !ko.is_empty() || status != "not_applicable" {
                bail!("{id}: structural entry must stay empty and `not_applicable`");
            }
            continue;
        }

        targets += 1;
        if ko.trim().is_empty() {
            bail!("{id}: empty cutscene translation");
        }
        if !matches!(status, "needs_review" | "needs_human_review" | "complete") {
            bail!("{id}: invalid translated status {status:?}");
        }
        if notes.trim().is_empty() {
            bail!("{id}: translated entry needs a review note");
        }
        let source = string_field(raw_entry, "text", id)?;
        if source.bytes().filter(|&byte| byte == b'\n').count()
            != ko.bytes().filter(|&byte| byte == b'\n').count()
        {
            bail!("{id}: newline count changed");
        }
        if source.contains("{omit_raw:") {
            bail!("{id}: immutable source cannot contain an omitted-raw directive");
        }
        if raw_token_payloads(source).with_context(|| format!("{id}: source raw tokens"))?
            != raw_token_payloads(ko).with_context(|| format!("{id}: translation raw tokens"))?
        {
            bail!("{id}: raw token sequence changed");
        }
        if contains_source_japanese(ko) {
            bail!("{id}: translation still contains Japanese characters");
        }
        let byte_budget = field(raw_entry, "byte_budget", id)?
            .as_u64()
            .with_context(|| format!("{id}: `byte_budget` is not an integer"))?
            as usize;
        // The Rulue/Schezo cutscene consumer treats every visible character as
        // one two-byte cell (control CR/$ remain one byte). ASCII in an OVL
        // translation must therefore become full-width Shift-JIS; otherwise an
        // ASCII space consumes the following Hangul lead byte as its trail.
        // Arle DAT consumers are not yet mapped, so do not project this policy
        // onto them without runtime proof.
        let wide_cells = raw_resource.ends_with(".OVL");
        let pointer_grid =
            raw_resource == "SHEZO_ED.OVL" && string_field(raw_entry, "region", id)? == "credits";
        let required =
            encoded_len(ko, wide_cells).with_context(|| format!("{id}: encode translation"))? + 1;
        if pointer_grid {
            pointer_grid_required += required;
            pointer_grid_capacity += byte_budget;
        }
        if required > byte_budget {
            over_budget.push(CutsceneOverflow {
                id: id.to_string(),
                required,
                budget: byte_budget,
            });
            max_growth = max_growth.max(required - byte_budget);
        }
    }

    if pointer_grid_required > pointer_grid_capacity {
        bail!(
            "{raw_resource}: credits need {pointer_grid_required} bytes after repack but the bounded region holds {pointer_grid_capacity}"
        );
    }
    Ok(CutsceneTranslationReport {
        entries: raw_entries.len(),
        targets,
        structural,
        over_budget,
        max_growth,
        pointer_grid_repack: (pointer_grid_capacity != 0).then(|| CutsceneRepackUsage {
            region: "credits".to_owned(),
            required: pointer_grid_required,
            capacity: pointer_grid_capacity,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn gaiji() -> GaijiTable {
        GaijiTable::from_json_str(&format!(
            r#"{{"glyphs":[{{"char":"가","jis":"0x7621","sjis":"eb9f","glyph_hex":"{}"}}]}}"#,
            "00".repeat(32),
        ))
        .unwrap()
    }

    fn pair() -> (Value, Value) {
        let raw = json!({
            "schema": "test",
            "resource": "TEST.OVL",
            "protected_fields": ["id", "text", "raw_hex", "byte_budget", "flags"],
            "editable_fields": ["ko", "status", "notes"],
            "entries": [{
                "id": "TEST_0100",
                "text": "あ{raw:8547}\n",
                "raw_hex": "828085470d",
                "byte_budget": 32,
                "flags": ["raw_tokens"],
                "ko": "",
                "status": "untranslated",
                "notes": ""
            }]
        });
        let mut translation = raw.clone();
        let entry = &mut translation["entries"][0];
        entry["ko"] = json!("가{raw:8547}\n");
        entry["status"] = json!("needs_review");
        entry["notes"] = json!("draft");
        (raw, translation)
    }

    #[test]
    fn credit_length_counts_ascii_as_full_width_cells() {
        assert_eq!(encoded_len("ABC", false).unwrap(), 3);
        assert_eq!(encoded_len("ABC", true).unwrap(), 6);
        assert_eq!(encoded_len("가 A", true).unwrap(), 6);
    }

    #[test]
    fn production_encoder_preserves_lf_cr_sequence_and_wide_credit_cells() {
        assert_eq!(
            encode_cutscene_text("가\n{raw:8547}A\n", false, &gaiji(), &[0x0A, 0x0D]).unwrap(),
            [0xEB, 0x9F, 0x0A, 0x85, 0x47, 0x41, 0x0D],
        );
        assert_eq!(
            encode_cutscene_text("가 A", true, &gaiji(), &[]).unwrap(),
            [0xEB, 0x9F, 0x81, 0x40, 0x82, 0x60],
        );
        assert_eq!(
            encode_cutscene_text("가{omit_raw:8643}", false, &gaiji(), &[]).unwrap(),
            [0xEB, 0x9F],
        );
    }

    #[test]
    fn production_encoder_rejects_newline_control_count_mismatch() {
        assert!(encode_cutscene_text("가\n", false, &gaiji(), &[]).is_err());
        assert!(encode_cutscene_text("가", false, &gaiji(), &[0x0A]).is_err());
        assert!(encode_cutscene_text("가\n", false, &gaiji(), &[0x0C]).is_err());
    }

    #[test]
    fn refreshes_protected_metadata_without_touching_translation_fields() {
        let (raw, mut translation) = pair();
        let mut refreshed_raw = raw.clone();
        refreshed_raw["entries"][0]["rewrite_sites"] =
            json!([{"site":"0x1234","kind":"pointer_grid"}]);
        translation["entries"][0]["notes"] = json!("keep this review note");

        let refreshed = refresh_catalog_metadata(&refreshed_raw, &translation).unwrap();

        assert_eq!(
            refreshed["entries"][0]["rewrite_sites"],
            refreshed_raw["entries"][0]["rewrite_sites"]
        );
        assert_eq!(refreshed["entries"][0]["ko"], "가{raw:8547}\n");
        assert_eq!(refreshed["entries"][0]["notes"], "keep this review note");
    }

    #[test]
    fn accepts_protected_translation_with_raw_tokens() {
        let (raw, translation) = pair();
        let report = validate_catalog_pair(&raw, &translation).unwrap();
        assert_eq!(report.targets, 1);
        assert!(report.over_budget.is_empty());
    }

    #[test]
    fn accepts_an_explicitly_omitted_protected_raw_token() {
        let (raw, mut translation) = pair();
        translation["entries"][0]["ko"] = json!("가{omit_raw:8547}\n");
        translation["entries"][0]["notes"] = json!("reviewed omission");

        let report = validate_catalog_pair(&raw, &translation).unwrap();

        assert_eq!(report.targets, 1);
        assert_eq!(encoded_len("가{omit_raw:8547}\n", true).unwrap(), 3);
    }

    #[test]
    fn rejects_protected_field_change() {
        let (raw, mut translation) = pair();
        translation["entries"][0]["raw_hex"] = json!("00");
        assert!(validate_catalog_pair(&raw, &translation).is_err());
    }

    #[test]
    fn rejects_missing_raw_token_or_residual_japanese() {
        let (raw, mut translation) = pair();
        translation["entries"][0]["ko"] = json!("あ\n");
        assert!(validate_catalog_pair(&raw, &translation).is_err());
    }
}
