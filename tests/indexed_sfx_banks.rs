use pc98_madou_ars::{
    bsamp_resource::parse_named_indexed_bsamp_bank,
    received_damage_sfx::S0_DAMAGE_MAX_SELECTED_PAYLOAD_BYTES,
};

#[path = "common/mod.rs"]
mod common;

const ARLE_DATA: &str =
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Arle Data disk).hdm";
const RULUE_DATA: &str =
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Rurue Data disk).hdm";
const SCHEZO_DATA: &str =
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (She-zo Data disk).hdm";
const ARLE_GAME: &str =
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Arle Game disk).hdm";
const RULUE_GAME: &str =
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Rurue Game disk).hdm";
const SCHEZO_GAME: &str =
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (She-zo Game disk).hdm";
const LOGICAL_ORIGIN: usize = 0x0100;

fn logical_slice(bytes: &[u8], logical: usize, len: usize) -> &[u8] {
    &bytes[logical - LOGICAL_ORIGIN..logical - LOGICAL_ORIGIN + len]
}

fn logical_word(bytes: &[u8], logical: usize) -> u16 {
    u16::from_le_bytes(logical_slice(bytes, logical, 2).try_into().unwrap())
}

fn decode_disk_file(disk: &[u8], name: &str) -> Vec<u8> {
    let packed = pc98_madou_ars::read_fat12_file_from_hdm(disk, name).unwrap();
    let report = pc98_madou_ars::overlay_lz::decode_overlay_lz(&packed).unwrap();
    assert_eq!(report.bytes_consumed, packed.len(), "{name} packed tail");
    report.output
}

