use pc98_madou_ars::graphics_resource::{ScreenLayout, render_named_screen_resource};

#[path = "common/mod.rs"]
mod common;

const DEMO_DISK: &str = "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Demo disk).hdm";
const LOGICAL_ORIGIN: usize = 0x0100;

fn logical_slice(bytes: &[u8], logical: usize, len: usize) -> &[u8] {
    &bytes[logical - LOGICAL_ORIGIN..logical - LOGICAL_ORIGIN + len]
}

fn decode_disk_file(disk: &[u8], name: &str) -> Vec<u8> {
    let packed = pc98_madou_ars::read_fat12_file_from_hdm(disk, name).unwrap();
    let report = pc98_madou_ars::overlay_lz::decode_overlay_lz(&packed).unwrap();
    assert_eq!(report.bytes_consumed, packed.len(), "{name} packed tail");
    report.output
}

fn int79_blit_signature(bp: u16, width_bytes: u16, height: u16, si: u16, di: u16) -> Vec<u8> {
    let mut out = vec![0xBD];
    out.extend_from_slice(&bp.to_le_bytes());
    out.extend_from_slice(&[0xC7, 0x46, 0x00]);
    out.extend_from_slice(&width_bytes.to_le_bytes());
    out.extend_from_slice(&[0xC7, 0x46, 0x02]);
    out.extend_from_slice(&height.to_le_bytes());
    out.push(0xBE);
    out.extend_from_slice(&si.to_le_bytes());
    out.push(0xBF);
    out.extend_from_slice(&di.to_le_bytes());
    out.extend_from_slice(&[0xB4, 0x01, 0xCD, 0x79]);
    out
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn exact_tc_and_mu_primary_streams_render_through_the_ah5_layout() {
    let Some(disk) = common::try_read(DEMO_DISK) else {
        return;
    };
    let cases = [
        (
            "TC.CNS",
            "2215e4a5f0ddc48c767e3a4c1c73bd091e2c2fdfd80f999a75e0c542a5495591",
            4_740,
            "a66a8bf07e51c49b1e1bbf71c20e0cc679fea69e8299831b375fd562e0d0c885",
        ),
        (
            "MU.CNS",
            "7a3107c51b249eca44266c62f536868b773e3e3cbdff9ae02e8068b46a28ebdb",
            12_163,
            "e952fa6b42c3c12ede7ac5b948dc43a23fb4e58e8e1bacf9473c94ac35576119",
        ),
    ];
    for (name, packed_sha256, companion_size, rgb_sha256) in cases {
        let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, name).unwrap();
        assert_eq!(
            pc98_madou_ars::media_identity::sha256_hex(&packed),
            packed_sha256
        );
        let rendered = render_named_screen_resource(name, &packed).unwrap();
        assert_eq!(rendered.layout, ScreenLayout::ColumnMajorBrgiPlanes);
        assert_eq!(rendered.stream_sizes, [128_000, companion_size]);
        assert_eq!(rendered.companion_stream_sizes, [companion_size]);
        assert!(rendered.companion_audio.is_some());
        assert_eq!(
            pc98_madou_ars::media_identity::sha256_hex(&rendered.rgb),
            rgb_sha256
        );
    }
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn gaoo_and_madomenu_route_ah5_chunks_to_brgi_source_slots() {
    let Some(disk) = common::try_read(DEMO_DISK) else {
        return;
    };
    let gaoo = decode_disk_file(&disk, "GAOO.OVL");
    let menu = decode_disk_file(&disk, "MADOMENU.OVL");
    assert_eq!(
        pc98_madou_ars::media_identity::sha256_hex(&gaoo),
        "b4ae1b7a324c1718356f2c35273a38f6705f2193d76a821e12628e7af8efbd02"
    );
    assert_eq!(
        pc98_madou_ars::media_identity::sha256_hex(&menu),
        "16e21b35a0e7de6f160bb7cca417fa0dd87157408368c7bff5454f85c0213ead"
    );

    // Both overlays pass four consecutive 32K destinations to INT 7Ch/AH=5,
    // then store ES/BX/CX/DX in descriptor offsets +12/+10/+0E/+0C. The
    // descriptor's destination group +0A/+08/+06/+04 is A800/B000/B800/E000,
    // establishing primary chunks B/R/G/I in that order for INT 79h/AH=1.
    let gaoo_load = [
        0xBA, 0x22, 0x0A, 0x8B, 0x3E, 0xC0, 0x09, 0xE8, 0x2C, 0x04, 0x8E, 0x06, 0xC8, 0x09, 0x8C,
        0x06, 0xEA, 0x09, 0x8B, 0x1E, 0xCA, 0x09, 0x89, 0x1E, 0xE8, 0x09, 0x8B, 0x0E, 0xCC, 0x09,
        0x89, 0x0E, 0xE6, 0x09, 0x8B, 0x16, 0xCE, 0x09, 0x89, 0x16, 0xE4, 0x09, 0x8E, 0x1E, 0xC0,
        0x09, 0x33, 0xF6, 0xB4, 0x05, 0xCD, 0x7C,
    ];
    assert_eq!(logical_slice(&gaoo, 0x021A, gaoo_load.len()), gaoo_load);
    assert_eq!(logical_slice(&gaoo, 0x0A22, 9), b"6:TC.CNS\0");
    assert_eq!(
        logical_slice(&gaoo, 0x09DC, 16),
        [
            0x00, 0xE0, 0x00, 0xB8, 0x00, 0xB0, 0x00, 0xA8, 0x00, 0xE0, 0x00, 0xB8, 0x00, 0xB0,
            0x00, 0xA8,
        ]
    );

    let menu_load = [
        0xBA, 0x66, 0x0F, 0x8B, 0x3E, 0x60, 0x08, 0xE8, 0xFA, 0x02, 0x8E, 0x06, 0x68, 0x08, 0x8C,
        0x06, 0x8A, 0x08, 0x8B, 0x1E, 0x6A, 0x08, 0x89, 0x1E, 0x88, 0x08, 0x8B, 0x0E, 0x6C, 0x08,
        0x89, 0x0E, 0x86, 0x08, 0x8B, 0x16, 0x6E, 0x08, 0x89, 0x16, 0x84, 0x08, 0x8E, 0x1E, 0x60,
        0x08, 0x33, 0xF6, 0xB4, 0x05, 0xCD, 0x7C,
    ];
    assert_eq!(logical_slice(&menu, 0x018C, menu_load.len()), menu_load);
    assert_eq!(logical_slice(&menu, 0x0F66, 9), b"6:MU.CNS\0");
    assert_eq!(
        logical_slice(&menu, 0x087C, 16),
        [
            0x00, 0xE0, 0x00, 0xB8, 0x00, 0xB0, 0x00, 0xA8, 0x00, 0xE0, 0x00, 0xB8, 0x00, 0xB0,
            0x00, 0xA8,
        ]
    );

    // TC's initial central copy and MU's four initial menu copies use the
    // same AH=1 descriptor. Width is in bytes and source rows stride by 80.
    assert_eq!(
        logical_slice(&gaoo, 0x025A, 22),
        [
            0xBD, 0xD8, 0x09, 0xC7, 0x46, 0x00, 0x30, 0x00, 0xC7, 0x46, 0x02, 0x40, 0x01, 0xBE,
            0x10, 0x0F, 0x8B, 0xFE, 0xB4, 0x01, 0xCD, 0x79,
        ]
    );
    for (logical, width, height, si, di) in [
        (0x01CC, 0x1A, 0x18, 0x7300, 0x141B),
        (0x01E3, 0x0C, 0xB0, 0x3C00, 0x370A),
        (0x01FA, 0x0E, 0xC8, 0x348C, 0x2FA0),
        (0x0211, 0x10, 0xC8, 0x349A, 0x2FB6),
    ] {
        let signature = int79_blit_signature(0x0878, width, height, si, di);
        assert_eq!(logical_slice(&menu, logical, signature.len()), signature);
    }
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn resident_main_proves_ah5_transpose_and_int79_copy_geometry() {
    let Some(disk) = common::try_read(DEMO_DISK) else {
        return;
    };
    let main = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "MAIN.COM").unwrap();
    let report = pc98_madou_ars::overlay_lz::decode_overlay_lz(&main[0x71..]).unwrap();
    assert_eq!(report.bytes_consumed, main.len() - 0x71);
    assert_eq!(report.output.len(), 11_088);
    assert_eq!(
        pc98_madou_ars::media_identity::sha256_hex(&report.output),
        "cae16a8287e21d1c90a19bc9caff26e599542c67e68d2c14897af3e0821362ee"
    );
    let body = report.output;

    // INT 79h/AH=1: width bytes / 2 words, height rows. Source descriptor
    // offsets +12/+10/+0E/+0C map to destination +0A/+08/+06/+04. The latter
    // contain A800/B000/B800/E000 in the consumer descriptors.
    assert_eq!(
        logical_slice(&body, 0x0F81, 13),
        [
            0xFB, 0x8B, 0x5E, 0x00, 0xD1, 0xEB, 0x8B, 0x46, 0x02, 0xBA, 0x02, 0x00, 0xFC,
        ]
    );
    for signature in [
        [0x8E, 0x5E, 0x12, 0x8E, 0x46, 0x0A],
        [0x8E, 0x5E, 0x10, 0x8E, 0x46, 0x08],
        [0x8E, 0x5E, 0x0E, 0x8E, 0x46, 0x06],
        [0x8E, 0x5E, 0x0C, 0x8E, 0x46, 0x04],
    ] {
        assert!(
            logical_slice(&body, 0x0F81, 0x4F)
                .windows(signature.len())
                .any(|window| window == signature)
        );
    }
    assert!(
        logical_slice(&body, 0x0F81, 0x4F)
            .windows(6)
            .any(|window| window == [0x83, 0xC6, 0x52, 0x83, 0xC7, 0x52])
    );

    // INT 7Ch/AH=5 calls the same worker for ES/BX/CX/DX. The worker fills
    // 80 columns; each column consumes 400 bytes and writes them with +0x4F
    // after MOVSB, yielding an 80-byte destination row stride.
    assert_eq!(
        logical_slice(&body, 0x0A23, 24),
        [
            0x33, 0xDB, 0x07, 0xE8, 0x16, 0x00, 0x33, 0xDB, 0x07, 0xE8, 0x10, 0x00, 0x33, 0xDB,
            0x07, 0xE8, 0x0A, 0x00, 0x33, 0xDB, 0x07, 0xE8, 0x04, 0x00,
        ]
    );
    assert_eq!(
        logical_slice(&body, 0x0A54, 30),
        [
            0x87, 0xF2, 0x8E, 0xDD, 0x8B, 0xFB, 0xB9, 0x90, 0x01, 0x81, 0xE6, 0xFF, 0x03, 0xA4,
            0x83, 0xC7, 0x4F, 0xE2, 0xF6, 0x87, 0xD6, 0x5F, 0x1F, 0x43, 0x83, 0xFB, 0x50, 0x72,
            0xCE, 0xC3,
        ]
    );
}
