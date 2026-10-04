#[path = "common/mod.rs"]
mod common;

const ARLE_GAME: &str =
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Arle Game disk).hdm";
const DEMO: &str = "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Demo disk).hdm";
const ARLE_DATA: &str =
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Arle Data disk).hdm";

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn root_file_add_can_merge_boot_smoke_dependencies() {
    let Some(mut game_disk) = common::try_read(ARLE_GAME) else {
        return;
    };
    let Some(demo_disk) = common::try_read(DEMO) else {
        return;
    };
    let Some(data_disk) = common::try_read(ARLE_DATA) else {
        return;
    };

    add_from_source(&mut game_disk, &demo_disk, "TC.CNS");
    add_from_source(&mut game_disk, &data_disk, "MADO-A2.DAT");

    assert_eq!(
        pc98_madou_ars::read_fat12_file_from_hdm(&game_disk, "TC.CNS").expect("read TC.CNS"),
        pc98_madou_ars::read_fat12_file_from_hdm(&demo_disk, "TC.CNS").expect("source TC.CNS")
    );
    assert_eq!(
        pc98_madou_ars::read_fat12_file_from_hdm(&game_disk, "MADO-A2.DAT")
            .expect("read MADO-A2.DAT"),
        pc98_madou_ars::read_fat12_file_from_hdm(&data_disk, "MADO-A2.DAT")
            .expect("source MADO-A2.DAT")
    );
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn root_file_add_rejects_duplicate_file() {
    let Some(mut game_disk) = common::try_read(ARLE_GAME) else {
        return;
    };
    let Some(demo_disk) = common::try_read(DEMO) else {
        return;
    };
    let tc = pc98_madou_ars::read_fat12_file_from_hdm(&demo_disk, "TC.CNS").expect("source TC.CNS");
    let meta = pc98_madou_ars::fat12_add::root_file_meta(&demo_disk, "TC.CNS").expect("meta");

    pc98_madou_ars::fat12_add::add_root_file(&mut game_disk, "TC.CNS", &tc, meta)
        .expect("add once");
    let err =
        pc98_madou_ars::fat12_add::add_root_file(&mut game_disk, "TC.CNS", &tc, meta).unwrap_err();
    assert!(err.to_string().contains("already exists"));
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn root_file_ensure_reuses_only_an_identical_dependency() {
    let Some(mut game_disk) = common::try_read(ARLE_GAME) else {
        return;
    };
    let Some(demo_disk) = common::try_read(DEMO) else {
        return;
    };
    let tc = pc98_madou_ars::read_fat12_file_from_hdm(&demo_disk, "TC.CNS").expect("source TC.CNS");
    let meta = pc98_madou_ars::fat12_add::root_file_meta(&demo_disk, "TC.CNS").expect("meta");

    assert!(!pc98_madou_ars::fat12_add::root_file_exists(&game_disk, "TC.CNS").unwrap());

    let first = pc98_madou_ars::fat12_add::ensure_root_file(&mut game_disk, "TC.CNS", &tc, meta)
        .expect("add dependency");
    assert!(matches!(
        first,
        pc98_madou_ars::fat12_add::EnsureReport::Added(_)
    ));
    assert!(pc98_madou_ars::fat12_add::root_file_exists(&game_disk, "TC.CNS").unwrap());

    let after_add = game_disk.clone();
    let second = pc98_madou_ars::fat12_add::ensure_root_file(&mut game_disk, "TC.CNS", &tc, meta)
        .expect("reuse dependency");
    assert_eq!(
        second,
        pc98_madou_ars::fat12_add::EnsureReport::Reused {
            file_name: "TC.CNS".to_string(),
            bytes: tc.len(),
        }
    );
    assert_eq!(game_disk, after_add, "reuse must not mutate the disk");

    let mut changed = tc.clone();
    changed[0] ^= 0xFF;
    let err = pc98_madou_ars::fat12_add::ensure_root_file(&mut game_disk, "TC.CNS", &changed, meta)
        .unwrap_err();
    assert!(err.to_string().contains("differs from staged source"));
    assert_eq!(game_disk, after_add, "mismatch must not mutate the disk");
}

fn add_from_source(target: &mut [u8], source: &[u8], file_name: &str) {
    let data = pc98_madou_ars::read_fat12_file_from_hdm(source, file_name).expect("source file");
    let meta = pc98_madou_ars::fat12_add::root_file_meta(source, file_name).expect("source meta");
    pc98_madou_ars::fat12_add::add_root_file(target, file_name, &data, meta).expect("add file");
}
