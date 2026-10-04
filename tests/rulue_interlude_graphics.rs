use std::path::Path;

use pc98_madou_ars::{
    font_build::FontProfile,
    graphics_resource::render_named_screen_resource,
    overlay_lz::decode_overlay_lz,
    rulue_interlude_graphics::{HEIGHT, WIDTH, build_rulue_interlude_choices},
};

#[path = "common/mod.rs"]
mod common;

const RULUE_DATA: &str =
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Rurue Data disk).hdm";
const IMAGE_BYTES: usize = 0x6C00;

fn mutable_button_pixel(x: usize, y: usize) -> bool {
    (40..104).contains(&x) && (160..176).contains(&y)
        || (184..248).contains(&x) && (160..176).contains(&y)
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/, graphics masters in assets/graphics_text/ and fonts in assets/fonts/"]
fn exact_rtu1_build_changes_only_button_interiors_and_preserves_its_tail() {
    let Some(mut disk) = common::try_read(RULUE_DATA) else {
        return;
    };
    let source = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "RTU1.DAT").unwrap();
    let profile = FontProfile::load(Path::new("assets/fonts/font_profile.json")).unwrap();
    let build = build_rulue_interlude_choices(&source, Path::new("assets"), &profile, true)
        .expect("compile exact RTU1 choice graphic");

    assert_eq!(build.surfaces, 2);
    assert!(build.changed_pixels > 0);
    assert_eq!(build.protected_pixels, WIDTH * HEIGHT - 2 * 64 * 16);

    let source_decoded = decode_overlay_lz(&source).unwrap();
    assert_eq!(source_decoded.bytes_consumed, source.len());
    assert_eq!(
        build.decoded[IMAGE_BYTES..],
        source_decoded.output[IMAGE_BYTES..]
    );

    let source_render = render_named_screen_resource("RTU1.DAT", &source).unwrap();
    let target_render = render_named_screen_resource("RTU1.DAT", &build.packed).unwrap();
    for y in 0..HEIGHT {
        let preview_row = &build.preview_rgb[y * WIDTH * 3..(y + 1) * WIDTH * 3];
        let atlas_row = &target_render.rgb[y * 640 * 3..y * 640 * 3 + WIDTH * 3];
        assert_eq!(preview_row, atlas_row, "RTU1 preview row {y}");
    }
    let mut visible_changes = 0usize;
    for y in 0..400 {
        for x in 0..640 {
            let start = (y * 640 + x) * 3;
            let changed =
                source_render.rgb[start..start + 3] != target_render.rgb[start..start + 3];
            if changed {
                assert!(
                    x < WIDTH && y < HEIGHT && mutable_button_pixel(x, y),
                    "RTU1 render changed protected screen pixel ({x}, {y})"
                );
                visible_changes += 1;
            }
        }
    }
    assert_eq!(visible_changes, build.changed_pixels);

    let replacement =
        pc98_madou_ars::fat12_add::replace_file_grow(&mut disk, "RTU1.DAT", &build.packed).unwrap();
    assert!(replacement.clusters > 0);
    assert_eq!(
        pc98_madou_ars::read_fat12_file_from_hdm(&disk, "RTU1.DAT").unwrap(),
        build.packed
    );
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/, graphics masters in assets/graphics_text/ and fonts in assets/fonts/"]
fn needs_review_rtu1_wording_is_rejected_without_the_draft_gate() {
    let Some(disk) = common::try_read(RULUE_DATA) else {
        return;
    };
    let source = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "RTU1.DAT").unwrap();
    let profile = FontProfile::load(Path::new("assets/fonts/font_profile.json")).unwrap();
    let error =
        build_rulue_interlude_choices(&source, Path::new("assets"), &profile, false).unwrap_err();
    assert!(error.to_string().contains("explicitly permit needs_review"));
}
