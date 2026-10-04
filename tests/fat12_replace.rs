use pc98_madou_ars::fat12_replace::replace_file_in_place;

#[path = "common/mod.rs"]
mod common;

const ARLE_GAME: &str =
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Arle Game disk).hdm";

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn replace_shorter_root_file_updates_size_and_contents() {
    let Some(mut disk) = common::try_read(ARLE_GAME) else {
        return;
    };
    let replacement = b"@echo off\r\n";

    let report = replace_file_in_place(&mut disk, "MADO_A.BAT", replacement).expect("replace");

    assert_eq!(report.file_name, "MADO_A.BAT");
    assert_eq!(report.bytes, replacement.len());
    assert_eq!(report.capacity, 1024);
    assert_eq!(report.clusters, 1);
    assert_eq!(
        pc98_madou_ars::read_fat12_file_from_hdm(&disk, "MADO_A.BAT").expect("read file"),
        replacement
    );
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn replacement_larger_than_existing_cluster_chain_is_rejected() {
    let Some(mut disk) = common::try_read(ARLE_GAME) else {
        return;
    };
    let oversized = vec![b'X'; 1025];

    let err = replace_file_in_place(&mut disk, "MADO_A.BAT", &oversized).unwrap_err();

    assert!(
        err.to_string().contains("only has 1024 allocated bytes"),
        "{err}"
    );
}
