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
const TILE_BYTES: usize = 0x80;
const TILE_BANK_BYTES: usize = 0x8000;

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
fn exact_mg_resources_render_as_consumer_proven_tile_atlases() {
    let cases = [
        (
            ARLE_GAME,
            "MG_A1.CNS",
            "cbfba8e0b864bfeba0e811c5ee50a315ecb9a8026c6aaf7ad55aa8ae27eef49c",
            105_456,
            "8985be045a64a13b92fe10d29470d8bda3890407c4f2ac4cb01d077055040b95",
            0x09F0,
            804,
            "90188caa3ee20a91716e3ce45973c3aac0797fdf215306fe038df163e9f6d7a2",
        ),
        (
            ARLE_GAME,
            "MG_A2.CNS",
            "d872a1b066a64bf6a81ebc8b46e71d61692db47493f95069f8a152940645f413",
            85_904,
            "78ce208cb104b3cecf8211c50d69f5d454fa919599c2edab43ca881c085e99ce",
            0x0A90,
            650,
            "2385703ac55750832d6e514070e54f44e78453bd5e3330004a36b6216ee681e3",
        ),
        (
            RULUE_GAME,
            "MG_R1.CNS",
            "d4aea11f18540e851005b9e72873ad82751649fbd5803a9fc7e88c1ea186fef5",
            114_576,
            "ab3bf3f1f08260415c1fb066cef4cd50de63fd37b3b0a3538645942e56fbb8ef",
            0x0E10,
            867,
            "84e5806d80e0dcd05dad08d694487348624f6bc717a20d1bffa801013ecb06fa",
        ),
        (
            SCHEZO_GAME,
            "MG_S1.CNS",
            "f41f1e009939f155816ed0d8739263ea1b676c0834d24b9b4bca814cb6e623f8",
            114_256,
            "151999a2a2e5dd35fc31eac9bcbf60e31fa9eda68a6609056a923c422a3cc447",
            0x1250,
            856,
            "4a328695cf7e0b033abd5d4f14e63dd9b5a17f433afef396e85ab958d11afa32",
        ),
    ];

    for (
        disk_path,
        name,
        packed_sha256,
        decoded_size,
        decoded_sha256,
        tile_base,
        tile_count,
        rgb_sha256,
    ) in cases
    {
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
        assert_eq!(decoded.output.len(), decoded_size);
        assert_eq!(
            pc98_madou_ars::media_identity::sha256_hex(&decoded.output),
            decoded_sha256
        );
        assert_eq!(
            usize::from(u16::from_le_bytes(decoded.output[0..2].try_into().unwrap())),
            tile_base
        );
        let tile_region_bytes = decoded_size - tile_base;
        let actual_tile_count = tile_region_bytes / TILE_BANK_BYTES * 256
            + tile_region_bytes % TILE_BANK_BYTES / TILE_BYTES;
        assert_eq!(actual_tile_count, tile_count);

        let rendered = render_named_screen_resource(name, &packed).unwrap();
        assert_eq!(rendered.layout, ScreenLayout::BrgiTileAtlas);
        assert_eq!(rendered.stream_sizes, [decoded_size]);
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
fn game_overlays_build_the_mg_tile_bank_table_from_resource_word_zero() {
    let cases = [
        (
            ARLE_GAME,
            "GAME_A.OVL",
            "dae27d4d4d8f4227a05cf717c629465c2e5bafb9e45770b236c18ec7713fb34e",
            0x0308,
            0x5AF8,
            0x6A75,
            b"0:MG_A1.CNS\0".as_slice(),
            [
                0x8B, 0x3E, 0x48, 0xB4, 0xE8, 0x04, 0x46, 0x1E, 0x8E, 0x06, 0x4A, 0xB4, 0x8E, 0x1E,
                0x48, 0xB4,
            ],
            [0x1F, 0xA1, 0x4A, 0xB4, 0x8E, 0xC0],
            [
                0xA3, 0x12, 0xB4, 0x80, 0xC4, 0x08, 0xA3, 0x14, 0xB4, 0x80, 0xC4, 0x08, 0xA3, 0x16,
                0xB4, 0x80, 0xC4, 0x08, 0xA3, 0x18, 0xB4,
            ],
        ),
        (
            RULUE_GAME,
            "GAME_R.OVL",
            "8310393c965439eadc788664e6d7f3119ad3fe670016afee0665ea7724f9e791",
            0x0321,
            0x5B28,
            0x6BF7,
            b"2:MG_R1.CNS\0".as_slice(),
            [
                0x8B, 0x3E, 0xC8, 0xBE, 0xE8, 0x2B, 0x46, 0x1E, 0x8E, 0x06, 0xCA, 0xBE, 0x8E, 0x1E,
                0xC8, 0xBE,
            ],
            [0x1F, 0xA1, 0xCA, 0xBE, 0x8E, 0xC0],
            [
                0xA3, 0x92, 0xBE, 0x80, 0xC4, 0x08, 0xA3, 0x94, 0xBE, 0x80, 0xC4, 0x08, 0xA3, 0x96,
                0xBE, 0x80, 0xC4, 0x08, 0xA3, 0x98, 0xBE,
            ],
        ),
        (
            SCHEZO_GAME,
            "GAME_S.OVL",
            "7b5f79e7ca0b2c2b1f7b4b562d8cd411715c36456b84a26bf96f426f696f6837",
            0x0314,
            0x5AB8,
            0x6A47,
            b"4:MG_S1.CNS\0".as_slice(),
            [
                0x8B, 0x3E, 0x58, 0xA7, 0xE8, 0xC8, 0x45, 0x1E, 0x8E, 0x06, 0x5A, 0xA7, 0x8E, 0x1E,
                0x58, 0xA7,
            ],
            [0x1F, 0xA1, 0x5A, 0xA7, 0x8E, 0xC0],
            [
                0xA3, 0x22, 0xA7, 0x80, 0xC4, 0x08, 0xA3, 0x24, 0xA7, 0x80, 0xC4, 0x08, 0xA3, 0x26,
                0xA7, 0x80, 0xC4, 0x08, 0xA3, 0x28, 0xA7,
            ],
        ),
    ];
    let load_prefix = [
        0x8C, 0xC8, 0x8E, 0xD8, 0xB4, 0x02, 0xBA, 0x18, 0x00, 0xCD, 0x7B, 0x8B, 0xD0,
    ];
    let decode = [0x33, 0xF6, 0x8B, 0xFE, 0xB4, 0x03, 0xCD, 0x7C];
    let read_base = [0x26, 0x8B, 0x16, 0x00, 0x00, 0xC1, 0xEA, 0x04, 0x03, 0xC2];

    for (
        disk_path,
        overlay_name,
        decoded_sha256,
        load_site,
        pointer_site,
        name_address,
        name,
        variant,
        base_variant,
        segment_store,
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
        assert_eq!(
            logical_slice(&overlay, load_site, load_prefix.len()),
            load_prefix
        );
        assert_eq!(
            logical_slice(&overlay, load_site + load_prefix.len(), variant.len()),
            variant
        );
        let decode_site = load_site + load_prefix.len() + variant.len();
        assert_eq!(logical_slice(&overlay, decode_site, decode.len()), decode);
        let base_site = decode_site + decode.len();
        assert_eq!(
            logical_slice(&overlay, base_site, base_variant.len()),
            base_variant
        );
        let read_site = base_site + base_variant.len();
        assert_eq!(
            logical_slice(&overlay, read_site, read_base.len()),
            read_base
        );
        let store_site = read_site + read_base.len();
        assert_eq!(
            logical_slice(&overlay, store_site, segment_store.len()),
            segment_store
        );
    }
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn resident_tile_consumer_uses_16_by_16_brgi_blocks_and_0x8000_banks() {
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

    assert_eq!(
        logical_slice(&body, 0x10BA, 18),
        [
            0x86, 0xC4, 0x8B, 0xF0, 0x83, 0xE6, 0x07, 0xD1, 0xE6, 0x8E, 0x5A, 0x10, 0x32, 0xC0,
            0xD1, 0xE8, 0x8B, 0xF0,
        ]
    );
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

    // GAME descriptors place A800/B000/B800/E000 at +8/+A/+C/+E. Each
    // LODSW plus ADD SI,0x1E advances one 0x20-byte 16x16 plane, and the
    // unrolled row body advances its VRAM destination by 0x50 bytes.
    assert!(
        logical_slice(&body, 0x10D3, 0x160)
            .windows(6)
            .any(|window| window == [0x83, 0xC1, 0x50, 0x8B, 0xF9, 0x43])
    );
}
