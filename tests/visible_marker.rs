use pc98_madou_ars::fat12_replace::replace_file_in_place;
use pc98_madou_ars::hangul_probe::{
    FILE_GAOO_OVL, FILE_MAIN_COM, GAIJI_JIS_7621_SJIS, patch_gaoo_boot_prompt_hangul_probe,
    patch_main_com_register_gaiji,
};
use pc98_madou_ars::sjis_marker::{encode_sjis, replace_all_exact, replace_first_exact};

#[path = "common/mod.rs"]
mod common;

const ARLE_GAME: &str =
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Arle Game disk).hdm";

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn arle_game_overlay_title_marker_can_be_reinserted() {
    let Some(mut disk) = common::try_read(ARLE_GAME) else {
        return;
    };
    let mut overlay =
        pc98_madou_ars::read_fat12_file_from_hdm(&disk, "GAME_A.OVL").expect("read overlay");
    let original_len = overlay.len();

    let from = encode_sjis("魔導物語Ａ.Ｒ.Ｓ アルル編の").expect("encode from");
    let to = encode_sjis("魔導物語Ａ.Ｒ.Ｓ テスト編の").expect("encode to");
    assert_eq!(from.len(), to.len());

    let offset = replace_first_exact(&mut overlay, &from, &to).expect("patch marker");
    assert_eq!(offset, 0x391F);
    assert_eq!(overlay.len(), original_len);

    let report = replace_file_in_place(&mut disk, "GAME_A.OVL", &overlay).expect("replace file");
    assert_eq!(report.bytes, original_len);
    assert!(report.capacity >= original_len);

    let reloaded =
        pc98_madou_ars::read_fat12_file_from_hdm(&disk, "GAME_A.OVL").expect("read overlay");
    assert!(contains(&reloaded, &to));
    assert!(!contains(&reloaded, &from));
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn gaoo_boot_prompt_marker_can_be_reinserted() {
    let Some(mut disk) = common::try_read(ARLE_GAME) else {
        return;
    };
    let mut overlay =
        pc98_madou_ars::read_fat12_file_from_hdm(&disk, "GAOO.OVL").expect("read overlay");
    let original_len = overlay.len();

    let from = encode_sjis("好きなドライブ").expect("encode from");
    let to = encode_sjis("テストテストテ").expect("encode to");
    assert_eq!(from.len(), to.len());

    let offsets = replace_all_exact(&mut overlay, &from, &to).expect("patch marker");
    assert_eq!(offsets, vec![0x0387, 0x04C1]);
    assert_eq!(overlay.len(), original_len);

    let report = replace_file_in_place(&mut disk, "GAOO.OVL", &overlay).expect("replace file");
    assert_eq!(report.bytes, original_len);
    assert!(report.capacity >= original_len);

    let reloaded =
        pc98_madou_ars::read_fat12_file_from_hdm(&disk, "GAOO.OVL").expect("read overlay");
    assert!(contains(&reloaded, &to));
    assert!(!contains(&reloaded, &from));
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn gaoo_boot_prompt_hangul_probe_can_be_reinserted() {
    let Some(mut disk) = common::try_read(ARLE_GAME) else {
        return;
    };
    let mut overlay =
        pc98_madou_ars::read_fat12_file_from_hdm(&disk, FILE_GAOO_OVL).expect("read overlay");
    let original_len = overlay.len();

    let offsets = patch_gaoo_boot_prompt_hangul_probe(&mut overlay).expect("patch probe marker");
    assert_eq!(offsets, vec![0x0387, 0x04C1]);
    assert_eq!(overlay.len(), original_len);

    let report = replace_file_in_place(&mut disk, FILE_GAOO_OVL, &overlay).expect("replace file");
    assert_eq!(report.bytes, original_len);
    assert!(report.capacity >= original_len);

    let reloaded =
        pc98_madou_ars::read_fat12_file_from_hdm(&disk, FILE_GAOO_OVL).expect("read overlay");
    assert!(contains(&reloaded, &GAIJI_JIS_7621_SJIS));
    assert!(!contains(
        &reloaded,
        &encode_sjis("好きなドライブ").expect("encode from")
    ));
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn main_com_gaiji_registration_hook_can_be_reinserted() {
    let Some(mut disk) = common::try_read(ARLE_GAME) else {
        return;
    };
    let main_com =
        pc98_madou_ars::read_fat12_file_from_hdm(&disk, FILE_MAIN_COM).expect("read MAIN.COM");
    let original_len = main_com.len();

    let patched_main = patch_main_com_register_gaiji(&main_com).expect("patch MAIN.COM");
    assert!(patched_main.len() > original_len);
    assert_eq!(patched_main[0], 0xE9);
    assert_eq!(&patched_main[3..6], &[0x90, 0x90, 0x90]);
    assert!(contains(
        &patched_main,
        &pc98_madou_ars::hangul_probe::HANGUL_GA_GLYPH_16X16_1BPP
    ));

    let report =
        replace_file_in_place(&mut disk, FILE_MAIN_COM, &patched_main).expect("replace MAIN.COM");
    assert_eq!(report.bytes, patched_main.len());
    assert!(report.capacity >= patched_main.len());

    let reloaded =
        pc98_madou_ars::read_fat12_file_from_hdm(&disk, FILE_MAIN_COM).expect("read MAIN.COM");
    assert_eq!(reloaded, patched_main);
}
