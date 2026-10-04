//! Static layout bounds for translated renderer text.
//!
//! The normal in-game message pane has 28 full-width cells between its text
//! origin and right edge. Runtime-prefixed status strings also reserve eight
//! cells for the name drawn immediately before the cataloged suffix. Rulue and
//! Schezo cutscene overlays instead pass a variable window to their text-VRAM
//! consumer at every dialogue call, so those entries are checked against their
//! extracted call-site geometry. This gate measures the selected translation
//! tree on every build; it does not freeze a list of current strings or expected
//! warning counts.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::Value;

use crate::cutscene_catalog::CutsceneWindow;

pub const MESSAGE_PANE_HALF_CELLS: usize = 56;
pub const DYNAMIC_PREFIX_HALF_CELLS: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogueLayout {
    pub line_half_cells: Vec<usize>,
    pub dynamic_prefix_half_cells: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogueLayoutReport {
    pub entries: usize,
    pub normal_pane_entries: usize,
    pub exact_cutscene_entries: usize,
    pub row_only_cutscene_entries: usize,
    pub widest_id: String,
    pub widest_half_cells: usize,
}

pub fn validate_dialogue_layout(
    source: &str,
    korean: &str,
    wide_ascii: bool,
    label: &str,
) -> Result<DialogueLayout> {
    validate_dialogue_layout_with_rows(source, korean, wide_ascii, label, None)
}

fn validate_dialogue_layout_with_rows(
    source: &str,
    korean: &str,
    wide_ascii: bool,
    label: &str,
    verified_rows: Option<usize>,
) -> Result<DialogueLayout> {
    let source_lines = measure_authored_lines(source, wide_ascii)?;
    let mut korean_lines = measure_authored_lines(korean, wide_ascii)?;
    let max_rows = verified_rows.unwrap_or(source_lines.len());
    if korean_lines.len() > max_rows {
        bail!(
            "{label}: Korean text advances through {} rows, exceeding the verified {} rows",
            korean_lines.len(),
            max_rows,
        );
    }

    let dynamic_prefix_half_cells = if has_dynamic_prefix(source, korean) {
        DYNAMIC_PREFIX_HALF_CELLS
    } else {
        0
    };
    if let Some(first) = korean_lines.first_mut() {
        *first = first
            .checked_add(dynamic_prefix_half_cells)
            .context("dialogue-line width overflow")?;
    }

    for (line, &half_cells) in korean_lines.iter().enumerate() {
        if half_cells > MESSAGE_PANE_HALF_CELLS {
            bail!(
                "{label}: line {} needs {} but the message pane holds {}",
                line + 1,
                format_half_cells(half_cells),
                format_half_cells(MESSAGE_PANE_HALF_CELLS),
            );
        }
    }

    Ok(DialogueLayout {
        line_half_cells: korean_lines,
        dynamic_prefix_half_cells,
    })
}

pub fn validate_cutscene_layout(
    source: &str,
    korean: &str,
    wide_ascii: bool,
    windows: &[CutsceneWindow],
    label: &str,
) -> Result<DialogueLayout> {
    let source_lines = measure_authored_lines(source, wide_ascii)?;
    let korean_lines = measure_authored_lines(korean, wide_ascii)?;
    validate_source_proven_rows(&source_lines, &korean_lines, label)?;

    for window in windows {
        if source_lines.len() > window.rows {
            bail!(
                "{label}: protected source has {} rows but call-site window 0x{:04X} holds {}",
                source_lines.len(),
                window.rewrite_site,
                window.rows,
            );
        }
        for (line, &half_cells) in source_lines.iter().enumerate() {
            if half_cells > window.width_half_cells {
                bail!(
                    "{label}: protected source line {} needs {} but call-site window 0x{:04X} holds {}; catalog geometry is inconsistent",
                    line + 1,
                    format_half_cells(half_cells),
                    window.rewrite_site,
                    format_half_cells(window.width_half_cells),
                );
            }
        }
        for (line, &half_cells) in korean_lines.iter().enumerate() {
            if half_cells > window.width_half_cells {
                bail!(
                    "{label}: line {} needs {} but call-site window 0x{:04X} holds {}; the consumer would add an unintended wrap",
                    line + 1,
                    format_half_cells(half_cells),
                    window.rewrite_site,
                    format_half_cells(window.width_half_cells),
                );
            }
        }
    }

    Ok(DialogueLayout {
        line_half_cells: korean_lines,
        dynamic_prefix_half_cells: 0,
    })
}

fn validate_row_only_layout(
    source: &str,
    korean: &str,
    wide_ascii: bool,
    label: &str,
) -> Result<DialogueLayout> {
    let source_lines = measure_authored_lines(source, wide_ascii)?;
    let korean_lines = measure_authored_lines(korean, wide_ascii)?;
    validate_source_proven_rows(&source_lines, &korean_lines, label)?;
    Ok(DialogueLayout {
        line_half_cells: korean_lines,
        dynamic_prefix_half_cells: 0,
    })
}

fn validate_source_proven_rows(
    source_lines: &[usize],
    korean_lines: &[usize],
    label: &str,
) -> Result<()> {
    if korean_lines.len() > source_lines.len() {
        bail!(
            "{label}: Korean text advances through {} rows, exceeding the source-proven {} rows",
            korean_lines.len(),
            source_lines.len(),
        );
    }
    Ok(())
}

fn parse_verified_message_rows(value: &Value, label: &str) -> Result<Option<usize>> {
    let Some(layout) = value.get("verified_message_rows") else {
        return Ok(None);
    };
    let rows = layout
        .get("rows")
        .and_then(Value::as_u64)
        .filter(|rows| (1..=7).contains(rows))
        .with_context(|| {
            format!("{label}: verified message rows must be within the seven-row pane")
        })?;
    let origin = layout
        .get("origin_row")
        .and_then(Value::as_u64)
        .filter(|row| *row < 7)
        .with_context(|| {
            format!("{label}: verified message rows require an origin within the pane")
        })?;
    let following = layout
        .get("following_rows")
        .and_then(Value::as_u64)
        .filter(|rows| (1..=7).contains(rows))
        .with_context(|| {
            format!("{label}: verified message rows require following consumer rows")
        })?;
    if origin + rows - 1 + following > 7 {
        bail!("{label}: composed message exceeds the seven-row pane");
    }
    if layout
        .get("evidence")
        .and_then(Value::as_str)
        .is_none_or(|s| s.trim().is_empty())
    {
        bail!("{label}: verified message rows require consumer evidence");
    }
    Ok(Some(rows as usize))
}

pub fn validate_translation_tree(root: &Path) -> Result<DialogueLayoutReport> {
    let mut paths = json_files(root)?;
    for subdir in ["dat", "cutscene"] {
        let path = root.join(subdir);
        if path.is_dir() {
            paths.extend(json_files(&path)?);
        }
    }
    paths.sort();

    let mut entries = 0usize;
    let mut normal_pane_entries = 0usize;
    let mut exact_cutscene_entries = 0usize;
    let mut row_only_cutscene_entries = 0usize;
    let mut widest_id = String::new();
    let mut widest_half_cells = 0usize;
    for path in paths {
        let mut catalog: Value = serde_json::from_slice(
            &std::fs::read(&path).with_context(|| format!("read {}", path.display()))?,
        )
        .with_context(|| format!("parse {}", path.display()))?;
        if crate::translation_binding::has_bound_translations(&catalog) {
            catalog = crate::translation_binding::resolve_bound_translations(&catalog, root)
                .with_context(|| format!("resolve layout bindings in {}", path.display()))?;
        }
        let Some(values) = catalog.get("entries").and_then(Value::as_array) else {
            continue;
        };
        let is_cutscene = path.parent().is_some_and(|parent| {
            parent.file_name().and_then(|name| name.to_str()) == Some("cutscene")
        });
        let wide_ascii = is_cutscene
            && catalog
                .get("resource")
                .and_then(Value::as_str)
                .is_some_and(|resource| resource.ends_with(".OVL"));

        for (index, value) in values.iter().enumerate() {
            let Some(korean) = value.get("ko").and_then(Value::as_str) else {
                continue;
            };
            if korean.is_empty() {
                continue;
            }
            let source = value.get("text").and_then(Value::as_str).with_context(|| {
                format!("{}: entry {index} missing source text", path.display())
            })?;
            let id = value
                .get("id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| format!("entry[{index}]"));
            let label = format!("{}:{id}", path.display());
            let layout = if is_cutscene {
                let windows = parse_cutscene_windows(value, &label)?;
                let mov_dx_sites = cutscene_mov_dx_sites(value, &label)?;
                if !mov_dx_sites.is_empty() {
                    let window_sites = windows
                        .iter()
                        .map(|window| window.rewrite_site)
                        .collect::<Vec<_>>();
                    if window_sites != mov_dx_sites {
                        bail!(
                            "{label}: mov_dx rewrite sites {mov_dx_sites:?} do not match protected layout-window sites {window_sites:?}",
                        );
                    }
                }
                if windows.is_empty() {
                    row_only_cutscene_entries += 1;
                    validate_row_only_layout(source, korean, wide_ascii, &label)?
                } else {
                    exact_cutscene_entries += 1;
                    validate_cutscene_layout(source, korean, wide_ascii, &windows, &label)?
                }
            } else {
                normal_pane_entries += 1;
                let verified_rows = parse_verified_message_rows(value, &label)?;
                validate_dialogue_layout_with_rows(
                    source,
                    korean,
                    wide_ascii,
                    &label,
                    verified_rows,
                )?
            };
            entries += 1;
            if let Some(&width) = layout.line_half_cells.iter().max()
                && width > widest_half_cells
            {
                widest_half_cells = width;
                widest_id = id;
            }
        }
    }
    if entries == 0 {
        bail!("no translated dialogue entries found in {}", root.display());
    }

    Ok(DialogueLayoutReport {
        entries,
        normal_pane_entries,
        exact_cutscene_entries,
        row_only_cutscene_entries,
        widest_id,
        widest_half_cells,
    })
}

fn parse_cutscene_windows(value: &Value, label: &str) -> Result<Vec<CutsceneWindow>> {
    let Some(windows) = value.get("layout_windows") else {
        return Ok(Vec::new());
    };
    windows
        .as_array()
        .with_context(|| format!("{label}: layout_windows must be an array"))?
        .iter()
        .map(|window| {
            let integer = |name: &str| {
                window
                    .get(name)
                    .and_then(Value::as_u64)
                    .with_context(|| format!("{label}: layout window missing integer {name}"))
                    .and_then(|value| {
                        usize::try_from(value)
                            .with_context(|| format!("{label}: layout window {name} is too large"))
                    })
            };
            let hex_offset = |name: &str| -> Result<usize> {
                let raw = window
                    .get(name)
                    .and_then(Value::as_str)
                    .with_context(|| format!("{label}: layout window missing {name}"))?;
                usize::from_str_radix(raw.strip_prefix("0x").unwrap_or(raw), 16)
                    .with_context(|| format!("{label}: invalid layout window {name} {raw:?}"))
            };
            Ok(CutsceneWindow {
                rewrite_site: hex_offset("rewrite_site")?,
                origin_column: integer("origin_column")?,
                origin_row: integer("origin_row")?,
                width_half_cells: integer("width_half_cells")?,
                rows: integer("rows")?,
                initializer_decoded_offset: hex_offset("initializer_decoded_offset")?,
            })
        })
        .collect()
}

fn cutscene_mov_dx_sites(value: &Value, label: &str) -> Result<Vec<usize>> {
    let Some(sites) = value.get("rewrite_sites") else {
        return Ok(Vec::new());
    };
    sites
        .as_array()
        .with_context(|| format!("{label}: rewrite_sites must be an array"))?
        .iter()
        .filter(|site| site.get("kind").and_then(Value::as_str) == Some("mov_dx"))
        .map(|site| {
            let raw = site
                .get("site")
                .and_then(Value::as_str)
                .with_context(|| format!("{label}: mov_dx rewrite site is missing its offset"))?;
            usize::from_str_radix(raw.strip_prefix("0x").unwrap_or(raw), 16)
                .with_context(|| format!("{label}: invalid mov_dx rewrite site {raw:?}"))
        })
        .collect()
}

pub fn format_half_cells(half_cells: usize) -> String {
    if half_cells.is_multiple_of(2) {
        format!("{} cells", half_cells / 2)
    } else {
        format!("{}.5 cells", half_cells / 2)
    }
}

fn measure_authored_lines(text: &str, wide_ascii: bool) -> Result<Vec<usize>> {
    let mut lines = vec![0usize];
    let mut peaks = vec![0usize];
    let mut remaining = text;
    while !remaining.is_empty() {
        if let Some(marker) = remaining.strip_prefix("{josa:") {
            let close = marker
                .find('}')
                .context("unterminated runtime particle marker")?;
            add_width(&mut lines, &mut peaks, 2)?;
            remaining = &marker[close + 1..];
            continue;
        }
        if let Some(raw) = remaining.strip_prefix("{raw:") {
            let close = raw.find('}').context("unterminated raw token")?;
            let hex = &raw[..close];
            if !hex.len().is_multiple_of(2) || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                bail!("invalid raw token {{{hex}}}");
            }
            add_width(&mut lines, &mut peaks, hex.len() / 2)?;
            remaining = &raw[close + 1..];
            continue;
        }
        if let Some(raw) = remaining.strip_prefix("{omit_raw:") {
            let close = raw.find('}').context("unterminated omitted-raw token")?;
            let hex = &raw[..close];
            if !hex.len().is_multiple_of(2) || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                bail!("invalid omitted-raw token {{{hex}}}");
            }
            remaining = &raw[close + 1..];
            continue;
        }

        let ch = remaining.chars().next().context("dialogue text ended")?;
        let ch_len = ch.len_utf8();
        if ch == '\n' {
            lines.push(0);
            peaks.push(0);
        } else if ch == '\r' {
            *lines.last_mut().context("dialogue line missing")? = 0;
        } else if ch == '\u{0008}' {
            let current = lines.last_mut().context("dialogue line missing")?;
            *current = current
                .checked_add(8)
                .context("dialogue tab width overflow")?
                & !7;
            let peak = peaks.last_mut().context("dialogue peak missing")?;
            *peak = (*peak).max(*current);
        } else if ch == '\u{0001}' {
            let after = &remaining[ch_len..];
            let mut params = after.chars();
            let low = params.next();
            let high = params.next();
            if low.is_some_and(|value| value <= '\u{001F}')
                && high.is_some_and(|value| value <= '\u{001F}')
            {
                let low = low.context("cursor low byte missing")? as usize;
                let high = high.context("cursor high byte missing")? as usize;
                add_width(&mut lines, &mut peaks, low | high << 8)?;
                remaining = params.as_str();
                continue;
            }
        } else if ch < '\u{0020}' {
            // Attribute controls, no-op controls, and their authoring-only
            // parameter scalars do not draw cells.
        } else if ch.is_ascii() {
            add_width(&mut lines, &mut peaks, usize::from(wide_ascii) + 1)?;
        } else {
            add_width(&mut lines, &mut peaks, 2)?;
        }
        remaining = &remaining[ch_len..];
    }
    Ok(peaks)
}

