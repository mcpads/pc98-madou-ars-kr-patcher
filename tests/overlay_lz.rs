use pc98_madou_ars::overlay_lz::{decode_overlay_lz, encode_overlay_lz};
use pc98_madou_ars::sjis_marker::encode_sjis;

#[path = "common/mod.rs"]
mod common;

const GAME_A_EXTRACT: &str = "research/extracted/ars/GAME_A.OVL";
const OVERLAY_EXTRACTS: [&str; 3] = [
    "research/extracted/ars/GAME_A.OVL",
    "research/extracted/ars/GAME_R.OVL",
    "research/extracted/ars/GAME_S.OVL",
];

/// Criterion A (`references/strategy/compression.md` 4절): the re-encoder must
/// be the exact inverse of the game's decompressor for the real overlays.
/// `decode(encode(decode(file))) == decode(file)`, run over the full corpus.
#[test]
#[ignore = "requires extracted disk files in research/extracted/"]
fn reencoder_roundtrips_real_overlays() {
    for path in OVERLAY_EXTRACTS {
        let Some(packed) = common::try_read(path) else {
            continue;
        };
        let decoded = decode_overlay_lz(&packed).expect("decode original").output;

        let reencoded = encode_overlay_lz(&decoded);
        let redecoded = decode_overlay_lz(&reencoded).expect("decode re-encoded");

        assert_eq!(
            redecoded.output, decoded,
            "{path}: decode(encode(x)) must equal x"
        );
        assert_eq!(
            redecoded.bytes_consumed,
            reencoded.len(),
            "{path}: re-encoded stream must consume fully to terminator"
        );

        // Size-regression log: re-encoded vs original packed (greedy vs. the
        // game's original packer). Informational, not a hard gate.
        eprintln!(
            "{path}: decoded={} original_packed={} reencoded={} (delta {:+})",
            decoded.len(),
            packed.len(),
            reencoded.len(),
            reencoded.len() as isize - packed.len() as isize,
        );
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[test]

#[ignore = "requires extracted disk files in research/extracted/"]
fn game_a_overlay_decodes_runtime_battle_text_table() {
    let Some(input) = common::try_read(GAME_A_EXTRACT) else {
        return;
    };

    let decoded = decode_overlay_lz(&input).expect("decode GAME_A.OVL");
    assert_eq!(decoded.bytes_consumed, input.len());
    assert_eq!(decoded.output.len(), 45_910);

    let appeared = encode_sjis("現れた").expect("encode");
    let worthy = encode_sjis("相手にとって　不足なし").expect("encode");
    assert_eq!(find(&decoded.output, &appeared), Some(0xB10A));
    assert_eq!(find(&decoded.output, &worthy), Some(0xB125));
    assert_eq!(find(&input, &appeared), None);
    assert_eq!(find(&input, &worthy), None);
}
