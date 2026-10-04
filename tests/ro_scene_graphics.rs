use pc98_madou_ars::graphics_resource::{DecodedRange, ScreenLayout, render_named_screen_resource};

#[path = "common/mod.rs"]
mod common;

const RULUE_DATA: &str =
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Rurue Data disk).hdm";
const RULUE_GAME: &str =
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Rurue Game disk).hdm";
const LOGICAL_ORIGIN: usize = 0x0100;
const RO1_DIRECTLY_UNCONSUMED: [DecodedRange; 2] = [
    DecodedRange {
        offset: 0x2B80,
        bytes: 0x2100,
    },
    DecodedRange {
        offset: 0xAE80,
        bytes: 0x1000,
    },
];

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
    call_site: usize,
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

fn call_target(overlay: &[u8], call_site: usize) -> usize {
    let call = logical_slice(overlay, call_site, 3);
    assert_eq!(call[0], 0xE8, "call opcode at 0x{call_site:04X}");
    let displacement = i16::from_le_bytes([call[1], call[2]]);
    usize::try_from((call_site + 3) as isize + displacement as isize).unwrap()
}

fn assert_consumer(overlay: &[u8], case: ConsumerCase) {
    let setup = logical_slice(overlay, case.call_site - 0x30, 0x30);

    let mut source = vec![0xBE];
    source.extend_from_slice(&case.source.to_le_bytes());
    assert!(
        contains(setup, &source),
        "source at call 0x{:04X}",
        case.call_site
    );

    let mut segment = vec![0x2E, 0x8E, 0x1E];
    segment.extend_from_slice(&case.segment_variable.to_le_bytes());
    assert!(
        contains(setup, &segment),
        "segment variable at call 0x{:04X}",
        case.call_site
    );

    let mut dimensions = vec![0xBA];
    dimensions.extend_from_slice(&case.width_argument.to_le_bytes());
    dimensions.push(0xB9);
    dimensions.extend_from_slice(&case.height.to_le_bytes());
    assert!(
        contains(setup, &dimensions),
        "dimensions at call 0x{:04X}",
        case.call_site
    );
    assert_eq!(
        call_target(overlay, case.call_site),
        case.target,
        "target at call 0x{:04X}",
        case.call_site
    );
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
    assert!(contains(loader, &open), "open at loader 0x{site:04X}");

    let mut decode = vec![0x2E, 0x8E, 0x06];
    decode.extend_from_slice(&segment_variable.to_le_bytes());
    decode.extend_from_slice(&[0xBF, 0x00, 0x00, 0xB4, 0x03, 0xCD, 0x7C]);
    assert!(contains(loader, &decode), "decode at loader 0x{site:04X}");
}

