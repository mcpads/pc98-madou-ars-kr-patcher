use pc98_madou_ars::cutscene_catalog::{
    BIOS_GAIJI_CAPACITY, CUTSCENE_RESOURCES, CutsceneRewriteKind, ResourceLayout, catalog_resource,
    verify_entries,
};
use pc98_madou_ars::cutscene_reinsert::apply_cutscene_translations;
use pc98_madou_ars::cutscene_reinsert::repack_pointer_grid_region;
use pc98_madou_ars::cutscene_text::{
    ARLE_CD_PHASE_KEY, ARLE_ED_PHASE_KEY, ARLE_OP_PHASE_KEY, build_dat_gaiji_blob,
    install_demo_dat_gaiji_registration, install_demo_dat_gaiji_registration_from_segment,
    install_overlay_gaiji_registration, install_rulue_ending_sample_wait,
    plan_integrated_dat_gaiji_layout,
};
use pc98_madou_ars::cutscene_translation::validate_catalog_pair;
use pc98_madou_ars::font_build::{
    FontProfile, build_renderer_font, build_resource_gaiji_font, collect_renderer_demand,
};
use pc98_madou_ars::gaiji_table::GaijiTable;
use pc98_madou_ars::hangul_probe::GAIJI_PATTERN_BYTES;
use pc98_madou_ars::hook_geometry::{RendererOverlay, renderer_hook};
use pc98_madou_ars::overlay_lz::{decode_overlay_lz, encode_overlay_lz};
use std::collections::{BTreeMap, BTreeSet};

#[path = "common/mod.rs"]
mod common;

const EXTRACT_DIR: &str = "research/extracted/cutscene";
const ARLE_GAME: &str =
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Arle Game disk).hdm";

fn parse_hex_offset(text: &str) -> usize {
    usize::from_str_radix(text.trim_start_matches("0x"), 16).unwrap()
}

fn staged_catalog(file: &str) -> serde_json::Value {
    let lower = file.to_ascii_lowercase();
    let name = lower
        .strip_suffix(".dat")
        .or_else(|| lower.strip_suffix(".ovl"))
        .unwrap()
        .to_owned();
    serde_json::from_slice(
        &std::fs::read(format!(
            "assets/translations/needs_review/cutscene/{name}.json"
        ))
        .unwrap(),
    )
    .unwrap()
}

