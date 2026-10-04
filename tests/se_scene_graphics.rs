use pc98_madou_ars::graphics_resource::{ScreenLayout, render_named_screen_resource};

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
fn exact_se_resources_render_with_consumer_proven_scene_layouts() {
    let Some(disk) = common::try_read(SCHEZO_DATA) else {
        return;
    };
    let cases = [
        ResourceCase {
            name: "SE1.DAT",
            packed_size: 45_193,
            packed_sha256: "9b4bc7a5823f4f90fb02bf4ac3d586f3368c56f82a0f8f980cef43b70abd5602",
            decoded_size: 63_936,
            decoded_sha256: "8ffdd37c2c84a5889859383597f99dbe42e3bbd5b5b82416962fa6a9b2d71cc8",
            rgb_sha256: "c5a7df73e64593c9752b29edb3200341bf192d892a1c0803bf4a82aa7d65b6e3",
        },
        ResourceCase {
            name: "SE3.DAT",
            packed_size: 33_228,
            packed_sha256: "a171afd503e49ef4781297eea470cc6f3bb4ad9161e5cd40a0400a398a8a45e2",
            decoded_size: 43_776,
            decoded_sha256: "d91c17f1d781c79a96de1842559b7f282b6556bd18e3ed01c33cbbf601fc5f0a",
            rgb_sha256: "7bdbb8830cc186e44ce367a7d7c741ef11b88fddf43546ba94c1004f9815b23b",
        },
        ResourceCase {
            name: "SE4.DAT",
            packed_size: 37_232,
            packed_sha256: "26bdcfe0caa8b92f03ef3793e6a045d25b41a0b3e0c8e6be0cb275a4332c685f",
            decoded_size: 43_136,
            decoded_sha256: "55d4b6644c79526ff74b2e273db97c0924bb1883feac92377722e6d20731987f",
            rgb_sha256: "0fbc59d3e135c7bcba0c6cf697b0f3554614bd71b402d0d824c965d51a23f162",
        },
        ResourceCase {
            name: "SE5.DAT",
            packed_size: 30_174,
            packed_sha256: "a916b9308472e661c48347977eb8964a6c7669b9376a628049fd6abcfc9942c5",
            decoded_size: 42_272,
            decoded_sha256: "4e7107f9735522b051ea32181e55e2e5b855c490ce8b6ce21ac13c308dc2ee7e",
            rgb_sha256: "50489c7162b9e9601224044109960148dd5202c5484f46a9fc910819a5ea497a",
        },
        ResourceCase {
            name: "SE7.DAT",
            packed_size: 24_786,
            packed_sha256: "3ee997e522ac1aaadc8e008365ea4307baa05e0a89f4a58e919af144ab28abe0",
            decoded_size: 55_296,
            decoded_sha256: "314b9abb6b383498b2c498ff3dc23782b9f75ba0c702f63270ea03850dda67f6",
            rgb_sha256: "f7d1808be635ee4f0d0fe9ce176e88af3a8103d5981625f674c9f25d1f37b931",
        },
        ResourceCase {
            name: "SE7_1.DAT",
            packed_size: 2_895,
            packed_sha256: "0423473851d7d56d8d88ffba9e87e27ec0ab15d0c7be72dc6267a510d177b9a3",
            decoded_size: 4_640,
            decoded_sha256: "09be5f7d38fc1fb2c055e9950f2a5ca5aca6990331693b63e62547af59ead472",
            rgb_sha256: "f8d9dfadb6f612b3803b92379b972ee09ec01c01b01e9ecb2a8ffbbf444d45c4",
        },
        ResourceCase {
            name: "SE10.DAT",
            packed_size: 17_345,
            packed_sha256: "fda3e0b8fab701d87a37b1cf21adbc52f443a127061701f8b5e3d201636f3e1e",
            decoded_size: 27_648,
            decoded_sha256: "c80d373c5a4a2db0a952a6b469d7f15f762fbd51b3b6e697693862268ba94ef7",
            rgb_sha256: "4b0396f3aba8f7b7f9cd8d8cbda98f301f288e378eb9b68ec8b0a5064c5588ba",
        },
        ResourceCase {
            name: "SE10_1.DAT",
            packed_size: 24_018,
            packed_sha256: "4d5731401fdf14b6881baf2a3b7879d90169a22c6ad79254094d41ec265d0f28",
            decoded_size: 40_064,
            decoded_sha256: "a239a79413cb3c8017990e5ce9f44dd7ad81ff245a0ff4dba358f5b189264427",
            rgb_sha256: "ffa0e766b07cb3b2d84abab23b0bb5049b964c87b095648383ac4ef04f63dfd0",
        },
        ResourceCase {
            name: "SE12.DAT",
            packed_size: 24_659,
            packed_sha256: "56645441ac7b2b42fc06e15650fe6101d7a1f2ebe75c98c52485ab75d5bf4440",
            decoded_size: 30_592,
            decoded_sha256: "fd16544e893a4ddc006daff3c3df27d9764f23ef3ad732a99675d85409c59149",
            rgb_sha256: "ba9a5de806be8d06c65c77e4f65fdfb4242207a5de2a53497c0da4b91e544662",
        },
        ResourceCase {
            name: "SE13.DAT",
            packed_size: 9_391,
            packed_sha256: "5fb11bc95a4aced3caad45ad38c40a4c8158956209c234f7731a965cef5cc474",
            decoded_size: 14_976,
            decoded_sha256: "cefea5c08972c0a2fe189d540cec2f66cca2b1efc937a30f131e39f7fab5b1b8",
            rgb_sha256: "27faac6a3a4127005c974acaeaa01799a6121aa1e5e6f54d59ade13b7568e812",
        },
        ResourceCase {
            name: "SE17.DAT",
            packed_size: 31_192,
            packed_sha256: "cbd7dc338999393c4c7308776922af9f1eefa55421e1abb306d1d67c5fcf383f",
            decoded_size: 54_528,
            decoded_sha256: "f7ef809eab0ab977921c8116ae600453e49283fb82bf5234aae143b1ca77e43b",
            rgb_sha256: "e4abd3c72aa12ee33d2ef67325d4bd7112e87b3bccbfc50faabb781986bd7d0b",
        },
        ResourceCase {
            name: "SE18.DAT",
            packed_size: 39_977,
            packed_sha256: "1897bce23e31650819fa416baeacf5eeba0c692672b683fa4e615b8838e73573",
            decoded_size: 50_048,
            decoded_sha256: "f7d5e0a9faaeb1acc4931d9278ef2f6e98fda59dd9d29bba87ce058d1be89f9a",
            rgb_sha256: "d24da6bb65b308b2d255767d4c9ae12ccb7746cad337a64ab625806cbe78f779",
        },
        ResourceCase {
            name: "SE19.DAT",
            packed_size: 19_810,
            packed_sha256: "d53e7e5782bf2072a383feabadd1e438d0edb8c5be8dce8874e08168db5bbda2",
            decoded_size: 28_608,
            decoded_sha256: "cfcbb1bcb8e4207189477c2589cbef322d5f6ae0905a4cb5e15b7dbf5e6490e5",
            rgb_sha256: "8ec81aba40c11876d7778d627c5737bc0c9d13e21ac7607293032ca737169cb7",
        },
        ResourceCase {
            name: "SE23.DAT",
            packed_size: 13_033,
            packed_sha256: "4f35dd77b46d81766df901535eef3182d3c46d013347997c732a3deb1751a8cb",
            decoded_size: 47_808,
            decoded_sha256: "03e59d4345e1c981d477bd6a1316f4e7e732a667e9209aa0fb09139b8f0ceb76",
            rgb_sha256: "73c22cf365756c98195789c01ec048383d6d8bc77c7b4fa62a4bf38c5816045e",
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
        assert!(rendered.unrendered_ranges.is_empty());
        assert_eq!(
            pc98_madou_ars::media_identity::sha256_hex(&rendered.rgb),
            case.rgb_sha256
        );
    }
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn shezo_ed_loads_and_consumes_all_fourteen_se_resources() {
    let Some(disk) = common::try_read(SCHEZO_GAME) else {
        return;
    };
    let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "SHEZO_ED.OVL").unwrap();
    assert_eq!(packed.len(), 12_647);
    assert_eq!(
        pc98_madou_ars::media_identity::sha256_hex(&packed),
        "6c9d303920c17f7c86db135a46bce12500b97f38b0ade254d1c4b5c255394772"
    );
    let report = pc98_madou_ars::overlay_lz::decode_overlay_lz(&packed).unwrap();
    assert_eq!(report.bytes_consumed, packed.len());
    assert_eq!(report.output.len(), 23_013);
    assert_eq!(
        pc98_madou_ars::media_identity::sha256_hex(&report.output),
        "8b23022cd15dd8b841374a2d56caf2592878fbca615e98659e04f9df5d59f408"
    );
    let overlay = report.output;

    for (address, name) in [
        (0x0231, b"se1.dat\0".as_slice()),
        (0x0239, b"se3.dat\0".as_slice()),
        (0x0241, b"se4.dat\0".as_slice()),
        (0x0249, b"se5.dat\0".as_slice()),
        (0x0251, b"se7.dat\0".as_slice()),
        (0x0259, b"se7_1.dat\0".as_slice()),
        (0x0263, b"se10.dat\0".as_slice()),
        (0x026C, b"se10_1.dat\0".as_slice()),
        (0x0277, b"se12.dat\0".as_slice()),
        (0x0280, b"se13.dat\0".as_slice()),
        (0x0289, b"se17.dat\0".as_slice()),
        (0x0292, b"se18.dat\0".as_slice()),
        (0x029B, b"se19.dat\0".as_slice()),
        (0x02A4, b"se23.dat\0".as_slice()),
    ] {
        assert_eq!(logical_slice(&overlay, address, name.len()), name);
    }

    for (site, filename, destination) in [
        (0x0409, 0x0231u16, 0x50F6u16),
        (0x0442, 0x0239, 0x50F8),
        (0x047B, 0x0241, 0x50FA),
        (0x04B4, 0x0249, 0x50FC),
        (0x04ED, 0x0251, 0x50FE),
        (0x0526, 0x0259, 0x5100),
        (0x055F, 0x0263, 0x5104),
        (0x0598, 0x026C, 0x5102),
        (0x07F0, 0x0277, 0x50FA),
        (0x0829, 0x0280, 0x50F8),
        (0x0862, 0x0289, 0x50F6),
        (0x0D8D, 0x0292, 0x50FE),
        (0x0DC6, 0x029B, 0x50FC),
        (0x0F62, 0x02A4, 0x50F6),
        (0x1057, 0x0231, 0x50F6),
        (0x15FA, 0x0277, 0x5104),
        (0x1633, 0x0263, 0x50FA),
        (0x166C, 0x026C, 0x5102),
    ] {
        assert_loader(&overlay, site, filename, destination);
    }

    let row_interleaved = logical_slice(&overlay, 0x4614, 0x30);
    assert!(contains(
        row_interleaved,
        &[
            0xAC, 0xE6, 0x7E, 0x8A, 0xE0, 0xAC, 0xE6, 0x7E, 0x0A, 0xE0, 0xAC, 0xE6, 0x7E, 0x0A,
            0xE0, 0xAC, 0xE6, 0x7E,
        ]
    ));
    assert!(contains(row_interleaved, &[0xAA, 0xE2, 0xE9]));

    for (site, opcode) in [(0x4645, 0xA4), (0x4675, 0xA5)] {
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
    let strided_word = logical_slice(&overlay, 0x46A5, 0x40);
    assert!(contains(strided_word, &[0xF3, 0xA5]));
    assert!(contains(strided_word, &[0x2E, 0x03, 0x36, 0x41, 0x51]));
    assert!(contains(strided_word, &[0x2E, 0x03, 0x36, 0x45, 0x51]));
    let strided_interleaved = logical_slice(&overlay, 0x46E4, 0x38);
    assert!(contains(
        strided_interleaved,
        &[0x2E, 0x03, 0x36, 0x41, 0x51]
    ));
    assert!(contains(strided_interleaved, &[0xAC, 0xE6, 0x7E]));

    assert_consumers(
        &overlay,
        &[
            ConsumerCase {
                site: 0x1F57,
                source: 0x0000,
                segment_variable: 0x50F6,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x1F40,
                source: 0x6C00,
                segment_variable: 0x50F6,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x2538,
                source: 0xD800,
                segment_variable: 0x50F6,
                width_argument: 0x09,
                height: 0x30,
                target: 0x46E4,
            },
            ConsumerCase {
                site: 0x2555,
                source: 0xDEC0,
                segment_variable: 0x50F6,
                width_argument: 0x06,
                height: 0x20,
                target: 0x46E4,
            },
            ConsumerCase {
                site: 0x2572,
                source: 0xE1C0,
                segment_variable: 0x50F6,
                width_argument: 0x06,
                height: 0x10,
                target: 0x46E4,
            },
            ConsumerCase {
                site: 0x258F,
                source: 0xE340,
                segment_variable: 0x50F6,
                width_argument: 0x0C,
                height: 0x20,
                target: 0x46E4,
            },
            ConsumerCase {
                site: 0x25AC,
                source: 0xE940,
                segment_variable: 0x50F6,
                width_argument: 0x04,
                height: 0x10,
                target: 0x46E4,
            },
            ConsumerCase {
                site: 0x24F6,
                source: 0xEA40,
                segment_variable: 0x50F6,
                width_argument: 0x08,
                height: 0x20,
                target: 0x46E4,
            },
            ConsumerCase {
                site: 0x2406,
                source: 0xF240,
                segment_variable: 0x50F6,
                width_argument: 0x0A,
                height: 0x30,
                target: 0x46E4,
            },
            ConsumerCase {
                site: 0x25CA,
                source: 0x0000,
                segment_variable: 0x50F8,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x25E1,
                source: 0x6C00,
                segment_variable: 0x50F8,
                width_argument: 0x05,
                height: 0x10,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x25F7,
                source: 0x6E80,
                segment_variable: 0x50F8,
                width_argument: 0x08,
                height: 0x30,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x260D,
                source: 0x7A80,
                segment_variable: 0x50F8,
                width_argument: 0x0A,
                height: 0x10,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x2623,
                source: 0x7F80,
                segment_variable: 0x50F8,
                width_argument: 0x0C,
                height: 0x70,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x2639,
                source: 0xA980,
                segment_variable: 0x50F8,
                width_argument: 0x01,
                height: 0x30,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x2650,
                source: 0x0000,
                segment_variable: 0x50FA,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x2667,
                source: 0x6C00,
                segment_variable: 0x50FA,
                width_argument: 0x0B,
                height: 0xB0,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x267E,
                source: 0x0000,
                segment_variable: 0x50FC,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x2695,
                source: 0x6C00,
                segment_variable: 0x50FC,
                width_argument: 0x05,
                height: 0x40,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x27EA,
                source: 0x7600,
                segment_variable: 0x50FC,
                width_argument: 0x05,
                height: 0x40,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x2825,
                source: 0x8000,
                segment_variable: 0x50FC,
                width_argument: 0x0D,
                height: 0x10,
                target: 0x4645,
            },
            ConsumerCase {
                site: 0x283B,
                source: 0x8340,
                segment_variable: 0x50FC,
                width_argument: 0x11,
                height: 0x10,
                target: 0x4645,
            },
            ConsumerCase {
                site: 0x2851,
                source: 0x8780,
                segment_variable: 0x50FC,
                width_argument: 0x15,
                height: 0x48,
                target: 0x4645,
            },
            ConsumerCase {
                site: 0x2867,
                source: 0x9F20,
                segment_variable: 0x50FC,
                width_argument: 0x0E,
                height: 0x10,
                target: 0x4645,
            },
            ConsumerCase {
                site: 0x287D,
                source: 0xA2A0,
                segment_variable: 0x50FC,
                width_argument: 0x0A,
                height: 0x10,
                target: 0x4645,
            },
            ConsumerCase {
                site: 0x2732,
                source: 0x0000,
                segment_variable: 0x5100,
                width_argument: 0x04,
                height: 0x78,
                target: 0x4645,
            },
            ConsumerCase {
                site: 0x2748,
                source: 0x0780,
                segment_variable: 0x5100,
                width_argument: 0x05,
                height: 0x28,
                target: 0x4645,
            },
            ConsumerCase {
                site: 0x275E,
                source: 0x0AA0,
                segment_variable: 0x5100,
                width_argument: 0x04,
                height: 0x28,
                target: 0x4645,
            },
            ConsumerCase {
                site: 0x2787,
                source: 0x0D20,
                segment_variable: 0x5100,
                width_argument: 0x04,
                height: 0x28,
                target: 0x4645,
            },
            ConsumerCase {
                site: 0x279D,
                source: 0x0FA0,
                segment_variable: 0x5100,
                width_argument: 0x04,
                height: 0x28,
                target: 0x4645,
            },
            ConsumerCase {
                site: 0x2A41,
                source: 0x0000,
                segment_variable: 0x5104,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x28B8,
                source: 0x0000,
                segment_variable: 0x5102,
                width_argument: 0x0E,
                height: 0x10,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x28CE,
                source: 0x0380,
                segment_variable: 0x5102,
                width_argument: 0x12,
                height: 0x10,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x28E4,
                source: 0x0800,
                segment_variable: 0x5102,
                width_argument: 0x14,
                height: 0x10,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x28FA,
                source: 0x0D00,
                segment_variable: 0x5102,
                width_argument: 0x15,
                height: 0x10,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x2910,
                source: 0x1240,
                segment_variable: 0x5102,
                width_argument: 0x14,
                height: 0x20,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x2926,
                source: 0x1C40,
                segment_variable: 0x5102,
                width_argument: 0x18,
                height: 0x10,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x293C,
                source: 0x2240,
                segment_variable: 0x5102,
                width_argument: 0x1E,
                height: 0x10,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x2952,
                source: 0x29C0,
                segment_variable: 0x5102,
                width_argument: 0x20,
                height: 0x10,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x2A58,
                source: 0x31C0,
                segment_variable: 0x5102,
                width_argument: 0x0D,
                height: 0x10,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x2A6E,
                source: 0x3500,
                segment_variable: 0x5102,
                width_argument: 0x0F,
                height: 0x10,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x2A84,
                source: 0x38C0,
                segment_variable: 0x5102,
                width_argument: 0x11,
                height: 0x50,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x2A9A,
                source: 0x4E00,
                segment_variable: 0x5102,
                width_argument: 0x19,
                height: 0x10,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x2AB0,
                source: 0x5440,
                segment_variable: 0x5102,
                width_argument: 0x1F,
                height: 0x20,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x2A02,
                source: 0x63C0,
                segment_variable: 0x5102,
                width_argument: 0x0B,
                height: 0x60,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x2A18,
                source: 0x7440,
                segment_variable: 0x5102,
                width_argument: 0x08,
                height: 0x40,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x2AC7,
                source: 0x7C40,
                segment_variable: 0x5102,
                width_argument: 0x04,
                height: 0x40,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x2968,
                source: 0x8440,
                segment_variable: 0x5102,
                width_argument: 0x05,
                height: 0xA8,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x297E,
                source: 0x9160,
                segment_variable: 0x5102,
                width_argument: 0x03,
                height: 0x90,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x2994,
                source: 0x9820,
                segment_variable: 0x5102,
                width_argument: 0x03,
                height: 0x38,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x29AA,
                source: 0x9AC0,
                segment_variable: 0x5102,
                width_argument: 0x01,
                height: 0x38,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x29C0,
                source: 0x9BA0,
                segment_variable: 0x5102,
                width_argument: 0x01,
                height: 0x20,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x29D6,
                source: 0x9C20,
                segment_variable: 0x5102,
                width_argument: 0x01,
                height: 0x10,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x29EC,
                source: 0x9C60,
                segment_variable: 0x5102,
                width_argument: 0x01,
                height: 0x08,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x2AF1,
                source: 0x0000,
                segment_variable: 0x50FA,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x2B80,
                source: 0x6C00,
                segment_variable: 0x50FA,
                width_argument: 0x04,
                height: 0x28,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x2B97,
                source: 0x7100,
                segment_variable: 0x50FA,
                width_argument: 0x04,
                height: 0x28,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x2B2D,
                source: 0x7600,
                segment_variable: 0x50FA,
                width_argument: 0x01,
                height: 0x18,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x2B44,
                source: 0x76C0,
                segment_variable: 0x50FA,
                width_argument: 0x01,
                height: 0x18,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x2BAE,
                source: 0x0000,
                segment_variable: 0x50F8,
                width_argument: 0x06,
                height: 0x10,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x2BC4,
                source: 0x0180,
                segment_variable: 0x50F8,
                width_argument: 0x10,
                height: 0x10,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x2BDA,
                source: 0x0580,
                segment_variable: 0x50F8,
                width_argument: 0x11,
                height: 0x10,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x2BF0,
                source: 0x09C0,
                segment_variable: 0x50F8,
                width_argument: 0x12,
                height: 0x10,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x2C06,
                source: 0x0E40,
                segment_variable: 0x50F8,
                width_argument: 0x11,
                height: 0x10,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x2C1C,
                source: 0x1280,
                segment_variable: 0x50F8,
                width_argument: 0x10,
                height: 0x10,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x2C32,
                source: 0x1680,
                segment_variable: 0x50F8,
                width_argument: 0x0F,
                height: 0x10,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x2C48,
                source: 0x1A40,
                segment_variable: 0x50F8,
                width_argument: 0x0C,
                height: 0x10,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x2C5E,
                source: 0x1D40,
                segment_variable: 0x50F8,
                width_argument: 0x0B,
                height: 0x10,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x2C74,
                source: 0x2000,
                segment_variable: 0x50F8,
                width_argument: 0x0A,
                height: 0x10,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x2C8A,
                source: 0x2280,
                segment_variable: 0x50F8,
                width_argument: 0x0C,
                height: 0x10,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x2D2C,
                source: 0x2580,
                segment_variable: 0x50F8,
                width_argument: 0x06,
                height: 0x20,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x2D67,
                source: 0x2880,
                segment_variable: 0x50F8,
                width_argument: 0x06,
                height: 0x20,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x2DDD,
                source: 0x2B80,
                segment_variable: 0x50F8,
                width_argument: 0x06,
                height: 0x20,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x2E18,
                source: 0x2E80,
                segment_variable: 0x50F8,
                width_argument: 0x06,
                height: 0x20,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x2DA2,
                source: 0x3180,
                segment_variable: 0x50F8,
                width_argument: 0x08,
                height: 0x48,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x2E4D,
                source: 0x0000,
                segment_variable: 0x50F6,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x2E2F,
                source: 0x6C00,
                segment_variable: 0x50F6,
                width_argument: 0x01,
                height: 0xA0,
                target: 0x46E4,
            },
            ConsumerCase {
                site: 0x2EFC,
                source: 0x0000,
                segment_variable: 0x50FE,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x3051,
                source: 0x6C00,
                segment_variable: 0x50FE,
                width_argument: 0x03,
                height: 0x48,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x3068,
                source: 0x72C0,
                segment_variable: 0x50FE,
                width_argument: 0x03,
                height: 0x48,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x307F,
                source: 0x7980,
                segment_variable: 0x50FE,
                width_argument: 0x03,
                height: 0x48,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x3096,
                source: 0x8040,
                segment_variable: 0x50FE,
                width_argument: 0x09,
                height: 0x80,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x2F38,
                source: 0xA440,
                segment_variable: 0x50FE,
                width_argument: 0x04,
                height: 0x28,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x2F4F,
                source: 0xA940,
                segment_variable: 0x50FE,
                width_argument: 0x04,
                height: 0x28,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x2F66,
                source: 0xAE40,
                segment_variable: 0x50FE,
                width_argument: 0x04,
                height: 0x28,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x2F7D,
                source: 0xB340,
                segment_variable: 0x50FE,
                width_argument: 0x04,
                height: 0x28,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x2FB9,
                source: 0xB840,
                segment_variable: 0x50FE,
                width_argument: 0x03,
                height: 0x18,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x2FD0,
                source: 0xBA80,
                segment_variable: 0x50FE,
                width_argument: 0x03,
                height: 0x18,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x2FE7,
                source: 0xBCC0,
                segment_variable: 0x50FE,
                width_argument: 0x03,
                height: 0x18,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x2FFE,
                source: 0xBF00,
                segment_variable: 0x50FE,
                width_argument: 0x03,
                height: 0x18,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x3015,
                source: 0xC140,
                segment_variable: 0x50FE,
                width_argument: 0x03,
                height: 0x18,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x30BB,
                source: 0x0000,
                segment_variable: 0x50FC,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x4675,
            },
            ConsumerCase {
                site: 0x30F7,
                source: 0x6C00,
                segment_variable: 0x50FC,
                width_argument: 0x05,
                height: 0x18,
                target: 0x4645,
            },
            ConsumerCase {
                site: 0x310E,
                source: 0x6DE0,
                segment_variable: 0x50FC,
                width_argument: 0x05,
                height: 0x18,
                target: 0x4645,
            },
            ConsumerCase {
                site: 0x3162,
                source: 0x0000,
                segment_variable: 0x50F6,
                width_argument: 0x06,
                height: 0x30,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x3178,
                source: 0x0480,
                segment_variable: 0x50F6,
                width_argument: 0x0C,
                height: 0x58,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x318E,
                source: 0x1500,
                segment_variable: 0x50F6,
                width_argument: 0x04,
                height: 0x40,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x31A4,
                source: 0x1900,
                segment_variable: 0x50F6,
                width_argument: 0x05,
                height: 0x70,
                target: 0x4614,
            },
            ConsumerCase {
                site: 0x314C,
                source: 0x21C0,
                segment_variable: 0x50F6,
                width_argument: 0x12,
                height: 0x50,
                target: 0x4675,
            },
        ],
    );

    let se7_state = logical_slice(&overlay, 0x08C0, 0x60);
    assert!(contains(
        se7_state,
        &[0x2E, 0xC7, 0x06, 0x17, 0x51, 0xB8, 0x35]
    ));
    assert!(contains(se7_state, &[0x2E, 0x83, 0x2E, 0x17, 0x51, 0x24]));
    let se7_consumer = logical_slice(&overlay, 0x26BF, 0x30);
    assert!(contains(se7_consumer, &[0x2E, 0x8B, 0x36, 0x17, 0x51]));
    assert!(contains(
        se7_consumer,
        &[0x2E, 0xC7, 0x06, 0x45, 0x51, 0x00, 0x36]
    ));
    assert_eq!(call_target(0x26BF, se7_consumer), 0x46A5);

    let se17_state = logical_slice(&overlay, 0x0CF9, 0x90);
    assert!(contains(
        se17_state,
        &[0x2E, 0xC7, 0x06, 0x17, 0x51, 0x00, 0x6C]
    ));
    assert!(contains(
        se17_state,
        &[0x2E, 0xC7, 0x06, 0x17, 0x51, 0x80, 0xA0]
    ));
    let se17_dynamic = logical_slice(&overlay, 0x2E64, 0x30);
    assert!(contains(se17_dynamic, &[0x2E, 0x8B, 0x36, 0x17, 0x51]));
    assert!(contains(
        se17_dynamic,
        &[0x2E, 0xC7, 0x06, 0x41, 0x51, 0x54, 0x00]
    ));
    assert_eq!(call_target(0x2E64, se17_dynamic), 0x2ED7);

    let se23_state = logical_slice(&overlay, 0x0F50, 0x90);
    assert!(contains(
        se23_state,
        &[0x2E, 0xC7, 0x06, 0x27, 0x51, 0x00, 0x5A]
    ));
    let se23_dynamic = logical_slice(&overlay, 0x3125, 0xC0);
    assert!(contains(
        se23_dynamic,
        &[0x2E, 0xA1, 0x27, 0x51, 0x1E, 0x8B, 0xF0]
    ));
    assert!(contains(
        se23_dynamic,
        &[0x2E, 0x81, 0x3E, 0x27, 0x51, 0xC0, 0x4E]
    ));
    assert!(contains(
        se23_dynamic,
        &[0x2E, 0x83, 0x2E, 0x27, 0x51, 0x24]
    ));
    assert_eq!(call_target(0x3125, se23_dynamic), 0x46A5);
}
