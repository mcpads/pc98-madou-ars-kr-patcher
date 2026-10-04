use pc98_madou_ars::graphics_resource::{ScreenLayout, render_named_screen_resource};

#[path = "common/mod.rs"]
mod common;

const DEMO_DISK: &str = "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Demo disk).hdm";
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

fn short_branch_target(bytes: &[u8], logical: usize, opcode: u8) -> usize {
    let instruction = logical_slice(bytes, logical, 2);
    assert_eq!(instruction[0], opcode);
    let displacement = i8::from_le_bytes([instruction[1]]);
    (logical as isize + 2 + isize::from(displacement)) as usize
}

fn near_call_target(bytes: &[u8], logical: usize) -> usize {
    let instruction = logical_slice(bytes, logical, 3);
    assert_eq!(instruction[0], 0xE8);
    let displacement = i16::from_le_bytes([instruction[1], instruction[2]]);
    (logical as isize + 3 + isize::from(displacement)) as usize
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn exact_ishi_resources_render_as_mixed_sprite_atlases() {
    let cases = [
        (
            ARLE_GAME,
            "ISHI_A.CNS",
            6_062,
            "d0ec6db79cc469888062ec080253664ff575cff86b8b94c15b6c9037a640c61f",
            "5f8ce376e2b4dca35aa32ba9f9357845951e7ec0b9d63d5aad0aa94d2d30ebb4",
            "229b180c41bab7273ece44e76fa64376b6cf94722d05e8df194806078552587b",
        ),
        (
            RULUE_GAME,
            "ISHI_R.CNS",
            5_400,
            "5f21b7b89395e293faf1df92fbae34dceca0fffd730dc97e87cda8ef9b0fe190",
            "268df5c91e495d9a828f82ba5f75d9395c6319e9f39825f3ce36d147917af6f9",
            "f9b3c957650cff1fb16a62641ba5a7cf1a215059c982596d2c3854acc8c04a32",
        ),
        (
            SCHEZO_GAME,
            "ISHI_S.CNS",
            5_495,
            "b18c5edd6a7a024fbcecd7176d7223dcdd91181047c4588a6888f79553b7a13a",
            "2d2f61f3ba918339c0a92d31d57f4f53030281e7c658580ecd024c0d1fe3187d",
            "4245c0ca938703831715a8a6ad8730932446e312ca6e71f796955dfd1c02fa70",
        ),
    ];

    for (disk_path, name, packed_size, packed_sha256, decoded_sha256, rgb_sha256) in cases {
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
        assert_eq!(report.output.len(), 0x2C00);
        assert_eq!(
            pc98_madou_ars::media_identity::sha256_hex(&report.output),
            decoded_sha256
        );

        let rendered = render_named_screen_resource(name, &packed).unwrap();
        assert_eq!(rendered.layout, ScreenLayout::BrgiMixedSpriteAtlas);
        assert_eq!(rendered.stream_sizes, [0x2C00]);
        assert!(rendered.metadata_tail_bytes.is_empty());
        assert!(rendered.companion_stream_sizes.is_empty());
        assert!(rendered.companion_audio.is_none());
        assert_eq!(
            pc98_madou_ars::media_identity::sha256_hex(&rendered.rgb),
            rgb_sha256
        );
    }
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn game_overlays_load_ishi_and_expose_each_mixed_storage_region() {
    let cases = [
        (
            ARLE_GAME,
            "GAME_A.OVL",
            "dae27d4d4d8f4227a05cf717c629465c2e5bafb9e45770b236c18ec7713fb34e",
            0x5AF6,
            0x6A1F,
            b"0:ISHI_A.CNS\0".as_slice(),
            0xB44E,
            0x017E,
            0xB398,
            0x5530,
            0x2594,
            0x2615,
            0x38CF,
            0x1C26,
        ),
        (
            RULUE_GAME,
            "GAME_R.OVL",
            "8310393c965439eadc788664e6d7f3119ad3fe670016afee0665ea7724f9e791",
            0x5B26,
            0x6B83,
            b"2:ISHI_R.CNS\0".as_slice(),
            0xBECE,
            0x017E,
            0xBE18,
            0x5570,
            0x25A1,
            0x2622,
            0x38F5,
            0x1C56,
        ),
        (
            SCHEZO_GAME,
            "GAME_S.OVL",
            "7b5f79e7ca0b2c2b1f7b4b562d8cd411715c36456b84a26bf96f426f696f6837",
            0x5AB6,
            0x6A00,
            b"4:ISHI_S.CNS\0".as_slice(),
            0xA75E,
            0x017E,
            0xA6A8,
            0x5500,
            0x256A,
            0x25EB,
            0x388B,
            0x1C47,
        ),
    ];
    let animation_offsets = [
        0x00, 0x08, 0x10, 0x18, 0x10, 0x08, 0x00, 0x20, 0x28, 0x30, 0x28, 0x20,
    ];
    let animation_copy = [
        0x8E, 0xC0, 0xA5, 0x2B, 0xFD, 0x8E, 0xC3, 0xA5, 0x2B, 0xFD, 0x8E, 0xC1, 0xA5, 0x2B, 0xFD,
        0x8E, 0xC2, 0xA5, 0x83, 0xC7, 0x4E,
    ];
    let direct_32_prefix = [
        0xBA, 0x22, 0x00, 0xB4, 0x02, 0xCD, 0x7B, 0x32, 0xC0, 0xD0, 0xE4, 0x8B, 0xF0,
    ];
    let direct_32_geometry = [
        0xC7, 0x46, 0x00, 0x04, 0x00, 0xC7, 0x46, 0x02, 0x20, 0x00, 0x33, 0xC0, 0xCD, 0x79,
    ];
    let masked_geometry = [
        0x68, 0x00, 0xA8, 0x68, 0x00, 0xB0, 0x68, 0x00, 0xB8, 0x68, 0x00, 0xE0, 0x6A, 0x10, 0x6A,
        0x02, 0x8B, 0xEC, 0xB8, 0x80, 0x00, 0xCD, 0x79,
    ];

    for (
        disk_path,
        overlay_name,
        decoded_sha256,
        pointer_site,
        name_address,
        name,
        ishi_segment_word,
        register_site,
        animation_table,
        animation_copy_site,
        direct_32_site,
        small_selector_site,
        tile_site,
        large_site,
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
        assert_eq!(logical_word(&overlay, pointer_site), name_address);
        assert_eq!(
            logical_slice(&overlay, usize::from(name_address), name.len()),
            name
        );

        // The packed file is loaded through the table pointer and expanded into
        // the dedicated ISHI segment.
        assert_eq!(logical_slice(&overlay, 0x0204, 2), [0x8B, 0x16]);
        assert_eq!(
            logical_word(&overlay, 0x0206),
            u16::try_from(pointer_site).unwrap()
        );
        let loader = logical_slice(&overlay, 0x0204, 0x1D);
        assert!(contains(loader, &[0xB4, 0x03, 0xCD, 0x7C]));
        let load_es = [
            0x8E,
            0x06,
            ishi_segment_word as u8,
            (ishi_segment_word >> 8) as u8,
        ];
        assert!(contains(loader, &load_es));

        // The animation path registers base+0x110 and selects seven unique
        // 0x80-byte row-interleaved 16x16 frames through paragraph offsets.
        let register = logical_slice(&overlay, register_site, 0x12);
        let register_base = [
            0x2E,
            0xA1,
            ishi_segment_word as u8,
            (ishi_segment_word >> 8) as u8,
            0x05,
            0x10,
            0x01,
        ];
        assert!(contains(register, &register_base));
        assert_eq!(
            logical_slice(&overlay, animation_table, animation_offsets.len()),
            animation_offsets
        );
        let animation = logical_slice(&overlay, animation_copy_site, 0x180);
        assert!(contains(animation, &[0x2E, 0xD7, 0x32, 0xE4]));
        assert!(contains(animation, &animation_copy));

        // Four 32x32 slots start at +0x1600; AH selects a 0x200-byte slot.
        let direct_32 = logical_slice(&overlay, direct_32_site, 0x2B);
        assert!(contains(direct_32, &direct_32_prefix));
        let direct_32_base = [
            0x2E,
            0xA1,
            ishi_segment_word as u8,
            (ishi_segment_word >> 8) as u8,
            0x05,
            0x60,
            0x01,
        ];
        assert!(contains(direct_32, &direct_32_base));
        assert!(contains(direct_32, &direct_32_geometry));

        // The final three 16x16 slots use overlapping selector entries. The
        // forward/backward loops call +0/+6 for +0x1480/+0x1580, while both
        // terminal comparisons branch to +3 for +0x1500.
        assert_eq!(
            logical_slice(&overlay, small_selector_site, 9),
            [0xB1, 0x48, 0xBA, 0xB1, 0x50, 0xBA, 0xB1, 0x58, 0xB5]
        );
        assert_eq!(
            short_branch_target(&overlay, small_selector_site - 0x21, 0x7F),
            small_selector_site + 3
        );
        assert_eq!(
            near_call_target(&overlay, small_selector_site - 0x1F),
            small_selector_site
        );
        assert_eq!(
            short_branch_target(&overlay, small_selector_site - 0x0A, 0x7C),
            small_selector_site + 3
        );
        assert_eq!(
            near_call_target(&overlay, small_selector_site - 0x08),
            small_selector_site + 6
        );

        let masked = logical_slice(&overlay, small_selector_site, 0x38);
        let masked_base = [
            0x2E,
            0x03,
            0x0E,
            ishi_segment_word as u8,
            (ishi_segment_word >> 8) as u8,
        ];
        assert!(contains(masked, &[0xB1, 0x48]));
        assert!(contains(masked, &[0xB1, 0x50]));
        assert!(contains(masked, &[0xB1, 0x58]));
        assert!(contains(masked, &[0xB5, 0x01]));
        assert!(contains(masked, &masked_base));
        assert!(contains(masked, &masked_geometry));

        // The initial region is installed as a conventional tile source.
        let tile = logical_slice(&overlay, tile_site, 0x10);
        let tile_base = [
            0x2E,
            0xA1,
            ishi_segment_word as u8,
            (ishi_segment_word >> 8) as u8,
            0x89,
            0x46,
            0x10,
        ];
        assert!(contains(tile, &tile_base));

        // The final region starts at +0x1E00 and is consumed in 28 transition
        // steps with an eight-byte (64-pixel) source width.
        let large = logical_slice(&overlay, large_site, 0x70);
        let large_base = [
            0x2E,
            0xA1,
            ishi_segment_word as u8,
            (ishi_segment_word >> 8) as u8,
            0x05,
            0xE0,
            0x01,
        ];
        assert!(contains(large, &large_base));
        assert!(contains(large, &[0xB9, 0x1C, 0x00]));
        assert!(contains(large, &[0xC7, 0x46, 0x00, 0x08, 0x00]));
        assert!(contains(large, &[0x33, 0xC0, 0xCD, 0x79]));
    }
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn resident_blitters_prove_plane_major_tiles_and_row_interleaved_sprites() {
    let Some(disk) = common::try_read(DEMO_DISK) else {
        return;
    };
    let main = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "MAIN.COM").unwrap();
    let report = pc98_madou_ars::overlay_lz::decode_overlay_lz(&main[0x71..]).unwrap();
    assert_eq!(report.bytes_consumed, main.len() - 0x71);
    assert_eq!(
        pc98_madou_ars::media_identity::sha256_hex(&report.output),
        "cae16a8287e21d1c90a19bc9caff26e599542c67e68d2c14897af3e0821362ee"
    );
    let body = report.output;

    // AL=0 reaches the direct row-interleaved copier. AL=0x80 reaches the
    // masked row-interleaved copier at 0x0E3B.
    assert_eq!(
        logical_slice(&body, 0x0CE0, 32),
        [
            0xFB, 0x8A, 0xE0, 0xD0, 0xE4, 0x72, 0x0F, 0x74, 0x17, 0xD0, 0xE4, 0x73, 0x03, 0xE9,
            0x9E, 0x00, 0xD0, 0xE4, 0x73, 0x0C, 0xEB, 0x40, 0xA8, 0x10, 0x75, 0x03, 0xE9, 0x3E,
            0x01, 0xE9, 0xF6, 0x01,
        ]
    );
    assert_eq!(
        logical_slice(&body, 0x0D00, 54),
        [
            0xFC, 0x8B, 0xD7, 0x8B, 0x5E, 0x00, 0xD1, 0xEB, 0x8B, 0x46, 0x02, 0x8B, 0xFA, 0x8E,
            0x46, 0x0A, 0x8B, 0xCB, 0xF3, 0xA5, 0x8B, 0xFA, 0x8E, 0x46, 0x08, 0x8B, 0xCB, 0xF3,
            0xA5, 0x8B, 0xFA, 0x8E, 0x46, 0x06, 0x8B, 0xCB, 0xF3, 0xA5, 0x8B, 0xFA, 0x8E, 0x46,
            0x04, 0x8B, 0xCB, 0xF3, 0xA5, 0x83, 0xC2, 0x50, 0x48, 0x75, 0xD6, 0xC3,
        ]
    );
    assert_eq!(
        logical_slice(&body, 0x0E3B, 68),
        [
            0xFC, 0x8B, 0xD7, 0x8B, 0x5E, 0x00, 0x2B, 0xE3, 0xD1, 0xEB, 0x89, 0x5E, 0x00, 0x8C,
            0xD0, 0x8E, 0xC0, 0x8B, 0xFC, 0x56, 0x8B, 0x4E, 0x00, 0x8B, 0xD9, 0x4B, 0xD1, 0xE3,
            0xAD, 0x0B, 0x00, 0xAB, 0xE2, 0xFA, 0x8B, 0x4E, 0x00, 0x8B, 0xFC, 0x47, 0x47, 0x8D,
            0x70, 0x02, 0xAD, 0x26, 0x0B, 0x05, 0xAB, 0xE2, 0xF9, 0x8B, 0x4E, 0x00, 0x8B, 0xFC,
            0x47, 0x47, 0xAD, 0x26, 0x0B, 0x05, 0xF7, 0xD0, 0xAB, 0xE2, 0xF7, 0x5E,
        ]
    );
    for (logical, segment_offset) in [
        (0x0E7F, 0x0A),
        (0x0E98, 0x08),
        (0x0EB1, 0x06),
        (0x0ECA, 0x04),
    ] {
        assert_eq!(
            logical_slice(&body, logical, 3),
            [0x8E, 0x46, segment_offset]
        );
    }

    // The separate AH=2 tile path retains conventional +0x00/+0x20/+0x40/+0x60
    // plane blocks for the first ISHI region.
    assert_eq!(
        logical_slice(&body, 0x10D3, 66),
        [
            0x8B, 0xCF, 0x36, 0x8B, 0x97, 0x80, 0x00, 0x8E, 0x46, 0x08, 0xAD, 0x23, 0xC2, 0x36,
            0x0B, 0x07, 0xAB, 0x83, 0xC6, 0x1E, 0x8B, 0xF9, 0x8E, 0x46, 0x0A, 0xAD, 0x23, 0xC2,
            0x36, 0x0B, 0x47, 0x20, 0xAB, 0x83, 0xC6, 0x1E, 0x8B, 0xF9, 0x8E, 0x46, 0x0C, 0xAD,
            0x23, 0xC2, 0x36, 0x0B, 0x47, 0x40, 0xAB, 0x83, 0xC6, 0x1E, 0x8B, 0xF9, 0x8E, 0x46,
            0x0E, 0xAD, 0x23, 0xC2, 0x36, 0x0B, 0x47, 0x60, 0xAB, 0x83,
        ]
    );
}
