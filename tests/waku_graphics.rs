use pc98_madou_ars::graphics_resource::{ScreenLayout, render_named_screen_resource};

#[path = "common/mod.rs"]
mod common;

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

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn exact_waku_resources_render_through_the_single_stream_ah5_layout() {
    let cases = [
        (
            ARLE_GAME,
            "WAKU_A.CS",
            "bffb91b26a8b3060da136b809484708aa07c610c8a860c06787fe5db266340b6",
            "f5bc8fdc3fd4ef96b34837f57bb43e8dfad370562fe422d0b2fc491a6f9d1ca2",
            "462b9b93160c93200498f7941d1f9143d9d41ac18f3e9b392bb34f66d8a3429d",
        ),
        (
            RULUE_GAME,
            "WAKU_R.CNS",
            "3f9a3722644b9d67f5561623c3c6c41e595b5e3461fd7f118dfba5f5e42d31fb",
            "abcb4a0de0d39814f8712a5a88bb2a3cda0fc3075bf42f268a7d1d76349e00ec",
            "51ce2f8eb66da7607cea5ebf42e832505642aec457be2f33e17f7e2b8653f9fc",
        ),
        (
            SCHEZO_GAME,
            "WAKU_S.CS",
            "5d2fa1ac77248f67f16673c0a9578e33fb47fd882ee5977552bdf421eef9fc6e",
            "981f2717fc6e7aa2174cc6300eafa4b8b183c90871c3ad5a5c7b08735fdc2bff",
            "c92006b2c0bc8af20da491d39cb27c00a2a1b96b3245d0ab4f92bc832bd9c171",
        ),
    ];

    for (disk_path, name, packed_sha256, decoded_sha256, rgb_sha256) in cases {
        let Some(disk) = common::try_read(disk_path) else {
            return;
        };
        let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, name).unwrap();
        assert_eq!(
            pc98_madou_ars::media_identity::sha256_hex(&packed),
            packed_sha256
        );
        let decoded = pc98_madou_ars::overlay_lz::decode_overlay_lz(&packed).unwrap();
        assert_eq!(decoded.bytes_consumed, packed.len(), "{name} packed tail");
        assert_eq!(decoded.output.len(), 128_000);
        assert_eq!(
            pc98_madou_ars::media_identity::sha256_hex(&decoded.output),
            decoded_sha256
        );

        let rendered = render_named_screen_resource(name, &packed).unwrap();
        assert_eq!(rendered.layout, ScreenLayout::ColumnMajorBrgiPlanes);
        assert_eq!(rendered.stream_sizes, [128_000]);
        assert_eq!(rendered.metadata_tail_bytes, [0, 0, 0, 0]);
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
fn game_overlays_route_each_waku_resource_directly_through_ah5() {
    let cases = [
        (
            ARLE_GAME,
            "GAME_A.OVL",
            "dae27d4d4d8f4227a05cf717c629465c2e5bafb9e45770b236c18ec7713fb34e",
            0x5AF4,
            0x6A13,
            b"0:WAKU_A.CS\0".as_slice(),
            [
                0x8B, 0x16, 0xF4, 0x5A, 0x8B, 0x3E, 0x48, 0xB4, 0xE8, 0xEE, 0x46, 0x8E, 0x1E, 0x48,
                0xB4,
            ],
        ),
        (
            RULUE_GAME,
            "GAME_R.OVL",
            "8310393c965439eadc788664e6d7f3119ad3fe670016afee0665ea7724f9e791",
            0x5B24,
            0x6B76,
            b"2:WAKU_R.CNS\0".as_slice(),
            [
                0x8B, 0x16, 0x24, 0x5B, 0x8B, 0x3E, 0xC8, 0xBE, 0xE8, 0x2E, 0x47, 0x8E, 0x1E, 0xC8,
                0xBE,
            ],
        ),
        (
            SCHEZO_GAME,
            "GAME_S.OVL",
            "7b5f79e7ca0b2c2b1f7b4b562d8cd411715c36456b84a26bf96f426f696f6837",
            0x5AB4,
            0x69F4,
            b"4:WAKU_S.CS\0".as_slice(),
            [
                0x8B, 0x16, 0xB4, 0x5A, 0x8B, 0x3E, 0x58, 0xA7, 0xE8, 0xBE, 0x46, 0x8E, 0x1E, 0x58,
                0xA7,
            ],
        ),
    ];
    let fixed_ah5_suffix = [
        0x33, 0xF6, 0xB9, 0x00, 0xA8, 0x8E, 0xC1, 0xBB, 0x00, 0xB0, 0xB9, 0x00, 0xB8, 0xBA, 0x00,
        0xE0, 0xB4, 0x05, 0xCD, 0x7C,
    ];

    for (disk_path, overlay_name, decoded_sha256, pointer_site, name_address, name, prefix) in cases
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
        assert_eq!(logical_slice(&overlay, 0x0227, prefix.len()), prefix);
        assert_eq!(
            logical_slice(&overlay, 0x0227 + prefix.len(), fixed_ah5_suffix.len()),
            fixed_ah5_suffix
        );
    }
}
