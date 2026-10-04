use std::path::{Path, PathBuf};

use pc98_madou_ars::{
    dat_catalog::validate_dat_translation_catalog, overlay_lz::decode_overlay_lz,
    read_fat12_file_from_hdm, translation_binding::resolve_bound_translations,
};

#[path = "common/mod.rs"]
mod common;

const ARLE_GAME: &str =
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Arle Game disk).hdm";
const RULUE_GAME: &str =
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Rurue Game disk).hdm";
const SCHEZO_GAME: &str =
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (She-zo Game disk).hdm";

#[test]

#[ignore = "requires original A.R.S HDMs in roms/ and translations in assets/translations/"]
fn all_translated_dat_catalogs_cover_complete_referenced_slots() {
    let Some(arle) = common::try_read(ARLE_GAME) else {
        return;
    };
    let Some(rulue) = common::try_read(RULUE_GAME) else {
        return;
    };
    let Some(schezo) = common::try_read(SCHEZO_GAME) else {
        return;
    };

    let mut paths = json_files(Path::new("assets/translations/needs_review/dat"));
    paths.sort();
    assert!(
        !paths.is_empty(),
        "DAT translation population must be present"
    );
    let mut failures = Vec::new();
    for path in paths {
        let catalog: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let catalog =
            resolve_bound_translations(&catalog, Path::new("assets/translations/needs_review"))
                .unwrap_or_else(|error| panic!("{}: {error:#}", path.display()));
        let catalog_entries = catalog
            .get("entries")
            .and_then(serde_json::Value::as_array)
            .unwrap();
        let rejected_false_text = [
            "ENEMY02R_GT_0F6A",
            "ENEMY03R_1E2A",
            "ENEMY04R_01AE",
            "ENEMY16R_2EAB",
            "ENEMY203_0405",
            "ENEMY213_168D",
        ];
        assert!(
            catalog_entries.iter().all(|entry| {
                !entry
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|id| rejected_false_text.contains(&id))
            }),
            "{}: accidental Shift-JIS decode in non-text data returned",
            path.display()
        );
        let character = catalog.get("char").and_then(serde_json::Value::as_str);
        let disk = match character {
            Some("A") => &arle,
            Some("R") => &rulue,
            Some("S") => &schezo,
            other => panic!("{}: unsupported char {other:?}", path.display()),
        };
        let dat = catalog
            .get("dat")
            .and_then(serde_json::Value::as_str)
            .unwrap();
        let packed = read_fat12_file_from_hdm(disk, dat).unwrap();
        let decoded = decode_overlay_lz(&packed).unwrap().output;
        let report = match validate_dat_translation_catalog(&decoded, &catalog) {
            Ok(report) => report,
            Err(error) => {
                failures.push(format!("{}: {error:#}", path.display()));
                continue;
            }
        };
        assert_eq!(
            report.entries,
            catalog_entries.len(),
            "{}: every declared entry must be validated",
            path.display()
        );
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn json_files(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("json"))
        .collect()
}
