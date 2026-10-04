use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct ApprovedTerminologyFile {
    rules: Vec<ApprovedTerminologyRule>,
}

#[derive(Debug, Deserialize)]
struct ApprovedTerminologyRule {
    source_contains: String,
    korean_contains: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ApprovedTerminologyReport {
    pub rules_checked: usize,
    pub matching_entries: usize,
}

pub fn validate_approved_terminology(
    translations_dir: &Path,
    rules_path: &Path,
) -> Result<ApprovedTerminologyReport> {
    let rules_file: ApprovedTerminologyFile = serde_json::from_slice(
        &std::fs::read(rules_path)
            .with_context(|| format!("read terminology rules {}", rules_path.display()))?,
    )
    .with_context(|| format!("parse terminology rules {}", rules_path.display()))?;

    let mut catalog_paths = Vec::new();
    collect_json_paths(translations_dir, &mut catalog_paths)?;
    catalog_paths.sort();
    let mut catalogs = Vec::with_capacity(catalog_paths.len());
    for path in catalog_paths {
        let catalog = serde_json::from_slice(
            &std::fs::read(&path)
                .with_context(|| format!("read translation catalog {}", path.display()))?,
        )
        .with_context(|| format!("parse translation catalog {}", path.display()))?;
        catalogs.push((path.display().to_string(), catalog));
    }

    validate_catalog_values(&rules_file.rules, &catalogs)
}

fn collect_json_paths(root: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    for entry in
        std::fs::read_dir(root).with_context(|| format!("read directory {}", root.display()))?
    {
        let path = entry
            .with_context(|| format!("read directory entry under {}", root.display()))?
            .path();
        if path.is_dir() {
            collect_json_paths(&path, output)?;
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("json") {
            output.push(path);
        }
    }
    Ok(())
}

fn validate_catalog_values(
    rules: &[ApprovedTerminologyRule],
    catalogs: &[(String, Value)],
) -> Result<ApprovedTerminologyReport> {
    if rules.is_empty() {
        bail!("approved terminology rule set is empty");
    }

    let mut unique_rules = HashSet::new();
    for rule in rules {
        if rule.source_contains.is_empty() || rule.korean_contains.is_empty() {
            bail!("approved terminology rules must not contain empty terms");
        }
        if !unique_rules.insert((&rule.source_contains, &rule.korean_contains)) {
            bail!(
                "duplicate approved terminology rule {:?} -> {:?}",
                rule.source_contains,
                rule.korean_contains
            );
        }
    }

    let mut hits = vec![0usize; rules.len()];
    let mut failures = Vec::new();
    for (catalog_name, catalog) in catalogs {
        let Some(entries) = catalog.get("entries").and_then(Value::as_array) else {
            continue;
        };
        for entry in entries {
            let Some(source) = entry.get("text").and_then(Value::as_str) else {
                continue;
            };
            let Some(korean) = entry.get("ko").and_then(Value::as_str) else {
                continue;
            };
            let id = entry
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("<missing id>");

            for (index, rule) in rules.iter().enumerate() {
                if !source.contains(&rule.source_contains) {
                    continue;
                }
                hits[index] += 1;
                if !korean.contains(&rule.korean_contains) {
                    failures.push(format!(
                        "{catalog_name} {id}: source term {:?} must retain Korean term {:?}, got {korean:?}",
                        rule.source_contains, rule.korean_contains
                    ));
                }
            }
        }
    }

    for (rule, hits) in rules.iter().zip(&hits) {
        if *hits == 0 {
            failures.push(format!(
                "approved source term {:?} did not occur in the selected translation tree",
                rule.source_contains
            ));
        }
    }
    if !failures.is_empty() {
        bail!(failures.join("\n"));
    }

    Ok(ApprovedTerminologyReport {
        rules_checked: rules.len(),
        matching_entries: hits.into_iter().sum(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn rules() -> Vec<ApprovedTerminologyRule> {
        vec![ApprovedTerminologyRule {
            source_contains: "大打撃".to_owned(),
            korean_contains: "대타격".to_owned(),
        }]
    }

    #[test]
    fn accepts_the_approved_term_inside_a_longer_message() {
        let catalogs = vec![(
            "battle.json".to_owned(),
            json!({
                "entries": [{
                    "id": "BATTLE_MESSAGE",
                    "text": "大打撃をくらわせた！",
                    "ko": "대타격을 먹였다！"
                }]
            }),
        )];

        let report = validate_catalog_values(&rules(), &catalogs).unwrap();
        assert_eq!(report.rules_checked, 1);
        assert_eq!(report.matching_entries, 1);
    }

    #[test]
    fn rejects_a_synonym_for_an_approved_term() {
        let catalogs = vec![(
            "battle.json".to_owned(),
            json!({
                "entries": [{
                    "id": "BATTLE_MESSAGE",
                    "text": "大打撃をくらわせた！",
                    "ko": "강타를 먹였다！"
                }]
            }),
        )];

        let error = validate_catalog_values(&rules(), &catalogs)
            .unwrap_err()
            .to_string();
        assert!(error.contains("대타격"));
        assert!(error.contains("강타"));
    }
}
