use pc98_madou_ars::graphics_resource::{DecodedRange, ScreenLayout, render_named_screen_resource};

#[path = "common/mod.rs"]
mod common;

const RULUE_DATA: &str =
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Rurue Data disk).hdm";
const RULUE_GAME: &str =
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Rurue Game disk).hdm";
const LOGICAL_ORIGIN: usize = 0x0100;

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

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn exact_rtu_resources_render_with_consumer_proven_scene_layouts() {
    let Some(disk) = common::try_read(RULUE_DATA) else {
        return;
    };
    let cases = [
        (
            "RTU1.DAT",
            15_716,
            "2b494bc8cff76d673c67274fc080ff244f9aa0f0410fb76091ec72b9979977af",
            28_672,
            "1a18f43b0c0e169308d7410bb88214c5bdfae5e34ee3243700048d91dcb32a9c",
            "06ab1ae095a002f6383ed159fa0c92d1bc0a022e5cb6ca6de30e0b3315691521",
            Some(DecodedRange {
                offset: 0x6C00,
                bytes: 0x400,
            }),
        ),
        (
            "RTU2.DAT",
            23_046,
            "8afb706b4fef6f96d7a3e45879aa7521fdb15952dcb6ed0402dd86e60e7c61fe",
            39_936,
            "2b167bd9ec059d93715ad8fc40192e75256285fd924f22583bdd8474276b05b3",
            "636253744d4cbcf8ff54eb5be0f4d16c1a3950d6032177e83b48192fb97a19da",
            None,
        ),
        (
            "RTU3.DAT",
            28_528,
            "baa79e8f734a1f6284c6325cb1644bf528cf1fb0b87c5374bc4a77f4842155a7",
            57_600,
            "eac882ef73b8df74a7b7be3ae1ccfd8038164cc660817021e931929c227e5d26",
            "b1edbecadced973b2ceef51421c950b11e2022224707f41be0ecb881cfb2453b",
            None,
        ),
        (
            "RTU4.DAT",
            24_443,
            "b9b88d1bf37a4fade7557a8ba09fc957835e321d517ea53941ca5e9386caa85a",
            30_912,
            "8ed903d1a0bfc1599bd1c2f348d8dba255764ed6bbd07637116f13d924939954",
            "9237094431e7b1661061df7b6d34eb8c24f418d56d1adc1eccfaf1764e0109a7",
            None,
        ),
    ];

    for (name, packed_size, packed_sha256, decoded_size, decoded_sha256, rgb_sha256, unrendered) in
        cases
    {
        let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, name).unwrap();
        assert_eq!(packed.len(), packed_size);
        assert_eq!(
            pc98_madou_ars::media_identity::sha256_hex(&packed),
            packed_sha256
        );
        let decoded = pc98_madou_ars::overlay_lz::decode_overlay_lz(&packed).unwrap();
        assert_eq!(decoded.bytes_consumed, packed.len());
        assert_eq!(decoded.output.len(), decoded_size);
        assert_eq!(
            pc98_madou_ars::media_identity::sha256_hex(&decoded.output),
            decoded_sha256
        );

        let rendered = render_named_screen_resource(name, &packed).unwrap();
        assert_eq!(rendered.layout, ScreenLayout::BrgiSceneAtlas);
        assert_eq!(rendered.stream_sizes, [decoded_size]);
        assert_eq!(
            rendered.unrendered_ranges,
            unrendered.into_iter().collect::<Vec<_>>()
        );
        assert_eq!(
            pc98_madou_ars::media_identity::sha256_hex(&rendered.rgb),
            rgb_sha256
        );
    }
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn tyukan_r_loads_all_four_rtu_files_and_consumes_their_exact_regions() {
    let Some(disk) = common::try_read(RULUE_GAME) else {
        return;
    };
    let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "TYUKAN_R.OVL").unwrap();
    assert_eq!(packed.len(), 5_164);
    assert_eq!(
        pc98_madou_ars::media_identity::sha256_hex(&packed),
        "d129984b49ffd2a65c863bf1ad9c85e30402aeadcea74683d293f33fb418e022"
    );
    let report = pc98_madou_ars::overlay_lz::decode_overlay_lz(&packed).unwrap();
    assert_eq!(report.bytes_consumed, packed.len());
    assert_eq!(report.output.len(), 9_218);
    assert_eq!(
        pc98_madou_ars::media_identity::sha256_hex(&report.output),
        "d3d85226f204837d53b8606a9f75adf759fba23d17c2b3cde38438b41781ba78"
    );
    let overlay = report.output;

    for (address, name) in [
        (0x01D3, b"rtu1.dat\0".as_slice()),
        (0x01DC, b"rtu2.dat\0".as_slice()),
        (0x01E5, b"rtu3.dat\0".as_slice()),
        (0x01EE, b"rtu4.dat\0".as_slice()),
    ] {
        assert_eq!(logical_slice(&overlay, address, name.len()), name);
    }

    for (site, filename, destination) in [
        (0x0323, 0x01D3u16, 0x1C06u16),
        (0x035F, 0x01DC, 0x1C08),
        (0x039B, 0x01E5, 0x1C0A),
        (0x03D7, 0x01EE, 0x1C0C),
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

    let word_blitter = logical_slice(&overlay, 0x1175, 0x30);
    assert!(contains(
        word_blitter,
        &[
            0xB8, 0x00, 0xA8, 0xE8, 0x13, 0x00, 0xB8, 0x00, 0xB0, 0xE8, 0x0D, 0x00, 0xB8, 0x00,
            0xB8, 0xE8, 0x07, 0x00, 0xB8, 0x00, 0xE0,
        ]
    ));
    assert!(contains(word_blitter, &[0xF3, 0xA5]));
    let byte_blitter = logical_slice(&overlay, 0x1145, 0x30);
    assert!(contains(byte_blitter, &[0xF3, 0xA4]));
    let strided_blitter = logical_slice(&overlay, 0x11A5, 0x40);
    assert!(contains(strided_blitter, &[0xF3, 0xA5]));
    assert!(contains(strided_blitter, &[0x2E, 0x03, 0x36, 0x4E, 0x1C]));
    assert!(contains(strided_blitter, &[0x2E, 0x03, 0x36, 0x52, 0x1C]));

    let rtu1_primary = logical_slice(&overlay, 0x084B, 0x19);
    assert!(contains(
        rtu1_primary,
        &[
            0xBE, 0x00, 0x00, 0x2E, 0x8E, 0x1E, 0x06, 0x1C, 0xBF, 0x16, 0x14, 0xBA, 0x12, 0x00,
            0xB9, 0xC0, 0x00,
        ]
    ));
    let rtu1_sources = immediate_sources_for_segment(&overlay, 0x1C06);
    assert_eq!(
        rtu1_sources,
        [(0x084B, 0x0000), (0x086C, 0x1697), (0x08A3, 0x1685)]
    );
    assert_eq!(
        occurrence_sites(&overlay, &[0x2E, 0x8E, 0x1E, 0x06, 0x1C]),
        rtu1_sources
            .iter()
            .map(|(site, _)| site + 4)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        occurrence_sites(&overlay, &[0x06, 0x1C]),
        [
            0x0121, 0x0238, 0x026E, 0x027D, 0x0354, 0x0852, 0x0873, 0x08AA
        ]
    );

    assert_eq!(36 * 192 * 4, 0x6C00);
    for (site, source) in &rtu1_sources[1..] {
        let consumer = logical_slice(&overlay, *site, 0x5A);
        assert!(contains(consumer, &[0xBA, 0x04, 0x00, 0xB9, 0x10, 0x00]));
        assert!(contains(
            consumer,
            &[0x2E, 0xC7, 0x06, 0x4E, 0x1C, 0x24, 0x00]
        ));
        assert!(contains(
            consumer,
            &[0x2E, 0xC7, 0x06, 0x52, 0x1C, 0x00, 0x1B]
        ));
        let end = usize::from(*source) + 3 * 0x1B00 + 15 * 0x24 + 8;
        assert!(end <= 0x6C00);
    }

    let rtu2_pan = logical_slice(&overlay, 0x08DA, 0x25);
    assert!(contains(
        rtu2_pan,
        &[
            0xBE, 0x10, 0x00, 0x2E, 0x8E, 0x1E, 0x08, 0x1C, 0xBF, 0x16, 0x14, 0xBA, 0x12, 0x00,
            0xB9, 0xC0, 0x00,
        ]
    ));
    assert!(contains(
        rtu2_pan,
        &[0x2E, 0xC7, 0x06, 0x4E, 0x1C, 0x34, 0x00]
    ));
    assert!(contains(
        rtu2_pan,
        &[0x2E, 0xC7, 0x06, 0x52, 0x1C, 0x00, 0x27]
    ));

    let rtu3_pan = logical_slice(&overlay, 0x0927, 0x25);
    assert!(contains(
        rtu3_pan,
        &[
            0xBE, 0x00, 0x00, 0x2E, 0x8E, 0x1E, 0x0A, 0x1C, 0xBF, 0x16, 0x14, 0xBA, 0x12, 0x00,
            0xB9, 0xC0, 0x00,
        ]
    ));
    assert!(contains(
        rtu3_pan,
        &[0x2E, 0xC7, 0x06, 0x4E, 0x1C, 0x24, 0x00]
    ));
    assert!(contains(
        rtu3_pan,
        &[0x2E, 0xC7, 0x06, 0x52, 0x1C, 0x40, 0x38]
    ));

    for (site, source, width, height, blitter) in [
        (0x09C7, 0x0000u16, 0x0012u16, 0x00C0u16, 0x1175u16),
        (0x0A03, 0x6C00, 0x0009, 0x0028, 0x1145),
        (0x0A1A, 0x71A0, 0x0009, 0x0028, 0x1145),
        (0x0999, 0x7740, 0x0001, 0x0018, 0x1175),
        (0x09B0, 0x7800, 0x0001, 0x0018, 0x1175),
    ] {
        let consumer = logical_slice(&overlay, site, 0x1F);
        let mut signature = vec![0x1E, 0xBE];
        signature.extend_from_slice(&source.to_le_bytes());
        signature.extend_from_slice(&[0x2E, 0x8E, 0x1E, 0x0C, 0x1C]);
        assert!(contains(consumer, &signature));
        let mut dimensions = vec![0xBA];
        dimensions.extend_from_slice(&width.to_le_bytes());
        dimensions.push(0xB9);
        dimensions.extend_from_slice(&height.to_le_bytes());
        assert!(contains(consumer, &dimensions));
        let call_index = consumer
            .windows(3)
            .position(|window| window[0] == 0xE8)
            .expect("consumer call");
        let call_site = &consumer[call_index..call_index + 3];
        let displacement = i16::from_le_bytes([call_site[1], call_site[2]]);
        let call_logical = site + call_index;
        let target = usize::try_from((call_logical + 3) as isize + displacement as isize).unwrap();
        assert_eq!(target, usize::from(blitter));
    }
}
