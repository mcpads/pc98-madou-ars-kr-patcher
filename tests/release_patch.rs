use pc98_madou_ars::media_identity::validate_build_inputs;
use pc98_madou_ars::release_patch::{
    apply_release_patch, create_release_patch, identify_original_game_disk,
};

#[path = "common/mod.rs"]
mod common;

const DEMO_DISK: &str = "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Demo disk).hdm";

const BUILD_MEDIA: [(&str, &str, &str); 3] = [
    (
        "arle_game",
        "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Arle Game disk).hdm",
        "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Arle Data disk).hdm",
    ),
    (
        "rulue_game",
        "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Rurue Game disk).hdm",
        "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Rurue Data disk).hdm",
    ),
    (
        "schezo_game",
        "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (She-zo Game disk).hdm",
        "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (She-zo Data disk).hdm",
    ),
];

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn primary_game_disks_build_and_apply_release_bps() {
    let Some(demo) = common::try_read(DEMO_DISK) else {
        return;
    };
    for (expected_id, game_path, data_path) in BUILD_MEDIA {
        let Some(source) = common::try_read(game_path) else {
            return;
        };
        let Some(data) = common::try_read(data_path) else {
            return;
        };
        let identity = identify_original_game_disk(&source).expect("identify primary Game HDM");
        assert_eq!(identity.id, expected_id);
        let character =
            validate_build_inputs(&source, &demo, &data).expect("validate exact build media");
        assert_eq!(character.game.id, expected_id);

        let mut target = source.clone();
        target[0x1000] ^= 0x5a;
        target[0x80000] ^= 0xa5;
        let created = create_release_patch(&source, &target).expect("create release BPS");
        assert_eq!(created.source.id, expected_id);
        assert!(created.patch.len() < source.len());

        let applied = apply_release_patch(&source, &created.patch).expect("apply release BPS");
        assert_eq!(applied.target, target);
        assert_eq!(applied.target_sha256, created.target_sha256);
    }
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn character_media_rejects_a_valid_but_mismatched_data_disk() {
    let Some(schezo_game) = common::try_read(BUILD_MEDIA[2].1) else {
        return;
    };
    let Some(demo) = common::try_read(DEMO_DISK) else {
        return;
    };
    let Some(rulue_data) = common::try_read(BUILD_MEDIA[1].2) else {
        return;
    };
    let error = validate_build_inputs(&schezo_game, &demo, &rulue_data)
        .unwrap_err()
        .to_string();
    assert!(error.contains("validate schezo Data disk"), "{error}");
}