fn assert_strided_configuration(
    overlay: &[u8],
    call_site: usize,
    row_pitch: u16,
    plane_stride: u16,
) {
    let setup = logical_slice(overlay, call_site - 0x30, 0x30);
    let mut pitch = vec![0x2E, 0xC7, 0x06, 0x5F, 0x48];
    pitch.extend_from_slice(&row_pitch.to_le_bytes());
    assert!(contains(setup, &pitch), "row pitch at 0x{call_site:04X}");
    let mut stride = vec![0x2E, 0xC7, 0x06, 0x63, 0x48];
    stride.extend_from_slice(&plane_stride.to_le_bytes());
    assert!(
        contains(setup, &stride),
        "plane stride at 0x{call_site:04X}"
    );
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/"]
fn exact_ro_resources_render_with_consumer_proven_scene_layouts() {
    let Some(disk) = common::try_read(RULUE_DATA) else {
        return;
    };
    let cases = [
        ResourceCase {
            name: "RO1.DAT",
            packed_size: 14_459,
            packed_sha256: "a40c7fb0d740f323db008736dcb430b389b2a50710adf6895af0666816dff501",
            decoded_size: 49_152,
            decoded_sha256: "345ae93a8591ec3b0daf550f9f455887f5c400501f190aaeb85633ff373650ec",
            rgb_sha256: "6e257d7b14aef9d8b7b4ac84e139e8197f496eea9d406815d5b09859b62e271c",
            unrendered_ranges: &RO1_DIRECTLY_UNCONSUMED,
        },
        ResourceCase {
            name: "RO2.DAT",
            packed_size: 17_938,
            packed_sha256: "9b64d069014d8b8a3ec7b9b1398e93a4e82f653271c2c6960d8c44d9ba18a643",
            decoded_size: 27_648,
            decoded_sha256: "cbb0e58415243c3845beb0ff613d8bef977fc142fcc414905c154cdeb53b292c",
            rgb_sha256: "f3d9298ce17b92770c20674e38df344a21861390e7ab0722f9b6bab0a6a56f1b",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "RO3.DAT",
            packed_size: 36_635,
            packed_sha256: "2aa9fdd39f8a321570f8edaa30661ec1a20483430d800b762d63854e4f75eeee",
            decoded_size: 55_296,
            decoded_sha256: "4efbdfc762f06aec81a0c22d4595eb3a2011a800bbadc7d2d7ec08d326b0290f",
            rgb_sha256: "c32ba9796df7258e5b7cad3aefa63bdf231f197bbcfc2f78fef45bcad7ae6120",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "RO5.DAT",
            packed_size: 34_341,
            packed_sha256: "1c1e796026485cc2137b56b44e58b9a31044641e2a229f5844fa77538fcb3f37",
            decoded_size: 55_456,
            decoded_sha256: "885f1254bad5820b830de0d582d5360f6b5728e51c182024bed2e08cdd73fbf0",
            rgb_sha256: "132567708eb03761c6d08ffad28ec1d8c9362931205fcf4227b2f3ba2c408f35",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "RO6.DAT",
            packed_size: 27_638,
            packed_sha256: "483c87e1ec1a80847a3a3aef6a19d6db1d3715e3d602a03b9114ab1f5f085386",
            decoded_size: 44_736,
            decoded_sha256: "25d463059925eaf318c89d587cda88e8bdd6f368f9754a488ca7fae8dc7d0814",
            rgb_sha256: "f59c11ada1d8ae6b1e90b0ea25fea548866dc41475a3cb28cf9de8d8b45f8d7e",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "RO9.DAT",
            packed_size: 15_745,
            packed_sha256: "d895609a15c2118312a21edcdaae25577af24dd179a2dcda3cfec2e756cfcee8",
            decoded_size: 27_648,
            decoded_sha256: "23a75f093989216d8e3d22c78c4b32732427e98062db5272f2565032aa72335a",
            rgb_sha256: "d2898bf39b6a61aaf27d7e8f3e0b476b0a51a46db6cabe39db7c75d4692d91ad",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "RO10.DAT",
            packed_size: 49_458,
            packed_sha256: "79ed7fe0f8d7f773c61c53a85b41fa612dbce4a4acddaa8a2e5cc8c3733b841a",
            decoded_size: 63_648,
            decoded_sha256: "ac2842d44d52ed319cb24e049a2012c2eb3b4aed6e68dd5c866596fa8879d476",
            rgb_sha256: "b7ba405a1b7fce29cea67b2747105c54b25cc00ed29780318737b5d826654a3e",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "RO11.DAT",
            packed_size: 18_872,
            packed_sha256: "3fd79896dc3c73cee8d329d3cc91270158c4612dcc2e87b84560f2fecaf1e4f2",
            decoded_size: 33_792,
            decoded_sha256: "58d19391f92bd9239f709aa76d59634e6b4abc0c726038926a9de5e136294955",
            rgb_sha256: "d52159f02f37ce469883fd869490564917db082bf16df8c562d3f935bfe9eb38",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "RO12.DAT",
            packed_size: 30_260,
            packed_sha256: "3085ed01c3c553e4db49a4aab6dcec468114316e415304752816783a323cc921",
            decoded_size: 51_072,
            decoded_sha256: "1347ed94beb15b08cd0956c268d1a74d5b7ac79005731c43840fc8e82b2d232e",
            rgb_sha256: "458ba6626bb0d7a4983c32cc5881d5f2c4f258bb9307738bea1173235d77967d",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "RO15.DAT",
            packed_size: 15_124,
            packed_sha256: "f19476992396a3b8d912eedec4601cd2a48ff3c83d017379c1df551f543873b7",
            decoded_size: 55_296,
            decoded_sha256: "70f5a96ed5b5867e8c7ba83c5fdd42fa39311a07116e834d12c199126e484669",
            rgb_sha256: "e8f57496744d186e44297097ecf27638925da002472da7ab7ea2c885614368e9",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "RO15_1.DAT",
            packed_size: 6_733,
            packed_sha256: "e09ff70a17a50d2c9cbbfa9b71eabe38cc8e4266b1230d5a216578b2d61d2378",
            decoded_size: 32_160,
            decoded_sha256: "3e0eae9ce4b0fa4ad183f1c09e16c30381708d3bf3a1f1c5b3e5a155f4360dcb",
            rgb_sha256: "d672a65695d0c98cbac019b3e4f7ddac3f38d00d4d5befa22ca7f945524bc04b",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "RO18.DAT",
            packed_size: 11_213,
            packed_sha256: "c8ea20716ad8975ee6cab38f4bde26beb429bff87e6a51c22cd95719442c97d3",
            decoded_size: 35_328,
            decoded_sha256: "d720f0e38072c5b6cc6d1bfaad903fc2ad764a62d03638fe0677e4980464d872",
            rgb_sha256: "b9f1c95e22a321a44370d94a3921617b248ff9c0c97f623bf32ae28546115e56",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "RO19.DAT",
            packed_size: 10_740,
            packed_sha256: "3384ab74142c8835f6d8063f2e8ec149b27df27f4f394c0ec33e7c4973498cdf",
            decoded_size: 21_856,
            decoded_sha256: "7bb877296c1c846ee6855d5659d9ef0558732041cfdf155027754edbfba7ad47",
            rgb_sha256: "bc4b873566952b354aa7581f90ba0b18d7d4e19d643943ddefed0dbeefb9eacd",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "RO20.DAT",
            packed_size: 17_700,
            packed_sha256: "67b339498a6108fcf3cdaf840dde2cfe849b3f7fdf5a84d65ba14b55e2c5aec9",
            decoded_size: 36_864,
            decoded_sha256: "516e1a6ad43e1076af511ffedc6d1d25e0a1f646b626e351485c92562ab84bc0",
            rgb_sha256: "026275a0794b5e16378b237fe0f9d698bfbb88922e252003b58d6e65eeb1419e",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "RO21.DAT",
            packed_size: 23_182,
            packed_sha256: "7f0d9c14fc6c9b00f18de5b9ef0e72597577b402cc6399ce749ebd06f294281a",
            decoded_size: 55_808,
            decoded_sha256: "c502345fda0b1d4b4b6abbdd4bcd7763a089919d433fdffcc012c8cd609fa312",
            rgb_sha256: "158a4a6feed8c4b36f43b316cd991f1fd0c7e2c644fbea55f83012da69c547f1",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "RO22.DAT",
            packed_size: 19_456,
            packed_sha256: "28ceedff3260c695f84a0f2f834f677aa9e211626655968c2a81102bd5290da9",
            decoded_size: 32_352,
            decoded_sha256: "a256261c9c64bbcc8463f793e5869fb383c24aceff0dd8a177d6120807069f10",
            rgb_sha256: "a1fd1d1020badc2d9eca0ff2f96ef87fa187b663e96ff039d3d0aab545e30052",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "RO23.DAT",
            packed_size: 27_171,
            packed_sha256: "8d8f9d567c6b036706fb17f892d6943421950ce094fb7455638b61d00e8c3a9c",
            decoded_size: 55_296,
            decoded_sha256: "8792e1f254d1f5073fb85fc58f461ad34b9b58ce28bcd579c3f96b451127b15a",
            rgb_sha256: "0fc6fa41804d2ad37888e6837313e50236a28acafbd2be5d9b0687483826de10",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "RO23_1.DAT",
            packed_size: 14_022,
            packed_sha256: "06c606fa091436de59d7d1883ff73b39c8411983223a4f9ec6732e474aae495c",
            decoded_size: 27_648,
            decoded_sha256: "2d3f71a9d14ba537876989785801724016efe8737bed25fe90ff8de748eb28ae",
            rgb_sha256: "80932e0714f75416ef5e4a2883b55dc029fb569e79935038df8e23345cad58e3",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "RO24.DAT",
            packed_size: 31_102,
            packed_sha256: "6b0b6955b3a5cc2aad3e2886de40d3bc48cecf25474ce984f8f9961e8e1ba746",
            decoded_size: 45_376,
            decoded_sha256: "abce0012fd3724a6d893eb34a129d29d362653e2d1c9754c272cfd2fcbb573f1",
            rgb_sha256: "38a0742374b3e6ca403fc25b516952cb6db97bc83081f3c98318a04c7f0f449c",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "RO30.DAT",
            packed_size: 17_389,
            packed_sha256: "bf4c9ffb03a97c120a932628ffbdb6cd8f0b325283983473dc43a6437fb506a6",
            decoded_size: 39_616,
            decoded_sha256: "e03963aa6eec0f865fc58fa0af59ccaa63f10bec66da65804c555fa3b1bbbff2",
            rgb_sha256: "51db1b2cf88ede9aeb1df7d3ced901e860d46621001f6674af47ac7ffbf73d94",
            unrendered_ranges: &[],
        },
        ResourceCase {
            name: "RO31.DAT",
            packed_size: 14_277,
            packed_sha256: "d8ffbd367a9a2fdc8ba9e95a8cea2305a9f835247a222f852250e76869d95dfa",
            decoded_size: 26_752,
            decoded_sha256: "6722aced496e9c3a0cdab2ed08370660c36fcf55bbe029e12e03874e16b605c0",
            rgb_sha256: "f9aafbcb07b43a7937140abb7bb0c23764980cec6b259d505e18f0ec457fdcd4",
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
fn opening_r_loads_and_consumes_all_twenty_one_ro_resources() {
    let Some(disk) = common::try_read(RULUE_GAME) else {
        return;
    };
    let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "OPENINGR.OVL").unwrap();
    assert_eq!(packed.len(), 12_276);
    assert_eq!(
        pc98_madou_ars::media_identity::sha256_hex(&packed),
        "174daf7ee7c9ba53d873753b68677892156916ab0f6acb0e608dd328c58a1347"
    );
    let report = pc98_madou_ars::overlay_lz::decode_overlay_lz(&packed).unwrap();
    assert_eq!(report.bytes_consumed, packed.len());
    assert_eq!(report.output.len(), 20_595);
    assert_eq!(
        pc98_madou_ars::media_identity::sha256_hex(&report.output),
        "4decd9e3c1532b7979813fff16dde7e747b05c2baf7f788f8d53951ed4633fc7"
    );
    let overlay = report.output;

    for (address, name) in [
        (0x0223, b"ro1.dat\0".as_slice()),
        (0x022B, b"ro2.dat\0".as_slice()),
        (0x0233, b"ro3.dat\0".as_slice()),
        (0x023B, b"ro5.dat\0".as_slice()),
        (0x0243, b"ro6.dat\0".as_slice()),
        (0x024B, b"ro9.dat\0".as_slice()),
        (0x0253, b"ro10.dat\0".as_slice()),
        (0x025C, b"ro11.dat\0".as_slice()),
        (0x0265, b"ro12.dat\0".as_slice()),
        (0x026E, b"ro15.dat\0".as_slice()),
        (0x0277, b"ro15_1.dat\0".as_slice()),
        (0x0282, b"ro18.dat\0".as_slice()),
        (0x028B, b"ro19.dat\0".as_slice()),
        (0x0294, b"ro20.dat\0".as_slice()),
        (0x029D, b"ro21.dat\0".as_slice()),
        (0x02A6, b"ro22.dat\0".as_slice()),
        (0x02AF, b"ro23.dat\0".as_slice()),
        (0x02B8, b"ro23_1.dat\0".as_slice()),
        (0x02C3, b"ro24.dat\0".as_slice()),
        (0x02CC, b"ro30.dat\0".as_slice()),
        (0x02D5, b"ro31.dat\0".as_slice()),
    ] {
        assert_eq!(logical_slice(&overlay, address, name.len()), name);
    }
    assert_eq!(logical_slice(&overlay, 0x02E9, 10), b"ARS_RX.CS\0");

    for (site, filename, destination) in [
        (0x0401, 0x0223u16, 0x4816u16),
        (0x043D, 0x022B, 0x4818),
        (0x0479, 0x0233, 0x481A),
        (0x04B5, 0x023B, 0x481C),
        (0x04F1, 0x0243, 0x481E),
        (0x052D, 0x024B, 0x4820),
        (0x0569, 0x0253, 0x4822),
        (0x082A, 0x026E, 0x481A),
        (0x08BC, 0x0265, 0x481C),
        (0x0A5B, 0x025C, 0x481E),
        (0x0AED, 0x028B, 0x4820),
        (0x0B7C, 0x0294, 0x4822),
        (0x0C4D, 0x0277, 0x481E),
        (0x0E02, 0x0282, 0x4816),
        (0x0EBB, 0x029D, 0x481A),
        (0x0FB5, 0x02AF, 0x481C),
        (0x10D5, 0x02C3, 0x4822),
        (0x11A7, 0x02A6, 0x481E),
        (0x1233, 0x02B8, 0x4820),
        (0x1353, 0x0243, 0x481E),
        (0x1438, 0x02CC, 0x481C),
        (0x14C7, 0x02D5, 0x4820),
        (0x160E, 0x026E, 0x4822),
        (0x1715, 0x0277, 0x481A),
    ] {
        assert_loader(&overlay, site, filename, destination);
    }

    let row_interleaved = logical_slice(&overlay, 0x3D24, 0x31);
    assert!(contains(
        row_interleaved,
        &[
            0xAC, 0xE6, 0x7E, 0x8A, 0xE0, 0xAC, 0xE6, 0x7E, 0x0A, 0xE0, 0xAC, 0xE6, 0x7E, 0x0A,
            0xE0, 0xAC, 0xE6, 0x7E,
        ]
    ));
    assert!(contains(row_interleaved, &[0xAA, 0xE2, 0xE9]));
    for (site, opcode) in [(0x3D55, 0xA4), (0x3D85, 0xA5)] {
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
    let strided_word = logical_slice(&overlay, 0x3DB5, 0x40);
    assert!(contains(strided_word, &[0xF3, 0xA5]));
    assert!(contains(strided_word, &[0x2E, 0x03, 0x36, 0x5F, 0x48]));
    assert!(contains(strided_word, &[0x2E, 0x03, 0x36, 0x63, 0x48]));
    let strided_byte = logical_slice(&overlay, 0x3EC9, 0x40);
    assert!(contains(strided_byte, &[0xF3, 0xA4]));
    assert!(contains(strided_byte, &[0x2E, 0x03, 0x36, 0x5F, 0x48]));
    assert!(contains(strided_byte, &[0x2E, 0x03, 0x36, 0x63, 0x48]));

    assert_consumers(
        &overlay,
        &[
            // RO1 direct plane-major regions.
            ConsumerCase {
                call_site: 0x1C21,
                source: 0x0000,
                segment_variable: 0x4816,
                width_argument: 0x01,
                height: 0x10,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x1C3B,
                source: 0x0080,
                segment_variable: 0x4816,
                width_argument: 0x01,
                height: 0x10,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x1C55,
                source: 0x0100,
                segment_variable: 0x4816,
                width_argument: 0x01,
                height: 0x10,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x1C6F,
                source: 0x0180,
                segment_variable: 0x4816,
                width_argument: 0x01,
                height: 0x10,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x1C89,
                source: 0x0200,
                segment_variable: 0x4816,
                width_argument: 0x01,
                height: 0x10,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x1CA3,
                source: 0x0280,
                segment_variable: 0x4816,
                width_argument: 0x01,
                height: 0x10,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x1CBD,
                source: 0x0300,
                segment_variable: 0x4816,
                width_argument: 0x01,
                height: 0x10,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x1CD7,
                source: 0x0380,
                segment_variable: 0x4816,
                width_argument: 0x01,
                height: 0x10,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x1CF1,
                source: 0x0400,
                segment_variable: 0x4816,
                width_argument: 0x01,
                height: 0x10,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x1D0B,
                source: 0x0480,
                segment_variable: 0x4816,
                width_argument: 0x01,
                height: 0x10,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x1D25,
                source: 0x0500,
                segment_variable: 0x4816,
                width_argument: 0x01,
                height: 0x10,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x1D3B,
                source: 0x0580,
                segment_variable: 0x4816,
                width_argument: 0x02,
                height: 0x10,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x1D51,
                source: 0x0680,
                segment_variable: 0x4816,
                width_argument: 0x02,
                height: 0x20,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x1D67,
                source: 0x0880,
                segment_variable: 0x4816,
                width_argument: 0x05,
                height: 0x30,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x1D7D,
                source: 0x1000,
                segment_variable: 0x4816,
                width_argument: 0x02,
                height: 0x40,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x1D93,
                source: 0x1400,
                segment_variable: 0x4816,
                width_argument: 0x03,
                height: 0x50,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x1DA9,
                source: 0x1B80,
                segment_variable: 0x4816,
                width_argument: 0x02,
                height: 0x70,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x1DBF,
                source: 0x2280,
                segment_variable: 0x4816,
                width_argument: 0x02,
                height: 0x90,
                target: 0x3D85,
            },
            // RO2, RO3, RO5.
            ConsumerCase {
                call_site: 0x2007,
                source: 0x0000,
                segment_variable: 0x4818,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x201E,
                source: 0x0000,
                segment_variable: 0x481A,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x2035,
                source: 0x6C00,
                segment_variable: 0x481A,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x204C,
                source: 0x0000,
                segment_variable: 0x481C,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x2063,
                source: 0xD5E0,
                segment_variable: 0x481C,
                width_argument: 0x0B,
                height: 0x10,
                target: 0x3D55,
            },
            ConsumerCase {
                call_site: 0x207A,
                source: 0x6C00,
                segment_variable: 0x481C,
                width_argument: 0x15,
                height: 0x60,
                target: 0x3D55,
            },
            ConsumerCase {
                call_site: 0x2091,
                source: 0x8B80,
                segment_variable: 0x481C,
                width_argument: 0x15,
                height: 0x88,
                target: 0x3D55,
            },
            ConsumerCase {
                call_site: 0x20AB,
                source: 0xB820,
                segment_variable: 0x481C,
                width_argument: 0x07,
                height: 0x88,
                target: 0x3D85,
            },
            // RO6, including the formerly misclassified adjacent small parts.
            ConsumerCase {
                call_site: 0x20C2,
                source: 0x0000,
                segment_variable: 0x481E,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x21FE,
                source: 0x6C00,
                segment_variable: 0x481E,
                width_argument: 0x01,
                height: 0x08,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x21E8,
                source: 0x6C40,
                segment_variable: 0x481E,
                width_argument: 0x03,
                height: 0x20,
                target: 0x3D55,
            },
            ConsumerCase {
                call_site: 0x2215,
                source: 0x6DC0,
                segment_variable: 0x481E,
                width_argument: 0x03,
                height: 0x20,
                target: 0x3D55,
            },
            ConsumerCase {
                call_site: 0x222C,
                source: 0x6F40,
                segment_variable: 0x481E,
                width_argument: 0x03,
                height: 0x20,
                target: 0x3D55,
            },
            ConsumerCase {
                call_site: 0x2243,
                source: 0x70C0,
                segment_variable: 0x481E,
                width_argument: 0x03,
                height: 0x20,
                target: 0x3D55,
            },
            ConsumerCase {
                call_site: 0x225A,
                source: 0x7240,
                segment_variable: 0x481E,
                width_argument: 0x03,
                height: 0x20,
                target: 0x3D55,
            },
            ConsumerCase {
                call_site: 0x2271,
                source: 0x73C0,
                segment_variable: 0x481E,
                width_argument: 0x03,
                height: 0x20,
                target: 0x3D55,
            },
            ConsumerCase {
                call_site: 0x2288,
                source: 0x7540,
                segment_variable: 0x481E,
                width_argument: 0x03,
                height: 0x20,
                target: 0x3D55,
            },
            ConsumerCase {
                call_site: 0x229F,
                source: 0x76C0,
                segment_variable: 0x481E,
                width_argument: 0x05,
                height: 0x20,
                target: 0x3D55,
            },
            ConsumerCase {
                call_site: 0x20D8,
                source: 0x7940,
                segment_variable: 0x481E,
                width_argument: 0x02,
                height: 0x40,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x20EE,
                source: 0x7B40,
                segment_variable: 0x481E,
                width_argument: 0x0C,
                height: 0x90,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x2104,
                source: 0x9640,
                segment_variable: 0x481E,
                width_argument: 0x18,
                height: 0x20,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x211A,
                source: 0xA240,
                segment_variable: 0x481E,
                width_argument: 0x02,
                height: 0x80,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x2130,
                source: 0xA640,
                segment_variable: 0x481E,
                width_argument: 0x02,
                height: 0x60,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x2146,
                source: 0xA940,
                segment_variable: 0x481E,
                width_argument: 0x02,
                height: 0x50,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x215C,
                source: 0xABC0,
                segment_variable: 0x481E,
                width_argument: 0x02,
                height: 0x30,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x2172,
                source: 0xAD40,
                segment_variable: 0x481E,
                width_argument: 0x02,
                height: 0x20,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x2188,
                source: 0xAE40,
                segment_variable: 0x481E,
                width_argument: 0x02,
                height: 0x10,
                target: 0x3D24,
            },
            // RO9 and RO10.
            ConsumerCase {
                call_site: 0x2303,
                source: 0x0000,
                segment_variable: 0x4820,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x231A,
                source: 0x0000,
                segment_variable: 0x4822,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x2334,
                source: 0x6C00,
                segment_variable: 0x4822,
                width_argument: 0x03,
                height: 0x80,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x234A,
                source: 0x7800,
                segment_variable: 0x4822,
                width_argument: 0x09,
                height: 0x98,
                target: 0x3D55,
            },
            ConsumerCase {
                call_site: 0x2364,
                source: 0x8D60,
                segment_variable: 0x4822,
                width_argument: 0x01,
                height: 0x48,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x237A,
                source: 0x8FA0,
                segment_variable: 0x4822,
                width_argument: 0x05,
                height: 0x78,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x2390,
                source: 0xA260,
                segment_variable: 0x4822,
                width_argument: 0x09,
                height: 0x98,
                target: 0x3D55,
            },
            ConsumerCase {
                call_site: 0x23AA,
                source: 0xB7C0,
                segment_variable: 0x4822,
                width_argument: 0x04,
                height: 0x48,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x23C0,
                source: 0xC0C0,
                segment_variable: 0x4822,
                width_argument: 0x02,
                height: 0x68,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x23D6,
                source: 0xC740,
                segment_variable: 0x4822,
                width_argument: 0x09,
                height: 0x98,
                target: 0x3D55,
            },
            ConsumerCase {
                call_site: 0x23F0,
                source: 0xDCA0,
                segment_variable: 0x4822,
                width_argument: 0x01,
                height: 0x08,
                target: 0x3D55,
            },
            ConsumerCase {
                call_site: 0x2406,
                source: 0xDCC0,
                segment_variable: 0x4822,
                width_argument: 0x02,
                height: 0x68,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x241C,
                source: 0xE340,
                segment_variable: 0x4822,
                width_argument: 0x09,
                height: 0x98,
                target: 0x3D55,
            },
            // RO11, RO12, RO15, RO15_1, and RO18.
            ConsumerCase {
                call_site: 0x2466,
                source: 0x0008,
                segment_variable: 0x481E,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x3DB5,
            },
            ConsumerCase {
                call_site: 0x24E1,
                source: 0x0000,
                segment_variable: 0x481C,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x3DB5,
            },
            ConsumerCase {
                call_site: 0x251D,
                source: 0x8B80,
                segment_variable: 0x481C,
                width_argument: 0x03,
                height: 0x18,
                target: 0x3D55,
            },
            ConsumerCase {
                call_site: 0x2534,
                source: 0x8CA0,
                segment_variable: 0x481C,
                width_argument: 0x03,
                height: 0x18,
                target: 0x3D55,
            },
            ConsumerCase {
                call_site: 0x254B,
                source: 0x8DC0,
                segment_variable: 0x481C,
                width_argument: 0x03,
                height: 0x18,
                target: 0x3D55,
            },
            ConsumerCase {
                call_site: 0x2562,
                source: 0x8EE0,
                segment_variable: 0x481C,
                width_argument: 0x03,
                height: 0x18,
                target: 0x3D55,
            },
            ConsumerCase {
                call_site: 0x2781,
                source: 0x9000,
                segment_variable: 0x481C,
                width_argument: 0x02,
                height: 0x18,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x2798,
                source: 0x9180,
                segment_variable: 0x481C,
                width_argument: 0x02,
                height: 0x18,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x26AB,
                source: 0x9300,
                segment_variable: 0x481C,
                width_argument: 0x03,
                height: 0x10,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x26C1,
                source: 0x93C0,
                segment_variable: 0x481C,
                width_argument: 0x0D,
                height: 0x10,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x26D7,
                source: 0x9700,
                segment_variable: 0x481C,
                width_argument: 0x0F,
                height: 0x20,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x26ED,
                source: 0x9E80,
                segment_variable: 0x481C,
                width_argument: 0x02,
                height: 0x50,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x2703,
                source: 0xA100,
                segment_variable: 0x481C,
                width_argument: 0x11,
                height: 0x40,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x2719,
                source: 0xB200,
                segment_variable: 0x481C,
                width_argument: 0x13,
                height: 0x10,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x272F,
                source: 0xB6C0,
                segment_variable: 0x481C,
                width_argument: 0x15,
                height: 0x10,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x2745,
                source: 0xBC00,
                segment_variable: 0x481C,
                width_argument: 0x17,
                height: 0x20,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x2587,
                source: 0x240E,
                segment_variable: 0x481A,
                width_argument: 0x0B,
                height: 0x80,
                target: 0x3DB5,
            },
            ConsumerCase {
                call_site: 0x25A1,
                source: 0x0000,
                segment_variable: 0x481E,
                width_argument: 0x16,
                height: 0x40,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x25BB,
                source: 0x1600,
                segment_variable: 0x481E,
                width_argument: 0x14,
                height: 0x58,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x25D5,
                source: 0x3180,
                segment_variable: 0x481E,
                width_argument: 0x12,
                height: 0x60,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x25EF,
                source: 0x4C80,
                segment_variable: 0x481E,
                width_argument: 0x0F,
                height: 0x70,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x2609,
                source: 0x66C0,
                segment_variable: 0x481E,
                width_argument: 0x09,
                height: 0x78,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x2623,
                source: 0x77A0,
                segment_variable: 0x481E,
                width_argument: 0x03,
                height: 0x80,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x2695,
                source: 0x0000,
                segment_variable: 0x4816,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x3DB5,
            },
            // RO19 through RO24.
            ConsumerCase {
                call_site: 0x27AF,
                source: 0x0000,
                segment_variable: 0x4820,
                width_argument: 0x08,
                height: 0x98,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x27C6,
                source: 0x3320,
                segment_variable: 0x4820,
                width_argument: 0x09,
                height: 0x30,
                target: 0x3D55,
            },
            ConsumerCase {
                call_site: 0x27DD,
                source: 0x2600,
                segment_variable: 0x4820,
                width_argument: 0x07,
                height: 0x28,
                target: 0x3D55,
            },
            ConsumerCase {
                call_site: 0x27F4,
                source: 0x2A60,
                segment_variable: 0x4820,
                width_argument: 0x07,
                height: 0x28,
                target: 0x3D55,
            },
            ConsumerCase {
                call_site: 0x280B,
                source: 0x2EC0,
                segment_variable: 0x4820,
                width_argument: 0x07,
                height: 0x28,
                target: 0x3D55,
            },
            ConsumerCase {
                call_site: 0x2822,
                source: 0x39E0,
                segment_variable: 0x4820,
                width_argument: 0x0B,
                height: 0x28,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x2839,
                source: 0x40C0,
                segment_variable: 0x4820,
                width_argument: 0x0B,
                height: 0x28,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x2850,
                source: 0x47A0,
                segment_variable: 0x4820,
                width_argument: 0x0B,
                height: 0x28,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x2867,
                source: 0x4E80,
                segment_variable: 0x4820,
                width_argument: 0x0B,
                height: 0x28,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x28A9,
                source: 0x0000,
                segment_variable: 0x4822,
                width_argument: 0x10,
                height: 0xC0,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x28C6,
                source: 0x3000,
                segment_variable: 0x4822,
                width_argument: 0x10,
                height: 0xC0,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x28E3,
                source: 0x6000,
                segment_variable: 0x4822,
                width_argument: 0x10,
                height: 0xC0,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x28FA,
                source: 0x3C00,
                segment_variable: 0x481A,
                width_argument: 0x12,
                height: 0xC0,
                target: 0x3D85,
            },
            ConsumerCase {
                call_site: 0x2910,
                source: 0xA800,
                segment_variable: 0x481A,
                width_argument: 0x0C,
                height: 0x50,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x2926,
                source: 0xB700,
                segment_variable: 0x481A,
                width_argument: 0x1C,
                height: 0x50,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x293C,
                source: 0x0000,
                segment_variable: 0x481A,
                width_argument: 0x14,
                height: 0xC0,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x2B87,
                source: 0x0000,
                segment_variable: 0x481E,
                width_argument: 0x15,
                height: 0x38,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x29D9,
                source: 0x1260,
                segment_variable: 0x481E,
                width_argument: 0x24,
                height: 0xC0,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x2A18,
                source: 0x0000,
                segment_variable: 0x481C,
                width_argument: 0x24,
                height: 0xC0,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x2A32,
                source: 0x6C00,
                segment_variable: 0x481C,
                width_argument: 0x24,
                height: 0xC0,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x2A4C,
                source: 0x0000,
                segment_variable: 0x4820,
                width_argument: 0x24,
                height: 0xC0,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x2A95,
                source: 0x0000,
                segment_variable: 0x4822,
                width_argument: 0x12,
                height: 0xB8,
                target: 0x3DB5,
            },
            ConsumerCase {
                call_site: 0x2BB0,
                source: 0x6C00,
                segment_variable: 0x4822,
                width_argument: 0x04,
                height: 0x30,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x2BC6,
                source: 0x6F00,
                segment_variable: 0x4822,
                width_argument: 0x20,
                height: 0x60,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x2BDC,
                source: 0x9F00,
                segment_variable: 0x4822,
                width_argument: 0x0A,
                height: 0x60,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x2D5E,
                source: 0xAE00,
                segment_variable: 0x4822,
                width_argument: 0x03,
                height: 0x18,
                target: 0x3D55,
            },
            ConsumerCase {
                call_site: 0x2BF2,
                source: 0xAF20,
                segment_variable: 0x4822,
                width_argument: 0x03,
                height: 0x18,
                target: 0x3D55,
            },
            ConsumerCase {
                call_site: 0x2C58,
                source: 0xAF21,
                segment_variable: 0x4822,
                width_argument: 0x02,
                height: 0x18,
                target: 0x3EC9,
            },
            // RO30 and RO31.
            ConsumerCase {
                call_site: 0x2E84,
                source: 0x0000,
                segment_variable: 0x481C,
                width_argument: 0x18,
                height: 0xB0,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x2F00,
                source: 0x4200,
                segment_variable: 0x481C,
                width_argument: 0x1A,
                height: 0xB0,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x2F3B,
                source: 0x8980,
                segment_variable: 0x481C,
                width_argument: 0x06,
                height: 0x48,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x2F76,
                source: 0x9040,
                segment_variable: 0x481C,
                width_argument: 0x06,
                height: 0x70,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x2FD9,
                source: 0x0000,
                segment_variable: 0x4820,
                width_argument: 0x13,
                height: 0xB0,
                target: 0x3D24,
            },
            ConsumerCase {
                call_site: 0x3017,
                source: 0x3440,
                segment_variable: 0x4820,
                width_argument: 0x13,
                height: 0xB0,
                target: 0x3D24,
            },
        ],
    );

    for (call_site, row_pitch, plane_stride) in [
        (0x2466, 0x002C, 0x2100),
        (0x24E1, 0x0024, 0x22E0),
        (0x2587, 0x0024, 0x3600),
        (0x2695, 0x002E, 0x2280),
        (0x2A95, 0x0024, 0x1B00),
        (0x2C58, 0x0003, 0x0048),
    ] {
        assert_strided_configuration(&overlay, call_site, row_pitch, plane_stride);
    }

    // Segment 0x4816 is reused. ARS_RX.CS replaces its memory immediately
    // before the full-screen consumer at 0x181F, while RO18 replaces RO1 at
    // loader 0x0E02 before the later 0x2679..0x2FE2 consumers. The complete
    // RO1-owning range therefore has 22 DS loads: nineteen immediate sources
    // and the three table-driven animation paths below.
    assert_eq!(
        logical_slice(&overlay, 0x1801, 7),
        [0xBA, 0xE9, 0x02, 0xB4, 0x00, 0xCD, 0x7C]
    );
    assert_eq!(
        logical_slice(&overlay, 0x1810, 8),
        [0x8B, 0x3E, 0x16, 0x48, 0xB4, 0x01, 0xCD, 0x7C]
    );
    assert!(contains(
        logical_slice(&overlay, 0x181D, 0x1D),
        &[
            0x1E, 0x55, 0x2E, 0x8E, 0x1E, 0x16, 0x48, 0x33, 0xF6, 0xB8, 0x00, 0xA8, 0x8E, 0xC0,
            0xBB, 0x00, 0xB0, 0xB9, 0x00, 0xB8, 0xBA, 0x00, 0xE0, 0xB4, 0x05, 0xCD, 0x7C,
        ]
    ));
    let all_segment_sites = occurrence_sites(&overlay, &[0x2E, 0x8E, 0x1E, 0x16, 0x48]);
    assert_eq!(
        all_segment_sites,
        [
            0x181F, 0x1C14, 0x1C2E, 0x1C48, 0x1C62, 0x1C7C, 0x1C96, 0x1CB0, 0x1CCA, 0x1CE4, 0x1CFE,
            0x1D18, 0x1D2D, 0x1D43, 0x1D59, 0x1D6F, 0x1D85, 0x1D9B, 0x1DB1, 0x1E5C, 0x1EDE, 0x1F2B,
            0x1FE2, 0x2679, 0x2870, 0x29A7, 0x29E2, 0x2D67, 0x2E52, 0x2E8D, 0x2ECB, 0x2F09, 0x2F44,
            0x2F7F, 0x2FA4, 0x2FE2,
        ]
    );
    let ro1_segment_sites = all_segment_sites
        .iter()
        .copied()
        .filter(|site| (0x1C0C..0x1FF5).contains(site))
        .collect::<Vec<_>>();
    assert_eq!(
        ro1_segment_sites,
        [
            0x1C14, 0x1C2E, 0x1C48, 0x1C62, 0x1C7C, 0x1C96, 0x1CB0, 0x1CCA, 0x1CE4, 0x1CFE, 0x1D18,
            0x1D2D, 0x1D43, 0x1D59, 0x1D6F, 0x1D85, 0x1D9B, 0x1DB1, 0x1E5C, 0x1EDE, 0x1F2B, 0x1FE2,
        ]
    );
    let ro1_immediate_sources = immediate_sources_for_segment(&overlay, 0x4816)
        .into_iter()
        .filter(|(site, _)| (0x1C0C..0x1FF5).contains(site))
        .collect::<Vec<_>>();
    assert_eq!(
        ro1_immediate_sources,
        [
            (0x1C11, 0x0000),
            (0x1C2B, 0x0080),
            (0x1C45, 0x0100),
            (0x1C5F, 0x0180),
            (0x1C79, 0x0200),
            (0x1C93, 0x0280),
            (0x1CAD, 0x0300),
            (0x1CC7, 0x0380),
            (0x1CE1, 0x0400),
            (0x1CFB, 0x0480),
            (0x1D15, 0x0500),
            (0x1D2A, 0x0580),
            (0x1D40, 0x0680),
            (0x1D56, 0x0880),
            (0x1D6C, 0x1000),
            (0x1D82, 0x1400),
            (0x1D98, 0x1B80),
            (0x1DAE, 0x2280),
            (0x1FDF, 0x8E80),
        ]
    );
    let immediate_ro1_segment_sites = ro1_immediate_sources
        .iter()
        .map(|(site, _)| site + 3)
        .collect::<Vec<_>>();
    assert_eq!(
        ro1_segment_sites
            .iter()
            .copied()
            .filter(|site| !immediate_ro1_segment_sites.contains(site))
            .collect::<Vec<_>>(),
        [0x1E5C, 0x1EDE, 0x1F2B]
    );

    assert_consumer(
        &overlay,
        ConsumerCase {
            call_site: 0x1FF0,
            source: 0x8E80,
            segment_variable: 0x4816,
            width_argument: 0x10,
            height: 0x40,
            target: 0x3D24,
        },
    );

    // RO1's three remaining animation sources are selected from exact,
    // bounded tables rather than immediates.
    assert_eq!(
        logical_slice(&overlay, 0x1E76, 4),
        &[0x80, 0x4C, 0x80, 0x6D]
    );
    assert_eq!(
        logical_slice(&overlay, 0x1EF8, 4),
        &[0x80, 0x8E, 0x80, 0x9E]
    );
    let ro1_small_table = logical_slice(&overlay, 0x1F42, 0x4E)
        .as_chunks::<2>()
        .0
        .iter()
        .map(|word| u16::from_le_bytes(*word))
        .collect::<Vec<_>>();
    assert_eq!(
        ro1_small_table,
        [
            0xBF80, 0xBF00, 0xBE80, 0xBF00, 0xBF80, 0xBF00, 0xBE80, 0xBF00, 0xBF80, 0xBF00, 0xBE80,
            0xBF00, 0xBF80, 0xBF00, 0xBE80, 0xBF00, 0xBF80, 0xBF00, 0xBE80, 0xBF00, 0xBF80, 0xBF00,
            0xBE80, 0xBF00, 0xBF80, 0xBF00, 0xBE80, 0xBF00, 0xBF80, 0xBF00, 0xBE80, 0xBF00, 0xBF80,
            0xBF00, 0xBE80, 0xBF00, 0xBF80, 0xBF00, 0xFFFF,
        ]
    );
    for (site, segment, width, height, target) in [
        (0x1E69, 0x4816u16, 0x16u16, 0x60u16, 0x3D24usize),
        (0x1EEB, 0x4816, 0x10, 0x40, 0x3D24),
        (0x1F38, 0x4816, 0x02, 0x10, 0x3D24),
    ] {
        let setup = logical_slice(&overlay, site - 0x30, 0x30);
        let mut segment_signature = vec![0x2E, 0x8E, 0x1E];
        segment_signature.extend_from_slice(&segment.to_le_bytes());
        assert!(contains(setup, &segment_signature));
        let mut dimensions = vec![0xBA];
        dimensions.extend_from_slice(&width.to_le_bytes());
        dimensions.push(0xB9);
        dimensions.extend_from_slice(&height.to_le_bytes());
        assert!(contains(setup, &dimensions));
        assert_eq!(call_target(&overlay, site), target);
    }

    // RO24 advances through eight adjacent 8x8 row-interleaved tiles at
    // 0xB040..0xB140 while reusing one dynamic consumer.
    assert!(contains(
        logical_slice(&overlay, 0x13FC, 6),
        &[0x2E, 0xC7, 0x46, 0x11, 0x40, 0xB0]
    ));
    assert_eq!(
        logical_slice(&overlay, 0x2CF6, 5),
        &[0x2E, 0x83, 0x46, 0x11, 0x20]
    );
    let ro24_dynamic = logical_slice(&overlay, 0x2D08, 0x1F);
    assert!(contains(ro24_dynamic, &[0x2E, 0x8B, 0x5E, 0x11]));
    assert!(contains(ro24_dynamic, &[0x8B, 0xF3]));
    assert!(contains(ro24_dynamic, &[0x2E, 0x8E, 0x1E, 0x22, 0x48]));
    assert!(contains(
        ro24_dynamic,
        &[0xBA, 0x01, 0x00, 0xB9, 0x08, 0x00]
    ));
    assert_eq!(call_target(&overlay, 0x2D24), 0x3D24);
}
