use pc98_madou_ars::graphics_resource::{DecodedRange, ScreenLayout, render_named_screen_resource};

#[path = "common/mod.rs"]
mod common;

const RULUE_DATA: &str =
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Rurue Data disk).hdm";
const RULUE_GAME: &str =
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Rurue Game disk).hdm";
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

#[derive(Clone, Copy)]
struct ConsumerCase {
    site: usize,
    source: u16,
    segment_variable: u16,
    width_argument: u16,
    height: u16,
    target: usize,
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
        .windows(9)
        .enumerate()
        .filter(|(_, window)| {
            window[0..2] == [0x1E, 0xBE]
                && window[4..9] == [0x2E, 0x8E, 0x1E, segment_lo, segment_hi]
        })
        .map(|(index, window)| {
            (
                index + LOGICAL_ORIGIN,
                u16::from_le_bytes([window[2], window[3]]),
            )
        })
        .collect()
}

fn near_call_sites_to(bytes: &[u8], target: usize) -> Vec<usize> {
    bytes
        .windows(3)
        .enumerate()
        .filter_map(|(index, window)| {
            (window[0] == 0xE8).then(|| {
                let site = index + LOGICAL_ORIGIN;
                let displacement = i16::from_le_bytes([window[1], window[2]]);
                usize::try_from((site + 3) as isize + displacement as isize)
                    .ok()
                    .filter(|resolved| *resolved == target)
                    .map(|_| site)
            })?
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

fn assert_consumer(overlay: &[u8], case: ConsumerCase) {
    let consumer = logical_slice(overlay, case.site, 0x30);
    assert_eq!(
        &consumer[0..2],
        &[0x1E, 0xBE],
        "consumer at 0x{:04X}",
        case.site
    );
    assert_eq!(
        &consumer[2..4],
        &case.source.to_le_bytes(),
        "consumer at 0x{:04X}",
        case.site
    );
    assert_eq!(
        &consumer[4..7],
        &[0x2E, 0x8E, 0x1E],
        "consumer at 0x{:04X}",
        case.site
    );
    assert_eq!(
        &consumer[7..9],
        &case.segment_variable.to_le_bytes(),
        "consumer at 0x{:04X}",
        case.site
    );

    let mut dimensions = vec![0xBA];
    dimensions.extend_from_slice(&case.width_argument.to_le_bytes());
    dimensions.push(0xB9);
    dimensions.extend_from_slice(&case.height.to_le_bytes());
    assert!(contains(consumer, &dimensions));
    assert_eq!(call_target(case.site, consumer), case.target);
}

fn assert_consumers(overlay: &[u8], cases: &[ConsumerCase]) {
    for case in cases {
        assert_consumer(overlay, *case);
    }
}

fn assert_loader(overlay: &[u8], site: usize, filename_address: u16, segment_variable: u16) {
    let loader = logical_slice(overlay, site, 0x40);
    let mut open = vec![0xBA];
    open.extend_from_slice(&filename_address.to_le_bytes());
    open.extend_from_slice(&[0xB4, 0x00, 0xCD, 0x7C]);
    assert!(contains(loader, &open));

    let mut decode = vec![0x2E, 0x8E, 0x06];
    decode.extend_from_slice(&segment_variable.to_le_bytes());
    decode.extend_from_slice(&[0xBF, 0x00, 0x00, 0xB4, 0x03, 0xCD, 0x7C]);
    assert!(contains(loader, &decode));
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn exact_re_resources_render_with_consumer_proven_scene_layouts() {
    let Some(disk) = common::try_read(RULUE_DATA) else {
        return;
    };
    let cases = [
        ResourceCase {
            name: "RE1.DAT",
            packed_size: 36_923,
            packed_sha256: "2ae35f49e2ba839c1f0062c21fd347c46e27491754d96b4cc9cd5c587e5ecde8",
            decoded_size: 52_800,
            decoded_sha256: "67f3c35a7917e4e269ab57d67d45db2abccf8dce546e6d659e98f7bde478dfc2",
            rgb_sha256: "c0d56738df87f5fbc1e9515d770eee58313708d0db9b5984574374fe569eea0b",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "RE2.DAT",
            packed_size: 18_007,
            packed_sha256: "7d1adb6a2c88f86241a4b8dc8e63bef1071acfa058dcc27fd916aa86e25d0cea",
            decoded_size: 28_416,
            decoded_sha256: "5faea8da4a682403a65bda2142d40aec2bc1bad86d0351ce4863206ad9248b4b",
            rgb_sha256: "05d61402a5ae8cf554ce9be28799504bea45f69fd76fabb5a9e0a32c551c7c29",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "RE3.DAT",
            packed_size: 44_391,
            packed_sha256: "88645c9b373b8cc6a5ca1e0b4aac58188161c9b92ebd9e1bee096cc790d87a31",
            decoded_size: 58_624,
            decoded_sha256: "38bff3cb6957d91081cc8b9102a1db02caf902809f47cb6533e64baf3247df42",
            rgb_sha256: "76855a4555a29698ab3bc3c8f8e7fd848c1e9f4ebcccdcfb9681b6607cfd54d1",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "RE6.DAT",
            packed_size: 17_734,
            packed_sha256: "b6f8c7f331933d2782cac973c08b5690e159d26e8ff3faf50d22ea26a34e8658",
            decoded_size: 29_376,
            decoded_sha256: "93a637f8cda4b1a4778759ec9c3312a76f67ad9c570909958c12990e0d9c1fb3",
            rgb_sha256: "17682e6c42a60cc1f6e3058d1060e78bb80c33bf163aa6efb27973de6eed2241",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "RE7.DAT",
            packed_size: 35_213,
            packed_sha256: "aaef23cb555bde9f84e77acedc186ac4b56955e29b57b3dc56c363f6d07e12eb",
            decoded_size: 51_456,
            decoded_sha256: "76b668647755d84beec8336efabd0dc005141f72e0446a26d2df9fe84f9a8ea6",
            rgb_sha256: "c4a1bd218fa52420a0e8daea1b2457b0facfcf3846a0724d518bcdbace762a28",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "RE9.DAT",
            packed_size: 25_904,
            packed_sha256: "c73e4bcf07788ab5ea15c15a63ae872d4176c1e25fe8027f65e03b82aa11aad5",
            decoded_size: 31_808,
            decoded_sha256: "e5ccbb24663b976589e33ad5aeec303adcd22a4bc3808320893ac965413b375f",
            rgb_sha256: "40f7adaba2025f3a5b648e3d7e3a4e4bb3b79015ec1b6385cc03201285fb0791",
            unrendered_ranges: &[DecodedRange {
                offset: 0x6CC0,
                bytes: 0x00C0,
            }],
        },
        ResourceCase {
            name: "RE11.DAT",
            packed_size: 22_893,
            packed_sha256: "319cc20e43e230fb647471a61a4340241ee19b7401fb01643371e20fa819be8a",
            decoded_size: 28_576,
            decoded_sha256: "fa4709a580cc06f67870cfad57b46873e120fd712fcd4358243e35dd6920a4f7",
            rgb_sha256: "5730bbb528d41f5dbfebb6cc12db464d3e2799564edcc4922bbf95a6cfb644d4",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "RE15.DAT",
            packed_size: 28_527,
            packed_sha256: "055d0510c266ba54780ab57f8b01c3c95c492b3f39687c6d983e6138abda3083",
            decoded_size: 36_672,
            decoded_sha256: "b6c4e723a2701e47dcd98ec12c4701ddc5d2c51b6c552d278d390b2aad74069a",
            rgb_sha256: "15449d5ff9c7a018020fb7bdb1535783da902e856606ae833680840e29647e0b",
            unrendered_ranges: &[DecodedRange {
                offset: 0x8EC0,
                bytes: 0x0080,
            }],
        },
        ResourceCase {
            name: "RE17.DAT",
            packed_size: 48_275,
            packed_sha256: "83df56fb63f9fc939ded3c471d82a93978b6e68a32fd0e984fcf05ab96ec51ce",
            decoded_size: 61_440,
            decoded_sha256: "ee12d845e9f7e1985daebe4f9020fd7cca53bf805161a6750953e1a4b98530e2",
            rgb_sha256: "0c0090aee83639a4a88db307eca3e8c3232074f742684f3420226d45e700f8c7",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "RE17_1.DAT",
            packed_size: 14_310,
            packed_sha256: "d78cc31fa4750ad3d7eb2e8c9447bd7f84878fa4138db0db2cd0e4391fcca6b4",
            decoded_size: 17_952,
            decoded_sha256: "4ea18f3d58c97d3d0a95804c8cdabf86db04db5eab5aa1441b8c0a2e58b4f32d",
            rgb_sha256: "bb61adc2f648ac2fd5a422b0b81227b5a4fe2b664d7a6598864cc77ccbf311b6",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "RE19.DAT",
            packed_size: 18_114,
            packed_sha256: "1cf8f4aef607c16961a33f8c1c61d2b1476053a98184354d3bff3ba806681940",
            decoded_size: 27_648,
            decoded_sha256: "71966cca3745bbc31529e4ba740d8272d1e2302c79c400474f5e8ff662bd8534",
            rgb_sha256: "fb817699a8030c08f8a2fd2b7450e445a52155327c685e57431c11670c3e671e",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "RE21_1.DAT",
            packed_size: 17_941,
            packed_sha256: "b5f511e0e100f80f775729950c62f97426bd0be4dde28fd4da22f244d818973a",
            decoded_size: 57_216,
            decoded_sha256: "668c449311fb12e44bc86cd5868e87bce2d2c16add7d2e89ef9bc1fc923582b8",
            rgb_sha256: "986586e70cc04dd3de36aab5fa2080915b5bf65fe6bffd159fa34df9e34971bd",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "RE21_2.DAT",
            packed_size: 9_011,
            packed_sha256: "24b30de14c877a837e179d26d1200f25b0954c5e1acf46d5f10b243e3db58d7a",
            decoded_size: 16_384,
            decoded_sha256: "620bb65f8b784cbf484e75b7ec42e0425beb24cea56bd0ce229538e0eb9cde41",
            rgb_sha256: "1b5c4b6c0c797f807ccf3884bfd8dcb5ea8de27ed9fcb85685fbb022040657db",
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
fn ending_r_loads_and_consumes_all_thirteen_re_resources() {
    let Some(disk) = common::try_read(RULUE_GAME) else {
        return;
    };
    let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "ENDING_R.OVL").unwrap();
    assert_eq!(packed.len(), 9_864);
    assert_eq!(
        pc98_madou_ars::media_identity::sha256_hex(&packed),
        "ec27083758827b041552cf0974299f296b090437bcd31361b0d1525e9c3b639c"
    );
    let report = pc98_madou_ars::overlay_lz::decode_overlay_lz(&packed).unwrap();
    assert_eq!(report.bytes_consumed, packed.len());
    assert_eq!(report.output.len(), 18_018);
    assert_eq!(
        pc98_madou_ars::media_identity::sha256_hex(&report.output),
        "154a7fb504550b5d077f69f88736eabe739a46ad95cb755eb73145021750324b"
    );
    let overlay = report.output;

    for (address, name) in [
        (0x01EC, b"re1.dat\0".as_slice()),
        (0x01F4, b"re2.dat\0".as_slice()),
        (0x01FC, b"re3.dat\0".as_slice()),
        (0x0204, b"re6.dat\0".as_slice()),
        (0x020C, b"re7.dat\0".as_slice()),
        (0x0214, b"re9.dat\0".as_slice()),
        (0x021C, b"re11.dat\0".as_slice()),
        (0x0225, b"re15.dat\0".as_slice()),
        (0x022E, b"re17.dat\0".as_slice()),
        (0x0237, b"re17_1.dat\0".as_slice()),
        (0x0242, b"re19.dat\0".as_slice()),
        (0x024B, b"re21_1.dat\0".as_slice()),
        (0x0256, b"re21_2.dat\0".as_slice()),
    ] {
        assert_eq!(logical_slice(&overlay, address, name.len()), name);
    }

    for (site, filename, destination) in [
        (0x0383, 0x01ECu16, 0x3E6Eu16),
        (0x03BF, 0x01F4, 0x3E70),
        (0x03FB, 0x01FC, 0x3E66),
        (0x0437, 0x0204, 0x3E72),
        (0x0473, 0x020C, 0x3E68),
        (0x04AF, 0x0214, 0x3E6A),
        (0x04EB, 0x022E, 0x3E6C),
        (0x0527, 0x0237, 0x3E74),
        (0x08B8, 0x021C, 0x3E70),
        (0x0AEB, 0x0225, 0x3E6E),
        (0x0C35, 0x0242, 0x3E72),
        (0x0D68, 0x024B, 0x3E66),
        (0x0DA4, 0x0256, 0x3E74),
    ] {
        assert_loader(&overlay, site, filename, destination);
    }

    let row_interleaved = logical_slice(&overlay, 0x3384, 0x30);
    assert!(contains(
        row_interleaved,
        &[
            0xAC, 0xE6, 0x7E, 0x8A, 0xE0, 0xAC, 0xE6, 0x7E, 0x0A, 0xE0, 0xAC, 0xE6, 0x7E, 0x0A,
            0xE0, 0xAC, 0xE6, 0x7E,
        ]
    ));
    assert!(contains(row_interleaved, &[0xAA, 0xE2, 0xE9]));

    for (site, opcode) in [(0x33B5, 0xA4), (0x33E5, 0xA5)] {
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
    for (site, opcode) in [(0x3415, 0xA5), (0x3529, 0xA4)] {
        let blitter = logical_slice(&overlay, site, 0x42);
        assert!(contains(blitter, &[0xF3, opcode]));
        assert!(contains(blitter, &[0x2E, 0x03, 0x36, 0xAE, 0x3E]));
        assert!(contains(blitter, &[0x2E, 0x03, 0x36, 0xB2, 0x3E]));
    }

    assert_consumers(
        &overlay,
        &[
            ConsumerCase {
                site: 0x1494,
                source: 0x0000,
                segment_variable: 0x3E6E,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x3415,
            },
            ConsumerCase {
                site: 0x1506,
                source: 0xCC00,
                segment_variable: 0x3E6E,
                width_argument: 0x03,
                height: 0x18,
                target: 0x33B5,
            },
            ConsumerCase {
                site: 0x151D,
                source: 0xCD20,
                segment_variable: 0x3E6E,
                width_argument: 0x03,
                height: 0x18,
                target: 0x33B5,
            },
            ConsumerCase {
                site: 0x1534,
                source: 0x0000,
                segment_variable: 0x3E70,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x154B,
                source: 0x6C00,
                segment_variable: 0x3E70,
                width_argument: 0x04,
                height: 0x18,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1562,
                source: 0x0000,
                segment_variable: 0x3E66,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x165B,
                source: 0x6C00,
                segment_variable: 0x3E66,
                width_argument: 0x04,
                height: 0x10,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1672,
                source: 0x6E00,
                segment_variable: 0x3E66,
                width_argument: 0x01,
                height: 0x08,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x159E,
                source: 0x6E40,
                segment_variable: 0x3E66,
                width_argument: 0x03,
                height: 0x28,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x15B5,
                source: 0x7200,
                segment_variable: 0x3E66,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1608,
                source: 0xDE00,
                segment_variable: 0x3E66,
                width_argument: 0x05,
                height: 0x20,
                target: 0x33B5,
            },
            ConsumerCase {
                site: 0x15F1,
                source: 0xE080,
                segment_variable: 0x3E66,
                width_argument: 0x05,
                height: 0x20,
                target: 0x33B5,
            },
            ConsumerCase {
                site: 0x1C2B,
                source: 0xE300,
                segment_variable: 0x3E66,
                width_argument: 0x02,
                height: 0x18,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1644,
                source: 0xE480,
                segment_variable: 0x3E66,
                width_argument: 0x01,
                height: 0x10,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1689,
                source: 0x0000,
                segment_variable: 0x3E72,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x16C5,
                source: 0x6C00,
                segment_variable: 0x3E72,
                width_argument: 0x03,
                height: 0x28,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1701,
                source: 0x6FC0,
                segment_variable: 0x3E72,
                width_argument: 0x05,
                height: 0x18,
                target: 0x33B5,
            },
            ConsumerCase {
                site: 0x173D,
                source: 0x71A0,
                segment_variable: 0x3E72,
                width_argument: 0x01,
                height: 0x18,
                target: 0x33B5,
            },
            ConsumerCase {
                site: 0x1779,
                source: 0x7200,
                segment_variable: 0x3E72,
                width_argument: 0x01,
                height: 0x18,
                target: 0x33B5,
            },
            ConsumerCase {
                site: 0x1790,
                source: 0x7260,
                segment_variable: 0x3E72,
                width_argument: 0x01,
                height: 0x18,
                target: 0x33B5,
            },
            ConsumerCase {
                site: 0x17D8,
                source: 0x0120,
                segment_variable: 0x3E68,
                width_argument: 0x12,
                height: 0xB8,
                target: 0x3415,
            },
            ConsumerCase {
                site: 0x1821,
                source: 0x6C00,
                segment_variable: 0x3E68,
                width_argument: 0x15,
                height: 0x40,
                target: 0x33B5,
            },
            ConsumerCase {
                site: 0x18EC,
                source: 0x8100,
                segment_variable: 0x3E68,
                width_argument: 0x12,
                height: 0x80,
                target: 0x3415,
            },
            ConsumerCase {
                site: 0x1A33,
                source: 0x0000,
                segment_variable: 0x3E6A,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1A6F,
                source: 0x6C00,
                segment_variable: 0x3E6A,
                width_argument: 0x03,
                height: 0x10,
                target: 0x33B5,
            },
            ConsumerCase {
                site: 0x1AC2,
                source: 0x6D80,
                segment_variable: 0x3E6A,
                width_argument: 0x01,
                height: 0x10,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1AD9,
                source: 0x6E00,
                segment_variable: 0x3E6A,
                width_argument: 0x01,
                height: 0x10,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1B2B,
                source: 0x6E80,
                segment_variable: 0x3E6A,
                width_argument: 0x02,
                height: 0x38,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1B6E,
                source: 0x7200,
                segment_variable: 0x3E6A,
                width_argument: 0x07,
                height: 0x50,
                target: 0x33B5,
            },
            ConsumerCase {
                site: 0x1B15,
                source: 0x7AC0,
                segment_variable: 0x3E6A,
                width_argument: 0x03,
                height: 0x10,
                target: 0x33B5,
            },
            ConsumerCase {
                site: 0x1B58,
                source: 0x7B80,
                segment_variable: 0x3E6A,
                width_argument: 0x03,
                height: 0x10,
                target: 0x33B5,
            },
            ConsumerCase {
                site: 0x1B85,
                source: 0x0000,
                segment_variable: 0x3E70,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1BC1,
                source: 0x6C00,
                segment_variable: 0x3E70,
                width_argument: 0x01,
                height: 0x10,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1BFD,
                source: 0x6C80,
                segment_variable: 0x3E70,
                width_argument: 0x05,
                height: 0x28,
                target: 0x33B5,
            },
            ConsumerCase {
                site: 0x1C42,
                source: 0x0000,
                segment_variable: 0x3E6E,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1CFF,
                source: 0x6C00,
                segment_variable: 0x3E6E,
                width_argument: 0x04,
                height: 0x38,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1D15,
                source: 0x7300,
                segment_variable: 0x3E6E,
                width_argument: 0x04,
                height: 0x18,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1D2C,
                source: 0x7600,
                segment_variable: 0x3E6E,
                width_argument: 0x04,
                height: 0x60,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1D42,
                source: 0x8200,
                segment_variable: 0x3E6E,
                width_argument: 0x04,
                height: 0x18,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1C7E,
                source: 0x8500,
                segment_variable: 0x3E6E,
                width_argument: 0x04,
                height: 0x18,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1C95,
                source: 0x8800,
                segment_variable: 0x3E6E,
                width_argument: 0x04,
                height: 0x18,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1CAC,
                source: 0x8B00,
                segment_variable: 0x3E6E,
                width_argument: 0x04,
                height: 0x18,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1CE8,
                source: 0x8E00,
                segment_variable: 0x3E6E,
                width_argument: 0x03,
                height: 0x10,
                target: 0x33B5,
            },
            ConsumerCase {
                site: 0x1DB2,
                source: 0x0000,
                segment_variable: 0x3E6C,
                width_argument: 0x14,
                height: 0xC0,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1DC9,
                source: 0x7800,
                segment_variable: 0x3E6C,
                width_argument: 0x07,
                height: 0xC0,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1DDF,
                source: 0xA200,
                segment_variable: 0x3E6C,
                width_argument: 0x11,
                height: 0xC0,
                target: 0x33B5,
            },
            ConsumerCase {
                site: 0x1DF5,
                source: 0xD500,
                segment_variable: 0x3E6C,
                width_argument: 0x09,
                height: 0xC0,
                target: 0x33B5,
            },
            ConsumerCase {
                site: 0x1E47,
                source: 0x0000,
                segment_variable: 0x3E74,
                width_argument: 0x06,
                height: 0xC0,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1E73,
                source: 0x2400,
                segment_variable: 0x3E74,
                width_argument: 0x07,
                height: 0xC0,
                target: 0x33B5,
            },
            ConsumerCase {
                site: 0x1EAF,
                source: 0x3900,
                segment_variable: 0x3E74,
                width_argument: 0x06,
                height: 0x20,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1EC6,
                source: 0x3F00,
                segment_variable: 0x3E74,
                width_argument: 0x06,
                height: 0x20,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1F01,
                source: 0x4500,
                segment_variable: 0x3E74,
                width_argument: 0x03,
                height: 0x18,
                target: 0x33B5,
            },
            ConsumerCase {
                site: 0x1E30,
                source: 0x0000,
                segment_variable: 0x3E72,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x33E5,
            },
        ],
    );

    assert_consumers(
        &overlay,
        &[
            ConsumerCase {
                site: 0x1F3C,
                source: 0x0000,
                segment_variable: 0x3E74,
                width_argument: 0x01,
                height: 0x20,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x206C,
                source: 0x0100,
                segment_variable: 0x3E74,
                width_argument: 0x01,
                height: 0x10,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1F5C,
                source: 0x0180,
                segment_variable: 0x3E74,
                width_argument: 0x02,
                height: 0x10,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x2088,
                source: 0x0280,
                segment_variable: 0x3E74,
                width_argument: 0x01,
                height: 0x10,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1F72,
                source: 0x0300,
                segment_variable: 0x3E74,
                width_argument: 0x01,
                height: 0x10,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1F88,
                source: 0x0380,
                segment_variable: 0x3E74,
                width_argument: 0x05,
                height: 0x50,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1F9E,
                source: 0x1000,
                segment_variable: 0x3E74,
                width_argument: 0x01,
                height: 0x20,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1FB4,
                source: 0x1100,
                segment_variable: 0x3E74,
                width_argument: 0x02,
                height: 0x10,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1FCA,
                source: 0x1200,
                segment_variable: 0x3E74,
                width_argument: 0x02,
                height: 0x20,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1FE0,
                source: 0x1400,
                segment_variable: 0x3E74,
                width_argument: 0x02,
                height: 0x10,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x1FF6,
                source: 0x1500,
                segment_variable: 0x3E74,
                width_argument: 0x03,
                height: 0x10,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x200C,
                source: 0x1680,
                segment_variable: 0x3E74,
                width_argument: 0x0B,
                height: 0x20,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x2022,
                source: 0x2180,
                segment_variable: 0x3E74,
                width_argument: 0x01,
                height: 0x10,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x2038,
                source: 0x2200,
                segment_variable: 0x3E74,
                width_argument: 0x03,
                height: 0x50,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x204E,
                source: 0x2980,
                segment_variable: 0x3E74,
                width_argument: 0x04,
                height: 0x70,
                target: 0x33E5,
            },
            ConsumerCase {
                site: 0x20AF,
                source: 0x3780,
                segment_variable: 0x3E74,
                width_argument: 0x04,
                height: 0x20,
                target: 0x3384,
            },
            ConsumerCase {
                site: 0x20C5,
                source: 0x3980,
                segment_variable: 0x3E74,
                width_argument: 0x05,
                height: 0x40,
                target: 0x3384,
            },
            ConsumerCase {
                site: 0x20DB,
                source: 0x3E80,
                segment_variable: 0x3E74,
                width_argument: 0x02,
                height: 0x30,
                target: 0x3384,
            },
        ],
    );

    assert_consumers(
        &overlay,
        &[
            ConsumerCase {
                site: 0x20F5,
                source: 0x0000,
                segment_variable: 0x3E66,
                width_argument: 0x08,
                height: 0x30,
                target: 0x3384,
            },
            ConsumerCase {
                site: 0x210B,
                source: 0x0600,
                segment_variable: 0x3E66,
                width_argument: 0x0A,
                height: 0x70,
                target: 0x3384,
            },
            ConsumerCase {
                site: 0x2125,
                source: 0x1780,
                segment_variable: 0x3E66,
                width_argument: 0x09,
                height: 0x30,
                target: 0x3384,
            },
            ConsumerCase {
                site: 0x213B,
                source: 0x1E40,
                segment_variable: 0x3E66,
                width_argument: 0x0B,
                height: 0x70,
                target: 0x3384,
            },
            ConsumerCase {
                site: 0x2155,
                source: 0x3180,
                segment_variable: 0x3E66,
                width_argument: 0x0D,
                height: 0xA0,
                target: 0x3384,
            },
            ConsumerCase {
                site: 0x216F,
                source: 0x5200,
                segment_variable: 0x3E66,
                width_argument: 0x02,
                height: 0x40,
                target: 0x3384,
            },
            ConsumerCase {
                site: 0x2185,
                source: 0x5400,
                segment_variable: 0x3E66,
                width_argument: 0x02,
                height: 0x50,
                target: 0x3384,
            },
            ConsumerCase {
                site: 0x219B,
                source: 0x5680,
                segment_variable: 0x3E66,
                width_argument: 0x02,
                height: 0x80,
                target: 0x3384,
            },
            ConsumerCase {
                site: 0x21B1,
                source: 0x5A80,
                segment_variable: 0x3E66,
                width_argument: 0x02,
                height: 0x90,
                target: 0x3384,
            },
            ConsumerCase {
                site: 0x21C7,
                source: 0x5F00,
                segment_variable: 0x3E66,
                width_argument: 0x13,
                height: 0xA0,
                target: 0x3384,
            },
            ConsumerCase {
                site: 0x21DD,
                source: 0x8E80,
                segment_variable: 0x3E66,
                width_argument: 0x02,
                height: 0x90,
                target: 0x3384,
            },
            ConsumerCase {
                site: 0x21F3,
                source: 0x9300,
                segment_variable: 0x3E66,
                width_argument: 0x02,
                height: 0x70,
                target: 0x3384,
            },
            ConsumerCase {
                site: 0x2210,
                source: 0x9680,
                segment_variable: 0x3E66,
                width_argument: 0x06,
                height: 0x10,
                target: 0x3384,
            },
            ConsumerCase {
                site: 0x2226,
                source: 0x9800,
                segment_variable: 0x3E66,
                width_argument: 0x08,
                height: 0x10,
                target: 0x3384,
            },
            ConsumerCase {
                site: 0x223C,
                source: 0x9A00,
                segment_variable: 0x3E66,
                width_argument: 0x06,
                height: 0x10,
                target: 0x3384,
            },
            ConsumerCase {
                site: 0x2252,
                source: 0x9B80,
                segment_variable: 0x3E66,
                width_argument: 0x04,
                height: 0x40,
                target: 0x3384,
            },
            ConsumerCase {
                site: 0x2268,
                source: 0x9F80,
                segment_variable: 0x3E66,
                width_argument: 0x0A,
                height: 0x10,
                target: 0x3384,
            },
            ConsumerCase {
                site: 0x227E,
                source: 0xA200,
                segment_variable: 0x3E66,
                width_argument: 0x1C,
                height: 0x60,
                target: 0x3384,
            },
            ConsumerCase {
                site: 0x2294,
                source: 0xCC00,
                segment_variable: 0x3E66,
                width_argument: 0x0A,
                height: 0x10,
                target: 0x3384,
            },
            ConsumerCase {
                site: 0x22AA,
                source: 0xCE80,
                segment_variable: 0x3E66,
                width_argument: 0x06,
                height: 0x20,
                target: 0x3384,
            },
            ConsumerCase {
                site: 0x22C0,
                source: 0xD180,
                segment_variable: 0x3E66,
                width_argument: 0x0A,
                height: 0x40,
                target: 0x3384,
            },
            ConsumerCase {
                site: 0x22D6,
                source: 0xDB80,
                segment_variable: 0x3E66,
                width_argument: 0x02,
                height: 0x30,
                target: 0x3384,
            },
            ConsumerCase {
                site: 0x22EC,
                source: 0xDD00,
                segment_variable: 0x3E66,
                width_argument: 0x04,
                height: 0x20,
                target: 0x3384,
            },
            ConsumerCase {
                site: 0x2302,
                source: 0xDF00,
                segment_variable: 0x3E66,
                width_argument: 0x02,
                height: 0x10,
                target: 0x3384,
            },
        ],
    );

    // Segment 0x3E6A belongs to RE9 throughout its phase. Every direct load
    // except 0x1005 has an immediate SI, and the one dynamic consumer draws
    // at most 0x140 bytes from a source record selected by the exact table at
    // 0x2821. The complete record union avoids 0x6CC0..0x6D80.
    let sources_3e6a = immediate_sources_for_segment(&overlay, 0x3E6A);
    assert_eq!(
        sources_3e6a,
        [
            (0x1A33, 0x0000),
            (0x1A4A, 0x0A35),
            (0x1A6F, 0x6C00),
            (0x1A86, 0x6C00),
            (0x1A9D, 0x10E9),
            (0x1AC2, 0x6D80),
            (0x1AD9, 0x6E00),
            (0x1AF0, 0x0D86),
            (0x1B15, 0x7AC0),
            (0x1B2B, 0x6E80),
            (0x1B42, 0x0000),
            (0x1B58, 0x7B80),
            (0x1B6E, 0x7200),
        ]
    );
    let immediate_3e6a_sites = sources_3e6a
        .iter()
        .map(|(site, _)| site + 4)
        .collect::<Vec<_>>();
    let dynamic_3e6a_sites = occurrence_sites(&overlay, &[0x2E, 0x8E, 0x1E, 0x6A, 0x3E])
        .into_iter()
        .filter(|site| !immediate_3e6a_sites.contains(site))
        .collect::<Vec<_>>();
    assert_eq!(dynamic_3e6a_sites, [0x1005]);

    assert_eq!(
        logical_slice(&overlay, 0x0F49, 6),
        [0x2E, 0xC7, 0x46, 0x11, 0x10, 0x00]
    );
    let dynamic_re9_consumer = logical_slice(&overlay, 0x0FF6, 0x1F);
    assert_eq!(
        dynamic_re9_consumer,
        [
            0x2E, 0x8B, 0x46, 0x0B, 0x2E, 0x8B, 0x5E, 0x0D, 0x2E, 0x8B, 0x4E, 0x11, 0x1E, 0x8B,
            0xF3, 0x2E, 0x8E, 0x1E, 0x6A, 0x3E, 0x8B, 0xF8, 0xBA, 0x0A, 0x00, 0x8B, 0xC9, 0xE8,
            0x54, 0x25, 0x1F,
        ]
    );
    assert_eq!(call_target(0x0FF6, dynamic_re9_consumer), 0x3568);
    assert_eq!(
        logical_slice(&overlay, 0x3568, 0x14),
        [
            0xFC, 0xB8, 0x00, 0xB0, 0x8E, 0xC0, 0x51, 0x57, 0x8B, 0xCA, 0xF3, 0xA5, 0x5F, 0x59,
            0x83, 0xC7, 0x50, 0xE2, 0xF3, 0xC3,
        ]
    );
    assert!(contains(
        logical_slice(&overlay, 0x0F5F, 0x34),
        &[0xBB, 0x21, 0x28, 0x2E, 0x03, 0x1E, 0x85, 0x3E]
    ));

    let state_table = (0..376)
        .map(|index| {
            u16::from_le_bytes(
                logical_slice(&overlay, 0x2821 + index * 2, 2)
                    .try_into()
                    .unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(state_table.last(), Some(&0xFFFF));
    assert!(!state_table[..state_table.len() - 1].contains(&0xFFFF));

    let mut referenced_records = state_table
        .iter()
        .copied()
        .filter(|address| *address != 0 && *address != 0xFFFF)
        .collect::<Vec<_>>();
    referenced_records.sort_unstable();
    referenced_records.dedup();
    assert_eq!(
        referenced_records,
        (0..=64).map(|index| 0x2B11 + index * 8).collect::<Vec<_>>()
    );
    for address in referenced_records {
        let record = logical_slice(&overlay, usize::from(address), 8);
        let source = u16::from_le_bytes([record[0], record[1]]);
        let end = usize::from(source) + 0x0140;
        assert!(
            end <= 0x6CC0 || usize::from(source) >= 0x6D80,
            "RE9 state record 0x{address:04X} reaches the directly-unconsumed gap"
        );
    }
    assert_eq!(
        logical_slice(&overlay, 0x2D11, 8),
        [0xC0, 0x7B, 0x00, 0x00, 0x1E, 0x78, 0xFF, 0xFF]
    );

    // Segment 0x3E6E is reused: RE1 occupies it at initial load, then RE15
    // replaces it at 0x0AEB. Its only dynamic source routine is referenced by
    // the two RE1-phase calls below. After the RE15 reload every DS load has a
    // fixed SI, and the last fixed source ends exactly at 0x8EC0.
    let sources_3e6e = immediate_sources_for_segment(&overlay, 0x3E6E);
    assert_eq!(
        sources_3e6e,
        [
            (0x1494, 0x0000),
            (0x14E1, 0x1352),
            (0x1506, 0xCC00),
            (0x151D, 0xCD20),
            (0x1C42, 0x0000),
            (0x1C59, 0x0A2E),
            (0x1C7E, 0x8500),
            (0x1C95, 0x8800),
            (0x1CAC, 0x8B00),
            (0x1CC3, 0x0EB0),
            (0x1CE8, 0x8E00),
            (0x1CFF, 0x6C00),
            (0x1D15, 0x7300),
            (0x1D2C, 0x7600),
            (0x1D42, 0x8200),
            (0x1D59, 0x8800),
            (0x1D6F, 0x6C00),
            (0x1D85, 0x7300),
            (0x1D9B, 0x8E00),
        ]
    );
    let immediate_3e6e_sites = sources_3e6e
        .iter()
        .map(|(site, _)| site + 4)
        .collect::<Vec<_>>();
    let dynamic_3e6e_sites = occurrence_sites(&overlay, &[0x2E, 0x8E, 0x1E, 0x6E, 0x3E])
        .into_iter()
        .filter(|site| !immediate_3e6e_sites.contains(site))
        .collect::<Vec<_>>();
    assert_eq!(dynamic_3e6e_sites, [0x14C0]);
    assert_eq!(near_call_sites_to(&overlay, 0x14B9), [0x05D0, 0x05EA]);
    assert!(
        near_call_sites_to(&overlay, 0x14B9)
            .iter()
            .all(|site| *site < 0x0AEB)
    );
    assert_eq!(
        logical_slice(&overlay, 0x05C3, 0x18),
        [
            0x2E, 0xC7, 0x46, 0x0B, 0x00, 0x00, 0xE8, 0xD3, 0x27, 0x2E, 0xFF, 0x46, 0x0B, 0xE8,
            0xE6, 0x0E, 0x2E, 0x83, 0x7E, 0x0B, 0x20, 0x74, 0x01, 0xC3,
        ]
    );
    assert_eq!(
        logical_slice(&overlay, 0x05DB, 0x12),
        [
            0x2E, 0xC7, 0x46, 0x0D, 0x00, 0x00, 0xE8, 0xBB, 0x27, 0x2E, 0xC7, 0x46, 0x0B, 0x20,
            0x00, 0xE8, 0xCC, 0x0E,
        ]
    );

    let raw_dynamic_references = occurrence_sites(&overlay, &[0xB9, 0x14]);
    assert_eq!(
        raw_dynamic_references,
        [0x2340, 0x2420, 0x243C, 0x2490, 0x2500, 0x251C, 0x258C]
    );
    for site in raw_dynamic_references {
        assert_eq!(logical_slice(&overlay, site, 3), [0xB9, 0x14, 0x00]);
    }

    let re15_phase_sources = sources_3e6e
        .iter()
        .filter(|(site, _)| *site >= 0x1C42)
        .map(|(_, source)| *source)
        .collect::<Vec<_>>();
    assert_eq!(
        re15_phase_sources,
        [
            0x0000, 0x0A2E, 0x8500, 0x8800, 0x8B00, 0x0EB0, 0x8E00, 0x6C00, 0x7300, 0x7600, 0x8200,
            0x8800, 0x6C00, 0x7300, 0x8E00,
        ]
    );
}
