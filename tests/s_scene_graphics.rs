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

fn occurrence_count(bytes: &[u8], signature: &[u8]) -> usize {
    bytes
        .windows(signature.len())
        .filter(|window| *window == signature)
        .count()
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

fn call_target(site: usize, consumer: &[u8]) -> usize {
    let call_index = consumer
        .windows(3)
        .position(|window| window[0] == 0xE8)
        .expect("consumer call");
    let displacement = i16::from_le_bytes([consumer[call_index + 1], consumer[call_index + 2]]);
    usize::try_from((site + call_index + 3) as isize + displacement as isize).unwrap()
}

fn assert_consumer(overlay: &[u8], case: ConsumerCase) {
    let consumer = logical_slice(overlay, case.site, 0x40);
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
    assert!(
        contains(consumer, &dimensions),
        "dimensions at 0x{:04X}",
        case.site
    );
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
    assert!(contains(loader, &open), "loader at 0x{site:04X}");

    let mut decode = vec![0x2E, 0x8E, 0x06];
    decode.extend_from_slice(&segment_variable.to_le_bytes());
    decode.extend_from_slice(&[0xBF, 0x00, 0x00, 0xB4, 0x03, 0xCD, 0x7C]);
    assert!(contains(loader, &decode), "loader at 0x{site:04X}");
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn exact_s_resources_render_with_consumer_proven_scene_layouts() {
    let Some(disk) = common::try_read(SCHEZO_DATA) else {
        return;
    };
    let cases = [
        ResourceCase {
            name: "S1.DAT",
            packed_size: 46_728,
            packed_sha256: "2e41896f79f47121f12b8b97f557a1238c91efd811f3f04e988bfedbcf32db39",
            decoded_size: 62_976,
            decoded_sha256: "bcbdf7af3344a37f89358e95783f2e506c2bb82010b48e7a24d04700250760bc",
            rgb_sha256: "05844eaba87c337f9703b02dd8dfef9baf135bc617f92996b70f35992c03b7dc",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "S2.DAT",
            packed_size: 43_936,
            packed_sha256: "7e3315444ba012477b7605a38ff8e5c71763e8adae3747ff157579fc99578e2d",
            decoded_size: 53_760,
            decoded_sha256: "5827324987ae90fcfbc0c12a48725f8654094043f5a05551cdfeb4b17431d4df",
            rgb_sha256: "0726715fe302009eca72b99615eced31d64ef612d885eaa78f9251319840bbec",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "S3.DAT",
            packed_size: 40_467,
            packed_sha256: "b07e13acb6e1832c4eab9470c013605f4150e870da8fb7d4f69f6cb8f3595ef2",
            decoded_size: 47_360,
            decoded_sha256: "d2e1f5eaa86f2c7bb2e10e9c00331eccdb5e20b371e746405cb8c1a9e49c3469",
            rgb_sha256: "38ea3a35779b6c8280d89c7ff1526d68566582284ce28dd5ff4796e4d99ed14e",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "S7.DAT",
            packed_size: 22_494,
            packed_sha256: "5f699b62c4086c8d40fe9a5d3e9a7233617eb6434c13966b2995f4971fb81e3b",
            decoded_size: 57_408,
            decoded_sha256: "30e86f85496290a616a2ec39a0d099e3387fac24bb2edcc113da2d7cf2ad0269",
            rgb_sha256: "9ff2118fbeb9384641d0f81f4d172bad7178c7bb1875906d19e8faf05449c764",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "S8.DAT",
            packed_size: 24_404,
            packed_sha256: "70b6014134136de6dbaa2cf697beb844e234fdf54c7e510ab0cf6f8488d231ec",
            decoded_size: 29_952,
            decoded_sha256: "a7b45dba8c867651293966fbcb2d6d56b8d2bc76176b101d5763ca42f6547451",
            rgb_sha256: "16de2343a21ba24bc354b6d3fc6af8b6353d41487efc1c2d3dec6dfd5843514e",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "S9.DAT",
            packed_size: 41_324,
            packed_sha256: "b4d00b67c0e65c721be943b91238dd8dddf47aeaff0002cef759447aef8e421b",
            decoded_size: 59_136,
            decoded_sha256: "913a12b5ee323fbf36f78ba5ac925c01ec11c9d320c81ea37780a1218338b926",
            rgb_sha256: "c3881d8ddc6a01bfd4368dd375021475db61c026947662c3cbcd5de016ce6df2",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "S11_1.DAT",
            packed_size: 39_998,
            packed_sha256: "f08f5580eb86fec31b5f94efa113734fdf8508d51344585608204b4281d0d24a",
            decoded_size: 63_488,
            decoded_sha256: "5a100cab1d30349134f528e5b4c74e5f3ffc36a26be67d8990420dccd14130ea",
            rgb_sha256: "66a92565eebf0ace07d7db3f4a5e77afdacc4f3c1867f9da181b314ccb5101c9",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "S11_2.DAT",
            packed_size: 31_371,
            packed_sha256: "88b619b809764d55c135fde803282e161a0284237cd1318ebc73358df084eadb",
            decoded_size: 60_672,
            decoded_sha256: "d1422765325866f72ba454cd2b5c8044aab694635f079e48f9777c46c7075bfe",
            rgb_sha256: "5879a9b5fbbbb76fddb7dc5f2c928549a95afd2c1b267fa5bba9e8ef28b43176",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "S13.DAT",
            packed_size: 12_586,
            packed_sha256: "185e86161fa566a740920a0ea58c91f997ff5dfc011a953dded9b08e272ae3d1",
            decoded_size: 21_120,
            decoded_sha256: "47090c897b8691e73a7fc7aa6ff9c572fdc5dbcbed4b396afd06f8099bde93c0",
            rgb_sha256: "7849736250db2a2f942dd4d77d0ee892cb6519ad1e776d63338b185d49a8ba2a",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "S15.DAT",
            packed_size: 22_995,
            packed_sha256: "415e29a5479117bf4c32d557dccebb7d42b566192e5c138d7b910e0b23deb01d",
            decoded_size: 42_432,
            decoded_sha256: "4ea8724a0f51544ca004d26ff6d6cb2ac9a6013c17e0d471d3d265a0e1caca68",
            rgb_sha256: "576e094da19fd8d421dfb29f74424bf4643d38330debc390a3690d158d402b79",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "S16.DAT",
            packed_size: 16_468,
            packed_sha256: "1bf2ee9963c8addbac0ed2d5eafe6bdae325d60b548fa6b22045918d4ce99a42",
            decoded_size: 21_024,
            decoded_sha256: "0be19fd7de4cd41dfd21e51dad113b9c8770a8d4b4bb6eb7c0fbb9fcce2b4b8d",
            rgb_sha256: "95511e7a9182e9d1feb1c6e2cf2f838e23377ca62dc0168649eeff9f0f428c0e",
            unrendered_ranges: &[DecodedRange {
                offset: 0x51A0,
                bytes: 0x0080,
            }],
        },
        ResourceCase {
            name: "S18.DAT",
            packed_size: 30_522,
            packed_sha256: "37268d8c6ea95c64128ef4dae703339dd691401a62e09c4c6a32323d4687bbfb",
            decoded_size: 56_544,
            decoded_sha256: "8df5516f7e6d59a8aa18fec2c0cf20d04b04e9901f86ab7ffd04c5d005ee96ce",
            rgb_sha256: "65e1169bb4991247c5077bbb9821679000061d5a5c8bd3442a411a5e93cc4d00",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "S20.DAT",
            packed_size: 12_757,
            packed_sha256: "3cdd5458da334eceea8c8e90d0f441f45323e28478c0c12d49b118e50b02e71d",
            decoded_size: 56_704,
            decoded_sha256: "a50031854b2ad3a6e8d4635c018b05cfff298bd0de813ef1133c514882e7fea1",
            rgb_sha256: "35b15080d0ba42a444f680e3742665919d0ec284a136861935dac6a9fbf664c0",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "S21.DAT",
            packed_size: 1_707,
            packed_sha256: "3eff5cd7f3d9a0a6c5dbeb12627036b1b0e80dbeff8f4923c61a78a84e0be616",
            decoded_size: 8_960,
            decoded_sha256: "e16a61bb4a68915922e40101da178cae763aef711f0052df17355a87f7e44535",
            rgb_sha256: "10cc9c570f4334e8cf7a3d045064ffa519878327e5d004bd089514d3b5d296c9",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "S1316.DAT",
            packed_size: 33_537,
            packed_sha256: "16a452beb9719bbb608212e2704c22a82ba82f5bf787bc88ec9cfcf25676ac50",
            decoded_size: 39_936,
            decoded_sha256: "6a9556535892e33094fb15c467ba417bc520ba25b92f8a6622536f455e6760da",
            rgb_sha256: "e0d773b621da6ee89db675f140fe05af9fc8138ad38aa7e53ff7f48d2a57f940",
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
fn shezo_op_loads_and_consumes_all_fifteen_s_resources() {
    let Some(disk) = common::try_read(SCHEZO_GAME) else {
        return;
    };
    let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "SHEZO_OP.OVL").unwrap();
    assert_eq!(packed.len(), 12_528);
    assert_eq!(
        pc98_madou_ars::media_identity::sha256_hex(&packed),
        "fa6e8494143e783a446f0a612a8a0c8b070bc6f7ef050a2c3c335a11687885b2"
    );
    let report = pc98_madou_ars::overlay_lz::decode_overlay_lz(&packed).unwrap();
    assert_eq!(report.bytes_consumed, packed.len());
    assert_eq!(report.output.len(), 22_511);
    assert_eq!(
        pc98_madou_ars::media_identity::sha256_hex(&report.output),
        "71f784f394d97a6252d051b7baf99d95b3f603810cdc43290af6b5fc1a9b1aa0"
    );
    let overlay = report.output;

    for (address, name) in [
        (0x0227, b"s1.dat\0".as_slice()),
        (0x022E, b"s2.dat\0".as_slice()),
        (0x0235, b"s3.dat\0".as_slice()),
        (0x023C, b"s7.dat\0".as_slice()),
        (0x0243, b"s8.dat\0".as_slice()),
        (0x024A, b"s9.dat\0".as_slice()),
        (0x0251, b"s11_1.dat\0".as_slice()),
        (0x025B, b"s11_2.dat\0".as_slice()),
        (0x0265, b"s1316.dat\0".as_slice()),
        (0x026F, b"s13.dat\0".as_slice()),
        (0x0277, b"s15.dat\0".as_slice()),
        (0x027F, b"s16.dat\0".as_slice()),
        (0x0287, b"s18.dat\0".as_slice()),
        (0x028F, b"s20.dat\0".as_slice()),
        (0x0297, b"s21.dat\0".as_slice()),
    ] {
        assert_eq!(logical_slice(&overlay, address, name.len()), name);
    }

    for (site, filename, destination) in [
        (0x03CF, 0x0227u16, 0x4F86u16),
        (0x040B, 0x022E, 0x4F88),
        (0x0447, 0x0235, 0x4F8A),
        (0x0483, 0x023C, 0x4F8C),
        (0x04BF, 0x0243, 0x4F8E),
        (0x04FB, 0x024A, 0x4F90),
        (0x0A87, 0x025B, 0x4F90),
        (0x0B5F, 0x0251, 0x4F86),
        (0x0C7F, 0x0265, 0x4F86),
        (0x0CBB, 0x026F, 0x4F88),
        (0x0E49, 0x0277, 0x4F8A),
        (0x0E85, 0x027F, 0x4F8C),
        (0x100C, 0x0287, 0x4F86),
        (0x10F1, 0x028F, 0x4F8C),
        (0x112D, 0x0297, 0x4F8E),
    ] {
        assert_loader(&overlay, site, filename, destination);
    }

    let row_interleaved = logical_slice(&overlay, 0x4494, 0x30);
    assert!(contains(
        row_interleaved,
        &[
            0xAC, 0xE6, 0x7E, 0x8A, 0xE0, 0xAC, 0xE6, 0x7E, 0x0A, 0xE0, 0xAC, 0xE6, 0x7E, 0x0A,
            0xE0, 0xAC, 0xE6, 0x7E,
        ]
    ));
    assert!(contains(row_interleaved, &[0xAA, 0xE2, 0xE9]));
    for (site, opcode) in [(0x44C5, 0xA4), (0x44F5, 0xA5)] {
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
    let strided_word = logical_slice(&overlay, 0x4525, 0x40);
    assert!(contains(strided_word, &[0xF3, 0xA5]));
    assert!(contains(strided_word, &[0x2E, 0x03, 0x36, 0xAB, 0x4F]));
    assert!(contains(strided_word, &[0x2E, 0x03, 0x36, 0xAF, 0x4F]));
    let strided_interleaved = logical_slice(&overlay, 0x4564, 0x38);
    assert!(contains(
        strided_interleaved,
        &[0x2E, 0x03, 0x36, 0xAB, 0x4F]
    ));
    assert!(contains(strided_interleaved, &[0xAC, 0xE6, 0x7E]));

    assert_consumers(
        &overlay,
        &[
            ConsumerCase {
                site: 0x0AF1,
                source: 0x0004,
                segment_variable: 0x4F86,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x4525,
            },
            ConsumerCase {
                site: 0x0B15,
                source: 0x9600,
                segment_variable: 0x4F86,
                width_argument: 0x10,
                height: 0x60,
                target: 0x4564,
            },
            ConsumerCase {
                site: 0x2C9C,
                source: 0xAE00,
                segment_variable: 0x4F86,
                width_argument: 0x10,
                height: 0x60,
                target: 0x4564,
            },
            ConsumerCase {
                site: 0x2CDE,
                source: 0xC600,
                segment_variable: 0x4F86,
                width_argument: 0x10,
                height: 0x60,
                target: 0x4564,
            },
            ConsumerCase {
                site: 0x1F08,
                source: 0xDE00,
                segment_variable: 0x4F86,
                width_argument: 0x10,
                height: 0x60,
                target: 0x4564,
            },
            ConsumerCase {
                site: 0x06A6,
                source: 0x0680,
                segment_variable: 0x4F88,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x4525,
            },
            ConsumerCase {
                site: 0x07CB,
                source: 0xB600,
                segment_variable: 0x4F88,
                width_argument: 0x06,
                height: 0x90,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x1FE4,
                source: 0xD100,
                segment_variable: 0x4F88,
                width_argument: 0x01,
                height: 0x10,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x1FFB,
                source: 0xD180,
                segment_variable: 0x4F88,
                width_argument: 0x01,
                height: 0x10,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x0717,
                source: 0x0000,
                segment_variable: 0x4F8A,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x4525,
            },
            ConsumerCase {
                site: 0x2359,
                source: 0x8400,
                segment_variable: 0x4F8A,
                width_argument: 0x07,
                height: 0x18,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x2370,
                source: 0x86A0,
                segment_variable: 0x4F8A,
                width_argument: 0x07,
                height: 0x18,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x2387,
                source: 0x8940,
                segment_variable: 0x4F8A,
                width_argument: 0x07,
                height: 0x18,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x23C3,
                source: 0x8BE0,
                segment_variable: 0x4F8A,
                width_argument: 0x07,
                height: 0x30,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x23DA,
                source: 0x9120,
                segment_variable: 0x4F8A,
                width_argument: 0x07,
                height: 0x30,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x23F1,
                source: 0x9660,
                segment_variable: 0x4F8A,
                width_argument: 0x07,
                height: 0x30,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x2444,
                source: 0x9BA0,
                segment_variable: 0x4F8A,
                width_argument: 0x06,
                height: 0x50,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x245B,
                source: 0xAAA0,
                segment_variable: 0x4F8A,
                width_argument: 0x05,
                height: 0x20,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x242D,
                source: 0xAD20,
                segment_variable: 0x4F8A,
                width_argument: 0x07,
                height: 0x28,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x2488,
                source: 0xB180,
                segment_variable: 0x4F8A,
                width_argument: 0x08,
                height: 0x10,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x24B5,
                source: 0xB380,
                segment_variable: 0x4F8A,
                width_argument: 0x08,
                height: 0x10,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x2544,
                source: 0xB580,
                segment_variable: 0x4F8A,
                width_argument: 0x01,
                height: 0x10,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x25D3,
                source: 0xB5C0,
                segment_variable: 0x4F8A,
                width_argument: 0x02,
                height: 0x18,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x25EA,
                source: 0xB680,
                segment_variable: 0x4F8A,
                width_argument: 0x02,
                height: 0x18,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x2626,
                source: 0xB740,
                segment_variable: 0x4F8A,
                width_argument: 0x03,
                height: 0x10,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x2580,
                source: 0xB800,
                segment_variable: 0x4F8A,
                width_argument: 0x02,
                height: 0x10,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x2597,
                source: 0xB880,
                segment_variable: 0x4F8A,
                width_argument: 0x02,
                height: 0x10,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x0907,
                source: 0x0000,
                segment_variable: 0x4F8C,
                width_argument: 0x17,
                height: 0xC0,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x2673,
                source: 0x4500,
                segment_variable: 0x4F8C,
                width_argument: 0x04,
                height: 0x20,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x268A,
                source: 0x4900,
                segment_variable: 0x4F8C,
                width_argument: 0x07,
                height: 0x30,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x26A1,
                source: 0x4E40,
                segment_variable: 0x4F8C,
                width_argument: 0x17,
                height: 0xC0,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x26CE,
                source: 0x9340,
                segment_variable: 0x4F8C,
                width_argument: 0x04,
                height: 0x40,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x26B8,
                source: 0x9B40,
                segment_variable: 0x4F8C,
                width_argument: 0x17,
                height: 0xC0,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x096A,
                source: 0x0000,
                segment_variable: 0x4F8E,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x2740,
                source: 0x6C00,
                segment_variable: 0x4F8E,
                width_argument: 0x03,
                height: 0x30,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x2757,
                source: 0x7080,
                segment_variable: 0x4F8E,
                width_argument: 0x03,
                height: 0x30,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x0A2E,
                source: 0x0000,
                segment_variable: 0x4F90,
                width_argument: 0x0E,
                height: 0x40,
                target: 0x4494,
            },
            ConsumerCase {
                site: 0x0A44,
                source: 0x0E00,
                segment_variable: 0x4F90,
                width_argument: 0x16,
                height: 0x60,
                target: 0x4494,
            },
            ConsumerCase {
                site: 0x2802,
                source: 0x2F00,
                segment_variable: 0x4F90,
                width_argument: 0x0E,
                height: 0x40,
                target: 0x4494,
            },
            ConsumerCase {
                site: 0x2818,
                source: 0x3D00,
                segment_variable: 0x4F90,
                width_argument: 0x16,
                height: 0x60,
                target: 0x4494,
            },
            ConsumerCase {
                site: 0x2853,
                source: 0x5E00,
                segment_variable: 0x4F90,
                width_argument: 0x0E,
                height: 0x40,
                target: 0x4494,
            },
            ConsumerCase {
                site: 0x2869,
                source: 0x6C00,
                segment_variable: 0x4F90,
                width_argument: 0x14,
                height: 0x60,
                target: 0x4564,
            },
            ConsumerCase {
                site: 0x0A18,
                source: 0x8D00,
                segment_variable: 0x4F90,
                width_argument: 0x12,
                height: 0xA0,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x0BE7,
                source: 0x0000,
                segment_variable: 0x4F86,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x0BFD,
                source: 0x6C00,
                segment_variable: 0x4F86,
                width_argument: 0x1C,
                height: 0xA0,
                target: 0x4494,
            },
            ConsumerCase {
                site: 0x2E1A,
                source: 0xB200,
                segment_variable: 0x4F86,
                width_argument: 0x1C,
                height: 0xA0,
                target: 0x4494,
            },
            ConsumerCase {
                site: 0x2E5D,
                source: 0x0000,
                segment_variable: 0x4F90,
                width_argument: 0x1C,
                height: 0xA0,
                target: 0x4494,
            },
            ConsumerCase {
                site: 0x2EA0,
                source: 0x4600,
                segment_variable: 0x4F90,
                width_argument: 0x1C,
                height: 0xA0,
                target: 0x4494,
            },
            ConsumerCase {
                site: 0x2E73,
                source: 0x8C00,
                segment_variable: 0x4F90,
                width_argument: 0x06,
                height: 0x90,
                target: 0x4494,
            },
            ConsumerCase {
                site: 0x2EB6,
                source: 0x9980,
                segment_variable: 0x4F90,
                width_argument: 0x06,
                height: 0x90,
                target: 0x4494,
            },
            ConsumerCase {
                site: 0x2EF9,
                source: 0xA700,
                segment_variable: 0x4F90,
                width_argument: 0x06,
                height: 0x90,
                target: 0x4494,
            },
            ConsumerCase {
                site: 0x2E30,
                source: 0xB480,
                segment_variable: 0x4F90,
                width_argument: 0x06,
                height: 0x90,
                target: 0x4494,
            },
            ConsumerCase {
                site: 0x3271,
                source: 0xC200,
                segment_variable: 0x4F90,
                width_argument: 0x06,
                height: 0x90,
                target: 0x4494,
            },
            ConsumerCase {
                site: 0x3315,
                source: 0xCF80,
                segment_variable: 0x4F90,
                width_argument: 0x06,
                height: 0x98,
                target: 0x4494,
            },
            ConsumerCase {
                site: 0x3342,
                source: 0xDDC0,
                segment_variable: 0x4F90,
                width_argument: 0x06,
                height: 0x98,
                target: 0x4494,
            },
            ConsumerCase {
                site: 0x0D7C,
                source: 0x0010,
                segment_variable: 0x4F86,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x4525,
            },
            ConsumerCase {
                site: 0x33FE,
                source: 0x0000,
                segment_variable: 0x4F88,
                width_argument: 0x0A,
                height: 0x80,
                target: 0x4564,
            },
            ConsumerCase {
                site: 0x3440,
                source: 0x1400,
                segment_variable: 0x4F88,
                width_argument: 0x0A,
                height: 0x80,
                target: 0x4564,
            },
            ConsumerCase {
                site: 0x3482,
                source: 0x2800,
                segment_variable: 0x4F88,
                width_argument: 0x0A,
                height: 0x80,
                target: 0x4564,
            },
            ConsumerCase {
                site: 0x3769,
                source: 0x3C00,
                segment_variable: 0x4F88,
                width_argument: 0x0A,
                height: 0x80,
                target: 0x4564,
            },
            ConsumerCase {
                site: 0x36F3,
                source: 0x5000,
                segment_variable: 0x4F88,
                width_argument: 0x04,
                height: 0x10,
                target: 0x4494,
            },
            ConsumerCase {
                site: 0x372E,
                source: 0x5100,
                segment_variable: 0x4F88,
                width_argument: 0x04,
                height: 0x10,
                target: 0x4494,
            },
            ConsumerCase {
                site: 0x37ED,
                source: 0x5200,
                segment_variable: 0x4F88,
                width_argument: 0x02,
                height: 0x10,
                target: 0x4494,
            },
            ConsumerCase {
                site: 0x0EDB,
                source: 0x0000,
                segment_variable: 0x4F8A,
                width_argument: 0x02,
                height: 0xC0,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x0EF1,
                source: 0x0C00,
                segment_variable: 0x4F8A,
                width_argument: 0x1B,
                height: 0x20,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x0F07,
                source: 0x1980,
                segment_variable: 0x4F8A,
                width_argument: 0x05,
                height: 0xC0,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x0F1D,
                source: 0x2880,
                segment_variable: 0x4F8A,
                width_argument: 0x1B,
                height: 0x30,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x0F33,
                source: 0x3CC0,
                segment_variable: 0x4F8A,
                width_argument: 0x02,
                height: 0x70,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x0F49,
                source: 0x43C0,
                segment_variable: 0x4F8A,
                width_argument: 0x03,
                height: 0x70,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x11D7,
                source: 0x4900,
                segment_variable: 0x4F8A,
                width_argument: 0x1B,
                height: 0x30,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x3A27,
                source: 0x5D40,
                segment_variable: 0x4F8A,
                width_argument: 0x05,
                height: 0x50,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x3A3D,
                source: 0x69C0,
                segment_variable: 0x4F8A,
                width_argument: 0x08,
                height: 0x20,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x3A66,
                source: 0x71C0,
                segment_variable: 0x4F8A,
                width_argument: 0x06,
                height: 0x40,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x3A8F,
                source: 0x7DC0,
                segment_variable: 0x4F8A,
                width_argument: 0x04,
                height: 0x10,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x0F5F,
                source: 0x7FC0,
                segment_variable: 0x4F8A,
                width_argument: 0x0A,
                height: 0x78,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x3848,
                source: 0xA540,
                segment_variable: 0x4F8A,
                width_argument: 0x01,
                height: 0x10,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x385F,
                source: 0xA580,
                segment_variable: 0x4F8A,
                width_argument: 0x01,
                height: 0x10,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x0FF1,
                source: 0x0000,
                segment_variable: 0x4F8C,
                width_argument: 0x05,
                height: 0x80,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x38B7,
                source: 0x1400,
                segment_variable: 0x4F8C,
                width_argument: 0x02,
                height: 0x10,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x38CE,
                source: 0x1500,
                segment_variable: 0x4F8C,
                width_argument: 0x02,
                height: 0x10,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x38E5,
                source: 0x1600,
                segment_variable: 0x4F8C,
                width_argument: 0x0B,
                height: 0x10,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x38FC,
                source: 0x18C0,
                segment_variable: 0x4F8C,
                width_argument: 0x0B,
                height: 0x28,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x3916,
                source: 0x1FA0,
                segment_variable: 0x4F8C,
                width_argument: 0x0D,
                height: 0x80,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x393F,
                source: 0x39A0,
                segment_variable: 0x4F8C,
                width_argument: 0x03,
                height: 0x30,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x3955,
                source: 0x3E20,
                segment_variable: 0x4F8C,
                width_argument: 0x01,
                height: 0x90,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x396C,
                source: 0x42A0,
                segment_variable: 0x4F8C,
                width_argument: 0x05,
                height: 0x60,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x10A5,
                source: 0x0000,
                segment_variable: 0x4F86,
                width_argument: 0x19,
                height: 0xA8,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x39E7,
                source: 0x41A0,
                segment_variable: 0x4F86,
                width_argument: 0x19,
                height: 0xA8,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x39FE,
                source: 0x8340,
                segment_variable: 0x4F86,
                width_argument: 0x19,
                height: 0xA8,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x39D0,
                source: 0xC4E0,
                segment_variable: 0x4F86,
                width_argument: 0x0E,
                height: 0x68,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x39B9,
                source: 0xDBA0,
                segment_variable: 0x4F86,
                width_argument: 0x05,
                height: 0x10,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x3AB9,
                source: 0x0000,
                segment_variable: 0x4F8C,
                width_argument: 0x08,
                height: 0x10,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x3ACF,
                source: 0x0400,
                segment_variable: 0x4F8C,
                width_argument: 0x06,
                height: 0x10,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x3AE5,
                source: 0x0700,
                segment_variable: 0x4F8C,
                width_argument: 0x05,
                height: 0x10,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x3AFB,
                source: 0x0980,
                segment_variable: 0x4F8C,
                width_argument: 0x04,
                height: 0x10,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x3B11,
                source: 0x0B80,
                segment_variable: 0x4F8C,
                width_argument: 0x03,
                height: 0x10,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x3B27,
                source: 0x0D00,
                segment_variable: 0x4F8C,
                width_argument: 0x02,
                height: 0x10,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x3B3D,
                source: 0x0E00,
                segment_variable: 0x4F8C,
                width_argument: 0x01,
                height: 0x10,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x3B53,
                source: 0x0E80,
                segment_variable: 0x4F8C,
                width_argument: 0x03,
                height: 0x30,
                target: 0x44F5,
            },
            ConsumerCase {
                site: 0x3B6A,
                source: 0x1300,
                segment_variable: 0x4F8C,
                width_argument: 0x1B,
                height: 0xA0,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x3B81,
                source: 0x5680,
                segment_variable: 0x4F8C,
                width_argument: 0x1B,
                height: 0xA0,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x3B98,
                source: 0x9A00,
                segment_variable: 0x4F8C,
                width_argument: 0x1B,
                height: 0xA0,
                target: 0x44C5,
            },
            ConsumerCase {
                site: 0x3C33,
                source: 0x0000,
                segment_variable: 0x4F8E,
                width_argument: 0x0A,
                height: 0x70,
                target: 0x44F5,
            },
        ],
    );

    for (site, row_pitch, plane_stride) in [
        (0x0AF1, 0x0028u16, 0x2580u16),
        (0x06A6, 0x0034, 0x2D80),
        (0x0717, 0x002C, 0x2100),
        (0x0D7C, 0x0034, 0x2700),
    ] {
        let consumer = logical_slice(&overlay, site, 0x40);
        let mut pitch = vec![0x2E, 0xC7, 0x06, 0xAB, 0x4F];
        pitch.extend_from_slice(&row_pitch.to_le_bytes());
        assert!(contains(consumer, &pitch));
        let mut stride = vec![0x2E, 0xC7, 0x06, 0xAF, 0x4F];
        stride.extend_from_slice(&plane_stride.to_le_bytes());
        assert!(contains(consumer, &stride));
    }

    assert_eq!(occurrence_count(&overlay, &[0xBE, 0x00, 0xEC]), 0);
    assert_eq!(occurrence_count(&overlay, &[0xBE, 0xA0, 0x51]), 0);

    // S11_2 reaches its final 0x100 bytes through one state-driven source,
    // rather than a `mov si,0xEC00` immediate. Every other direct 0x4F90 DS
    // load has an immediate SI, making 0x33A5 the only dynamic source site.
    let sources_4f90 = immediate_sources_for_segment(&overlay, 0x4F90);
    let immediate_ds_sites = sources_4f90
        .iter()
        .map(|(site, _)| site + 4)
        .collect::<Vec<_>>();
    let dynamic_ds_sites = occurrence_sites(&overlay, &[0x2E, 0x8E, 0x1E, 0x90, 0x4F])
        .into_iter()
        .filter(|site| !immediate_ds_sites.contains(site))
        .collect::<Vec<_>>();
    assert_eq!(dynamic_ds_sites, [0x33A5]);
    assert_eq!(
        logical_slice(&overlay, 0x0D13, 6),
        [0x2E, 0xC7, 0x46, 0x11, 0x00, 0xEC]
    );
    let dynamic_consumer = logical_slice(&overlay, 0x339E, 0x17);
    assert_eq!(
        &dynamic_consumer[..17],
        &[
            0x2E, 0x8B, 0x5E, 0x11, 0x1E, 0x8B, 0xF3, 0x2E, 0x8E, 0x1E, 0x90, 0x4F, 0x8B, 0xF8,
            0xBA, 0x01, 0x00,
        ]
    );
    assert!(contains(dynamic_consumer, &[0xB9, 0x08, 0x00]));
    assert_eq!(call_target(0x339E, dynamic_consumer), 0x4494);
    assert_eq!(
        logical_slice(&overlay, 0x3372, 0x18),
        [
            0x2E, 0xFF, 0x46, 0x0F, 0x2E, 0x83, 0x7E, 0x0F, 0x02, 0x74, 0x01, 0xC3, 0x2E, 0xC7,
            0x46, 0x0F, 0x00, 0x00, 0x2E, 0x83, 0x46, 0x11, 0x20, 0xC3,
        ]
    );
    let handler_table = (0..18)
        .map(|index| {
            u16::from_le_bytes(
                logical_slice(&overlay, 0x18F0 + index * 2, 2)
                    .try_into()
                    .unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let mut expected_handlers = vec![0x3359; 16];
    expected_handlers.extend([0x1AEA, 0xFFFF]);
    assert_eq!(handler_table, expected_handlers);
    let mut source = 0xEC00;
    let mut repeat = 0;
    let mut repeated_ranges = Vec::new();
    for handler in &handler_table[..16] {
        assert_eq!(*handler, 0x3359);
        repeated_ranges.push((source, source + 0x20));
        repeat += 1;
        if repeat == 2 {
            repeat = 0;
            source += 0x20;
        }
    }
    let dynamic_ranges = repeated_ranges
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            assert_eq!(pair[0], pair[1]);
            pair[0]
        })
        .collect::<Vec<_>>();
    assert_eq!(source, 0xED00);
    assert_eq!(dynamic_ranges.len(), 8);
    assert_eq!(dynamic_ranges.first(), Some(&(0xEC00, 0xEC20)));
    assert_eq!(dynamic_ranges.last(), Some(&(0xECE0, 0xED00)));

    // Segment 0x4F8C is reused for S7, S16, and S20. Every direct DS load has
    // an immediate SI in the exhaustive list below, so the S16 phase has ten
    // fixed sources and no hidden/dynamic selector for its 0x51A0 tail.
    let sources_4f8c = immediate_sources_for_segment(&overlay, 0x4F8C);
    assert_eq!(
        sources_4f8c,
        [
            (0x0907, 0x0000),
            (0x0FF1, 0x0000),
            (0x265C, 0x0000),
            (0x2673, 0x4500),
            (0x268A, 0x4900),
            (0x26A1, 0x4E40),
            (0x26B8, 0x9B40),
            (0x26CE, 0x9340),
            (0x26E5, 0x9B40),
            (0x3892, 0x0191),
            (0x38B7, 0x1400),
            (0x38CE, 0x1500),
            (0x38E5, 0x1600),
            (0x38FC, 0x18C0),
            (0x3916, 0x1FA0),
            (0x393F, 0x39A0),
            (0x3955, 0x3E20),
            (0x396C, 0x42A0),
            (0x3AB9, 0x0000),
            (0x3ACF, 0x0400),
            (0x3AE5, 0x0700),
            (0x3AFB, 0x0980),
            (0x3B11, 0x0B80),
            (0x3B27, 0x0D00),
            (0x3B3D, 0x0E00),
            (0x3B53, 0x0E80),
            (0x3B6A, 0x1300),
            (0x3B81, 0x5680),
            (0x3B98, 0x9A00),
        ]
    );
    assert_eq!(
        occurrence_sites(&overlay, &[0x2E, 0x8E, 0x1E, 0x8C, 0x4F]),
        sources_4f8c
            .iter()
            .map(|(site, _)| site + 4)
            .collect::<Vec<_>>()
    );
    let s16_sources = sources_4f8c
        .iter()
        .filter(|(site, _)| *site == 0x0FF1 || (0x3892..=0x396C).contains(site))
        .map(|(_, source)| *source)
        .collect::<Vec<_>>();
    assert_eq!(
        s16_sources,
        [
            0x0000, 0x0191, 0x1400, 0x1500, 0x1600, 0x18C0, 0x1FA0, 0x39A0, 0x3E20, 0x42A0,
        ]
    );
}
