use pc98_madou_ars::bsamp_resource::parse_named_bsamp_companion;

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

fn decode_streams(packed: &[u8]) -> Vec<Vec<u8>> {
    let mut offset = 0usize;
    let mut streams = Vec::new();
    while offset < packed.len() {
        let report = pc98_madou_ars::overlay_lz::decode_overlay_lz(&packed[offset..]).unwrap();
        assert!(!report.output.is_empty());
        offset += report.bytes_consumed;
        streams.push(report.output);
    }
    assert_eq!(offset, packed.len());
    streams
}

fn words(bytes: &[u8]) -> Vec<u16> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|word| u16::from_le_bytes(*word))
        .collect()
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn exact_tc_and_mu_companions_are_length_prefixed_bsamp_banks() {
    let Some(disk) = common::try_read(DEMO_DISK) else {
        return;
    };
    let cases = [
        (
            "TC.CNS",
            vec![(
                0,
                2,
                0x127E,
                "6e6900e727f8cb93001b91e375b0e52f8c5c40d3f92841d265aeb9db3c3fddb9",
            )],
            4,
        ),
        (
            "MU.CNS",
            vec![
                (
                    0,
                    2,
                    0x0D7E,
                    "e1e430045ded49b0b06ca8255f3bd86d2be366d6b0d9838dec1aeaccef96bcd3",
                ),
                (
                    0x0D80,
                    0x0D82,
                    0x0ABE,
                    "b83f2b7056722d0b83decec7149ce7115d8962464f9765081b6db44e63168d79",
                ),
                (
                    0x1840,
                    0x1842,
                    0x1741,
                    "f57c3b41410fef7232d870cb218a234937485f9cdcc64aca5fde0fb52825b3e9",
                ),
            ],
            0,
        ),
    ];

    for (name, expected_tracks, expected_trailing) in cases {
        let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, name).unwrap();
        let streams = decode_streams(&packed);
        let companion = &streams[1];
        let bank = parse_named_bsamp_companion(name, companion)
            .unwrap()
            .unwrap();
        assert_eq!(bank.tracks.len(), expected_tracks.len());
        assert_eq!(bank.trailing_bytes, expected_trailing);
        for (track, (offset, payload_offset, payload_bytes, payload_sha256)) in
            bank.tracks.iter().zip(expected_tracks)
        {
            assert_eq!(track.offset, offset);
            assert_eq!(track.payload_offset, payload_offset);
            assert_eq!(track.payload_bytes, payload_bytes);
            assert_eq!(
                pc98_madou_ars::media_identity::sha256_hex(
                    &companion[track.payload_offset..track.end_offset()]
                ),
                payload_sha256
            );
        }
    }

    let tc = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "TC.CNS").unwrap();
    assert_eq!(&decode_streams(&tc)[1][0x1280..], &[0x88, 0x88, 0x88, 0x80]);
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn consumers_decompress_and_select_the_exact_companion_track_offsets() {
    let Some(disk) = common::try_read(DEMO_DISK) else {
        return;
    };
    let gaoo = decode_disk_file(&disk, "GAOO.OVL");
    let menu = decode_disk_file(&disk, "MADOMENU.OVL");

    // After AH=5 consumes the first stream, both overlays increment SI and
    // decode the second LZ stream through INT 7Ch/AH=3 into a separate segment.
    assert_eq!(
        logical_slice(&gaoo, 0x024E, 12),
        [
            0x46, 0x2E, 0x8E, 0x06, 0xD0, 0x09, 0x33, 0xFF, 0xB4, 0x03, 0xCD, 0x7C,
        ]
    );
    assert_eq!(
        logical_slice(&menu, 0x01C0, 12),
        [
            0x46, 0x2E, 0x8E, 0x06, 0x70, 0x08, 0x33, 0xFF, 0xB4, 0x03, 0xCD, 0x7C,
        ]
    );

    // GAOO selects offset zero. MADOMENU loads BP+8 from one of three
    // character records, whose exact companion offsets are 0/0D80/1840.
    assert_eq!(
        logical_slice(&gaoo, 0x02B1, 13),
        [
            0x2E, 0x8E, 0x06, 0xD0, 0x09, 0x33, 0xDB, 0x8B, 0xD3, 0xB4, 0x04, 0xCD, 0x7D,
        ]
    );
    assert_eq!(
        logical_slice(&menu, 0x0340, 20),
        [
            0x8B, 0x76, 0x00, 0xB4, 0x06, 0xCD, 0x7C, 0x8E, 0x06, 0x70, 0x08, 0x8B, 0x5E, 0x08,
            0x33, 0xD2, 0xB4, 0x04, 0xCD, 0x7D,
        ]
    );
    assert_eq!(
        words(logical_slice(&menu, 0x0F3E, 30)),
        [
            0x0F6F, 0x0F7C, 0x0F8E, 0x0F9B, 0x0000, 0x0FA6, 0x0FB3, 0x0FC2, 0x0FCF, 0x0D80, 0x0FDA,
            0x0FE7, 0x0FF6, 0x1003, 0x1840,
        ]
    );
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn resident_sound_wrapper_routes_the_bank_to_bsamp_length_prefixed_mode() {
    let Some(disk) = common::try_read(DEMO_DISK) else {
        return;
    };
    let main = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "MAIN.COM").unwrap();
    let body = pc98_madou_ars::overlay_lz::decode_overlay_lz(&main[0x71..])
        .unwrap()
        .output;
    let bsamp = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "BSAMP.COM").unwrap();
    assert_eq!(
        pc98_madou_ars::media_identity::sha256_hex(&bsamp),
        "c8f1f5bea4f41d968f3790b1ceefcdf204a8e011015489346b402f4450c9cb8e"
    );

    // INT 7Dh/AH=4 dispatches to 24FA. That wrapper first keys off FPLAY,
    // then calls INT 7Eh with AX=1, CL=1, BP=0 while preserving ES:BX:DX.
    assert_eq!(words(logical_slice(&body, 0x087C, 2)), [0x24FA]);
    assert_eq!(
        logical_slice(&body, 0x251A, 35),
        [
            0xB4, 0x06, 0xCD, 0x7F, 0x80, 0x0E, 0xB9, 0x2A, 0x80, 0xE8, 0x54, 0x00, 0xFA, 0xE4,
            0x02, 0x0C, 0x01, 0xE6, 0x02, 0xE4, 0x0A, 0x0C, 0x10, 0xE6, 0x0A, 0xFB, 0xB8, 0x01,
            0x00, 0xB1, 0x01, 0x33, 0xED, 0xCD, 0x7E,
        ]
    );

    // BSAMP installs INT 7Eh at 0120. Its AH=0/CL=1 path reads DX from
    // ES:[BX], advances BX by two, then stores the count and sample pointer.
    assert_eq!(
        logical_slice(&bsamp, 0x1602, 5),
        [0xB8, 0x7E, 0x35, 0xCD, 0x21]
    );
    assert_eq!(words(logical_slice(&bsamp, 0x01B0, 2)), [0x01BA]);
    assert_eq!(
        logical_slice(&bsamp, 0x01EE, 17),
        [
            0xF6, 0xC1, 0x02, 0x75, 0x17, 0x0B, 0xD5, 0x75, 0x05, 0x26, 0x8B, 0x17, 0x2B, 0xED,
            0x83, 0xC3, 0x02,
        ]
    );
    assert_eq!(
        logical_slice(&bsamp, 0x020A, 28),
        [
            0x89, 0x16, 0xB4, 0x15, 0x89, 0x2E, 0xB6, 0x15, 0xA2, 0xE9, 0x15, 0xA8, 0x80, 0x74,
            0x08, 0x8C, 0xC0, 0x03, 0xDA, 0x13, 0xC5, 0x8E, 0xC0, 0x89, 0x1E, 0xB0, 0x15, 0x8C,
        ]
    );
}
