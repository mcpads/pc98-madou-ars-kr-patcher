//! Translation-ready catalogs for the A.R.S cutscene resources.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};
use encoding_rs::SHIFT_JIS;

use crate::overlay_lz::decode_overlay_lz;

pub const BIOS_GAIJI_CAPACITY: usize = 188;

/// ED_A's initial opcode 15 registers 38 original picture cells at JIS
/// 7621..7646. Hangul must coexist with those cells through the ending.
pub fn reserved_gaiji_slots(resource: &str) -> usize {
    if resource.eq_ignore_ascii_case("ED_A.DAT") {
        38
    } else {
        0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NulRegionSpec {
    pub label: &'static str,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerGridSpec {
    pub region: &'static str,
    /// Decoded byte range occupied by the little-endian word grid.
    pub start: usize,
    pub end: usize,
    /// Decoded offset of the imm16 in `mov si,[cs:bx+imm16]`.
    pub consumer_table_operand_site: usize,
    /// Decoded offset of the consumer's `mov dx,[cs:si]` sequence.
    pub consumer_string_read_site: usize,
    /// Decoded offset of the instruction that advances the grid index by one word.
    pub consumer_index_advance_site: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceLayout {
    OverlayDialogue {
        extra_nul_regions: &'static [NulRegionSpec],
        pointer_grids: &'static [PointerGridSpec],
    },
    NulRegions(&'static [NulRegionSpec]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CutsceneResourceSpec {
    pub character: &'static str,
    pub scene: &'static str,
    pub file: &'static str,
    pub layout: ResourceLayout,
}

const OP_A_REGIONS: &[NulRegionSpec] = &[NulRegionSpec {
    label: "dialogue",
    start: 0x0F5E,
    end: 0x11E9,
}];
const CD_A_REGIONS: &[NulRegionSpec] = &[NulRegionSpec {
    label: "dialogue",
    start: 0x02E8,
    end: 0x0387,
}];
const ED_A_REGIONS: &[NulRegionSpec] = &[
    NulRegionSpec {
        label: "dialogue",
        start: 0x0B7C,
        end: 0x0CB1,
    },
    NulRegionSpec {
        label: "credits",
        start: 0x0CB1,
        end: 0x161F,
    },
];
const SHEZO_ED_EXTRA_REGIONS: &[NulRegionSpec] = &[NulRegionSpec {
    label: "credits",
    start: 0x3A96,
    end: 0x3EB0,
}];
const SHEZO_ED_POINTER_GRIDS: &[PointerGridSpec] = &[PointerGridSpec {
    region: "credits",
    start: 0x3642,
    end: 0x3A96,
    consumer_table_operand_site: 0x1669,
    consumer_string_read_site: 0x16A7,
    consumer_index_advance_site: 0x16D2,
}];

pub const CUTSCENE_RESOURCES: &[CutsceneResourceSpec] = &[
    CutsceneResourceSpec {
        character: "arle",
        scene: "opening",
        file: "OP_A.DAT",
        layout: ResourceLayout::NulRegions(OP_A_REGIONS),
    },
    CutsceneResourceSpec {
        character: "arle",
        scene: "interlude",
        file: "CD_A.DAT",
        layout: ResourceLayout::NulRegions(CD_A_REGIONS),
    },
    CutsceneResourceSpec {
        character: "arle",
        scene: "ending",
        file: "ED_A.DAT",
        layout: ResourceLayout::NulRegions(ED_A_REGIONS),
    },
    CutsceneResourceSpec {
        character: "rulue",
        scene: "opening",
        file: "OPENINGR.OVL",
        layout: ResourceLayout::OverlayDialogue {
            extra_nul_regions: &[],
            pointer_grids: &[],
        },
    },
    CutsceneResourceSpec {
        character: "rulue",
        scene: "interlude",
        file: "TYUKAN_R.OVL",
        layout: ResourceLayout::OverlayDialogue {
            extra_nul_regions: &[],
            pointer_grids: &[],
        },
    },
    CutsceneResourceSpec {
        character: "rulue",
        scene: "ending",
        file: "ENDING_R.OVL",
        layout: ResourceLayout::OverlayDialogue {
            extra_nul_regions: &[],
            pointer_grids: &[],
        },
    },
    CutsceneResourceSpec {
        character: "schezo",
        scene: "opening",
        file: "SHEZO_OP.OVL",
        layout: ResourceLayout::OverlayDialogue {
            extra_nul_regions: &[],
            pointer_grids: &[],
        },
    },
    CutsceneResourceSpec {
        character: "schezo",
        scene: "interlude",
        file: "SHEZO_TU.OVL",
        layout: ResourceLayout::OverlayDialogue {
            extra_nul_regions: &[],
            pointer_grids: &[],
        },
    },
    CutsceneResourceSpec {
        character: "schezo",
        scene: "ending",
        file: "SHEZO_ED.OVL",
        layout: ResourceLayout::OverlayDialogue {
            extra_nul_regions: SHEZO_ED_EXTRA_REGIONS,
            pointer_grids: SHEZO_ED_POINTER_GRIDS,
        },
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CutsceneRewriteSite {
    /// Decoded offset of the little-endian logical pointer to rewrite.
    pub site: usize,
    pub kind: CutsceneRewriteKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CutsceneWindow {
    /// Decoded offset of the `mov dx, string` pointer operand governed by this
    /// window setup.
    pub rewrite_site: usize,
    /// Text-VRAM origin and extent, in the consumer's eight-pixel columns.
    pub origin_column: usize,
    pub origin_row: usize,
    pub width_half_cells: usize,
    pub rows: usize,
    /// Decoded target of the immediately preceding near call that installs the
    /// window geometry.
    pub initializer_decoded_offset: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CutsceneRewriteKind {
    MovDx,
    PointerGrid,
}

impl CutsceneRewriteKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MovDx => "mov_dx",
            Self::PointerGrid => "pointer_grid",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CutsceneEntry {
    pub region: &'static str,
    /// Start of the complete slot, including a resource control prefix.
    pub slot_decoded_offset: usize,
    /// Start of the editable text payload.
    pub string_decoded_offset: usize,
    pub string_logical_offset: Option<usize>,
    /// Editable payload bytes plus the one-byte terminator.
    pub byte_budget: usize,
    pub prefix: Vec<u8>,
    pub raw: Vec<u8>,
    pub text: String,
    pub terminator: u8,
    pub rewrite_sites: Vec<CutsceneRewriteSite>,
    pub layout_windows: Vec<CutsceneWindow>,
    pub structural: bool,
    pub had_decode_errors: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CutsceneCatalog {
    pub spec: CutsceneResourceSpec,
    pub packed_size: usize,
    pub decoded_size: usize,
    pub load_offset: Option<usize>,
    pub entries: Vec<CutsceneEntry>,
}

fn is_sjis_lead(byte: u8) -> bool {
    (0x81..=0x9F).contains(&byte) || (0xE0..=0xFC).contains(&byte)
}

fn is_sjis_trail(byte: u8) -> bool {
    (0x40..=0x7E).contains(&byte) || (0x80..=0xFC).contains(&byte)
}

fn validate_overlay_text(raw: &[u8]) -> bool {
    let mut index = 0usize;
    let mut double_bytes = 0usize;
    while index < raw.len() {
        let byte = raw[index];
        if byte == b'\r'
            || byte == b'\n'
            || (0x20..=0x7E).contains(&byte)
            || (0xA1..=0xDF).contains(&byte)
        {
            index += 1;
        } else if is_sjis_lead(byte) && index + 1 < raw.len() && is_sjis_trail(raw[index + 1]) {
            double_bytes += 1;
            index += 2;
        } else {
            return false;
        }
    }
    double_bytes > 0
}

fn decode_text(raw: &[u8]) -> (String, bool, bool) {
    let mut text = String::new();
    let mut had_errors = false;
    let mut has_mapped_visible_text = false;
    let mut index = 0usize;
    while index < raw.len() {
        let byte = raw[index];
        if byte == b'\r' || byte == b'\n' {
            text.push('\n');
            index += 1;
        } else if byte <= 0x7F || (0xA1..=0xDF).contains(&byte) {
            let (decoded, _, errors) = SHIFT_JIS.decode(&raw[index..index + 1]);
            if errors {
                had_errors = true;
                text.push_str(&format!("{{raw:{byte:02X}}}"));
            } else {
                has_mapped_visible_text |= decoded
                    .chars()
                    .any(|ch| !ch.is_whitespace() && ch != '\u{3000}');
                text.push_str(&decoded);
            }
            index += 1;
        } else if is_sjis_lead(byte) && index + 1 < raw.len() && is_sjis_trail(raw[index + 1]) {
            let trail = raw[index + 1];
            let (decoded, _, errors) = SHIFT_JIS.decode(&raw[index..index + 2]);
            if errors {
                had_errors = true;
                text.push_str(&format!("{{raw:{byte:02X}{trail:02X}}}"));
            } else {
                has_mapped_visible_text |= decoded
                    .chars()
                    .any(|ch| !ch.is_whitespace() && ch != '\u{3000}');
                text.push_str(&decoded);
            }
            index += 2;
        } else {
            had_errors = true;
            text.push_str(&format!("{{raw:{byte:02X}}}"));
            index += 1;
        }
    }
    (text, had_errors, has_mapped_visible_text)
}

fn catalog_overlay_at_load(decoded: &[u8], load_offset: usize) -> Vec<CutsceneEntry> {
    let mut by_target = BTreeMap::<usize, CutsceneEntry>::new();
    if decoded.len() < 3 {
        return Vec::new();
    }

    for opcode in 0..=decoded.len() - 3 {
        if decoded[opcode] != 0xBA {
            continue;
        }
        if decoded.get(opcode + 3) != Some(&0xE8) || decoded.get(opcode + 6) != Some(&0x1F) {
            continue;
        }
        let logical = u16::from_le_bytes([decoded[opcode + 1], decoded[opcode + 2]]) as usize;
        let Some(target) = logical.checked_sub(load_offset) else {
            continue;
        };
        if target == 0 || target >= decoded.len() {
            continue;
        }
        if !matches!(decoded[target - 1], 0x24 | 0xC3) {
            continue;
        }
        let Some(relative_end) = decoded[target..].iter().position(|&byte| byte == 0x24) else {
            continue;
        };
        if relative_end == 0 || relative_end > 1024 {
            continue;
        }
        let end = target + relative_end;
        let raw = &decoded[target..end];
        if !validate_overlay_text(raw) {
            continue;
        }
        let (text, had_decode_errors, _) = decode_text(raw);
        if had_decode_errors {
            continue;
        }

        let entry = by_target.entry(target).or_insert_with(|| CutsceneEntry {
            region: "dialogue",
            slot_decoded_offset: target,
            string_decoded_offset: target,
            string_logical_offset: Some(logical),
            byte_budget: raw.len() + 1,
            prefix: Vec::new(),
            raw: raw.to_vec(),
            text,
            terminator: 0x24,
            rewrite_sites: Vec::new(),
            layout_windows: Vec::new(),
            structural: false,
            had_decode_errors: false,
        });
        entry.rewrite_sites.push(CutsceneRewriteSite {
            site: opcode + 1,
            kind: CutsceneRewriteKind::MovDx,
        });
    }

    let mut entries: Vec<_> = by_target.into_values().collect();
    for entry in &mut entries {
        entry.rewrite_sites.sort_by_key(|site| site.site);
        entry.rewrite_sites.dedup_by_key(|site| site.site);
    }
    entries
}

fn attach_dialogue_windows(decoded: &[u8], entries: &mut [CutsceneEntry]) -> Result<()> {
    let mut initializer_targets = BTreeSet::new();
    for entry in entries {
        for site in &entry.rewrite_sites {
            if site.kind != CutsceneRewriteKind::MovDx {
                continue;
            }
            let start = site.site.checked_sub(16).with_context(|| {
                format!(
                    "cutscene pointer at decoded 0x{:04X} lacks its window setup",
                    site.site
                )
            })?;
            let setup = decoded.get(start..start + 15).with_context(|| {
                format!(
                    "cutscene pointer at decoded 0x{:04X} has a truncated window setup",
                    site.site
                )
            })?;
            if [setup[0], setup[3], setup[6], setup[9], setup[12]] != [0xB8, 0xBB, 0xB9, 0xBA, 0xE8]
            {
                bail!(
                    "cutscene pointer at decoded 0x{:04X} is not preceded by the proven ax/bx/cx/dx window setup",
                    site.site,
                );
            }

            let word =
                |offset: usize| u16::from_le_bytes([setup[offset], setup[offset + 1]]) as usize;
            let origin_column = word(1);
            let origin_row = word(4);
            let width_half_cells = word(7);
            let rows = word(10);
            if width_half_cells == 0
                || !width_half_cells.is_multiple_of(2)
                || rows == 0
                || origin_column + width_half_cells > 80
                || origin_row + rows > 25
            {
                bail!(
                    "cutscene pointer at decoded 0x{:04X} has invalid window x={origin_column} y={origin_row} width={width_half_cells} rows={rows}",
                    site.site,
                );
            }

            let relative = i16::from_le_bytes([setup[13], setup[14]]) as isize;
            let initializer_decoded_offset =
                (start + 15).checked_add_signed(relative).with_context(|| {
                    format!(
                        "cutscene pointer at decoded 0x{:04X} has an invalid initializer call",
                        site.site
                    )
                })?;
            if initializer_decoded_offset >= decoded.len() {
                bail!(
                    "cutscene pointer at decoded 0x{:04X} calls a window initializer outside the decoded image",
                    site.site,
                );
            }
            initializer_targets.insert(initializer_decoded_offset);
            entry.layout_windows.push(CutsceneWindow {
                rewrite_site: site.site,
                origin_column,
                origin_row,
                width_half_cells,
                rows,
                initializer_decoded_offset,
            });
        }
    }
    if initializer_targets.len() != 1 {
        bail!(
            "cutscene dialogue reaches {} distinct window initializers, expected one",
            initializer_targets.len(),
        );
    }
    Ok(())
}

fn verify_overlay_contiguity(decoded: &[u8], entries: &[CutsceneEntry]) -> Result<()> {
    let starts: BTreeSet<_> = entries
        .iter()
        .map(|entry| entry.string_decoded_offset)
        .collect();
    for entry in entries {
        let expected = entry.string_decoded_offset + entry.raw.len() + 1;
        let Some(relative_end) = decoded
            .get(expected..)
            .and_then(|tail| tail.iter().position(|&byte| byte == 0x24))
        else {
            continue;
        };
        if relative_end == 0 || relative_end > 1024 {
            continue;
        }
        let raw = &decoded[expected..expected + relative_end];
        if validate_overlay_text(raw) && !starts.contains(&expected) {
            bail!(
                "unreferenced cutscene text follows 0x{:04X} at 0x{expected:04X}",
                entry.string_decoded_offset,
            );
        }
    }
    Ok(())
}

pub fn catalog_overlay_dialogue(decoded: &[u8]) -> Result<(usize, Vec<CutsceneEntry>)> {
    let mut candidates = [0x100usize, 0x101]
        .into_iter()
        .map(|load_offset| (load_offset, catalog_overlay_at_load(decoded, load_offset)))
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(_, entries)| std::cmp::Reverse(entries.len()));

    let (load_offset, mut entries) = candidates.remove(0);
    if entries.is_empty() {
        bail!("no `$`-terminated cutscene text referenced by mov dx");
    }
    if candidates
        .first()
        .is_some_and(|(_, other)| other.len() == entries.len())
    {
        bail!("cutscene load offset is ambiguous");
    }
    attach_dialogue_windows(decoded, &mut entries)?;
    verify_overlay_contiguity(decoded, &entries)?;
    verify_entries(decoded, &entries)?;
    Ok((load_offset, entries))
}

pub fn catalog_nul_region(decoded: &[u8], spec: NulRegionSpec) -> Result<Vec<CutsceneEntry>> {
    if spec.start >= spec.end || spec.end > decoded.len() {
        bail!(
            "{} NUL region 0x{:04X}..0x{:04X} is outside decoded size 0x{:04X}",
            spec.label,
            spec.start,
            spec.end,
            decoded.len(),
        );
    }

    let mut entries = Vec::new();
    let mut cursor = spec.start;
    while cursor < spec.end {
        let relative_end = decoded[cursor..spec.end]
            .iter()
            .position(|&byte| byte == 0)
            .with_context(|| {
                format!(
                    "{} slot at 0x{cursor:04X} lacks a NUL before 0x{:04X}",
                    spec.label, spec.end
                )
            })?;
        let slot_end = cursor + relative_end;
        let slot = &decoded[cursor..slot_end];
        let prefix_len = usize::from(slot.len() >= 2 && slot[0] == 0x40) * 2;
        let prefix = slot[..prefix_len].to_vec();
        let raw = slot[prefix_len..].to_vec();
        let (text, had_decode_errors, has_mapped_visible_text) = decode_text(&raw);
        let structural = !has_mapped_visible_text;
        entries.push(CutsceneEntry {
            region: spec.label,
            slot_decoded_offset: cursor,
            string_decoded_offset: cursor + prefix_len,
            string_logical_offset: None,
            byte_budget: raw.len() + 1,
            prefix,
            raw,
            text,
            terminator: 0,
            rewrite_sites: Vec::new(),
            layout_windows: Vec::new(),
            structural,
            had_decode_errors,
        });
        cursor = slot_end + 1;
    }
    if cursor != spec.end {
        bail!(
            "{} NUL region ended at 0x{cursor:04X}, expected 0x{:04X}",
            spec.label,
            spec.end,
        );
    }
    verify_entries(decoded, &entries)?;
    Ok(entries)
}

fn verify_bytes(decoded: &[u8], offset: usize, expected: &[u8], label: &str) -> Result<()> {
    if decoded.get(offset..offset + expected.len()) != Some(expected) {
        bail!("{label} signature does not match at decoded 0x{offset:04X}");
    }
    Ok(())
}

fn apply_pointer_grid(
    decoded: &[u8],
    load_offset: usize,
    spec: PointerGridSpec,
    entries: &mut [CutsceneEntry],
) -> Result<()> {
    if spec.start >= spec.end || !(spec.end - spec.start).is_multiple_of(2) {
        bail!(
            "{} pointer grid 0x{:04X}..0x{:04X} is not a non-empty word range",
            spec.region,
            spec.start,
            spec.end,
        );
    }
    if spec.end > decoded.len() {
        bail!(
            "{} pointer grid ends outside decoded size 0x{:04X}",
            spec.region,
            decoded.len(),
        );
    }

    let table_logical = spec
        .start
        .checked_add(load_offset)
        .context("pointer-grid logical base overflow")?;
    let table_logical =
        u16::try_from(table_logical).context("pointer-grid logical base exceeds 16-bit segment")?;
    verify_bytes(
        decoded,
        spec.consumer_table_operand_site.saturating_sub(3),
        &[0x2E, 0x8B, 0xB7],
        "pointer-grid consumer table load",
    )?;
    verify_bytes(
        decoded,
        spec.consumer_table_operand_site,
        &table_logical.to_le_bytes(),
        "pointer-grid consumer table base",
    )?;
    verify_bytes(
        decoded,
        spec.consumer_string_read_site,
        &[0x2E, 0x8B, 0x14, 0x86, 0xF2, 0x80, 0xFE, 0x00],
        "pointer-grid consumer string read",
    )?;
    verify_bytes(
        decoded,
        spec.consumer_index_advance_site,
        &[0x2E, 0x83, 0x06, 0x31, 0x51, 0x02],
        "pointer-grid consumer index advance",
    )?;

    let region_indices = entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| (entry.region == spec.region).then_some(index))
        .collect::<Vec<_>>();
    if region_indices.is_empty() {
        bail!("pointer grid has no entries for region {:?}", spec.region);
    }

    let mut by_logical = BTreeMap::<u16, usize>::new();
    for &index in &region_indices {
        let entry = &entries[index];
        if entry.terminator != 0 {
            bail!(
                "{} entry 0x{:04X} is not NUL-terminated",
                spec.region,
                entry.string_decoded_offset,
            );
        }
        if !entry.raw.len().is_multiple_of(2) {
            bail!(
                "{} entry 0x{:04X} is not a two-byte cell stream",
                spec.region,
                entry.string_decoded_offset,
            );
        }
        let logical = entry
            .string_decoded_offset
            .checked_add(load_offset)
            .context("pointer-grid string logical offset overflow")?;
        let logical = u16::try_from(logical)
            .context("pointer-grid string logical offset exceeds 16-bit segment")?;
        if by_logical.insert(logical, index).is_some() {
            bail!("duplicate pointer-grid target 0x{logical:04X}");
        }
    }

    const SENTINELS: [u16; 4] = [0xFFFF, 0xFFFE, 0xFFFD, 0xFFFC];
    if decoded.get(spec.end - 2..spec.end) != Some(&0xFFFFu16.to_le_bytes()) {
        bail!(
            "{} pointer grid lacks its final 0xFFFF sentinel",
            spec.region
        );
    }
    for site in (spec.start..spec.end).step_by(2) {
        let value = u16::from_le_bytes([decoded[site], decoded[site + 1]]);
        if SENTINELS.contains(&value) {
            continue;
        }
        let Some(&entry_index) = by_logical.get(&value) else {
            bail!(
                "{} pointer grid site 0x{site:04X} has unknown target 0x{value:04X}",
                spec.region,
            );
        };
        entries[entry_index]
            .rewrite_sites
            .push(CutsceneRewriteSite {
                site,
                kind: CutsceneRewriteKind::PointerGrid,
            });
    }

    for index in region_indices {
        let entry = &mut entries[index];
        if entry.rewrite_sites.len() != 1 {
            bail!(
                "{} entry 0x{:04X} has {} pointer-grid references, expected exactly one",
                spec.region,
                entry.string_decoded_offset,
                entry.rewrite_sites.len(),
            );
        }
        entry.string_logical_offset = Some(entry.string_decoded_offset + load_offset);
    }
    Ok(())
}

pub fn catalog_resource(packed: &[u8], spec: CutsceneResourceSpec) -> Result<CutsceneCatalog> {
    let decoded = decode_overlay_lz(packed)
        .with_context(|| format!("decode {}", spec.file))?
        .output;
    let (load_offset, mut entries) = match spec.layout {
        ResourceLayout::OverlayDialogue {
            extra_nul_regions,
            pointer_grids,
        } => {
            let (load_offset, mut entries) = catalog_overlay_dialogue(&decoded)
                .with_context(|| format!("catalog {} dialogue", spec.file))?;
            for region in extra_nul_regions {
                entries.extend(catalog_nul_region(&decoded, *region)?);
            }
            for pointer_grid in pointer_grids {
                apply_pointer_grid(&decoded, load_offset, *pointer_grid, &mut entries)
                    .with_context(|| format!("catalog {} pointer grid", spec.file))?;
            }
            (Some(load_offset), entries)
        }
        ResourceLayout::NulRegions(regions) => {
            let mut entries = Vec::new();
            for region in regions {
                entries.extend(catalog_nul_region(&decoded, *region)?);
            }
            (None, entries)
        }
    };
    entries.sort_by_key(|entry| entry.slot_decoded_offset);

    Ok(CutsceneCatalog {
        spec,
        packed_size: packed.len(),
        decoded_size: decoded.len(),
        load_offset,
        entries,
    })
}

pub fn verify_entries(decoded: &[u8], entries: &[CutsceneEntry]) -> Result<()> {
    for entry in entries {
        let prefix_end = entry.slot_decoded_offset + entry.prefix.len();
        if decoded.get(entry.slot_decoded_offset..prefix_end) != Some(entry.prefix.as_slice()) {
            bail!(
                "entry 0x{:04X} control prefix does not match the decoded resource",
                entry.slot_decoded_offset
            );
        }
        let raw_end = entry.string_decoded_offset + entry.raw.len();
        if decoded.get(entry.string_decoded_offset..raw_end) != Some(entry.raw.as_slice()) {
            bail!(
                "entry 0x{:04X} raw bytes do not match the decoded resource",
                entry.string_decoded_offset
            );
        }
        if decoded.get(raw_end) != Some(&entry.terminator) {
            bail!(
                "entry 0x{:04X} terminator is not 0x{:02X}",
                entry.string_decoded_offset,
                entry.terminator,
            );
        }
        if let Some(logical) = entry.string_logical_offset {
            let expected = (logical as u16).to_le_bytes();
            for site in &entry.rewrite_sites {
                if decoded.get(site.site..site.site + 2) != Some(&expected[..]) {
                    bail!(
                        "entry 0x{:04X} rewrite site 0x{:04X} does not hold 0x{logical:04X}",
                        entry.string_decoded_offset,
                        site.site,
                    );
                }
            }
        }
    }
    Ok(())
}

pub fn unique_hangul_syllables<'a>(texts: impl IntoIterator<Item = &'a str>) -> BTreeSet<char> {
    texts
        .into_iter()
        .flat_map(str::chars)
        .filter(|ch| ('가'..='힣').contains(ch))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_mov_dx(decoded: &mut [u8], at: usize, logical: usize) {
        let setup = at - 15;
        decoded[setup..setup + 12].copy_from_slice(&[
            0xB8, 0x18, 0x00, // mov ax, 24: origin column
            0xBB, 0x12, 0x00, // mov bx, 18: origin row
            0xB9, 0x1E, 0x00, // mov cx, 30: 15 full-width cells
            0xBA, 0x04, 0x00, // mov dx, 4: rows
        ]);
        decoded[setup + 12] = 0xE8;
        let relative = 0x70isize - at as isize;
        decoded[setup + 13..setup + 15].copy_from_slice(&(relative as i16).to_le_bytes());
        decoded[at] = 0xBA;
        decoded[at + 1..at + 3].copy_from_slice(&(logical as u16).to_le_bytes());
        decoded[at + 3..at + 8].copy_from_slice(&[0xE8, 0x00, 0x00, 0x1F, 0xC3]);
    }

    #[test]
    fn overlay_catalog_uses_mov_dx_targets_and_dollar_terminators() {
        let mut decoded = vec![0u8; 0x100];
        let first = 0x80usize;
        let first_raw = [0x81, 0x75, 0x82, 0xA0, 0x0D]; // 「あ + CR
        decoded[first - 1] = 0xC3;
        decoded[first..first + first_raw.len()].copy_from_slice(&first_raw);
        decoded[first + first_raw.len()] = 0x24;
        let second = first + first_raw.len() + 1;
        let second_raw = [0x82, 0xA2, 0x82, 0xA4]; // いう
        decoded[second..second + second_raw.len()].copy_from_slice(&second_raw);
        decoded[second + second_raw.len()] = 0x24;
        write_mov_dx(&mut decoded, 0x20, first + 0x101);
        write_mov_dx(&mut decoded, 0x40, second + 0x101);

        let (load_offset, entries) = catalog_overlay_dialogue(&decoded).unwrap();

        assert_eq!(load_offset, 0x101);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].text, "「あ\n");
        assert_eq!(entries[0].rewrite_sites[0].site, 0x21);
        assert_eq!(entries[0].rewrite_sites[0].kind, CutsceneRewriteKind::MovDx);
        assert_eq!(entries[0].layout_windows[0].width_half_cells, 30);
        assert_eq!(entries[0].layout_windows[0].rows, 4);
        assert_eq!(
            entries[0].layout_windows[0].initializer_decoded_offset,
            0x70
        );
        assert_eq!(entries[1].text, "いう");
        verify_entries(&decoded, &entries).unwrap();
    }

    #[test]
    fn overlay_catalog_rejects_a_dialogue_pointer_without_its_window_setup() {
        let mut decoded = vec![0u8; 0x100];
        let text = 0x80usize;
        decoded[text - 1] = 0xC3;
        decoded[text..text + 3].copy_from_slice(&[0x82, 0xA0, 0x24]);
        write_mov_dx(&mut decoded, 0x20, text + 0x101);
        decoded[0x20 - 15 + 6] = 0x90;

        let error = catalog_overlay_dialogue(&decoded).unwrap_err().to_string();

        assert!(error.contains("window setup"), "{error}");
    }

    #[test]
    fn nul_catalog_strips_resource_prefix_and_keeps_structural_slots() {
        let mut decoded = vec![0u8; 0x40];
        decoded[0x10..0x17].copy_from_slice(&[0x40, 0xA1, 0x82, 0xA0, 0x82, 0xA2, 0]);
        decoded[0x17..0x1A].copy_from_slice(&[0x20, 0x20, 0]);
        decoded[0x1A..0x20].copy_from_slice(&[0x20, 0x85, 0x47, 0x81, 0x40, 0]);
        let spec = NulRegionSpec {
            label: "dialogue",
            start: 0x10,
            end: 0x20,
        };

        let entries = catalog_nul_region(&decoded, spec).unwrap();

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].prefix, [0x40, 0xA1]);
        assert_eq!(entries[0].string_decoded_offset, 0x12);
        assert_eq!(entries[0].text, "あい");
        assert!(!entries[0].structural);
        assert!(entries[1].structural);
        assert!(entries[2].structural);
        assert!(entries[2].had_decode_errors);
        assert!(entries[2].text.contains("{raw:8547}"));
        verify_entries(&decoded, &entries).unwrap();
    }

    #[test]
    fn pointer_grid_maps_every_nul_entry_and_verifies_the_consumer() {
        let load_offset = 0x100usize;
        let mut decoded = vec![0u8; 0x100];
        let region = NulRegionSpec {
            label: "credits",
            start: 0x80,
            end: 0x8A,
        };
        decoded[0x80..0x85].copy_from_slice(&[0x82, 0xA0, 0x82, 0xA2, 0]);
        decoded[0x85..0x8A].copy_from_slice(&[0x82, 0xA4, 0x82, 0xA6, 0]);
        let mut entries = catalog_nul_region(&decoded, region).unwrap();

        let grid = PointerGridSpec {
            region: "credits",
            start: 0x50,
            end: 0x5C,
            consumer_table_operand_site: 0x13,
            consumer_string_read_site: 0x20,
            consumer_index_advance_site: 0x30,
        };
        decoded[0x10..0x15].copy_from_slice(&[0x2E, 0x8B, 0xB7, 0x50, 0x01]);
        decoded[0x20..0x28].copy_from_slice(&[0x2E, 0x8B, 0x14, 0x86, 0xF2, 0x80, 0xFE, 0x00]);
        decoded[0x30..0x36].copy_from_slice(&[0x2E, 0x83, 0x06, 0x31, 0x51, 0x02]);
        decoded[0x50..0x5C].copy_from_slice(&[
            0x80, 0x01, 0xFE, 0xFF, 0x85, 0x01, 0xFD, 0xFF, 0xFC, 0xFF, 0xFF, 0xFF,
        ]);

        apply_pointer_grid(&decoded, load_offset, grid, &mut entries).unwrap();

        assert_eq!(entries[0].string_logical_offset, Some(0x180));
        assert_eq!(entries[0].rewrite_sites[0].site, 0x50);
        assert_eq!(entries[1].string_logical_offset, Some(0x185));
        assert_eq!(entries[1].rewrite_sites[0].site, 0x54);
        assert!(
            entries
                .iter()
                .all(|entry| { entry.rewrite_sites[0].kind == CutsceneRewriteKind::PointerGrid })
        );
        verify_entries(&decoded, &entries).unwrap();
    }

    #[test]
    fn demand_counts_only_hangul_syllables() {
        let demand = unique_hangul_syllables(["가나다{br}", "나다 A"]);
        assert_eq!(demand.into_iter().collect::<String>(), "가나다");
    }
}
