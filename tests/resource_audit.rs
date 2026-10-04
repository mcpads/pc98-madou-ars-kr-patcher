use pc98_madou_ars::resource_audit::audit_resource;

#[path = "common/mod.rs"]
mod common;

const DEMO_DISK: &str = "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Demo disk).hdm";
const RULUE_DATA: &str =
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Rurue Data disk).hdm";
const SCHEZO_DATA: &str =
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (She-zo Data disk).hdm";
const AUDIT_DISKS: [&str; 7] = [
    DEMO_DISK,
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Arle Game disk).hdm",
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Arle Data disk).hdm",
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Rurue Game disk).hdm",
    RULUE_DATA,
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (She-zo Game disk).hdm",
    SCHEZO_DATA,
];

fn visual_scene_dat(name: &str) -> bool {
    name.ends_with(".DAT")
        && (name.starts_with("RO")
            || name.starts_with("RTU")
            || name.starts_with("RE")
            || name.starts_with('S'))
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn all_rulue_and_schezo_scene_dats_are_exact_single_lz_streams() {
    let mut count = 0usize;
    for path in [RULUE_DATA, SCHEZO_DATA] {
        let Some(disk) = common::try_read(path) else {
            return;
        };
        let hdm = pc98_madou_ars::hdm::HdmFile::parse(&disk).unwrap();
        let volume = pc98_madou_ars::fat12::Fat12Volume::open(hdm.as_bytes()).unwrap();
        for entry in volume
            .list_files()
            .into_iter()
            .filter(|entry| visual_scene_dat(&entry.name))
        {
            let bytes = volume.read_entry(&entry).unwrap();
            let audit = audit_resource(&bytes, 8, 5);
            let lz = audit
                .lz
                .unwrap_or_else(|| panic!("{} is not A.R.S LZ", entry.name));
            assert!(lz.exact, "{} has a trailing non-LZ tail", entry.name);
            assert_eq!(lz.streams.len(), 1, "{} stream count", entry.name);
            count += 1;
        }
    }
    assert_eq!(count, 76);
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn demo_op1_has_the_recorded_four_stream_shape() {
    let Some(disk) = common::try_read(DEMO_DISK) else {
        return;
    };
    let bytes = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "OP1.CNS").unwrap();
    let audit = audit_resource(&bytes, 8, 5);
    let lz = audit.lz.unwrap();
    assert!(lz.exact);
    assert_eq!(
        lz.streams
            .iter()
            .map(|stream| stream.decoded_size)
            .collect::<Vec<_>>(),
        [32_000, 32_128, 32_000, 32_000]
    );
    assert_eq!(
        audit.packed.sha256,
        "175064a5ae48e0ab2e57ce484a0da03310477a6d5c0afeda7de910e83440bd03"
    );
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn demo_op1_renders_as_four_brgi_screen_planes() {
    let Some(disk) = common::try_read(DEMO_DISK) else {
        return;
    };
    let bytes = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "OP1.CNS").unwrap();
    let rendered = pc98_madou_ars::graphics_resource::render_screen_resource(&bytes).unwrap();
    assert_eq!(
        rendered.layout,
        pc98_madou_ars::graphics_resource::ScreenLayout::BrgiPlanes
    );
    assert_eq!(rendered.stream_sizes, [32_000, 32_128, 32_000, 32_000]);
    assert_eq!(rendered.metadata_tail_bytes, [0, 128, 0, 0]);
    assert!(rendered.companion_stream_sizes.is_empty());
    assert!(rendered.companion_audio.is_none());
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn demo_nocopy_preserves_empty_planes_and_renders_the_warning() {
    let Some(disk) = common::try_read(DEMO_DISK) else {
        return;
    };
    let bytes = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "NOCOPY.CNS").unwrap();
    assert_eq!(bytes.len(), 5_183);
    assert_eq!(
        pc98_madou_ars::media_identity::sha256_hex(&bytes),
        "901f21951580e45d7ebf7947d79ac4f53f09a3f83a6ca4b417a3f01fbc30f580"
    );

    let mut offset = 0usize;
    let mut decoded_sizes = Vec::new();
    let mut packed_sizes = Vec::new();
    while offset < bytes.len() {
        let report = pc98_madou_ars::overlay_lz::decode_overlay_lz(&bytes[offset..]).unwrap();
        decoded_sizes.push(report.output.len());
        packed_sizes.push(report.bytes_consumed);
        offset += report.bytes_consumed;
    }
    assert_eq!(offset, bytes.len());
    assert_eq!(decoded_sizes, [0, 0, 32_000, 32_000]);
    assert_eq!(packed_sizes, [1, 1, 4_324, 857]);

    let rendered =
        pc98_madou_ars::graphics_resource::render_named_screen_resource("NOCOPY.CNS", &bytes)
            .unwrap();
    assert_eq!(
        rendered.layout,
        pc98_madou_ars::graphics_resource::ScreenLayout::BrgiPlanes
    );
    assert_eq!(rendered.stream_sizes, [0, 0, 32_000, 32_000]);
    assert_eq!(rendered.metadata_tail_bytes, [0, 0, 0, 0]);
    assert_eq!(
        pc98_madou_ars::media_identity::sha256_hex(&rendered.rgb),
        "cd86fd7e2cc6b892c480eb64fc8ec28c9a643b4d2704fe8da29a5c6764e1d353"
    );
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn gaoo_conditional_nocopy_branch_routes_four_streams_to_brgi_vram() {
    let Some(disk) = common::try_read(DEMO_DISK) else {
        return;
    };
    let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "GAOO.OVL").unwrap();
    let decoded = pc98_madou_ars::overlay_lz::decode_overlay_lz(&packed)
        .unwrap()
        .output;
    assert_eq!(&decoded[0x04CB..0x04D8], b"6:NOCOPY.CNS\0");

    // The conditional branch loads logical offset 0x05CB (file offset 0x04CB),
    // then passes four standard graphics-VRAM segments to INT 7Ch / AH=4.
    let signature = [
        0xBA, 0xCB, 0x05, 0x8B, 0x3E, 0xC0, 0x09, 0xE8, 0x89, 0x01, 0x8E, 0x1E, 0xC0, 0x09, 0xB8,
        0x00, 0xA8, 0x8E, 0xC0, 0xBB, 0x00, 0xB0, 0xB9, 0x00, 0xB8, 0xBA, 0x00, 0xE0, 0x33, 0xF6,
        0xB4, 0x04, 0xCD, 0x7C,
    ];
    assert!(
        decoded
            .windows(signature.len())
            .any(|window| window == signature)
    );
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn demo_overlay_routes_op_buffers_to_brgi_vram_in_stream_order() {
    let Some(disk) = common::try_read(DEMO_DISK) else {
        return;
    };
    let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "ARS_DEMO.OVL").unwrap();
    let decoded = pc98_madou_ars::overlay_lz::decode_overlay_lz(&packed)
        .unwrap()
        .output;

    // OP1: AH=4 expands four streams into segment variables 678F/91/93/95.
    let load_signature = [
        0x2E, 0xA1, 0x8F, 0x67, 0x8E, 0xC0, 0x2E, 0x8B, 0x1E, 0x91, 0x67, 0x2E, 0x8B, 0x0E, 0x93,
        0x67, 0x2E, 0x8B, 0x16, 0x95, 0x67,
    ];
    assert!(
        decoded
            .windows(load_signature.len())
            .any(|window| window == load_signature)
    );

    // The graphics copy routine maps those same segments to A800/B000/B800/E000.
    let copy_signature = [
        0xB8, 0x00, 0xA8, 0x2E, 0x8E, 0x1E, 0x8F, 0x67, 0xE8, 0x22, 0x00, 0xB8, 0x00, 0xB0, 0x2E,
        0x8E, 0x1E, 0x91, 0x67, 0xE8, 0x17, 0x00, 0xB8, 0x00, 0xB8, 0x2E, 0x8E, 0x1E, 0x93, 0x67,
        0xE8, 0x0C, 0x00, 0xB8, 0x00, 0xE0, 0x2E, 0x8E, 0x1E, 0x95, 0x67,
    ];
    assert!(
        decoded
            .windows(copy_signature.len())
            .any(|window| window == copy_signature)
    );

    // One-stream OP resources decode into 679F, whose mask routine applies the
    // same 1bpp words to all four destination planes.
    let mask_load_signature = [
        0x2E, 0x8E, 0x06, 0x9F, 0x67, 0xBF, 0x00, 0x00, 0xB4, 0x03, 0xCD, 0x7C,
    ];
    let mask_copy_signature = [
        0x2E, 0x8E, 0x1E, 0x9F, 0x67, 0xB8, 0x00, 0xA8, 0xE8, 0x13, 0x00, 0xB8, 0x00, 0xB0, 0xE8,
        0x0D, 0x00, 0xB8, 0x00, 0xB8, 0xE8, 0x07, 0x00, 0xB8, 0x00, 0xE0, 0xE8, 0x01, 0x00,
    ];
    assert!(
        decoded
            .windows(mask_load_signature.len())
            .any(|window| window == mask_load_signature)
    );
    assert!(
        decoded
            .windows(mask_copy_signature.len())
            .any(|window| window == mask_copy_signature)
    );
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn all_twenty_demo_op_resources_have_a_proven_screen_layout() {
    let Some(disk) = common::try_read(DEMO_DISK) else {
        return;
    };
    let hdm = pc98_madou_ars::hdm::HdmFile::parse(&disk).unwrap();
    let volume = pc98_madou_ars::fat12::Fat12Volume::open(hdm.as_bytes()).unwrap();
    let mut names = volume
        .list_files()
        .into_iter()
        .filter(|entry| entry.name.starts_with("OP") && entry.name.ends_with(".CNS"))
        .map(|entry| {
            let packed = volume.read_entry(&entry).unwrap();
            pc98_madou_ars::graphics_resource::render_named_screen_resource(&entry.name, &packed)
                .unwrap_or_else(|error| panic!("{}: {error}", entry.name));
            entry.name
        })
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names.len(), 20);
    for number in 1..=20 {
        assert!(names.contains(&format!("OP{number}.CNS")));
    }
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn seven_disk_resource_audit_matches_the_recorded_classification() {
    let accepted_extensions = ["DAT", "OVL", "CNS", "CS", "Z04"];
    let mut total = 0usize;
    let mut exact = 0usize;
    let mut partial = 0usize;
    let mut none = 0usize;
    let mut cns_cs = 0usize;
    let mut exact_cns_cs = 0usize;
    for path in AUDIT_DISKS {
        let Some(disk) = common::try_read(path) else {
            return;
        };
        let hdm = pc98_madou_ars::hdm::HdmFile::parse(&disk).unwrap();
        let volume = pc98_madou_ars::fat12::Fat12Volume::open(hdm.as_bytes()).unwrap();
        for entry in volume.list_files() {
            let extension = entry.name.rsplit_once('.').map(|(_, extension)| extension);
            if !extension.is_some_and(|extension| accepted_extensions.contains(&extension)) {
                continue;
            }
            let bytes = volume.read_entry(&entry).unwrap();
            let audit = audit_resource(&bytes, 8, 5);
            let is_cns_cs = matches!(extension, Some("CNS" | "CS"));
            total += 1;
            cns_cs += usize::from(is_cns_cs);
            match audit.lz {
                Some(lz) if lz.exact => {
                    exact += 1;
                    exact_cns_cs += usize::from(is_cns_cs);
                }
                Some(_) => partial += 1,
                None => none += 1,
            }
        }
    }
    assert_eq!((total, exact, partial, none), (255, 247, 6, 2));
    assert_eq!((cns_cs, exact_cns_cs), (62, 61));
}
