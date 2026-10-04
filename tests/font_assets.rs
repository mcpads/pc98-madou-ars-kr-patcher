use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use pc98_madou_ars::font_build::{
    FontProfile, build_renderer_font, collect_renderer_demand, generate_full_build_fonts,
};
use pc98_madou_ars::josa::{
    CLASS_TABLE_LEN, RUNTIME_DATA_LEN, RUNTIME_MAGIC, build_runtime_data_from_json,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "pc98-madou-ars-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]

#[ignore = "requires translations in assets/translations/ and fonts in assets/fonts/"]
fn production_font_assets_are_derived_from_each_translation_scope() {
    let translations = Path::new("assets/translations/needs_review");
    let profile_path = Path::new("assets/fonts/font_profile.json");
    let profile = FontProfile::load(profile_path).unwrap();
    let expected = [
        ("arle", ["cd_a", "ed_a", "op_a"]),
        ("rulue", ["ending_r", "openingr", "tyukan_r"]),
        ("schezo", ["shezo_ed", "shezo_op", "shezo_tu"]),
    ];

    for (character, cutscene_phases) in expected {
        let demand = collect_renderer_demand(translations, character, true).unwrap();
        assert!(!demand.is_empty(), "{character} renderer demand");
        assert!(
            demand.len() <= 940,
            "{character} renderer demand exceeds the ten-row sheet"
        );

        let first = build_renderer_font(&profile, &demand).unwrap();
        let second = build_renderer_font(&profile, &demand).unwrap();
        assert_eq!(first.bytes, second.bytes, "{character} raster determinism");
        assert_eq!(
            first.metadata, second.metadata,
            "{character} metadata determinism"
        );
        assert_eq!(
            first.metadata["josa"]["non_hangul_fallback"], "no_final",
            "{character} runtime particle fallback",
        );
        let runtime_josa =
            build_runtime_data_from_json(&serde_json::to_string(&first.metadata).unwrap()).unwrap();
        assert_eq!(runtime_josa.len(), RUNTIME_DATA_LEN);
        assert_eq!(&runtime_josa[..4], RUNTIME_MAGIC);
        assert_eq!(CLASS_TABLE_LEN, 940);
        let temp = TempDir::new(character);
        let bundle =
            generate_full_build_fonts(profile_path, translations, character, true, temp.path())
                .unwrap();
        assert_eq!(bundle.renderer_glyphs, demand.len());
        let expected_phases = cutscene_phases.into_iter().collect::<BTreeSet<_>>();
        let actual_phases = bundle
            .cutscene_glyphs
            .iter()
            .map(|(phase, _)| phase.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            actual_phases, expected_phases,
            "{character} cutscene phases"
        );
        for (phase, count) in &bundle.cutscene_glyphs {
            assert!(*count > 0, "{character} {phase} has no Hangul demand");
            assert!(
                *count <= 188,
                "{character} {phase} exceeds the BIOS gaiji capacity"
            );
        }
        assert_eq!(std::fs::read(bundle.renderer_bin).unwrap().len(), 940 * 32);
    }
}
