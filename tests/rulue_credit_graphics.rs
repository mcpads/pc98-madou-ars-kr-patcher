use pc98_madou_ars::{
    font_build::FontProfile, overlay_lz::decode_overlay_lz, read_fat12_file_from_hdm,
    rulue_credit_graphics::build,
};
use std::path::Path;
#[path = "common/mod.rs"]
mod common;
// Translating role headings must leave every original staff-name row intact.
#[test]
#[ignore = "requires original A.R.S HDMs in roms/, graphics masters in assets/graphics_text/ and fonts in assets/fonts/"]
fn credits_change_only_declared_consumer_rows() {
    let Some(disk) = common::try_read(
        "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Rurue Data disk).hdm",
    ) else {
        return;
    };
    let source = read_fat12_file_from_hdm(&disk, "RMS.DAT").unwrap();
    let profile = FontProfile::load(Path::new("assets/fonts/font_profile.json")).unwrap();
    assert!(build(&source, Path::new("assets"), &profile, false).is_err());
    let result = build(&source, Path::new("assets"), &profile, true).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read("assets/graphics_text/rulue_credits.json").unwrap())
            .unwrap();
    let offsets = manifest["surfaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["offset"].as_u64().unwrap() as usize)
        .collect::<Vec<_>>();
    let before = decode_overlay_lz(&source).unwrap().output;
    for (i, source_byte) in before.iter().enumerate() {
        if !offsets.iter().any(|o| (*o..*o + 320).contains(&i)) {
            assert_eq!(
                result.decoded[i], *source_byte,
                "staff-name/protected byte {i:04X}"
            );
        }
    }
    for o in offsets {
        assert_ne!(&result.decoded[o..o + 320], &before[o..o + 320]);
        assert!(result.decoded[o..o + 320].iter().any(|b| *b != 0));
    }
    let restored = decode_overlay_lz(&result.packed).unwrap();
    assert_eq!(restored.output, result.decoded);
    assert_eq!(restored.bytes_consumed, result.packed.len());
    let mut wrong_source = source;
    wrong_source[8] ^= 1;
    assert!(build(&wrong_source, Path::new("assets"), &profile, true).is_err());
}
