use pc98_madou_ars::enemy_text::{
    find_enemy_attack_names, overwrite_first_enemy_name, overwrite_first_enemy_name_with_gaiji,
};

#[path = "common/mod.rs"]
mod common;

const ENEMY001_EXTRACT: &str = "research/extracted/arle_game_all/ENEMY001.DAT";
const ENEMY002_EXTRACT: &str = "research/extracted/arle_game_all/ENEMY002.DAT";

#[test]

#[ignore = "requires extracted disk files in research/extracted/"]
fn arle_enemy_files_list_attack_names() {
    let cases = [
        (ENEMY001_EXTRACT, 0x0387, 0x038F, "ぷよ"),
        (ENEMY002_EXTRACT, 0x040E, 0x041B, "ナスグレイブ"),
    ];

    for (path, name_offset, attack_text_offset, name) in cases {
        let Some(input) = common::try_read(path) else {
            return;
        };

        let names = find_enemy_attack_names(&input);
        let entry = names
            .iter()
            .find(|entry| entry.name == name)
            .unwrap_or_else(|| panic!("find {name} in {path}"));
        assert_eq!(entry.name_offset, name_offset, "{path} name offset");
        assert_eq!(
            entry.attack_text_offset, attack_text_offset,
            "{path} attack text offset"
        );
    }
}

#[test]

#[ignore = "requires extracted disk files in research/extracted/"]
fn arle_enemy001_name_overwrites_to_gaiji() {
    // Render-path Hangul PoC input: the uncompressed ENEMY001 Puyo name is a
    // length-preserving gaiji injection surface.
    let Some(mut input) = common::try_read(ENEMY001_EXTRACT) else {
        return;
    };
    let len_before = input.len();

    let patch = overwrite_first_enemy_name_with_gaiji(&mut input, [0xEB, 0x9F])
        .expect("overwrite ENEMY001 name with gaiji");

    assert_eq!(patch.name_offset, 0x0387);
    assert_eq!(patch.name_end_offset, 0x038B);
    assert_eq!(patch.original_name, "ぷよ");
    assert_eq!(patch.glyph_count, 2);
    assert_eq!(input.len(), len_before, "byte length preserved");
    assert_eq!(
        &input[patch.name_offset..patch.name_end_offset],
        &[0xEB, 0x9F, 0xEB, 0x9F]
    );
}

#[test]

#[ignore = "requires extracted disk files in research/extracted/"]
fn arle_enemy001_name_overwrites_to_korean_codes() {
    // Multi-glyph PoC: ぷよ (4 bytes) -> 뿌요 gaiji codes EB9F EBA0 (4 bytes).
    let Some(mut input) = common::try_read(ENEMY001_EXTRACT) else {
        return;
    };
    let len_before = input.len();
    let replacement = vec![0xEB, 0x9F, 0xEB, 0xA0];

    let patch = overwrite_first_enemy_name(&mut input, &replacement)
        .expect("overwrite ENEMY001 name with 뿌요 codes");

    assert_eq!(patch.original_name, "ぷよ");
    assert_eq!(input.len(), len_before);
    assert_eq!(
        &input[patch.name_offset..patch.name_end_offset],
        replacement.as_slice()
    );

    // A length mismatch is rejected (length-preserving invariant).
    let mut input2 = common::try_read(ENEMY001_EXTRACT).unwrap();
    assert!(overwrite_first_enemy_name(&mut input2, &[0xEB, 0x9F]).is_err());
}
