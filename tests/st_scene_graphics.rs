use pc98_madou_ars::graphics_resource::{DecodedRange, ScreenLayout, render_named_screen_resource};

#[path = "common/mod.rs"]
mod common;

const SCHEZO_DATA: &str =
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (She-zo Data disk).hdm";
const SCHEZO_GAME: &str =
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (She-zo Game disk).hdm";
const LOGICAL_ORIGIN: usize = 0x0100;

struct ResourceCase {
    name: &'static str,
    packed_size: usize,
    packed_sha256: &'static str,
    decoded_size: usize,
    decoded_sha256: &'static str,
    rgb_sha256: &'static str,
    unrendered_ranges: &'static [DecodedRange],
}

fn logical_slice(bytes: &[u8], logical: usize, len: usize) -> &[u8] {
    &bytes[logical - LOGICAL_ORIGIN..logical - LOGICAL_ORIGIN + len]
}

fn contains(bytes: &[u8], signature: &[u8]) -> bool {
    bytes
        .windows(signature.len())
        .any(|window| window == signature)
}

fn occurrence_sites(bytes: &[u8], signature: &[u8]) -> Vec<usize> {
    bytes
        .windows(signature.len())
        .enumerate()
        .filter_map(|(index, window)| (window == signature).then_some(index + LOGICAL_ORIGIN))
        .collect()
}

fn immediate_sources_for_segment(bytes: &[u8], segment_variable: u16) -> Vec<(usize, u16)> {
    let [segment_lo, segment_hi] = segment_variable.to_le_bytes();
    bytes
        .windows(8)
        .enumerate()
        .filter(|(_, window)| {
            window[0] == 0xBE && window[3..8] == [0x2E, 0x8E, 0x1E, segment_lo, segment_hi]
        })
        .map(|(index, window)| {
            (
                index + LOGICAL_ORIGIN,
                u16::from_le_bytes([window[1], window[2]]),
            )
        })
        .collect()
}

fn call_target(site: usize, consumer: &[u8]) -> usize {
    let call_index = consumer
        .windows(3)
        .position(|window| window[0] == 0xE8)
        .expect("consumer call");
    let displacement = i16::from_le_bytes([consumer[call_index + 1], consumer[call_index + 2]]);
    usize::try_from((site + call_index + 3) as isize + displacement as isize).unwrap()
}

fn direct_near_call_sites(bytes: &[u8], target: usize) -> Vec<usize> {
    let mut sites = Vec::new();
    for index in 0..bytes.len().saturating_sub(2) {
        if bytes[index] != 0xE8 {
            continue;
        }
        let logical = index + LOGICAL_ORIGIN;
        let displacement = i16::from_le_bytes([bytes[index + 1], bytes[index + 2]]);
        let observed = (logical as i32 + 3 + i32::from(displacement)).rem_euclid(0x1_0000) as usize;
        if observed == target {
            sites.push(logical);
        }
    }
    sites
}

fn st10_1_controller_source_ranges() -> Vec<(usize, usize)> {
    let mut remaining = 0x0688usize;
    let mut source = 0x0480usize;
    let mut height = 0x00B0usize;
    let mut state = 0u8;
    let mut ranges = Vec::new();

    while remaining != 0 {
        state += 1;
        if state >= 5 {
            ranges.push((source, source + height * 0x90));
            if state == 6 {
                state = 0;
            }
        }

        remaining -= 0x16;
        if source == 0 {
            height -= 1;
        } else {
            source -= 0x90;
        }
    }

    ranges
}

