use pc98_madou_ars::overlay_lz::decode_overlay_lz;
use pc98_madou_ars::overlay_text::{
    find_dominant_renderer_message_refs, find_renderer_message_refs,
};

#[path = "common/mod.rs"]
mod common;

const GAME_A_EXTRACT: &str = "research/extracted/ars/GAME_A.OVL";
const GAME_R_EXTRACT: &str = "research/extracted/ars/GAME_R.OVL";
const GAME_S_EXTRACT: &str = "research/extracted/ars/GAME_S.OVL";
const GAME_A_RENDERER_LOGICAL_OFFSET: usize = 0x52C0;
const GAME_A_LOAD_OFFSET: usize = 0x0100;

#[test]

#[ignore = "requires extracted disk files in research/extracted/"]
fn game_a_overlay_lists_dungeon_message_renderer_ref() {
    let Some(input) = common::try_read(GAME_A_EXTRACT) else {
        return;
    };

    let decoded = decode_overlay_lz(&input).expect("decode GAME_A.OVL").output;
    let refs =
        find_renderer_message_refs(&decoded, GAME_A_RENDERER_LOGICAL_OFFSET, GAME_A_LOAD_OFFSET);

    let ittai = refs
        .iter()
        .find(|entry| entry.string_logical_offset == 0xB13A)
        .expect("find ittaai renderer ref");
    assert_eq!(ittai.call_decoded_offset, 0xB025);
    assert_eq!(ittai.call_logical_offset, 0xB125);
    assert_eq!(ittai.string_decoded_offset, 0xB03A);
    assert_eq!(ittai.mode, 0x01);
    assert_eq!(ittai.text, "「いったーい");
}

#[test]

#[ignore = "requires extracted disk files in research/extracted/"]
fn game_overlays_auto_detect_dominant_renderer_targets() {
    for (path, expected_renderer, expected_refs) in [
        (GAME_A_EXTRACT, 0x52C0, 234),
        (GAME_R_EXTRACT, 0x5300, 252),
        (GAME_S_EXTRACT, 0x5290, 221),
    ] {
        let Some(input) = common::try_read(path) else {
            return;
        };

        let decoded =
            decode_overlay_lz(&input).unwrap_or_else(|err| panic!("decode {path}: {err}"));
        let scan = find_dominant_renderer_message_refs(&decoded.output, GAME_A_LOAD_OFFSET)
            .unwrap_or_else(|| panic!("detect renderer target in {path}"));
        assert_eq!(
            scan.renderer_logical_offset, expected_renderer,
            "{path} renderer"
        );
        assert_eq!(scan.refs.len(), expected_refs, "{path} refs");
    }
}