fn add_width(lines: &mut [usize], peaks: &mut [usize], width: usize) -> Result<()> {
    let current = lines.last_mut().context("dialogue line missing")?;
    *current = current
        .checked_add(width)
        .context("dialogue width overflow")?;
    let peak = peaks.last_mut().context("dialogue peak missing")?;
    *peak = (*peak).max(*current);
    Ok(())
}

fn has_dynamic_prefix(source: &str, korean: &str) -> bool {
    let first_source = source
        .chars()
        .find(|ch| *ch >= '\u{0020}' && !ch.is_whitespace());
    korean
        .trim_start_matches(|ch: char| ch < '\u{0020}' || ch.is_whitespace())
        .starts_with("{josa:")
        || matches!(first_source, Some('が' | 'は' | 'を' | 'に' | 'の'))
}

fn json_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = std::fs::read_dir(dir)
        .with_context(|| format!("read directory {}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_a_line_past_the_message_pane() {
        let source = "가".repeat(28);
        let korean = "가".repeat(29);
        let error = validate_dialogue_layout(&source, &korean, false, "TEST")
            .unwrap_err()
            .to_string();
        assert!(error.contains("29 cells"), "{error}");
        assert!(error.contains("28 cells"), "{error}");
    }

    #[test]
    fn reserves_name_cells_for_a_dynamic_suffix() {
        let korean = format!("{{josa:은}}{}", "가".repeat(20));
        let error = validate_dialogue_layout("は短い", &korean, false, "TEST")
            .unwrap_err()
            .to_string();
        assert!(error.contains("29 cells"), "{error}");
    }

    #[test]
    fn rejects_rows_beyond_the_source_proven_count() {
        let error = validate_dialogue_layout("가\n나", "가\n나\n다", false, "TEST")
            .unwrap_err()
            .to_string();
        assert!(error.contains("3 rows"), "{error}");
        assert!(error.contains("2 rows"), "{error}");
    }

    #[test]
    fn verified_rows_still_bound_the_composed_message_and_width() {
        let mut entry = serde_json::json!({"verified_message_rows": {
            "rows": 3, "origin_row": 2, "following_rows": 3,
            "evidence": "consumer-record"
        }});
        let rows = parse_verified_message_rows(&entry, "TEST").unwrap();
        assert!(
            validate_dialogue_layout_with_rows(
                "は短い",
                "{josa:은} 짧다!\n\n",
                false,
                "TEST",
                rows
            )
            .is_ok()
        );
        assert!(
            validate_dialogue_layout_with_rows("は短い", "짧다!\n\n\n", false, "TEST", rows)
                .is_err()
        );
        assert!(
            validate_dialogue_layout_with_rows("は短い", &"가".repeat(28), false, "TEST", rows)
                .is_err()
        );
        entry["verified_message_rows"]["following_rows"] = serde_json::json!(4);
        assert!(parse_verified_message_rows(&entry, "TEST").is_err());
        entry["verified_message_rows"]["following_rows"] = serde_json::json!(3);
        entry["verified_message_rows"]["evidence"] = serde_json::json!("");
        assert!(parse_verified_message_rows(&entry, "TEST").is_err());
    }

    #[test]
    fn cursor_move_and_ascii_width_are_measured_in_half_cells() {
        let layout =
            validate_dialogue_layout("\u{0001}\u{000e}\0A", "\u{0001}\u{000e}\0A", false, "TEST")
                .unwrap();
        assert_eq!(layout.line_half_cells, [15]);
    }

    #[test]
    fn rejects_a_cutscene_line_that_would_auto_wrap() {
        let window = CutsceneWindow {
            rewrite_site: 0x1234,
            origin_column: 24,
            origin_row: 18,
            width_half_cells: 30,
            rows: 4,
            initializer_decoded_offset: 0x4000,
        };
        let error =
            validate_cutscene_layout(&"あ".repeat(15), &"가".repeat(16), true, &[window], "TEST")
                .unwrap_err()
                .to_string();
        assert!(error.contains("16 cells"), "{error}");
        assert!(error.contains("15 cells"), "{error}");
        assert!(error.contains("unintended wrap"), "{error}");
    }
}