fn contains(bytes: &[u8], signature: &[u8]) -> bool {
    bytes
        .windows(signature.len())
        .any(|window| window == signature)
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn exact_character_s_resources_are_indexed_length_prefixed_sample_banks() {
    let cases = [
        (
            ARLE_DATA,
            "ARURU_S0.CNS",
            52_356,
            "38d75d1a606cb928639d330bbebde3bc8b16b39f9b0667c6cb913feae6cdb674",
            64_764,
            "0788370fd0167e418e4ea090af41f2da1a14d9cf2c5d5f6d90cba5f581caf510",
            0xA0,
            40,
            22,
            [23, 8, 6, 3],
        ),
        (
            ARLE_DATA,
            "ARURU_S1.CNS",
            43_628,
            "3c1af713ecc6140989672a28face77de1e5be70e4a67b8c1d47234528fe559d4",
            56_472,
            "59a3a5fe2b3a4268a9bc41c64044a76134a60e3739f9950a0679db86ffe37439",
            0x40,
            16,
            15,
            [16, 0, 0, 0],
        ),
        (
            ARLE_DATA,
            "ARURU_S2.CNS",
            38_178,
            "abc79caae6882df0994497ff02ddba12fc0a9d38485bd4af170a9da8acfedf0b",
            45_706,
            "71907a07a6fe53a8b2fe8ed49277e671a0e9100b578c463866051ecd4f5120c5",
            0x70,
            28,
            15,
            [21, 7, 0, 0],
        ),
        (
            RULUE_DATA,
            "RURUU_S0.CNS",
            46_662,
            "97d92a2191611bd7a61d04cf5b691ad60dfe005984189d669067cfbc44af3664",
            61_278,
            "243834173eceaae8d0ba688964271d81918778752c7ccbcfe3cb20681110f2fe",
            0x90,
            36,
            20,
            [32, 0, 4, 0],
        ),
        (
            RULUE_DATA,
            "RURUU_S1.CNS",
            37_389,
            "528b2547ccf3e3916e63a30101e14ca52d162e7aadd85728dd5902783b8790fa",
            47_371,
            "abcf9f5d2f879165360df9a995366e3ed62586e413cc9944fc643d98e8a6be8d",
            0x40,
            16,
            11,
            [16, 0, 0, 0],
        ),
        (
            RULUE_DATA,
            "RURUU_S2.CNS",
            23_033,
            "cbee1a0a1f90395a505c1984442f2fb115279617751d0850e13652a350eeeea9",
            26_817,
            "3b00314b9f5bd72fe12f3c5915034bb0bddf51d2fe121282e80894e99fdd79ed",
            0x20,
            8,
            8,
            [8, 0, 0, 0],
        ),
        (
            SCHEZO_DATA,
            "SHEZO_S0.CNS",
            54_997,
            "1c1ce232482076567f8c0b48c689c1a19a79b7cfcd1c4c24e7643db77933817c",
            64_890,
            "248527007e2a81b7513ec025653b864df2ac9cc8cea6e63f065882847869fcc5",
            0x90,
            36,
            23,
            [22, 10, 4, 0],
        ),
        (
            SCHEZO_DATA,
            "SHEZO_S1.CNS",
            51_802,
            "bdf007f7b3d1dd95d697f12d1f7a4af099f6427d5d290827640a2aca1141ed15",
            65_110,
            "f98c704bfbc623e1fbb7c14655b78d2195c2f4b7181bfa731008c95198568355",
            0x60,
            24,
            20,
            [24, 0, 0, 0],
        ),
    ];

    for (
        disk_path,
        name,
        packed_size,
        packed_sha256,
        decoded_size,
        decoded_sha256,
        table_bytes,
        entry_count,
        track_count,
        control_counts,
    ) in cases
    {
        let Some(disk) = common::try_read(disk_path) else {
            return;
        };
        let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, name).unwrap();
        assert_eq!(packed.len(), packed_size);
        assert_eq!(
            pc98_madou_ars::media_identity::sha256_hex(&packed),
            packed_sha256
        );
        let report = pc98_madou_ars::overlay_lz::decode_overlay_lz(&packed).unwrap();
        assert_eq!(report.bytes_consumed, packed.len(), "{name} packed tail");
        assert_eq!(report.output.len(), decoded_size);
        assert_eq!(
            pc98_madou_ars::media_identity::sha256_hex(&report.output),
            decoded_sha256
        );

        let bank = parse_named_indexed_bsamp_bank(name, &report.output)
            .unwrap()
            .unwrap();
        assert_eq!(bank.table_bytes, table_bytes);
        assert_eq!(bank.entries.len(), entry_count);
        assert_eq!(bank.tracks.len(), track_count);
        let actual_controls = [
            bank.entries
                .iter()
                .filter(|entry| entry.control == 0)
                .count(),
            bank.entries
                .iter()
                .filter(|entry| entry.control == 0x01F4)
                .count(),
            bank.entries
                .iter()
                .filter(|entry| entry.control == 0x8001)
                .count(),
            bank.entries
                .iter()
                .filter(|entry| entry.control == 0xFFFF)
                .count(),
        ];
        assert_eq!(actual_controls, control_counts);
        assert_eq!(actual_controls.iter().sum::<usize>(), entry_count);
        assert_eq!(bank.tracks[0].offset, table_bytes);
        assert_eq!(bank.tracks.last().unwrap().end_offset(), decoded_size);
    }
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn game_overlays_select_s_banks_and_pass_indexed_tracks_to_the_sound_wrapper() {
    let cases = [
        (
            ARLE_GAME,
            "GAME_A.OVL",
            "dae27d4d4d8f4227a05cf717c629465c2e5bafb9e45770b236c18ec7713fb34e",
            0x5AFE,
            0x6A48,
            b"1:ARURU_S0.CNS\0".as_slice(),
            [
                (0x6A57, b"1:ARURU_S1.CNS\0".as_slice()),
                (0x6A66, b"1:ARURU_S2.CNS\0".as_slice()),
            ],
            0x03AF,
            0x0441,
            0x0496,
        ),
        (
            RULUE_GAME,
            "GAME_R.OVL",
            "8310393c965439eadc788664e6d7f3119ad3fe670016afee0665ea7724f9e791",
            0x5B2E,
            0x6BCA,
            b"3:ruruu_s0.CNS\0".as_slice(),
            [
                (0x6BD9, b"3:ruruu_s1.CNS\0".as_slice()),
                (0x6BE8, b"3:ruruu_s2.CNS\0".as_slice()),
            ],
            0x03C8,
            0x0461,
            0x04B6,
        ),
        (
            SCHEZO_GAME,
            "GAME_S.OVL",
            "7b5f79e7ca0b2c2b1f7b4b562d8cd411715c36456b84a26bf96f426f696f6837",
            0x5ABE,
            0x6A29,
            b"5:SHEZO_S0.CNS\0".as_slice(),
            [
                (0x6A38, b"5:SHEZO_S1.CNS\0".as_slice()),
                (0, b"".as_slice()),
            ],
            0x03BC,
            0x044F,
            0x0497,
        ),
    ];
    let initial_resolve = [0xB4, 0x02, 0xBA, 0x1E, 0x00, 0xCD, 0x7B];
    let decode_to_a800 = [0xB8, 0x00, 0xA8, 0x8E, 0xC0, 0x2E, 0x8E, 0x1E];
    let entry_index = [
        0x8A, 0xC5, 0x8B, 0xF0, 0x83, 0xE6, 0x7F, 0xD1, 0xE6, 0xD1, 0xE6,
    ];
    let ds_pointer_to_far = [
        0x8B, 0x1C, 0x0B, 0xDB, 0x75, 0x01, 0xC3, 0x8C, 0xD8, 0x8B, 0xFB, 0xC1, 0xEF, 0x04, 0x03,
        0xC7, 0x8E, 0xC0, 0x83, 0xE3, 0x0F,
    ];
    let es_pointer_and_control = [
        0x26, 0x8B, 0x1C, 0x0B, 0xDB, 0x75, 0x01, 0xC3, 0x26, 0x8B, 0x7C, 0x02,
    ];
    let ds_sound_call = [0x8B, 0xD7, 0x80, 0xE6, 0x7F, 0xB4, 0x04, 0xCD, 0x7D];
    let es_sound_call = [0x26, 0x8B, 0x1F, 0xB4, 0x04, 0xCD, 0x7D];
    let terminal_play = [0x33, 0xD2, 0xB4, 0x04, 0xCD, 0x7D];
    let ds_link_step = [0xD1, 0xE7, 0x73, 0x13, 0xD1, 0xE7, 0x03, 0xF7];
    let es_link_step = [0xD1, 0xE7, 0x73, 0x19, 0xD1, 0xE7, 0x03, 0xF7];

    for (
        disk_path,
        overlay_name,
        decoded_sha256,
        s0_pointer_site,
        s0_name_address,
        s0_name,
        alternate_names,
        initial_load_site,
        select_site,
        consumer_site,
    ) in cases
    {
        let Some(disk) = common::try_read(disk_path) else {
            return;
        };
        let overlay = decode_disk_file(&disk, overlay_name);
        assert_eq!(
            pc98_madou_ars::media_identity::sha256_hex(&overlay),
            decoded_sha256
        );
        assert_eq!(logical_word(&overlay, s0_pointer_site), s0_name_address);
        assert_eq!(
            logical_slice(&overlay, usize::from(s0_name_address), s0_name.len()),
            s0_name
        );
        for (address, name) in alternate_names {
            if address != 0 {
                assert_eq!(logical_slice(&overlay, address, name.len()), name);
            }
        }

        let initial = logical_slice(&overlay, initial_load_site, 0x38);
        assert!(contains(initial, &initial_resolve));
        assert!(contains(initial, &decode_to_a800));
        assert!(contains(
            initial,
            &[0x33, 0xF6, 0x8B, 0xFE, 0xB4, 0x03, 0xCD, 0x7C]
        ));

        let selector = logical_slice(&overlay, select_site, 0x58);
        assert!(contains(selector, &[0xB8, 0x00, 0xA8]));
        assert!(contains(selector, &[0x05, 0x01, 0x10]));

        let consumer = logical_slice(&overlay, consumer_site, 0x90);
        assert!(contains(consumer, &entry_index));
        if overlay_name == "GAME_R.OVL" {
            // Rulue keeps the decoded bank in ES=A800 throughout this path;
            // Arle and Schezo convert DS:BX to an equivalent far pointer.
            assert!(contains(consumer, &es_pointer_and_control));
            assert!(contains(consumer, &es_sound_call));
            assert!(contains(consumer, &es_link_step));
        } else {
            assert!(contains(consumer, &ds_pointer_to_far));
            assert!(contains(consumer, &ds_sound_call));
            assert!(contains(consumer, &ds_link_step));
        }
        assert!(contains(consumer, &terminal_play));
    }
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn arle_combo_spells_select_valid_source_sfx_tracks() {
    let Some(game_disk) = common::try_read(ARLE_GAME) else {
        return;
    };
    let Some(data_disk) = common::try_read(ARLE_DATA) else {
        return;
    };

    let overlay = decode_disk_file(&game_disk, "GAME_A.OVL");
    let combo_sound_route = [
        0x26, 0x8A, 0x9F, 0x1E, 0x01, // mov bl,[es:bx+0x11e]
        0xD1, 0xE3, 0xD1, 0xE3, 0xD1, 0xE3, 0x83, 0xE3, 0x78, // spell index * 8
        0x2E, 0x8A, 0x87, 0x5D, 0x5D, // visual effect from record +5
        0xB4, 0x02, 0x2E, 0x8A, 0xAF, 0x5F, 0x5D, // sound selector from record +7
        0xB1, 0x00, // no replay count for the combo sound
        0xE8, 0x9C, 0xCE, // call the indexed S-bank consumer
    ];
    assert_eq!(logical_slice(&overlay, 0x3570, 31), combo_sound_route);

    let records = logical_slice(&overlay, 0x5D58, 16 * 8);
    let selectors = records
        .as_chunks::<8>()
        .0
        .iter()
        .map(|record| record[7])
        .collect::<Vec<_>>();
    assert_eq!(
        selectors,
        [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x4B, 0x0C, 0x0D,
            0x4E, 0xFF,
        ]
    );

    let s0_bytes = decode_disk_file(&data_disk, "ARURU_S0.CNS");
    let s2_bytes = decode_disk_file(&data_disk, "ARURU_S2.CNS");
    let s0 = parse_named_indexed_bsamp_bank("ARURU_S0.CNS", &s0_bytes)
        .unwrap()
        .unwrap();
    let s2 = parse_named_indexed_bsamp_bank("ARURU_S2.CNS", &s2_bytes)
        .unwrap()
        .unwrap();
    let expected_payload_bytes = [
        2_298, 2_814, 2_101, 4_410, 2_034, 4_502, 3_722, 2_532, 3_453, 4_285, 4_470, 2_122, 3_520,
        4_844, 2_494,
    ];

    for (tier, (&selector, &expected_bytes)) in
        selectors.iter().zip(&expected_payload_bytes).enumerate()
    {
        let (bank, index) = if selector & 0x40 != 0 {
            (&s2, usize::from(selector & 0x3F))
        } else {
            (&s0, usize::from(selector))
        };
        let entry = &bank.entries[index];
        let track_offset = entry
            .track_offset
            .unwrap_or_else(|| panic!("combo spell {tier} selects empty SFX entry {index}"));
        let track = bank
            .tracks
            .iter()
            .find(|track| track.offset == track_offset)
            .unwrap_or_else(|| panic!("combo spell {tier} has no track at 0x{track_offset:04X}"));
        assert_eq!(
            entry.control, 0x01F4,
            "combo spell {tier} must keep the source sample control"
        );
        assert_eq!(
            track.payload_bytes, expected_bytes,
            "combo spell {tier} selected a different source sample"
        );
    }

    assert_eq!(selectors[15], 0xFF, "terminal combo spell is silent");
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn rulue_and_schezo_combo_s0_tracks_fit_the_shared_ram_player() {
    let cases = [
        (
            RULUE_GAME,
            RULUE_DATA,
            "GAME_R.OVL",
            0x3569,
            [
                0x26, 0x8A, 0x9F, 0x1E, 0x01, 0xD1, 0xE3, 0xD1, 0xE3, 0xD1, 0xE3, 0x83, 0xE3, 0x78,
                0x2E, 0x8A, 0x87, 0xF1, 0x5D, 0xB4, 0x02, 0x2E, 0x8A, 0xAF, 0xF3, 0x5D, 0xB1, 0x00,
                0xE8, 0xBC, 0xCE,
            ],
            0x5DEC,
            [
                0xFF, 0xFF, 0x42, 0x43, 0x44, 0x09, 0x40, 0x40, 0x09, 0x45, 0x41, 0x46, 0x09, 0xFF,
                0xFF, 0xFF,
            ],
            "RURUU_S0.CNS",
            &[(9usize, 2_453usize, 0u16)][..],
            &[][..],
        ),
        (
            SCHEZO_GAME,
            SCHEZO_DATA,
            "GAME_S.OVL",
            0x3510,
            [
                0x26, 0x8A, 0x9F, 0x1E, 0x01, 0xD1, 0xE3, 0xD1, 0xE3, 0xD1, 0xE3, 0x83, 0xE3, 0x78,
                0x2E, 0x8A, 0x87, 0xB5, 0x5C, 0xB4, 0x02, 0x2E, 0x8A, 0xAF, 0xB7, 0x5C, 0xB1, 0x00,
                0xE8, 0x0A, 0xCF,
            ],
            0x5CB0,
            [
                0xFF, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D,
                0x0E, 0xFF,
            ],
            "SHEZO_S0.CNS",
            &[
                (2, 2_171, 0x01F4),
                (3, 3_758, 0x01F4),
                (4, 3_158, 0x01F4),
                (5, 3_323, 0x01F4),
                (7, 2_664, 0x01F4),
                (8, 2_377, 0x01F4),
                (9, 3_896, 0x01F4),
                (10, 2_970, 0x01F4),
                (11, 2_928, 0x01F4),
                (12, 2_780, 0x01F4),
            ][..],
            &[1usize, 6, 13, 14][..],
        ),
    ];

    for (
        game_path,
        data_path,
        overlay_name,
        route_logical,
        route_bytes,
        records_logical,
        expected_selectors,
        s0_name,
        expected_tracks,
        expected_empty,
    ) in cases
    {
        let Some(game_disk) = common::try_read(game_path) else {
            return;
        };
        let Some(data_disk) = common::try_read(data_path) else {
            return;
        };
        let overlay = decode_disk_file(&game_disk, overlay_name);
        assert_eq!(logical_slice(&overlay, route_logical, 31), route_bytes);
        let selectors = logical_slice(&overlay, records_logical, 16 * 8)
            .as_chunks::<8>()
            .0
            .iter()
            .map(|record| record[7])
            .collect::<Vec<_>>();
        assert_eq!(selectors, expected_selectors);

        let s0_bytes = decode_disk_file(&data_disk, s0_name);
        let s0 = parse_named_indexed_bsamp_bank(s0_name, &s0_bytes)
            .unwrap()
            .unwrap();
        for &(index, expected_payload_bytes, expected_control) in expected_tracks {
            assert!(
                selectors.contains(&(index as u8)),
                "{overlay_name} combo records do not select {s0_name}[{index}]"
            );
            let entry = &s0.entries[index];
            assert_eq!(entry.control, expected_control);
            let track_offset = entry.track_offset.expect("selected S0 track offset");
            let track = s0
                .tracks
                .iter()
                .find(|track| track.offset == track_offset)
                .expect("selected S0 track");
            assert_eq!(track.payload_bytes, expected_payload_bytes);
            assert!(
                track.payload_bytes <= usize::from(S0_DAMAGE_MAX_SELECTED_PAYLOAD_BYTES),
                "{overlay_name} {s0_name}[{index}] exceeds the shared RAM player bound"
            );
        }
        for &index in expected_empty {
            assert!(
                selectors.contains(&(index as u8)),
                "{overlay_name} combo records do not select empty {s0_name}[{index}]"
            );
            assert_eq!(s0.entries[index].track_offset, None);
        }
    }
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn received_damage_reactions_use_bounded_ram_playback_in_all_scenarios() {
    use pc98_madou_ars::{
        hook_geometry::RendererOverlay, received_damage_sfx::install_s0_damage_route,
    };
    let cases = [
        (
            ARLE_GAME,
            ARLE_DATA,
            "GAME_A.OVL",
            "ARURU_S0.CNS",
            RendererOverlay::Arle,
            0x104Cusize,
            0x1084usize,
            0x358Cusize,
            [0x12u8, 0x11, 0x10],
        ),
        (
            RULUE_GAME,
            RULUE_DATA,
            "GAME_R.OVL",
            "RURUU_S0.CNS",
            RendererOverlay::Rulue,
            0x108E,
            0x10C6,
            0x3585,
            [7, 6, 5],
        ),
        (
            SCHEZO_GAME,
            SCHEZO_DATA,
            "GAME_S.OVL",
            "SHEZO_S0.CNS",
            RendererOverlay::Schezo,
            0x106D,
            0x10A5,
            0x352C,
            [0x12, 0x11, 0x10],
        ),
    ];
    for (game, data, overlay_name, bank_name, profile, call, table, combo_call, selected) in cases {
        let Some(game_disk) = common::try_read(game) else {
            return;
        };
        let Some(data_disk) = common::try_read(data) else {
            return;
        };
        let original = decode_disk_file(&game_disk, overlay_name);
        // The ratio-selected reaction pointer, not the spell table, supplies CH.
        assert_eq!(
            logical_slice(&original, call - 5, 5),
            &[0x8A, 0x2C, 0x46, 0xB1, 0x01]
        );
        assert_eq!(logical_word(&original, call - 7), table as u16);
        let bank_bytes = decode_disk_file(&data_disk, bank_name);
        let bank = parse_named_indexed_bsamp_bank(bank_name, &bank_bytes)
            .unwrap()
            .unwrap();
        for tier in 0..16 {
            let pointer = logical_word(&original, table + tier * 2) as usize;
            let selector = logical_slice(&original, pointer, 1)[0];
            let expected = match tier {
                0..=3 => 0xFF,
                4..=7 => selected[0],
                8..=10 => selected[1],
                _ => selected[2],
            };
            assert_eq!(selector, expected, "{overlay_name} reaction {tier}");
            if selector == 0xFF {
                continue;
            }
            let entry = &bank.entries[usize::from(selector)];
            let expected_payload = match (bank_name, selector) {
                ("ARURU_S0.CNS", 16) => 3153,
                ("ARURU_S0.CNS", 17) => 4027,
                ("ARURU_S0.CNS", 18) => 4220,
                ("RURUU_S0.CNS", 5) => 3893,
                ("RURUU_S0.CNS", 6) => 5565,
                ("RURUU_S0.CNS", 7) => 5884,
                ("SHEZO_S0.CNS", 16) => 1714,
                ("SHEZO_S0.CNS", 17) => 4180,
                ("SHEZO_S0.CNS", 18) => 3612,
                _ => panic!("unproven received-damage selector"),
            };
            assert_eq!(
                entry.control, 0,
                "damage must use a terminal, unlinked sample"
            );
            let offset = entry
                .track_offset
                .expect("audible reaction must have a sample");
            let track = bank.tracks.iter().find(|t| t.offset == offset).unwrap();
            assert_eq!(track.payload_bytes, expected_payload);
            assert!(
                track.payload_bytes > 0
                    && track.payload_bytes <= usize::from(S0_DAMAGE_MAX_SELECTED_PAYLOAD_BYTES),
                "{bank_name}[{selector}] {} exceeds RAM capacity",
                track.payload_bytes
            );
        }
        let mut patched = original.clone();
        let patch = install_s0_damage_route(&mut patched, profile, 0x100, 0xBFC0, 0xDFC0).unwrap();
        for site in [call, combo_call] {
            let bytes = logical_slice(&patched, site, 3);
            assert_eq!(bytes[0], 0xE8);
            let target =
                (site as u16 + 3).wrapping_add_signed(i16::from_le_bytes([bytes[1], bytes[2]]));
            assert_eq!(
                target, patch.stub_logical_offset,
                "{overlay_name} call at {site:04X}"
            );
        }
        assert_eq!(logical_slice(&patched, call - 2, 2), &[0xB1, 1]);
        // Every existing byte except the two call displacements is preserved.
        for i in 0..original.len() {
            if [call, combo_call]
                .iter()
                .any(|site| (site - 0x100 + 1..site - 0x100 + 3).contains(&i))
            {
                continue;
            }
            assert_eq!(patched[i], original[i], "{overlay_name} byte {i:04X}");
        }
    }
}
