//! Resolve a physical resource catalog against one canonical translation catalog.
//!
//! Some A.R.S resources duplicate text that is already translated in another
//! executable. The physical catalog keeps the resource-local offsets and raw
//! bytes, while `translation_source_id` points at the one editable translation
//! entry. Resolution fails closed if the two protected source representations
//! drift.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::Value;

const SOURCE_CATALOG_FIELD: &str = "translation_source_catalog";
const SOURCE_OVERLAY_FIELD: &str = "translation_source_overlay";
const SOURCE_ID_FIELD: &str = "translation_source_id";

pub fn has_bound_translations(catalog: &Value) -> bool {
    catalog.get(SOURCE_CATALOG_FIELD).is_some()
        || catalog.get(SOURCE_OVERLAY_FIELD).is_some()
        || catalog
            .get("entries")
            .and_then(Value::as_array)
            .is_some_and(|entries| {
                entries
                    .iter()
                    .any(|entry| entry.get(SOURCE_ID_FIELD).is_some())
            })
}

pub fn resolve_bound_translations(catalog: &Value, translation_root: &Path) -> Result<Value> {
    if !has_bound_translations(catalog) {
        return Ok(catalog.clone());
    }

    let source_name = required_string(catalog, SOURCE_CATALOG_FIELD, "bound catalog")?;
    if Path::new(source_name)
        .file_name()
        .and_then(|name| name.to_str())
        != Some(source_name)
    {
        bail!(
            "bound catalog {SOURCE_CATALOG_FIELD} must be a file name in the translation root, got {source_name:?}"
        );
    }
    let source_path = translation_root.join(source_name);
    let source: Value = serde_json::from_slice(
        &std::fs::read(&source_path)
            .with_context(|| format!("read translation source {}", source_path.display()))?,
    )
    .with_context(|| format!("parse translation source {}", source_path.display()))?;
    resolve_from_catalog(catalog, &source)
        .with_context(|| format!("bind translations from {}", source_path.display()))
}

fn resolve_from_catalog(catalog: &Value, source: &Value) -> Result<Value> {
    let expected_overlay = required_string(catalog, SOURCE_OVERLAY_FIELD, "bound catalog")?;
    let actual_overlay = required_string(source, "overlay", "translation source catalog")?;
    if actual_overlay != expected_overlay {
        bail!(
            "translation source overlay {actual_overlay:?} does not match bound catalog {expected_overlay:?}"
        );
    }

    let source_entries = source
        .get("entries")
        .and_then(Value::as_array)
        .context("translation source catalog is missing entries")?;
    let mut source_by_id = HashMap::new();
    for entry in source_entries {
        let id = required_string(entry, "id", "translation source entry")?;
        if source_by_id.insert(id, entry).is_some() {
            bail!("translation source catalog has duplicate id {id:?}");
        }
    }

    let mut resolved = catalog.clone();
    let entries = resolved
        .get_mut("entries")
        .and_then(Value::as_array_mut)
        .context("bound catalog is missing entries")?;
    let mut seen_physical_ids = HashSet::new();
    for entry in entries {
        let physical_id = required_string(entry, "id", "bound entry")?.to_owned();
        if !seen_physical_ids.insert(physical_id.clone()) {
            bail!("bound catalog has duplicate id {physical_id:?}");
        }
        for field in ["ko", "status", "notes"] {
            if entry.get(field).is_some() {
                bail!(
                    "{physical_id}: bound entry must not duplicate editable field {field:?}; edit the canonical source entry"
                );
            }
        }

        let source_id = required_string(entry, SOURCE_ID_FIELD, &physical_id)?.to_owned();
        let source_entry = source_by_id.get(source_id.as_str()).with_context(|| {
            format!("{physical_id}: unknown translation source id {source_id:?}")
        })?;
        for field in ["raw_hex", "text", "byte_budget"] {
            let physical_value = entry
                .get(field)
                .with_context(|| format!("{physical_id}: missing protected field {field:?}"))?;
            let source_value = source_entry.get(field).with_context(|| {
                format!("{physical_id}: source {source_id} is missing protected field {field:?}")
            })?;
            if physical_value != source_value {
                bail!(
                    "{physical_id}: protected field {field:?} differs from translation source {source_id}"
                );
            }
        }

        let object = entry
            .as_object_mut()
            .with_context(|| format!("{physical_id}: entry must be an object"))?;
        for field in ["ko", "status"] {
            let value = source_entry.get(field).with_context(|| {
                format!("{physical_id}: source {source_id} is missing editable field {field:?}")
            })?;
            object.insert(field.to_owned(), value.clone());
        }
        if let Some(notes) = source_entry.get("notes") {
            object.insert("notes".to_owned(), notes.clone());
        }
    }
    Ok(resolved)
}

fn required_string<'a>(value: &'a Value, field: &str, label: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .with_context(|| format!("{label}: missing string field {field:?}"))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::resolve_from_catalog;

    fn source_catalog() -> Value {
        json!({
            "overlay": "GAME_R.OVL",
            "entries": [{
                "id": "GAME_R_MESSAGE",
                "byte_budget": 5,
                "raw_hex": "82a082a2",
                "text": "あい",
                "ko": "가나",
                "status": "needs_review",
                "notes": "canonical wording"
            }]
        })
    }

    fn bound_catalog() -> Value {
        json!({
            "translation_source_catalog": "game_r.json",
            "translation_source_overlay": "GAME_R.OVL",
            "entries": [{
                "id": "EVENT_MESSAGE",
                "translation_source_id": "GAME_R_MESSAGE",
                "byte_budget": 5,
                "raw_hex": "82a082a2",
                "text": "あい"
            }]
        })
    }

    #[test]
    fn resolves_editable_fields_from_the_canonical_entry() {
        let resolved = resolve_from_catalog(&bound_catalog(), &source_catalog()).unwrap();
        let entry = &resolved["entries"][0];
        assert_eq!(entry["ko"], "가나");
        assert_eq!(entry["status"], "needs_review");
        assert_eq!(entry["notes"], "canonical wording");
        assert_eq!(entry["id"], "EVENT_MESSAGE");
    }

    #[test]
    fn rejects_a_protected_source_mismatch() {
        let mut bound = bound_catalog();
        bound["entries"][0]["raw_hex"] = json!("82a082a4");
        let error = resolve_from_catalog(&bound, &source_catalog())
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("protected field \"raw_hex\" differs"),
            "{error}"
        );
    }

    #[test]
    fn rejects_a_second_editable_copy() {
        let mut bound = bound_catalog();
        bound["entries"][0]["ko"] = json!("duplicate");
        let error = resolve_from_catalog(&bound, &source_catalog())
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("must not duplicate editable field \"ko\""),
            "{error}"
        );
    }
}