fn staged_translations(value: &serde_json::Value) -> BTreeMap<usize, String> {
    value["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| {
            !entry["flags"]
                .as_array()
                .unwrap()
                .iter()
                .any(|flag| flag == "structural")
        })
        .map(|entry| {
            (
                parse_hex_offset(entry["string_decoded_offset"].as_str().unwrap()),
                entry["ko"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

fn phase_gaiji(value: &serde_json::Value) -> GaijiTable {
    let syllables = value["entries"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|entry| entry["ko"].as_str().unwrap_or("").chars())
        .filter(|ch| ('가'..='힣').contains(ch))
        .collect::<BTreeSet<_>>();
    assert!(syllables.len() <= 188);
    let glyphs = syllables
        .into_iter()
        .enumerate()
        .map(|(index, ch)| {
            let resource = value["resource"]
                .as_str()
                .or_else(|| value["file"].as_str())
                .unwrap_or("");
            let index = index + pc98_madou_ars::cutscene_catalog::reserved_gaiji_slots(resource);
            let row = 0x76 + index / 94;
            let cell = 0x21 + index % 94;
            let (lead, trail) = if index < 94 {
                (0xEBu8, (0x9F + index) as u8)
            } else {
                let cell = (0x21 + index - 94) as u8;
                (0xEC, cell + 0x1F + u8::from(cell > 0x5F))
            };
            serde_json::json!({
                "char": ch.to_string(),
                "jis": format!("0x{row:02X}{cell:02X}"),
                "sjis": format!("{lead:02x}{trail:02x}"),
                "glyph_hex": "00".repeat(32),
            })
        })
        .collect::<Vec<_>>();
    GaijiTable::from_json_str(&serde_json::json!({ "glyphs": glyphs }).to_string()).unwrap()
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/, extracted disk files in research/extracted/, translations in assets/translations/ and fonts in assets/fonts/"]
fn dynamic_cutscene_gaiji_tables_match_every_staged_phase() {
    let profile =
        FontProfile::load(std::path::Path::new("assets/fonts/font_profile.json")).unwrap();
    for spec in CUTSCENE_RESOURCES {
        let staged = staged_catalog(spec.file);
        let expected = staged["entries"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|entry| entry["ko"].as_str().unwrap_or("").chars())
            .filter(|ch| ('가'..='힣').contains(ch))
            .collect::<BTreeSet<_>>();
        let generated = build_resource_gaiji_font(&profile, &expected, spec.file)
            .unwrap_or_else(|error| panic!("generate {} gaiji table: {error:#}", spec.file));
        let table = GaijiTable::from_json_str(&serde_json::to_string(&generated.metadata).unwrap())
            .unwrap();
        let actual = table
            .entries
            .iter()
            .map(|entry| entry.ch)
            .collect::<BTreeSet<_>>();
        assert_eq!(actual, expected, "{} dynamic gaiji demand", spec.file);
    }
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/, extracted disk files in research/extracted/, translations in assets/translations/ and fonts in assets/fonts/"]
fn staged_shezo_credits_fit_the_proven_pointer_grid_region() {
    let raw: serde_json::Value = serde_json::from_slice(
        &std::fs::read("assets/translations/raw/cutscene/shezo_ed.json").unwrap(),
    )
    .unwrap();
    let translated: serde_json::Value = serde_json::from_slice(
        &std::fs::read("assets/translations/needs_review/cutscene/shezo_ed.json").unwrap(),
    )
    .unwrap();

    let report = validate_catalog_pair(&raw, &translated).unwrap();
    let repack = report.pointer_grid_repack.unwrap();

    assert_eq!(repack.region, "credits");
    assert_eq!(repack.capacity, 1_050);
    assert!(
        repack.required <= repack.capacity,
        "reviewed credits require {} bytes but the proven grid region holds {}",
        repack.required,
        repack.capacity,
    );
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/, extracted disk files in research/extracted/, translations in assets/translations/ and fonts in assets/fonts/"]
fn real_cutscene_resources_catalog_and_roundtrip() {
    let expected = [
        ("OP_A.DAT", 11_527, 25, 25, 0, 0),
        ("CD_A.DAT", 1_577, 7, 7, 0, 0),
        ("ED_A.DAT", 22_234, 65, 60, 0, 0),
        ("OPENINGR.OVL", 20_595, 29, 29, 29, 29),
        ("TYUKAN_R.OVL", 9_218, 2, 2, 2, 2),
        ("ENDING_R.OVL", 18_018, 25, 25, 25, 25),
        ("SHEZO_OP.OVL", 22_511, 15, 15, 15, 15),
        ("SHEZO_TU.OVL", 13_600, 4, 4, 4, 4),
        ("SHEZO_ED.OVL", 23_013, 89, 89, 89, 25),
    ];

    for (
        spec,
        (
            expected_file,
            expected_decoded_size,
            expected_entries,
            expected_targets,
            expected_rewrite_sites,
            expected_windows,
        ),
    ) in CUTSCENE_RESOURCES.iter().zip(expected)
    {
        assert_eq!(spec.file, expected_file);
        let path = format!("{EXTRACT_DIR}/{}", spec.file);
        let Some(packed) = common::try_read(&path) else {
            continue;
        };
        let catalog = catalog_resource(&packed, *spec)
            .unwrap_or_else(|error| panic!("catalog {}: {error:#}", spec.file));
        let decoded = decode_overlay_lz(&packed).unwrap().output;
        verify_entries(&decoded, &catalog.entries).unwrap();

        assert!(!catalog.entries.is_empty(), "{} has no entries", spec.file);
        assert_eq!(
            catalog.entries.len(),
            expected_entries,
            "{} entry count",
            spec.file
        );
        assert_eq!(
            catalog
                .entries
                .iter()
                .filter(|entry| !entry.structural)
                .count(),
            expected_targets,
            "{} translation-target count",
            spec.file,
        );
        assert_eq!(
            catalog.decoded_size, expected_decoded_size,
            "{} decoded size",
            spec.file
        );
        assert_eq!(catalog.decoded_size, decoded.len());
        assert_eq!(
            catalog
                .entries
                .iter()
                .map(|entry| entry.rewrite_sites.len())
                .sum::<usize>(),
            expected_rewrite_sites,
            "{} rewrite-site count",
            spec.file,
        );
        assert_eq!(
            catalog
                .entries
                .iter()
                .map(|entry| entry.layout_windows.len())
                .sum::<usize>(),
            expected_windows,
            "{} layout-window count",
            spec.file,
        );
        for entry in &catalog.entries {
            let mov_dx_sites = entry
                .rewrite_sites
                .iter()
                .filter(|site| site.kind == CutsceneRewriteKind::MovDx)
                .map(|site| site.site)
                .collect::<Vec<_>>();
            let window_sites = entry
                .layout_windows
                .iter()
                .map(|window| window.rewrite_site)
                .collect::<Vec<_>>();
            assert_eq!(window_sites, mov_dx_sites, "{} window binding", spec.file);
            assert!(entry.layout_windows.iter().all(|window| {
                window.width_half_cells > 0
                    && window.width_half_cells.is_multiple_of(2)
                    && window.origin_column + window.width_half_cells <= 80
                    && window.rows > 0
                    && window.origin_row + window.rows <= 25
            }));
        }
        assert!(
            catalog
                .entries
                .iter()
                .all(|entry| { !entry.had_decode_errors || entry.text.contains("{raw:") })
        );
        if matches!(spec.layout, ResourceLayout::OverlayDialogue { .. }) {
            assert!(catalog.load_offset.is_some());
            assert!(
                catalog
                    .entries
                    .iter()
                    .filter(|entry| entry.region == "dialogue")
                    .all(|entry| !entry.rewrite_sites.is_empty()),
                "{} has an unreferenced dialogue entry",
                spec.file,
            );
        }
        if spec.file == "SHEZO_ED.OVL" {
            let credits = catalog
                .entries
                .iter()
                .filter(|entry| entry.region == "credits")
                .collect::<Vec<_>>();
            assert_eq!(credits.len(), 64);
            assert!(credits.iter().all(|entry| {
                entry.string_logical_offset == Some(entry.string_decoded_offset + 0x100)
                    && entry.rewrite_sites.len() == 1
                    && entry.rewrite_sites[0].kind == CutsceneRewriteKind::PointerGrid
            }));

            let ResourceLayout::OverlayDialogue {
                extra_nul_regions, ..
            } = spec.layout
            else {
                unreachable!();
            };
            let region = extra_nul_regions
                .iter()
                .find(|region| region.label == "credits")
                .copied()
                .unwrap();
            let mut repacked = decoded.clone();
            let report = repack_pointer_grid_region(
                &mut repacked,
                region,
                catalog.load_offset.unwrap(),
                &catalog.entries,
                &Default::default(),
            )
            .unwrap();
            assert_eq!(report.capacity, 1_050);
            assert_eq!(report.used, 1_050);
            assert_eq!(report.spare, 0);
            assert_eq!(report.moved_entries, 0);
            assert_eq!(report.rewritten_sites, 64);
            assert_eq!(repacked, decoded, "raw credit repack must round-trip");
        }

        eprintln!(
            "{}: decoded={} entries={} targets={} load={:?}",
            spec.file,
            catalog.decoded_size,
            catalog.entries.len(),
            catalog
                .entries
                .iter()
                .filter(|entry| !entry.structural)
                .count(),
            catalog.load_offset,
        );
    }
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/, extracted disk files in research/extracted/, translations in assets/translations/ and fonts in assets/fonts/"]
fn staged_cutscene_translations_fit_and_round_trip_all_real_resources() {
    let expected = [
        ("OP_A.DAT", 0, None),
        ("CD_A.DAT", 0, None),
        ("ED_A.DAT", 0, None),
        ("OPENINGR.OVL", 0, Some((0x0000, 0x43DF))),
        ("TYUKAN_R.OVL", 0, Some((0x0000, 0x17CF))),
        ("ENDING_R.OVL", 0, Some((0x0023, 0x3A2F))),
        ("SHEZO_OP.OVL", 0, Some((0x0000, 0x4B4F))),
        ("SHEZO_TU.OVL", 0, Some((0x0000, 0x280F))),
        ("SHEZO_ED.OVL", 1, Some((0x0000, 0x4CBF))),
    ];

    for (spec, (expected_file, repacks, hook_sites)) in CUTSCENE_RESOURCES.iter().zip(expected) {
        assert_eq!(spec.file, expected_file);
        let Some(packed) = common::try_read(&format!("{EXTRACT_DIR}/{}", spec.file)) else {
            continue;
        };
        let catalog = catalog_resource(&packed, *spec).unwrap();
        let mut decoded = decode_overlay_lz(&packed).unwrap().output;
        let staged = staged_catalog(spec.file);
        let translations = staged_translations(&staged);
        let gaiji = phase_gaiji(&staged);

        let report = apply_cutscene_translations(&mut decoded, &catalog, &translations, &gaiji)
            .unwrap_or_else(|error| panic!("reinsert {}: {error:#}", spec.file));

        assert_eq!(
            report.pointer_grid_repacks.len(),
            repacks,
            "{} pointer-grid repacks",
            spec.file,
        );
        let repacked_entries = report
            .pointer_grid_repacks
            .iter()
            .map(|repack| {
                assert!(
                    repack.used <= repack.capacity,
                    "{} pointer-grid region overflow",
                    spec.file,
                );
                assert_eq!(repack.spare, repack.capacity - repack.used);
                repack.placements.len()
            })
            .sum::<usize>();
        assert_eq!(
            report.in_place + report.relocated + repacked_entries,
            translations.len(),
            "{} applied every staged translation",
            spec.file,
        );
        for repack in &report.pointer_grid_repacks {
            assert_eq!(
                repack.rewritten_sites,
                repack
                    .placements
                    .iter()
                    .map(|placement| placement.rewrite_sites.len())
                    .sum::<usize>(),
                "{} rewrote every pointer-grid site",
                spec.file,
            );
        }
        if let Some((prologue, consumer)) = hook_sites {
            if spec.file == "ENDING_R.OVL" {
                install_rulue_ending_sample_wait(&mut decoded).unwrap();
            }
            let hook = install_overlay_gaiji_registration(&mut decoded, &gaiji.gaiji_glyphs())
                .unwrap_or_else(|error| panic!("install {} phase hook: {error:#}", spec.file));
            assert_eq!(hook.prologue_decoded_offset, prologue);
            assert_eq!(hook.consumer_decoded_offset, consumer);
        }
        let reencoded = encode_overlay_lz(&decoded);
        assert_eq!(
            decode_overlay_lz(&reencoded).unwrap().output,
            decoded,
            "{} translated re-encode round-trip",
            spec.file,
        );
    }
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/, extracted disk files in research/extracted/, translations in assets/translations/ and fonts in assets/fonts/"]
fn staged_arle_phase_tables_fit_one_boot_loaded_blob() {
    let selectors = [
        ("OP_A.DAT", ARLE_OP_PHASE_KEY),
        ("CD_A.DAT", ARLE_CD_PHASE_KEY),
        ("ED_A.DAT", ARLE_ED_PHASE_KEY),
    ];
    let phases = selectors
        .iter()
        .map(|(file, selector)| {
            let staged = staged_catalog(file);
            (*selector, phase_gaiji(&staged).gaiji_glyphs())
        })
        .collect::<Vec<_>>();

    let blob = build_dat_gaiji_blob(&phases).unwrap();

    assert_eq!(blob.phases.len(), 3);
    for (report, (selector, glyphs)) in blob.phases.iter().zip(&phases) {
        assert_eq!(report.selector, *selector);
        assert_eq!(report.glyphs, glyphs.len());
        assert!((1..=BIOS_GAIJI_CAPACITY).contains(&report.glyphs));
    }
    assert!(blob.bytes.len() < u16::MAX as usize);
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/, extracted disk files in research/extracted/, translations in assets/translations/ and fonts in assets/fonts/"]
fn staged_arle_fonts_compose_behind_the_real_renderer_loader() {
    let Some(game_disk) = common::try_read(ARLE_GAME) else {
        return;
    };
    let main = pc98_madou_ars::read_fat12_file_from_hdm(&game_disk, "MAIN.COM").unwrap();
    let profile =
        FontProfile::load(std::path::Path::new("assets/fonts/font_profile.json")).unwrap();
    let demand = collect_renderer_demand(
        std::path::Path::new("assets/translations/needs_review"),
        "arle",
        true,
    )
    .unwrap();
    let generated = build_renderer_font(&profile, &demand).unwrap();
    let josa_data = pc98_madou_ars::josa::build_runtime_data_from_json(
        &serde_json::to_string(&generated.metadata).unwrap(),
    )
    .unwrap();
    let sheet = generated.bytes;
    let hook = renderer_hook(RendererOverlay::Arle).unwrap();
    let hook_offset = pc98_madou_ars::hook_geometry::HOOK_ENTRY_OFF as usize;
    let mut renderer = vec![0; hook_offset + hook.len()];
    renderer[..sheet.len()].copy_from_slice(&sheet);
    let josa_offset = pc98_madou_ars::josa::RUNTIME_DATA_OFF;
    renderer[josa_offset..josa_offset + josa_data.len()].copy_from_slice(&josa_data);
    renderer[hook_offset..].copy_from_slice(&hook);

    let phases = [
        ("OP_A.DAT", ARLE_OP_PHASE_KEY),
        ("CD_A.DAT", ARLE_CD_PHASE_KEY),
        ("ED_A.DAT", ARLE_ED_PHASE_KEY),
    ]
    .iter()
    .map(|(file, selector)| {
        let staged = staged_catalog(file);
        (*selector, phase_gaiji(&staged).gaiji_glyphs())
    })
    .collect::<Vec<_>>();
    let blob = build_dat_gaiji_blob(&phases).unwrap();
    let layout = plan_integrated_dat_gaiji_layout(renderer.len(), blob.bytes.len()).unwrap();
    let old_size = u16::try_from(renderer.len()).unwrap();
    let installed =
        pc98_madou_ars::hangul_probe::patch_main_com_load_combined(&main, old_size, "KFONT.BIN")
            .unwrap();
    let retargeted = pc98_madou_ars::hangul_probe::retarget_main_com_load_combined(
        &installed,
        old_size,
        layout.load_size,
        "KFONT.BIN",
    )
    .unwrap();

    assert_eq!(renderer.len(), hook_offset + hook.len());
    assert!(layout.blob_offset >= renderer.len());
    assert_eq!(layout.blob_offset % 0x100, 0);
    assert_eq!(
        layout.blob_segment,
        pc98_madou_ars::hook_geometry::HOOK_SEG + (layout.blob_offset / 16) as u16
    );
    assert_eq!(
        usize::from(layout.load_size),
        layout.blob_offset + blob.bytes.len()
    );
    assert_eq!(
        retargeted,
        pc98_madou_ars::hangul_probe::patch_main_com_load_combined(
            &main,
            layout.load_size,
            "KFONT.BIN",
        )
        .unwrap()
    );

    let mut combined = renderer.clone();
    combined.resize(layout.blob_offset, 0);
    combined.extend_from_slice(&blob.bytes);
    assert_eq!(combined.len(), layout.load_size as usize);
    assert_eq!(&combined[..renderer.len()], &renderer);
    assert_eq!(&combined[layout.blob_offset..], &blob.bytes);

    let Some(demo_packed) = common::try_read(&format!("{EXTRACT_DIR}/DEMO.OVL")) else {
        return;
    };
    let mut demo = decode_overlay_lz(&demo_packed).unwrap().output;
    let report =
        install_demo_dat_gaiji_registration_from_segment(&mut demo, layout.blob_segment).unwrap();
    assert_eq!(report.table_segment, layout.blob_segment);
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/, extracted disk files in research/extracted/, translations in assets/translations/ and fonts in assets/fonts/"]
fn game_a_dispatches_interlude_and_ending_through_demo_phase_commands() {
    let Some(packed) = common::try_read("research/extracted/GAME_A.OVL") else {
        return;
    };
    let decoded = decode_overlay_lz(&packed).unwrap().output;

    assert_eq!(&decoded[0x6B3A..0x6B50], b"A:DEMO.OVL 0:CD_A.DAT\0",);
    assert_eq!(&decoded[0x6B50..0x6B66], b"A:DEMO.OVL 0:ED_A.DAT\0",);
    assert_eq!(
        &decoded[0x8A0C..0x8A12],
        &[0xBE, 0x3A, 0x6C, 0xE9, 0xBE, 0x9C],
    );
    assert_eq!(
        &decoded[0x8BF6..0x8BFC],
        &[0xBE, 0x50, 0x6C, 0xE9, 0xD4, 0x9A],
    );
    assert_eq!(
        u16::from_le_bytes(decoded[0x5AF6..0x5AF8].try_into().unwrap()),
        0x8ADD,
    );
    assert_eq!(
        u16::from_le_bytes(decoded[0x5C54..0x5C56].try_into().unwrap()),
        0x8B7D,
    );
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/, extracted disk files in research/extracted/, translations in assets/translations/ and fonts in assets/fonts/"]
fn real_demo_overlay_accepts_the_video_reset_dat_gaiji_hook() {
    let Some(packed) = common::try_read(&format!("{EXTRACT_DIR}/DEMO.OVL")) else {
        return;
    };
    let original = decode_overlay_lz(&packed).unwrap().output;
    let mut decoded = original.clone();

    let hook = install_demo_dat_gaiji_registration(&mut decoded).unwrap();

    assert_eq!(hook.consumer_decoded_offset, 0x023F);
    assert_eq!(hook.video_reset_decoded_offset, 0x1093);
    assert_eq!(hook.stub_decoded_offset, 5_548);
    assert_eq!(
        hook.selector_scratch_logical,
        hook.bitmap_scratch_logical + GAIJI_PATTERN_BYTES as u16
    );
    assert!(hook.stack_reserve >= 0x1000);
    assert_eq!(original[hook.consumer_decoded_offset + 11], 0xAC);
    assert_eq!(
        &original[hook.consumer_decoded_offset + 12..hook.consumer_decoded_offset + 16],
        &[0x3C, 0x81, 0x72, 0x0C],
    );
    let reencoded = encode_overlay_lz(&decoded);
    assert_eq!(decode_overlay_lz(&reencoded).unwrap().output, decoded);
}

// The ending's picture initializer must not overwrite any Korean glyph, and
// incompatible tables must fail before changing the resource.
#[test]
#[ignore = "requires original A.R.S HDMs in roms/, extracted disk files in research/extracted/, translations in assets/translations/ and fonts in assets/fonts/"]
fn arle_ending_keeps_picture_gaiji_separate_from_hangul() {
    let Some(disk) = common::try_read(ARLE_GAME) else {
        return;
    };
    let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "ED_A.DAT").unwrap();
    let spec = *CUTSCENE_RESOURCES
        .iter()
        .find(|s| s.file == "ED_A.DAT")
        .unwrap();
    let catalog = catalog_resource(&packed, spec).unwrap();
    let source = decode_overlay_lz(&packed).unwrap().output;
    let staged = staged_catalog("ED_A.DAT");
    let demand = staged_translations(&staged)
        .values()
        .flat_map(|s| s.chars())
        .filter(|c| ('가'..='힣').contains(c))
        .collect();
    let profile =
        FontProfile::load(std::path::Path::new("assets/fonts/font_profile.json")).unwrap();
    let old = pc98_madou_ars::font_build::build_gaiji_font(&profile, &demand).unwrap();
    let old = GaijiTable::from_json_str(&old.metadata.to_string()).unwrap();
    let mut target = source.clone();
    assert!(
        apply_cutscene_translations(&mut target, &catalog, &staged_translations(&staged), &old)
            .is_err()
    );
    assert_eq!(target, source);
    let generated = build_resource_gaiji_font(&profile, &demand, "ED_A.DAT").unwrap();
    let table = GaijiTable::from_json_str(&generated.metadata.to_string()).unwrap();
    for entry in &table.entries {
        assert!(entry.jis > 0x7646);
    }
    apply_cutscene_translations(&mut target, &catalog, &staged_translations(&staged), &table)
        .unwrap();
    assert_eq!(&target[0x26..0x2C], &source[0x26..0x2C]);
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/, extracted disk files in research/extracted/, translations in assets/translations/ and fonts in assets/fonts/"]
fn arle_end_card_changes_only_picture_bank_and_its_four_rows() {
    let Some(disk) = common::try_read(ARLE_GAME) else {
        return;
    };
    let source =
        decode_overlay_lz(&pc98_madou_ars::read_fat12_file_from_hdm(&disk, "ED_A.DAT").unwrap())
            .unwrap()
            .output;
    let profile =
        FontProfile::load(std::path::Path::new("assets/fonts/font_profile.json")).unwrap();
    let label: serde_json::Value = serde_json::from_slice(
        &std::fs::read("assets/translations/needs_review/graphics/arle_end_card.json").unwrap(),
    )
    .unwrap();
    let mut target = source.clone();
    assert!(
        pc98_madou_ars::arle_ending_graphics::install(&mut target, &label, &profile, false)
            .is_err()
    );
    assert_eq!(target, source);
    pc98_madou_ars::arle_ending_graphics::install(&mut target, &label, &profile, true).unwrap();
    let rows = [0x1565, 0x1594, 0x15C3, 0x15F2];
    for i in 0..source.len() {
        if !(0x504A..0x550A).contains(&i) && !rows.iter().any(|o| (*o..*o + 44).contains(&i)) {
            assert_eq!(target[i], source[i]);
        }
    }
    assert_ne!(&target[0x504A..0x550A], &source[0x504A..0x550A]);
    for row in rows {
        assert_eq!(target[row + 44], 0);
        assert_eq!(&target[row..row + 18], &[b' '; 18]);
        assert_eq!(&target[row + 26..row + 44], &[b' '; 18]);
    }
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/, extracted disk files in research/extracted/, translations in assets/translations/ and fonts in assets/fonts/"]
fn rulue_ending_waits_before_music_resume_and_preserves_initialization() {
    let Some(disk) = common::try_read(
        "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Rurue Game disk).hdm",
    ) else {
        return;
    };
    let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "ENDING_R.OVL").unwrap();
    let original = decode_overlay_lz(&packed).unwrap().output;
    let mut decoded = original.clone();
    let offset = install_rulue_ending_sample_wait(&mut decoded).unwrap();
    assert_eq!(offset, original.len());
    let entry_target = 0x103u16.wrapping_add(u16::from_le_bytes([decoded[1], decoded[2]]));
    assert_eq!(decoded[0], 0xE9);
    assert_eq!(usize::from(entry_target), offset + 0x100);
    // Wait service precedes resume; jump back preserves the existing DX setup
    // and all subsequent initialization. No CALL relies on the new stack yet.
    assert_eq!(
        &decoded[offset..offset + 9],
        &[0xB4, 8, 0xCD, 0x7D, 0xB4, 2, 0xCD, 0x7A, 0xE9]
    );
    let return_ip = (offset as u16 + 0x100 + 11).wrapping_add(u16::from_le_bytes([
        decoded[offset + 9],
        decoded[offset + 10],
    ]));
    assert_eq!(return_ip, 0x104);
    assert_eq!(
        usize::from(u16::from_le_bytes([decoded[0x28], decoded[0x29]])),
        decoded.len() + 0x100
    );
    assert_eq!(&decoded[4..0x28], &original[4..0x28]);
    assert_eq!(&decoded[0x2A..offset], &original[0x2A..]);
    assert_eq!(
        decode_overlay_lz(&encode_overlay_lz(&decoded))
            .unwrap()
            .output,
        decoded
    );
    let installed = decoded.clone();
    assert!(install_rulue_ending_sample_wait(&mut decoded).is_err());
    assert_eq!(decoded, installed);
    let mut changed = original;
    changed[2] ^= 1;
    let before = changed.clone();
    assert!(install_rulue_ending_sample_wait(&mut changed).is_err());
    assert_eq!(changed, before);
}