fn assert_direct_consumer(
    overlay: &[u8],
    site: usize,
    source: u16,
    segment_variable: u16,
    width: u16,
    height: u16,
    target: usize,
) {
    let consumer = logical_slice(overlay, site, 0x18);
    assert_eq!(&consumer[0..2], &[0x1E, 0xBE]);
    assert_eq!(&consumer[2..4], &source.to_le_bytes());
    assert_eq!(&consumer[4..7], &[0x2E, 0x8E, 0x1E]);
    assert_eq!(&consumer[7..9], &segment_variable.to_le_bytes());
    assert_eq!(consumer[9], 0xBF);
    assert_eq!(consumer[12], 0xBA);
    assert_eq!(&consumer[13..15], &width.to_le_bytes());
    assert_eq!(consumer[15], 0xB9);
    assert_eq!(&consumer[16..18], &height.to_le_bytes());
    assert_eq!(call_target(site, consumer), target);
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn exact_st_resources_render_with_consumer_proven_scene_layouts() {
    let Some(disk) = common::try_read(SCHEZO_DATA) else {
        return;
    };
    let cases = [
        ResourceCase {
            name: "ST1.DAT",
            packed_size: 14_178,
            packed_sha256: "b737fdba35498c1cf4445042cc52fcce18d76286f1271fb8be83558e0673e9f9",
            decoded_size: 36_544,
            decoded_sha256: "557d784649ad31c8683315caac7010dcf4f5150128fa29b86ea711c8ae2938ed",
            rgb_sha256: "b94150ffa8d98f1e47ea7253558536aecaff70a3558569e5bb8f945b58188598",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "ST2.DAT",
            packed_size: 26_355,
            packed_sha256: "af6e4141646e8bffde565452235b1e3d9d06773707798a4722f15e2fb3f4fe12",
            decoded_size: 57_440,
            decoded_sha256: "5c51f6739c67895cf1517a72e82855bdd7a20dd710daecab30032f13b438d9c4",
            rgb_sha256: "0000bfc213085dd27ccff0372da92d8e2351cdd97bd378c05a5174f343226440",
            unrendered_ranges: &[
                DecodedRange {
                    offset: 0x87C0,
                    bytes: 0x540,
                },
                DecodedRange {
                    offset: 0x9000,
                    bytes: 0x380,
                },
            ],
        },
        ResourceCase {
            name: "ST4.DAT",
            packed_size: 18_030,
            packed_sha256: "e53a14bf0624dffff9edfaca2919d6cd4ffe5e004e055c0762c243ec7656b7e8",
            decoded_size: 37_664,
            decoded_sha256: "323db1cb8eda5c048443ae213465198bbb42d53581a8e2bc44b49b1baf59f2fb",
            rgb_sha256: "884afe16d4b69692990effa935f6bb4c33200b8b1be82f95bb49c8619b67e120",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "ST6.DAT",
            packed_size: 17_442,
            packed_sha256: "d4ce1c4ef7f4b6791e8b83f6d93abc5af0ce06393b8fecc0e3039e48a4a7a0d3",
            decoded_size: 30_784,
            decoded_sha256: "a167e6c775c477a6e17ed9d961c4226478dbdd29a17abdb4e1305a841424fc4d",
            rgb_sha256: "5def78d29698c34f283ba60bcc3a8a73bd27d51f10aa002bae52bbe90e9c303a",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "ST8.DAT",
            packed_size: 12_140,
            packed_sha256: "719e510d07109e034e63a465751f940b14d8d69ac6ae201d8d4a02f1577ce2b9",
            decoded_size: 24_320,
            decoded_sha256: "6c9eb5674159fa9889a85fcbef296cb1c8b5a80140d83a864b9a7530fc2b39e8",
            rgb_sha256: "993dd373e1be60355951661e03b1c7818bb3cdfe003bedc4de33f701b4cb5fb5",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "ST8_1.DAT",
            packed_size: 17_128,
            packed_sha256: "03ca9b32bf05dff801229281a500f5683098b22889d7709cf97d9c2eb17e710a",
            decoded_size: 44_160,
            decoded_sha256: "23f510b306dea8ede83cf03ef3bb30449bb4062debc1ee05f892ab7dde9b5969",
            rgb_sha256: "3f1fb0067b57051b884188247330743c8648edfb3fb2e58d018bc0f564ab9c60",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "ST10.DAT",
            packed_size: 6_626,
            packed_sha256: "c8c8f44539e20791d36cd7c126a748be41b448397dfac418994bd25f2aef8ccb",
            decoded_size: 55_296,
            decoded_sha256: "d4f3132efd99514068002e41f87f74d7b91eeb5ce2d53261005e12990ffdac0c",
            rgb_sha256: "01cc3aae38ca19114be32942ed7de9ce1d6fa618999778b05b25f296f5097b87",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "ST10_1.DAT",
            packed_size: 2_449,
            packed_sha256: "4f49cedc94aeb89ef0ec356f4f82aa2cfea589215d32a1c3bbafca5aea30d06d",
            decoded_size: 27_776,
            decoded_sha256: "ab4b1f09c274c917729a999fdcd0b302c12b65979b9509acf24dcd59cb687ecf",
            rgb_sha256: "7c7cf3bc89820cd6fccf6f9da7bd5c8e98498142b2cf6e7c52df727fc673107e",
            unrendered_ranges: &[DecodedRange {
                offset: 0x6C00,
                bytes: 0x80,
            }],
        },
        ResourceCase {
            name: "ST11.DAT",
            packed_size: 14_144,
            packed_sha256: "a26922dafe21b1f1c3c5ca736ae1e5d3152c2a289b766845f509a06bca7c19f6",
            decoded_size: 27_456,
            decoded_sha256: "02d7c2697dde45a5eb85df762e0653834f4c8109c8bd25a219d1dbbb33f1c82e",
            rgb_sha256: "382f2dc3366331c2a8ce0f46650ca655edfcb9cd82e00bf9662cc59ff94f99eb",
            unrendered_ranges: &[],
        },
    ];

    for case in cases {
        let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, case.name).unwrap();
        assert_eq!(packed.len(), case.packed_size);
        assert_eq!(
            pc98_madou_ars::media_identity::sha256_hex(&packed),
            case.packed_sha256
        );
        let decoded = pc98_madou_ars::overlay_lz::decode_overlay_lz(&packed).unwrap();
        assert_eq!(decoded.bytes_consumed, packed.len());
        assert_eq!(decoded.output.len(), case.decoded_size);
        assert_eq!(
            pc98_madou_ars::media_identity::sha256_hex(&decoded.output),
            case.decoded_sha256
        );

        let rendered = render_named_screen_resource(case.name, &packed).unwrap();
        assert_eq!(rendered.layout, ScreenLayout::BrgiSceneAtlas);
        assert_eq!(rendered.stream_sizes, [case.decoded_size]);
        assert_eq!(rendered.unrendered_ranges, case.unrendered_ranges);
        assert_eq!(
            pc98_madou_ars::media_identity::sha256_hex(&rendered.rgb),
            case.rgb_sha256
        );
    }
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn shezo_tu_loads_and_consumes_both_st_storage_orders() {
    let Some(disk) = common::try_read(SCHEZO_GAME) else {
        return;
    };
    let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "SHEZO_TU.OVL").unwrap();
    assert_eq!(packed.len(), 7_107);
    assert_eq!(
        pc98_madou_ars::media_identity::sha256_hex(&packed),
        "bfce3ae76f88eeee96c5b4434bd738f115738f1b84d3b112cbd80055dfeaa6f0"
    );
    let report = pc98_madou_ars::overlay_lz::decode_overlay_lz(&packed).unwrap();
    assert_eq!(report.bytes_consumed, packed.len());
    assert_eq!(report.output.len(), 13_600);
    assert_eq!(
        pc98_madou_ars::media_identity::sha256_hex(&report.output),
        "32ad6d0b6ec1975860fa8271e9872007e1dfeb0bce393f963008aa42f99658e6"
    );
    let overlay = report.output;

    let names = [
        (0x01E2, b"st1.dat\0".as_slice()),
        (0x01EA, b"st2.dat\0".as_slice()),
        (0x01F2, b"st4.dat\0".as_slice()),
        (0x01FA, b"st6.dat\0".as_slice()),
        (0x0202, b"st8.dat\0".as_slice()),
        (0x020A, b"st8_1.dat\0".as_slice()),
        (0x0214, b"st10.dat\0".as_slice()),
        (0x021D, b"st10_1.dat\0".as_slice()),
        (0x0228, b"st11.dat\0".as_slice()),
    ];
    for (address, name) in names {
        assert_eq!(logical_slice(&overlay, address, name.len()), name);
    }

    for (site, filename, destination) in [
        (0x0338, 0x01E2u16, 0x2C46u16),
        (0x0374, 0x01EA, 0x2C48),
        (0x03B0, 0x01F2, 0x2C4A),
        (0x03EC, 0x01FA, 0x2C4C),
        (0x0428, 0x0202, 0x2C4E),
        (0x0464, 0x020A, 0x2C50),
        (0x086E, 0x0214, 0x2C48),
        (0x08AA, 0x021D, 0x2C4A),
        (0x08E6, 0x0228, 0x2C46),
    ] {
        let loader = logical_slice(&overlay, site, 0x3C);
        let mut open = vec![0xBA];
        open.extend_from_slice(&filename.to_le_bytes());
        open.extend_from_slice(&[0xB4, 0x00, 0xCD, 0x7C]);
        assert!(contains(loader, &open));
        let mut decode = vec![0x2E, 0x8E, 0x06];
        decode.extend_from_slice(&destination.to_le_bytes());
        decode.extend_from_slice(&[0xBF, 0x00, 0x00, 0xB4, 0x03, 0xCD, 0x7C]);
        assert!(contains(loader, &decode));
    }

    for (site, opcode) in [(0x2185, 0xA4), (0x21B5, 0xA5)] {
        let blitter = logical_slice(&overlay, site, 0x30);
        assert!(contains(
            blitter,
            &[
                0xB8, 0x00, 0xA8, 0xE8, 0x13, 0x00, 0xB8, 0x00, 0xB0, 0xE8, 0x0D, 0x00, 0xB8, 0x00,
                0xB8, 0xE8, 0x07, 0x00, 0xB8, 0x00, 0xE0,
            ]
        ));
        assert!(contains(blitter, &[0xF3, opcode]));
    }
    let strided = logical_slice(&overlay, 0x21E5, 0x40);
    assert!(contains(strided, &[0xF3, 0xA5]));
    assert!(contains(strided, &[0x2E, 0x03, 0x36, 0x7C, 0x2C]));
    assert!(contains(strided, &[0x2E, 0x03, 0x36, 0x80, 0x2C]));
    let interleaved = logical_slice(&overlay, 0x2224, 0x38);
    assert!(contains(
        interleaved,
        &[
            0xAC, 0xE6, 0x7E, 0x8A, 0xE0, 0xAC, 0xE6, 0x7E, 0x0A, 0xE0, 0xAC, 0xE6, 0x7E, 0x0A,
            0xE0, 0xAC, 0xE6, 0x7E,
        ]
    ));
    assert!(contains(interleaved, &[0x2E, 0x03, 0x36, 0x7C, 0x2C]));

    assert_direct_consumer(&overlay, 0x0F25, 0x0000, 0x2C46, 0x000F, 0x0058, 0x21B5);
    assert_direct_consumer(&overlay, 0x0FC2, 0x6580, 0x2C46, 0x000F, 0x0058, 0x21B5);
    assert_direct_consumer(&overlay, 0x111D, 0x83A0, 0x2C48, 0x000B, 0x0018, 0x2185);
    assert_direct_consumer(&overlay, 0x11B2, 0xCDA0, 0x2C48, 0x0019, 0x0030, 0x2185);
    assert_direct_consumer(&overlay, 0x11F7, 0x0000, 0x2C4A, 0x0008, 0x0060, 0x21B5);
    assert_direct_consumer(&overlay, 0x1224, 0x2E80, 0x2C4A, 0x0015, 0x0098, 0x2185);
    assert_direct_consumer(&overlay, 0x1507, 0x0000, 0x2C4C, 0x000B, 0x0098, 0x21B5);
    assert_direct_consumer(&overlay, 0x151D, 0x7640, 0x2C4C, 0x0002, 0x0020, 0x21B5);
    assert_direct_consumer(&overlay, 0x18F2, 0x0000, 0x2C4E, 0x000A, 0x0060, 0x21B5);
    assert_direct_consumer(&overlay, 0x1920, 0x3C00, 0x2C4E, 0x000A, 0x0070, 0x21B5);
    assert_direct_consumer(&overlay, 0x1937, 0x0000, 0x2C50, 0x001B, 0x00A0, 0x2185);
    assert_direct_consumer(&overlay, 0x1977, 0x7800, 0x2C50, 0x0015, 0x00A0, 0x2185);

    let st2_primary = logical_slice(&overlay, 0x100F, 0x4A);
    assert!(contains(st2_primary, &[0x2E, 0x8E, 0x1E, 0x48, 0x2C]));
    assert!(contains(st2_primary, &[0xBA, 0x0F, 0x00, 0xB9, 0xB0, 0x00]));
    assert!(contains(
        st2_primary,
        &[0x2E, 0xC7, 0x06, 0x7C, 0x2C, 0x1E, 0x00]
    ));
    assert!(contains(
        st2_primary,
        &[0x2E, 0xC7, 0x06, 0x80, 0x2C, 0xE0, 0x1F]
    ));

    // ST2 owns 0x2C48 until ST10 replaces it at loader 0x086E. Four ST2
    // handlers take their primary-sheet SI from 0x2C64; every other ST2
    // source is immediate.
    let st2_ds_sites = occurrence_sites(&overlay, &[0x2E, 0x8E, 0x1E, 0x48, 0x2C])
        .into_iter()
        .filter(|site| (0x100F..0x11C9).contains(site))
        .collect::<Vec<_>>();
    assert_eq!(
        st2_ds_sites,
        [
            0x1015, 0x1037, 0x1063, 0x1085, 0x10A3, 0x10C5, 0x10E3, 0x1105, 0x1121, 0x1138, 0x114E,
            0x1165, 0x1189, 0x119F, 0x11B6,
        ]
    );
    let st2_immediate_sources = immediate_sources_for_segment(&overlay, 0x2C48)
        .into_iter()
        .filter(|(site, _)| (0x100F..0x11C9).contains(site))
        .collect::<Vec<_>>();
    assert_eq!(
        st2_immediate_sources,
        [
            (0x1034, 0x0D37),
            (0x1082, 0x8D00),
            (0x10C2, 0x8E00),
            (0x1102, 0x8F00),
            (0x111E, 0x83A0),
            (0x1135, 0x7F80),
            (0x114B, 0x9380),
            (0x1162, 0x0690),
            (0x1186, 0xBAE0),
            (0x119C, 0x8F00),
            (0x11B3, 0xCDA0),
        ]
    );
    let immediate_st2_ds_sites = st2_immediate_sources
        .iter()
        .map(|(site, _)| site + 3)
        .collect::<Vec<_>>();
    assert_eq!(
        st2_ds_sites
            .iter()
            .copied()
            .filter(|site| !immediate_st2_ds_sites.contains(site))
            .collect::<Vec<_>>(),
        [0x1015, 0x1063, 0x10A3, 0x10E3]
    );

    assert_eq!(
        logical_slice(&overlay, 0x0539, 14),
        [
            0x2E, 0xC7, 0x06, 0x64, 0x2C, 0x40, 0x0B, 0x2E, 0xC7, 0x06, 0x66, 0x2C, 0x33, 0x19,
        ]
    );
    assert_eq!(
        logical_slice(&overlay, 0x0FF9, 22),
        [
            0x2E, 0x83, 0x3E, 0x64, 0x2C, 0x00, 0x74, 0x0D, 0x2E, 0x83, 0x2E, 0x64, 0x2C, 0x3C,
            0x2E, 0x81, 0x06, 0x66, 0x2C, 0xA0, 0x00, 0xC3,
        ]
    );
    assert_eq!(
        direct_near_call_sites(&overlay, 0x0FF9),
        [0x1059, 0x1099, 0x10D9, 0x1119]
    );

    let st2_handler_table = (0..58)
        .map(|index| {
            u16::from_le_bytes(
                logical_slice(&overlay, 0x0BE7 + index * 2, 2)
                    .try_into()
                    .unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(st2_handler_table.last(), Some(&0xFFFF));
    for (handler, expected_count) in [
        (0x0FD9, 1),
        (0x100F, 16),
        (0x105D, 22),
        (0x109D, 8),
        (0x10DD, 6),
        (0x0F09, 3),
        (0x0FF5, 1),
        (0xFFFF, 1),
    ] {
        assert_eq!(
            st2_handler_table
                .iter()
                .filter(|observed| **observed == handler)
                .count(),
            expected_count
        );
    }

    let dynamic_handlers = [0x100F, 0x105D, 0x109D, 0x10DD];
    let mut source = 0x0B40usize;
    let mut dynamic_ranges = Vec::new();
    for handler in st2_handler_table {
        if dynamic_handlers.contains(&handler) {
            let end = source + 3 * 0x1FE0 + 175 * 0x1E + 30;
            dynamic_ranges.push((source, end));
            if source != 0 {
                source -= 0x3C;
            }
        }
    }
    assert_eq!(dynamic_ranges.len(), 52);
    assert_eq!(source, 0);
    assert_eq!(
        dynamic_ranges.iter().map(|(_, end)| *end).max(),
        Some(0x7F80)
    );

    let late = logical_slice(&overlay, 0x19A0, 0xA5);
    assert!(contains(late, &[0x2E, 0x8E, 0x1E, 0x46, 0x2C]));
    assert!(contains(late, &[0x2E, 0xC7, 0x06, 0x7C, 0x2C, 0x16, 0x00]));
    assert!(contains(late, &[0x2E, 0xC7, 0x06, 0x80, 0x2C, 0xD0, 0x1A]));
    assert!(contains(late, &[0x2E, 0x8E, 0x1E, 0x48, 0x2C]));
    assert!(contains(late, &[0x2E, 0x8E, 0x1E, 0x4A, 0x2C]));
    assert!(contains(late, &[0xBA, 0x24, 0x00]));
    assert!(contains(late, &[0x2E, 0xC7, 0x06, 0x7C, 0x2C, 0x90, 0x00]));

    // The post-reload controller reaches its sole ST10_1 source path through
    // two calls to 0x198E. Its pinned initializer runs 76 steps, alternates the
    // ST10_1 segment only in states 5/6, and never reaches the partial-row tail
    // at 0x6C00..0x6C80.
    assert_eq!(direct_near_call_sites(&overlay, 0x198E), [0x0957, 0x098B]);
    assert_eq!(
        logical_slice(&overlay, 0x0922, 29),
        [
            0x2E, 0xC7, 0x46, 0x0D, 0x88, 0x06, 0x2E, 0xC7, 0x46, 0x0F, 0x80, 0x04, 0x2E, 0xC7,
            0x46, 0x11, 0x96, 0x16, 0x2E, 0xC7, 0x46, 0x13, 0xB0, 0x00, 0x2E, 0xC6, 0x46, 0x16,
            0x00,
        ]
    );
    assert!(contains(
        logical_slice(&overlay, 0x1A21, 0x4C),
        &[0x2E, 0x8E, 0x1E, 0x4A, 0x2C]
    ));
    let ranges = st10_1_controller_source_ranges();
    assert_eq!(ranges.len(), 24);
    assert_eq!(ranges.iter().map(|(_, end)| *end).max(), Some(0x6540));
    assert!(ranges.iter().all(|(_, end)| *end <= 0x6C00));
}
