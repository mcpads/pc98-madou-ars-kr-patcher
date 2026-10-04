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
fn exact_character_resources_render_as_consumer_addressed_tile_atlases() {
    let cases = [
        (
            ARLE_GAME,
            "ARURU_F.CNS",
            17_838,
            "d46eb7b8ce373c68e3c6a6a0a28026ab603fa6cc2d2cc145951388122301e1d2",
            31_872,
            "34938ef8dcdedb945e81d4832ccdb0d4c423d4d3113c949c0f11797403beef39",
            0x0180,
            246,
            "944bd93c91a24f79a2cadd735d59056d61a45bc7d60a8be0fc51063e8b817494",
        ),
        (
            ARLE_GAME,
            "ARURU_B.CNS",
            23_208,
            "22de7148c87e2fb2026dfb9382693dc81050bc4118c81ccb02b8926a9efa01f9",
            36_176,
            "e0100d689051104b760397726e51ece259cfbc837c246305bd3d062bbd67a64e",
            0x03D0,
            275,
            "74e9b972a35c17938b755557cb0a265a28359dd5fbc9636c279132a6fba94764",
        ),
        (
            RULUE_GAME,
            "RURUU_F.CNS",
            22_485,
            "b392d13cb37ce383960c2365b35e6c72f61d1e6ae63b0e797a3713328a4863b4",
            30_416,
            "ce14b57568865d6e8ea41e6d41600c7d6f333058f4a158c8866631d6e47e0631",
            0x0150,
            235,
            "6ba2e9887d3614abe6d336367bb2063dd280a5e39b51e737f7ed96d4fadee223",
        ),
        (
            RULUE_GAME,
            "RURUU_B.CNS",
            24_153,
            "5514ec6ab6306cd2ba7c86b3bb93fc1596c6479512514b76b5eba31ea9e244c5",
            33_680,
            "2e39119440c86a3b12fdb751e21ce1a55352c3f549e7dc0fa2bc072ed3efc7e9",
            0x0410,
            255,
            "c50406f702cfdc05debca3a28f80b56f5b50136905c686f643148f18589a24a0",
        ),
        (
            RULUE_GAME,
            "RURUU_FC.CNS",
            21_617,
            "42148e2cc0e7d2ba7b5276c241ee9d91ddd1f239c0d8dad441bc5d2e82a79cfd",
            29_776,
            "80dffb97b6859b4a5a9bbbd668cd1213e2bebad122a2686b4fa130f96b07fafa",
            0x0150,
            230,
            "08dfd5ceff68b20d66a5b736801c60fbcee90cdfed003a46d893487568276289",
        ),
        (
            RULUE_GAME,
            "RURUU_BC.CNS",
            21_214,
            "38d3aaaead9f1f2d1c905eedcabf872d7ff853217b0bd8a2b7663fbbe02d4eff",
            29_264,
            "92e295ee74456731e73fe8da92386d7de1d6857ec4f93c20aa577b8ce4ba8648",
            0x03D0,
            221,
            "0f0fa4b3e59506ea96a1b35af3d4fad2c86637a69faa2dca3759e4912ef8b9fa",
        ),
        (
            SCHEZO_GAME,
            "SHEZO_F.CNS",
            18_736,
            "99ae9cdc0470ef13d07b86e3bcc72fbfaa250021a3ed87ad8da8f8880b13d952",
            30_816,
            "d48f187e6a4330b96b0d68bea4da85d7ded86a2c43e80013a20c20d9cc1b58d1",
            0x0160,
            238,
            "ef1334ac1b14db84587a51c67dc76514d94f4e8906309c4c2777923599c37177",
        ),
        (
            SCHEZO_GAME,
            "SHEZO_B.CNS",
            23_977,
            "1933fd169dba8a141cf708826f0654fcafc665993c7b10674a71e4107bb827fc",
            44_000,
            "4a511506ff5ff0b06a268ca811457308dbfc0a1cde6e6e1284e6f71182093af4",
            0x04E0,
            334,
            "921f01b3b89b4c76d98a6f431ea2a59c3ac8dc2ce6ccbf2afc44b4e34a85af35",
        ),
    ];

    for (
        disk_path,
        name,
        packed_size,
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
        assert_eq!(
            usize::from(u16::from_le_bytes(report.output[0..2].try_into().unwrap())),
            tile_base
        );
        assert_eq!((decoded_size - tile_base) / TILE_BYTES, tile_count);
        assert_eq!((decoded_size - tile_base) % TILE_BYTES, 0);

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
fn game_overlays_cache_expand_and_compose_character_tile_banks() {
    let cases = [
        (
            ARLE_GAME,
            "GAME_A.OVL",
            "dae27d4d4d8f4227a05cf717c629465c2e5bafb9e45770b236c18ec7713fb34e",
            0x5AFA,
            0x6A2C,
            b"0:ARURU_F.CNS\0".as_slice(),
            0x5AFC,
            0x6A3A,
            b"0:ARURU_B.CNS\0".as_slice(),
            0x0365,
            0x03DF,
            0x0556,
            0x05BD,
        ),
        (
            RULUE_GAME,
            "GAME_R.OVL",
            "8310393c965439eadc788664e6d7f3119ad3fe670016afee0665ea7724f9e791",
            0x5B2A,
            0x6B90,
            b"2:RURUU_F.CNS\0".as_slice(),
            0x5B2C,
            0x6B9E,
            b"2:RURUU_B.CNS\0".as_slice(),
            0x037E,
            0x03F8,
            0x05C0,
            0x0627,
        ),
        (
            SCHEZO_GAME,
            "GAME_S.OVL",
            "7b5f79e7ca0b2c2b1f7b4b562d8cd411715c36456b84a26bf96f426f696f6837",
            0x5ABA,
            0x6A0D,
            b"4:SHEZO_F.CNS\0".as_slice(),
            0x5ABC,
            0x6A1B,
            b"4:SHEZO_B.CNS\0".as_slice(),
            0x0371,
            0x03ED,
            0x0557,
            0x05BE,
        ),
    ];
    let load_f = [
        0xB4, 0x02, 0xBA, 0x1A, 0x00, 0xCD, 0x7B, 0x8B, 0xD0, 0xBF, 0x00, 0xB8,
    ];
    let load_b = [
        0xB4, 0x02, 0xBA, 0x1C, 0x00, 0xCD, 0x7B, 0x8B, 0xD0, 0xBF, 0x00, 0xE0,
    ];
    let select_cache = [0xB8, 0x00, 0xB8, 0x74, 0x03, 0xB8, 0x00, 0xE0, 0x8E, 0xD8];
    let decode = [0x33, 0xF6, 0x8B, 0xFE, 0xB4, 0x03, 0xCD, 0x7C];
    let tile_banks = [
        0xA1, 0x00, 0x00, 0xC1, 0xE8, 0x04, 0x03, 0xC1, 0x89, 0x46, 0x10, 0x80, 0xC4, 0x08, 0x89,
        0x46, 0x12, 0x8B, 0x36, 0x06, 0x00,
    ];
    let select_group = [
        0xD1, 0xE3, 0xD1, 0xE3, 0x8B, 0x77, 0x06, 0x8B, 0x47, 0x04, 0x86, 0xC4,
    ];
    let compose_group = [
        0xAD, 0x8B, 0xD8, 0x2A, 0xD4, 0xBA, 0x06, 0x00, 0xB0, 0x0E, 0x2A, 0xC4,
    ];
    let compose_call = [0xB9, 0x0E, 0x00, 0xBA, 0x0D, 0x00, 0xB4, 0x02, 0xCD, 0x79];

    for (
        disk_path,
        overlay_name,
        decoded_sha256,
        f_pointer_site,
        f_name_address,
        f_name,
        b_pointer_site,
        b_name_address,
        b_name,
        load_site,
        select_site,
        f_builder_site,
        b_builder_site,
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
        assert_eq!(logical_word(&overlay, f_pointer_site), f_name_address);
        assert_eq!(
            logical_slice(&overlay, usize::from(f_name_address), f_name.len()),
            f_name
        );
        assert_eq!(logical_word(&overlay, b_pointer_site), b_name_address);
        assert_eq!(
            logical_slice(&overlay, usize::from(b_name_address), b_name.len()),
            b_name
        );

        let loader = logical_slice(&overlay, load_site, 0x40);
        assert!(contains(loader, &load_f));
        assert!(contains(loader, &load_b));

        let selector = logical_slice(&overlay, select_site, 0x50);
        assert!(contains(selector, &select_cache));
        assert!(contains(selector, &decode));

        let f_builder = logical_slice(&overlay, f_builder_site, 0x70);
        assert!(contains(f_builder, &tile_banks));
        assert!(contains(f_builder, &compose_call));

        let b_builder = logical_slice(&overlay, b_builder_site, 0xD0);
        assert!(contains(b_builder, &tile_banks));
        assert!(contains(b_builder, &select_group));
        assert!(contains(b_builder, &compose_group));
        assert!(contains(b_builder, &compose_call));
    }
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn resident_tile_consumer_preserves_16_by_16_brgi_plane_offsets() {
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
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn rulue_state_branch_selects_the_fc_and_bc_resource_names() {
    let Some(disk) = common::try_read(RULUE_GAME) else {
        return;
    };
    let overlay = decode_disk_file(&disk, "GAME_R.OVL");
    assert_eq!(
        pc98_madou_ars::media_identity::sha256_hex(&overlay),
        "8310393c965439eadc788664e6d7f3119ad3fe670016afee0665ea7724f9e791"
    );

    for (address, name) in [
        (0x6B90, b"2:RURUU_F.CNS\0".as_slice()),
        (0x6B9E, b"2:RURUU_B.CNS\0".as_slice()),
        (0x6BAC, b"2:ruruu_fc.cns\0".as_slice()),
        (0x6BBB, b"2:ruruu_bc.cns\0".as_slice()),
    ] {
        assert_eq!(logical_slice(&overlay, address, name.len()), name);
    }
    assert_eq!(
        logical_slice(&overlay, 0x8A09, 11),
        [
            0xBA, 0x90, 0x01, 0xB4, 0x02, 0xCD, 0x7B, 0xA8, 0x01, 0x74, 0x1D
        ]
    );
    assert_eq!(
        logical_slice(&overlay, 0x8A18, 12),
        [
            0x26, 0xC7, 0x47, 0x1A, 0x90, 0x6B, 0x26, 0xC7, 0x47, 0x1C, 0x9E, 0x6B
        ]
    );
    assert_eq!(
        logical_slice(&overlay, 0x8A35, 12),
        [
            0x26, 0xC7, 0x47, 0x1A, 0xAC, 0x6B, 0x26, 0xC7, 0x47, 0x1C, 0xBB, 0x6B
        ]
    );
}
