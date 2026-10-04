use std::path::Path;

use pc98_madou_ars::{
    arle_opening_graphics::build_arle_opening_exclamation,
    demo_menu::build_demo_menu_text,
    demo_selector::{
        HANDOFF_CALL_DECODED_OFFSET, SELECTOR_OVERLAY, SOURCE_DECODED_BYTES,
        install_selector_renderer_load,
    },
    demo_title::build_demo_title,
    font_build::FontProfile,
    graphics_resource::{DEMO_LINEAR_PLANE_BYTES, PLANE_BYTES},
    overlay_lz::{decode_overlay_lz, encode_overlay_lz},
    scenario_title::{SCENARIO_TITLE_HEIGHT, SCENARIO_TITLE_PLANE_BYTES, build_scenario_title},
};

#[path = "common/mod.rs"]
mod common;

const DEMO_DISK: &str = "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Demo disk).hdm";
const ARLE_DATA_DISK: &str =
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Arle Data disk).hdm";

#[test]

#[ignore = "requires original A.R.S HDMs in roms/, graphics masters in assets/graphics_text/ and fonts in assets/fonts/"]
fn exact_arle_opening_exclamation_build_owns_only_its_map_and_tiles() {
    let Some(mut disk) = common::try_read(ARLE_DATA_DISK) else {
        return;
    };
    let source = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "OPA_PT1.CNS").unwrap();
    let source_decoded = decode_overlay_lz(&source).unwrap().output;
    let build = build_arle_opening_exclamation(&source, Path::new("assets")).unwrap();

    println!(
        "Arle opening: {} changed pixels, {} unique tiles, allocation {:#x}..={:#x}, {} packed bytes",
        build.changed_pixels,
        build.unique_tiles,
        build.allocated_tile_start,
        build.allocated_tile_end,
        build.packed.len(),
    );
    assert_eq!(build.changed_pixels, 13_349);
    assert_eq!(build.unique_tiles, 74);
    assert_eq!(build.allocated_tile_start, 0x223);
    assert_eq!(build.allocated_tile_end, 0x26c);
    assert_eq!(build.packed.len(), 55_615);
    assert_eq!(build.preview_rgb.len(), 288 * 192 * 3);

    // Maps 0..18 and every tile they can consume remain byte-identical. Map 19
    // starts at 0x0794 and its own data overlaps only unused tile zero before
    // the independently allocated tile band begins at index 0x223.
    assert_eq!(build.decoded[..0x0794], source_decoded[..0x0794]);
    let protected_tile_end = 0x08d0 + 0x223 * 128;
    assert_eq!(
        build.decoded[0x0946..protected_tile_end],
        source_decoded[0x0946..protected_tile_end]
    );

    pc98_madou_ars::fat12_add::replace_file_grow(&mut disk, "OPA_PT1.CNS", &build.packed).unwrap();
    let readback = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "OPA_PT1.CNS").unwrap();
    assert_eq!(readback, build.packed);
    assert_eq!(decode_overlay_lz(&readback).unwrap().output, build.decoded);
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/, graphics masters in assets/graphics_text/ and fonts in assets/fonts/"]
fn exact_demo_graphics_build_preserves_english_copyright_and_audio() {
    let Some(mut disk) = common::try_read(DEMO_DISK) else {
        return;
    };
    let source_disk = disk.clone();
    let assets = Path::new("assets");

    let source_title = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "OP20.CNS").unwrap();
    let source_title_decoded = decode_overlay_lz(&source_title).unwrap().output;
    let title = build_demo_title(&source_title, assets).unwrap();
    assert_eq!(title.changed_pixels, 14_119);
    assert_eq!(title.protected_pixels, 7_680);
    assert_eq!(title.source_background_pixels_preserved, 33_539);
    assert_eq!(title.source_logo_pixels_replaced_with_background, 1_295);
    assert_eq!(title.packed.len(), 22_098);
    for plane in 0..4 {
        let start = plane * DEMO_LINEAR_PLANE_BYTES + 176 * 40;
        let end = plane * DEMO_LINEAR_PLANE_BYTES + 200 * 40;
        assert_eq!(title.decoded[start..end], source_title_decoded[start..end]);
    }

    let source_menu = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "MU.CNS").unwrap();
    let source_menu_report = decode_overlay_lz(&source_menu).unwrap();
    let source_companion = &source_menu[source_menu_report.bytes_consumed..];
    let profile =
        FontProfile::load(Path::new("assets/fonts/maplestory_bold_menu_profile.json")).unwrap();
    let menu = build_demo_menu_text(&source_menu, assets, &profile).unwrap();
    assert_eq!(menu.changed_pixels, 2_314);
    assert_eq!(menu.protected_pixels, 251_008);
    assert_eq!(menu.surfaces, 1);
    assert_eq!(menu.packed.len(), 35_032);
    assert_eq!(menu.companion_packed, source_companion);

    // The three source-name bands cover ARLE, RURUE, and SHE-ZO. They are
    // byte-aligned in the AH=5 column-major sheet and must remain exact.
    assert_column_major_rect_exact(
        &source_menu_report.output,
        &menu.primary_decoded,
        0,
        342,
        336,
        26,
    );

    pc98_madou_ars::fat12_add::replace_file_grow(&mut disk, "OP20.CNS", &title.packed).unwrap();
    pc98_madou_ars::fat12_add::replace_file_grow(&mut disk, "MU.CNS", &menu.packed).unwrap();
    let source_selector =
        pc98_madou_ars::read_fat12_file_from_hdm(&disk, SELECTOR_OVERLAY).unwrap();
    let source_selector_report = decode_overlay_lz(&source_selector).unwrap();
    assert_eq!(source_selector_report.bytes_consumed, source_selector.len());
    assert_eq!(source_selector_report.output.len(), SOURCE_DECODED_BYTES);
    let source_selector_decoded = source_selector_report.output;
    let mut selector_decoded = source_selector_decoded.clone();
    let selector = install_selector_renderer_load(&mut selector_decoded).unwrap();
    assert_eq!(
        &selector_decoded[..HANDOFF_CALL_DECODED_OFFSET],
        &source_selector_decoded[..HANDOFF_CALL_DECODED_OFFSET]
    );
    assert_eq!(
        &selector_decoded[HANDOFF_CALL_DECODED_OFFSET + 3..SOURCE_DECODED_BYTES],
        &source_selector_decoded[HANDOFF_CALL_DECODED_OFFSET + 3..]
    );
    assert_eq!(selector.stub_decoded_offset, SOURCE_DECODED_BYTES);
    let selector_packed = encode_overlay_lz(&selector_decoded);
    assert_eq!(
        decode_overlay_lz(&selector_packed).unwrap().output,
        selector_decoded
    );
    pc98_madou_ars::fat12_add::replace_file_grow(&mut disk, SELECTOR_OVERLAY, &selector_packed)
        .unwrap();
    assert_eq!(
        pc98_madou_ars::read_fat12_file_from_hdm(&disk, "OP20.CNS").unwrap(),
        title.packed
    );
    assert_eq!(
        pc98_madou_ars::read_fat12_file_from_hdm(&disk, "MU.CNS").unwrap(),
        menu.packed
    );
    assert_eq!(
        pc98_madou_ars::read_fat12_file_from_hdm(&disk, SELECTOR_OVERLAY).unwrap(),
        selector_packed
    );

    let created = pc98_madou_ars::release_patch::create_release_patch(&source_disk, &disk).unwrap();
    assert_eq!(created.source.id, "demo");
    let applied =
        pc98_madou_ars::release_patch::apply_release_patch(&source_disk, &created.patch).unwrap();
    assert_eq!(applied.target, disk);
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/, graphics masters in assets/graphics_text/ and fonts in assets/fonts/"]
fn exact_scenario_title_build_preserves_background_texture_and_copyright() {
    let Some(disk) = common::try_read(ARLE_DATA_DISK) else {
        return;
    };
    let source_packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "ARS_RX.CS").unwrap();
    let source_decoded = decode_overlay_lz(&source_packed).unwrap().output;
    let title = build_scenario_title(&source_packed, Path::new("assets")).unwrap();

    // These counts belong to the pinned original resource and reviewed master.
    // They lock the reported regression: background-to-background pixels retain
    // the original PC-98 texture, while erased source-logo pixels stay erased.
    assert_eq!(title.source_background_pixels_preserved, 144_448);
    assert_eq!(title.source_logo_pixels_replaced_with_background, 5_054);
    assert_eq!(title.changed_pixels, 71_610);
    assert_eq!(title.protected_pixels, 20_480);

    for plane in 0..4 {
        for column in 0..80 {
            let start = plane * SCENARIO_TITLE_PLANE_BYTES + column * SCENARIO_TITLE_HEIGHT + 368;
            let end = start + 32;
            assert_eq!(
                title.decoded[start..end],
                source_decoded[start..end],
                "copyright band drifted in plane {plane}, column {column}"
            );
        }
    }
}

fn assert_column_major_rect_exact(
    source: &[u8],
    target: &[u8],
    x: usize,
    y: usize,
    width: usize,
    height: usize,
) {
    assert_eq!(x % 8, 0);
    assert_eq!(width % 8, 0);
    for plane in 0..4 {
        for column in x / 8..(x + width) / 8 {
            let start = plane * PLANE_BYTES + column * 400 + y;
            let end = start + height;
            assert_eq!(target[start..end], source[start..end]);
        }
    }
}
