use std::path::Path;

fn translation_by_id(path: &Path, id: &str) -> String {
    let catalog: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    catalog["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["id"].as_str() == Some(id))
        .and_then(|entry| entry["ko"].as_str())
        .unwrap_or_else(|| panic!("{} is missing translation {id}", path.display()))
        .to_owned()
}

fn visible_columns(row: &str) -> Vec<usize> {
    row.chars()
        .enumerate()
        .filter_map(|(column, ch)| (ch != '　' && ch != ' ').then_some(column))
        .collect()
}

#[test]

#[ignore = "requires translations in assets/translations/"]
fn battle_menu_two_syllable_commands_share_visible_columns() {
    let menus = [
        (
            "assets/translations/needs_review/game_a_system.json",
            "GAME_A_47B1",
        ),
        (
            "assets/translations/needs_review/game_r_dialogue_wf.json",
            "GAME_R_47F4",
        ),
        (
            "assets/translations/needs_review/game_s_all.json",
            "GAME_S_4784",
        ),
    ];

    for (path, id) in menus {
        let menu = translation_by_id(Path::new(path), id);
        let rows = menu.trim_end_matches('\r').split('\n').collect::<Vec<_>>();
        assert_eq!(rows.len(), 4, "{id} battle-menu row count");
        assert_eq!(
            visible_columns(rows[2]),
            visible_columns(rows[3]),
            "{id} escape and status labels must occupy the same columns",
        );
        assert_eq!(
            rows[2].chars().count(),
            4,
            "{id} escape row must preserve the source four-cell span",
        );
    }
}

#[test]

#[ignore = "requires translations in assets/translations/"]
fn field_menu_map_labels_preserve_the_centered_source_span() {
    let menus = [
        (
            "assets/translations/needs_review/game_a_uncovered.json",
            "GAME_A_UNC_41CD",
        ),
        (
            "assets/translations/needs_review/game_r_uncovered.json",
            "GAME_R_UNC_41FC",
        ),
        (
            "assets/translations/needs_review/game_s_uncovered.json",
            "GAME_S_UNC_418C",
        ),
    ];

    for (path, id) in menus {
        let menu = translation_by_id(Path::new(path), id);
        let map_row = menu
            .split('\n')
            .next()
            .expect("field menu must contain a map row")
            .trim_start_matches(['\u{4}', '\u{2}']);
        assert_eq!(
            map_row.chars().count(),
            4,
            "{id} map row must preserve the source four-cell span",
        );
        assert_eq!(
            visible_columns(map_row),
            vec![2],
            "{id} map label must occupy the centered third cell",
        );
    }
}
