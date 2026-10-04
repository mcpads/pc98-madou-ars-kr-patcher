#[path = "common/mod.rs"]
mod common;

const DEMO_DISK: &str = "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Demo disk).hdm";

struct CharacterCase {
    id: &'static str,
    game_disk: &'static str,
    data_disk: &'static str,
    expected_autoexec: &'static [u8],
    opening_music_file: &'static str,
    rewrites_autoexec: bool,
}

const CASES: [CharacterCase; 3] = [
    CharacterCase {
        id: "arle",
        game_disk: "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Arle Game disk).hdm",
        data_disk: "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Arle Data disk).hdm",
        expected_autoexec: b"FPLAY.COM\r\nBPLAY.COM\r\nBSAMP.COM\r\nREADJS.COM\r\nMAIN.COM /M D:GAOO.OVL A:GAME_A.OVL A:DEMO.OVL 0:OP_A\r\n",
        opening_music_file: "MADO-A2.DAT",
        rewrites_autoexec: false,
    },
    CharacterCase {
        id: "rulue",
        game_disk: "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Rurue Game disk).hdm",
        data_disk: "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Rurue Data disk).hdm",
        expected_autoexec: b"FPLAY.COM\r\nBPLAY.COM\r\nBSAMP.COM\r\nREADJS.COM\r\nMAIN.COM /M D:GAOO.OVL A:GAME_R.OVL A:OPENINGR.OVL\r\n",
        opening_music_file: "MADO-R.DAT",
        rewrites_autoexec: true,
    },
    CharacterCase {
        id: "schezo",
        game_disk: "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (She-zo Game disk).hdm",
        data_disk: "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (She-zo Data disk).hdm",
        expected_autoexec: b"FPLAY.COM\r\nBPLAY.COM\r\nBSAMP.COM\r\nREADJS.COM\r\nMAIN.COM /M D:GAOO.OVL A:GAME_S.OVL A:SHEZO_OP.OVL\r\n",
        opening_music_file: "MADO-S.DAT",
        rewrites_autoexec: true,
    },
];

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn two_disk_boot_stages_source_media_and_character_game_route() {
    let Some(demo) = common::try_read(DEMO_DISK) else {
        return;
    };
    let demo_tc =
        pc98_madou_ars::read_fat12_file_from_hdm(&demo, "TC.CNS").expect("read exact Demo TC.CNS");

    for case in CASES {
        let Some(game) = common::try_read(case.game_disk) else {
            return;
        };
        let Some(data) = common::try_read(case.data_disk) else {
            return;
        };
        let opening_music =
            pc98_madou_ars::read_fat12_file_from_hdm(&data, case.opening_music_file)
                .expect("read exact opening music data");
        let built = pc98_madou_ars::two_disk_boot::build_game_disk(&game, &demo, &data)
            .unwrap_or_else(|error| panic!("build {} two-disk Game: {error:#}", case.id));

        assert_eq!(built.character_id, case.id);
        assert!(built.tc_cns_added);
        assert_eq!(built.opening_music_file, case.opening_music_file);
        assert!(built.opening_music_added);
        assert_eq!(built.autoexec_rewritten, case.rewrites_autoexec);
        assert_eq!(
            pc98_madou_ars::read_fat12_file_from_hdm(&built.image, "TC.CNS")
                .expect("read staged TC.CNS"),
            demo_tc
        );
        assert_eq!(
            pc98_madou_ars::read_fat12_file_from_hdm(&built.image, "AUTOEXEC.BAT")
                .expect("read two-disk AUTOEXEC.BAT"),
            case.expected_autoexec
        );
        assert_eq!(
            pc98_madou_ars::read_fat12_file_from_hdm(&built.image, case.opening_music_file)
                .expect("read staged opening music data"),
            opening_music
        );

        let rebuilt = pc98_madou_ars::two_disk_boot::build_game_disk(&built.image, &demo, &data)
            .unwrap_or_else(|error| panic!("reapply {} two-disk transform: {error:#}", case.id));
        assert!(!rebuilt.tc_cns_added);
        assert!(!rebuilt.opening_music_added);
        assert!(!rebuilt.autoexec_rewritten);
        assert_eq!(rebuilt.image, built.image);
    }
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn two_disk_boot_rejects_a_different_characters_data_disk() {
    let Some(demo) = common::try_read(DEMO_DISK) else {
        return;
    };
    let Some(arle_game) = common::try_read(CASES[0].game_disk) else {
        return;
    };
    let Some(rulue_data) = common::try_read(CASES[1].data_disk) else {
        return;
    };

    let error = pc98_madou_ars::two_disk_boot::build_game_disk(&arle_game, &demo, &rulue_data)
        .expect_err("Arle convenience image must reject Rulue Data media");
    assert!(
        format!("{error:#}").contains("Arle Data disk is not the expected media"),
        "{error:#}"
    );
}
