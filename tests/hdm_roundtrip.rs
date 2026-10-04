use pc98_madou_ars::hdm::HdmFile;
use std::path::{Path, PathBuf};

#[path = "common/mod.rs"]
mod common;

const ROM_DIR: &str = "roms/Madou Monogatari A.R.S [FD]";

fn hdm_paths(dir: &str) -> Option<Vec<PathBuf>> {
    let dir_path = Path::new(dir);
    let entries = match std::fs::read_dir(dir_path) {
        Ok(entries) => entries,
        Err(err) => {
            if common::roms_required() {
                panic!(
                    "ROM required ({}=1) but no HDM directory found: {}: {}",
                    common::ROMS_REQUIRED_ENV,
                    dir_path.display(),
                    err
                );
            }
            eprintln!(
                "skip: {} absent. Set {}=1 to fail-loud.",
                dir_path.display(),
                common::ROMS_REQUIRED_ENV
            );
            return None;
        }
    };

    let mut paths = Vec::new();
    for entry in entries {
        let path = entry.expect("dir entry").path();
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("hdm"))
        {
            paths.push(path);
        }
    }
    paths.sort();
    Some(paths)
}

fn assert_hdm_roundtrip(path: &Path) {
    let original = std::fs::read(path).expect("read HDM");
    let parsed =
        HdmFile::parse(&original).unwrap_or_else(|err| panic!("parse {}: {}", path.display(), err));
    assert_eq!(parsed.geometry.cylinders, 77, "{}", path.display());
    assert_eq!(parsed.geometry.heads, 2, "{}", path.display());
    assert_eq!(parsed.geometry.sectors_per_track, 8, "{}", path.display());
    assert_eq!(parsed.geometry.sector_size, 1024, "{}", path.display());
    assert_eq!(
        parsed.serialize(),
        original,
        "round-trip failed: {}",
        path.display()
    );
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn hdm_round_trip_main_disks() {
    let Some(paths) = hdm_paths(ROM_DIR) else {
        return;
    };
    for path in &paths {
        assert_hdm_roundtrip(path);
    }
    if paths.is_empty() {
        if common::roms_required() {
            panic!(
                "ROM required ({}=1) but no HDM found under {}/",
                common::ROMS_REQUIRED_ENV,
                ROM_DIR
            );
        }
        eprintln!(
            "skip: {}/*.hdm absent. Set {}=1 to fail-loud.",
            ROM_DIR,
            common::ROMS_REQUIRED_ENV
        );
        return;
    }
    assert_eq!(paths.len(), 8, "expected 8 main FD images");
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn hdm_round_trip_alternate_dumps() {
    let dir = format!("{ROM_DIR}/alts");
    let Some(paths) = hdm_paths(&dir) else {
        return;
    };
    for path in &paths {
        assert_hdm_roundtrip(path);
    }
    if paths.is_empty() {
        if common::roms_required() {
            panic!(
                "ROM required ({}=1) but no HDM found under {}/",
                common::ROMS_REQUIRED_ENV,
                dir
            );
        }
        eprintln!(
            "skip: {}/*.hdm absent. Set {}=1 to fail-loud.",
            dir,
            common::ROMS_REQUIRED_ENV
        );
        return;
    }
    assert_eq!(paths.len(), 16, "expected 16 alternate FD images");
}
