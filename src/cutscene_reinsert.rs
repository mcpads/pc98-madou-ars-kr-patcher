//! Reinsertion primitives for the A.R.S cutscene resources.
//!
//! Schezo's ending credits are not fixed slots and are not consumed as one
//! sequential NUL stream. The overlay contains a word grid whose non-sentinel
//! cells point at each NUL-terminated credit. Repacking the whole bounded
//! string region and rewriting those words preserves the overlay size while
//! allowing individual translated credits to grow.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result, bail};

use crate::cutscene_catalog::{
    CutsceneCatalog, CutsceneEntry, CutsceneRewriteKind, NulRegionSpec, ResourceLayout,
    verify_entries,
};
use crate::cutscene_translation::encode_cutscene_text;
use crate::dialogue_layout::validate_cutscene_layout;
use crate::gaiji_table::GaijiTable;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointerGridPlacement {
    pub original_decoded_offset: usize,
    pub new_decoded_offset: usize,
    pub encoded_len: usize,
    pub rewrite_sites: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointerGridRepackReport {
    pub capacity: usize,
    pub used: usize,
    pub spare: usize,
    pub moved_entries: usize,
    pub rewritten_sites: usize,
    pub placements: Vec<PointerGridPlacement>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CutsceneReinsertReport {
    pub in_place: usize,
    pub relocated: usize,
    pub relocation_bytes: usize,
    pub rewritten_mov_dx_sites: usize,
    pub final_decoded_size: usize,
    pub pointer_grid_repacks: Vec<PointerGridRepackReport>,
}

/// Repack a bounded NUL region whose entries are independently addressed by a
/// little-endian pointer grid. Replacement payloads are keyed by the entries'
/// original decoded string offsets and do not include the NUL terminator.
/// Missing replacements preserve the original payload.
///
/// Every payload must remain a two-byte cell stream because the verified
/// consumer reads `word [cs:si]` and advances `si` by two. The function plans
/// and validates the complete write before mutating `decoded`.
pub fn repack_pointer_grid_region(
    decoded: &mut [u8],
    region: NulRegionSpec,
    load_offset: usize,
    entries: &[CutsceneEntry],
    replacements: &BTreeMap<usize, Vec<u8>>,
) -> Result<PointerGridRepackReport> {
    if region.start >= region.end || region.end > decoded.len() {
        bail!(
            "{} repack region 0x{:04X}..0x{:04X} is outside decoded size 0x{:04X}",
            region.label,
            region.start,
            region.end,
            decoded.len(),
        );
    }
    verify_entries(decoded, entries).context("verify pointer-grid entries before repack")?;

    let mut region_entries = entries
        .iter()
        .filter(|entry| entry.region == region.label)
        .collect::<Vec<_>>();
    region_entries.sort_by_key(|entry| entry.slot_decoded_offset);
    if region_entries.is_empty() {
        bail!(
            "no entries found for pointer-grid region {:?}",
            region.label
        );
    }

    let known_offsets = region_entries
        .iter()
        .map(|entry| entry.string_decoded_offset)
        .collect::<std::collections::BTreeSet<_>>();
    if let Some(unknown) = replacements
        .keys()
        .find(|offset| !known_offsets.contains(offset))
    {
        bail!("replacement targets unknown entry 0x{unknown:04X}");
    }

    struct Plan<'a> {
        entry: &'a CutsceneEntry,
        payload: &'a [u8],
        new_slot: usize,
        new_string: usize,
    }

    let mut cursor = region.start;
    let mut plans = Vec::with_capacity(region_entries.len());
    for entry in region_entries {
        if entry.terminator != 0 {
            bail!(
                "entry 0x{:04X} is not NUL-terminated",
                entry.string_decoded_offset,
            );
        }
        if entry.rewrite_sites.is_empty()
            || entry
                .rewrite_sites
                .iter()
                .any(|site| site.kind != CutsceneRewriteKind::PointerGrid)
        {
            bail!(
                "entry 0x{:04X} is not exclusively pointer-grid addressed",
                entry.string_decoded_offset,
            );
        }
        let payload = replacements
            .get(&entry.string_decoded_offset)
            .map(Vec::as_slice)
            .unwrap_or(&entry.raw);
        if !payload.len().is_multiple_of(2) {
            bail!(
                "entry 0x{:04X} replacement is {} bytes, not a two-byte cell stream",
                entry.string_decoded_offset,
                payload.len(),
            );
        }
        if payload.contains(&0) {
            bail!(
                "entry 0x{:04X} replacement contains an embedded NUL",
                entry.string_decoded_offset,
            );
        }
        let new_slot = cursor;
        let new_string = new_slot + entry.prefix.len();
        cursor = new_string
            .checked_add(payload.len() + 1)
            .context("pointer-grid repack size overflow")?;
        if cursor > region.end {
            bail!(
                "{} pointer-grid strings need {} bytes but the region holds {}",
                region.label,
                cursor - region.start,
                region.end - region.start,
            );
        }

        let old_logical = entry
            .string_logical_offset
            .context("pointer-grid entry lacks a logical offset")?;
        let expected = u16::try_from(old_logical)
            .context("old pointer-grid target exceeds 16 bits")?
            .to_le_bytes();
        for site in &entry.rewrite_sites {
            if region.start <= site.site && site.site < region.end {
                bail!(
                    "pointer-grid rewrite site 0x{:04X} overlaps its string region",
                    site.site
                );
            }
            if decoded.get(site.site..site.site + 2) != Some(expected.as_slice()) {
                bail!(
                    "pointer-grid site 0x{:04X} does not hold old target 0x{old_logical:04X}",
                    site.site,
                );
            }
        }
        u16::try_from(new_string + load_offset)
            .context("new pointer-grid target exceeds 16-bit segment")?;
        plans.push(Plan {
            entry,
            payload,
            new_slot,
            new_string,
        });
    }

    decoded[region.start..region.end].fill(0);
    let mut placements = Vec::with_capacity(plans.len());
    let mut rewritten_sites = 0usize;
    for plan in plans {
        let prefix_end = plan.new_slot + plan.entry.prefix.len();
        decoded[plan.new_slot..prefix_end].copy_from_slice(&plan.entry.prefix);
        let payload_end = plan.new_string + plan.payload.len();
        decoded[plan.new_string..payload_end].copy_from_slice(plan.payload);
        decoded[payload_end] = 0;

        let new_logical = u16::try_from(plan.new_string + load_offset)
            .expect("new target was validated before mutation");
        for site in &plan.entry.rewrite_sites {
            decoded[site.site..site.site + 2].copy_from_slice(&new_logical.to_le_bytes());
        }
        rewritten_sites += plan.entry.rewrite_sites.len();
        placements.push(PointerGridPlacement {
            original_decoded_offset: plan.entry.string_decoded_offset,
            new_decoded_offset: plan.new_string,
            encoded_len: plan.payload.len(),
            rewrite_sites: plan
                .entry
                .rewrite_sites
                .iter()
                .map(|site| site.site)
                .collect(),
        });
    }

    let used = cursor - region.start;
    Ok(PointerGridRepackReport {
        capacity: region.end - region.start,
        used,
        spare: region.end - cursor,
        moved_entries: placements
            .iter()
            .filter(|placement| placement.original_decoded_offset != placement.new_decoded_offset)
            .count(),
        rewritten_sites,
        placements,
    })
}

/// Apply a complete translated catalog to one decoded cutscene resource.
///
/// Length-fitting DAT/OVL entries stay in their original slots. Oversized OVL
/// dialogue is appended and every verified `mov dx` immediate is rewritten.
/// Pointer-grid regions are repacked through their separately bounded policy.
/// The whole operation is planned on a clone and committed only after every
/// encoding, capacity, pointer, and terminator check passes.
pub fn apply_cutscene_translations(
    decoded: &mut Vec<u8>,
    catalog: &CutsceneCatalog,
    translations: &BTreeMap<usize, String>,
    gaiji: &GaijiTable,
) -> Result<CutsceneReinsertReport> {
    if decoded.len() != catalog.decoded_size {
        bail!(
            "{} decoded size is 0x{:04X}, catalog expects 0x{:04X}",
            catalog.spec.file,
            decoded.len(),
            catalog.decoded_size,
        );
    }
    if crate::cutscene_catalog::reserved_gaiji_slots(catalog.spec.file) != 0 {
        // Original ED_A initializer: opcode 15, count 26h, first slot 21h,
        // source resource 10h, bitmap offset 504Ah.
        if decoded.get(0x26..0x2c) != Some(&[0x15, 0x26, 0x21, 0x10, 0x4a, 0x50]) {
            bail!("ED_A original picture-gaiji initializer changed");
        }
        if gaiji
            .entries
            .iter()
            .any(|entry| (0x7621..=0x7646).contains(&entry.jis))
        {
            bail!("ED_A Hangul collides with original picture gaiji 7621..7646");
        }
    }
    verify_entries(decoded, &catalog.entries)
        .with_context(|| format!("verify {} before reinsertion", catalog.spec.file))?;

    let expected = catalog
        .entries
        .iter()
        .filter(|entry| !entry.structural)
        .map(|entry| entry.string_decoded_offset)
        .collect::<BTreeSet<_>>();
    if let Some(missing) = expected
        .iter()
        .find(|offset| !translations.contains_key(offset))
    {
        bail!(
            "{} missing translation for 0x{missing:04X}",
            catalog.spec.file
        );
    }
    if let Some(extra) = translations
        .keys()
        .find(|offset| !expected.contains(offset))
    {
        bail!(
            "{} has translation for unknown or structural entry 0x{extra:04X}",
            catalog.spec.file
        );
    }

    struct Encoded<'a> {
        entry: &'a CutsceneEntry,
        bytes: Vec<u8>,
        pointer_grid: bool,
    }
    let overlay_wide_cells = matches!(catalog.spec.layout, ResourceLayout::OverlayDialogue { .. });
    let mut encoded = Vec::with_capacity(expected.len());
    for entry in catalog.entries.iter().filter(|entry| !entry.structural) {
        let pointer_grid = !entry.rewrite_sites.is_empty()
            && entry
                .rewrite_sites
                .iter()
                .all(|site| site.kind == CutsceneRewriteKind::PointerGrid);
        if entry
            .rewrite_sites
            .iter()
            .any(|site| site.kind == CutsceneRewriteKind::PointerGrid)
            && !pointer_grid
        {
            bail!(
                "{} entry 0x{:04X} mixes pointer-grid and other rewrite sites",
                catalog.spec.file,
                entry.string_decoded_offset,
            );
        }
        let text = translations
            .get(&entry.string_decoded_offset)
            .expect("translation coverage was checked");
        if !entry.layout_windows.is_empty() {
            validate_cutscene_layout(
                &entry.text,
                text,
                true,
                &entry.layout_windows,
                &format!(
                    "{} entry 0x{:04X}",
                    catalog.spec.file, entry.string_decoded_offset
                ),
            )?;
        }
        let newline_controls = entry
            .raw
            .iter()
            .copied()
            .filter(|byte| matches!(*byte, b'\n' | b'\r'))
            .collect::<Vec<_>>();
        let mut bytes = encode_cutscene_text(
            text,
            overlay_wide_cells || pointer_grid,
            gaiji,
            &newline_controls,
        )
        .with_context(|| {
            format!(
                "{} encode entry 0x{:04X}",
                catalog.spec.file, entry.string_decoded_offset
            )
        })?;
        if catalog.spec.file == "ED_A.DAT" && entry.region == "credits" {
            pad_arle_credit_row(&entry.raw, &mut bytes)?;
        }
        if bytes.contains(&entry.terminator) {
            bail!(
                "{} entry 0x{:04X} encodes an embedded 0x{:02X} terminator",
                catalog.spec.file,
                entry.string_decoded_offset,
                entry.terminator,
            );
        }
        encoded.push(Encoded {
            entry,
            bytes,
            pointer_grid,
        });
    }

    let mut planned = decoded.clone();
    let mut pointer_replacements = BTreeMap::<&'static str, BTreeMap<usize, Vec<u8>>>::new();
    let mut in_place = Vec::<&Encoded<'_>>::new();
    let mut relocate = Vec::<&Encoded<'_>>::new();
    for item in &encoded {
        if item.pointer_grid {
            pointer_replacements
                .entry(item.entry.region)
                .or_default()
                .insert(item.entry.string_decoded_offset, item.bytes.clone());
        } else if item.bytes.len() < item.entry.byte_budget {
            in_place.push(item);
        } else if !item.entry.rewrite_sites.is_empty()
            && item
                .entry
                .rewrite_sites
                .iter()
                .all(|site| site.kind == CutsceneRewriteKind::MovDx)
            && catalog.load_offset.is_some()
        {
            relocate.push(item);
        } else {
            bail!(
                "{} entry 0x{:04X} needs {} bytes but its slot holds {} and has no relocation policy",
                catalog.spec.file,
                item.entry.string_decoded_offset,
                item.bytes.len() + 1,
                item.entry.byte_budget,
            );
        }
    }

    let mut pointer_grid_repacks = Vec::new();
    if !pointer_replacements.is_empty() {
        let ResourceLayout::OverlayDialogue {
            extra_nul_regions,
            pointer_grids,
        } = catalog.spec.layout
        else {
            bail!(
                "{} has pointer-grid entries in a non-overlay layout",
                catalog.spec.file
            );
        };
        let load_offset = catalog
            .load_offset
            .context("pointer-grid catalog lacks a load offset")?;
        for (region_name, replacements) in pointer_replacements {
            if !pointer_grids.iter().any(|grid| grid.region == region_name) {
                bail!(
                    "{} region {region_name:?} lacks a pointer-grid specification",
                    catalog.spec.file,
                );
            }
            let region = extra_nul_regions
                .iter()
                .find(|region| region.label == region_name)
                .copied()
                .with_context(|| {
                    format!(
                        "{} region {region_name:?} lacks a bounded repack range",
                        catalog.spec.file
                    )
                })?;
            pointer_grid_repacks.push(repack_pointer_grid_region(
                &mut planned,
                region,
                load_offset,
                &catalog.entries,
                &replacements,
            )?);
        }
    }

    for item in &in_place {
        let start = item.entry.string_decoded_offset;
        let end = start + item.entry.byte_budget;
        planned[start..end].fill(0);
        planned[start..start + item.bytes.len()].copy_from_slice(&item.bytes);
        planned[start + item.bytes.len()] = item.entry.terminator;
    }

    let relocation_start = planned.len();
    let mut rewritten_mov_dx_sites = 0usize;
    for item in &relocate {
        let load_offset = catalog
            .load_offset
            .expect("relocation classification requires a load offset");
        let new_decoded = planned.len();
        let new_logical = new_decoded
            .checked_add(load_offset)
            .context("cutscene relocation logical offset overflow")?;
        if new_logical + item.bytes.len() + 1 > 0x10000 {
            bail!(
                "{} relocation for 0x{:04X} crosses the 16-bit segment",
                catalog.spec.file,
                item.entry.string_decoded_offset,
            );
        }
        planned.extend_from_slice(&item.bytes);
        planned.push(item.entry.terminator);
        let pointer = (new_logical as u16).to_le_bytes();
        for site in &item.entry.rewrite_sites {
            planned[site.site..site.site + 2].copy_from_slice(&pointer);
        }
        rewritten_mov_dx_sites += item.entry.rewrite_sites.len();
    }

    let report = CutsceneReinsertReport {
        in_place: in_place.len(),
        relocated: relocate.len(),
        relocation_bytes: planned.len() - relocation_start,
        rewritten_mov_dx_sites,
        final_decoded_size: planned.len(),
        pointer_grid_repacks,
    };
    *decoded = planned;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cutscene_catalog::{
        CutsceneResourceSpec, CutsceneRewriteKind, CutsceneRewriteSite, ResourceLayout,
    };

    fn gaiji() -> GaijiTable {
        GaijiTable::from_json_str(&format!(
            r#"{{"glyphs":[
                {{"char":"가","jis":"0x7621","sjis":"eb9f","glyph_hex":"{zero}"}},
                {{"char":"나","jis":"0x7622","sjis":"eba0","glyph_hex":"{zero}"}}
            ]}}"#,
            zero = "00".repeat(32),
        ))
        .unwrap()
    }

    fn entry(offset: usize, logical: usize, budget: usize, site: usize) -> CutsceneEntry {
        CutsceneEntry {
            region: "credits",
            slot_decoded_offset: offset,
            string_decoded_offset: offset,
            string_logical_offset: Some(logical),
            byte_budget: budget,
            prefix: Vec::new(),
            raw: vec![0x82; budget - 1],
            text: String::new(),
            terminator: 0,
            rewrite_sites: vec![CutsceneRewriteSite {
                site,
                kind: CutsceneRewriteKind::PointerGrid,
            }],
            layout_windows: Vec::new(),
            structural: false,
            had_decode_errors: false,
        }
    }

    #[test]
    fn repacks_variable_length_entries_and_rewrites_every_pointer() {
        let load = 0x100usize;
        let region = NulRegionSpec {
            label: "credits",
            start: 0x80,
            end: 0xA0,
        };
        let entries = vec![entry(0x80, 0x180, 5, 0x10), entry(0x85, 0x185, 5, 0x12)];
        let mut decoded = vec![0u8; 0x100];
        decoded[0x80..0x85].copy_from_slice(&[0x82, 0x82, 0x82, 0x82, 0]);
        decoded[0x85..0x8A].copy_from_slice(&[0x82, 0x82, 0x82, 0x82, 0]);
        decoded[0x10..0x12].copy_from_slice(&0x180u16.to_le_bytes());
        decoded[0x12..0x14].copy_from_slice(&0x185u16.to_le_bytes());
        let replacements = BTreeMap::from([
            (0x80usize, vec![0xEB, 0x9F]),
            (0x85usize, vec![0xEB, 0xA0, 0xEB, 0xA1, 0xEB, 0xA2]),
        ]);

        let report =
            repack_pointer_grid_region(&mut decoded, region, load, &entries, &replacements)
                .unwrap();

        assert_eq!(report.capacity, 0x20);
        assert_eq!(report.used, 10);
        assert_eq!(report.spare, 0x16);
        assert_eq!(report.moved_entries, 1);
        assert_eq!(report.rewritten_sites, 2);
        assert_eq!(&decoded[0x10..0x12], &0x180u16.to_le_bytes());
        assert_eq!(&decoded[0x12..0x14], &0x183u16.to_le_bytes());
        assert_eq!(
            &decoded[0x80..0x8A],
            &[0xEB, 0x9F, 0, 0xEB, 0xA0, 0xEB, 0xA1, 0xEB, 0xA2, 0]
        );
    }

    #[test]
    fn rejects_capacity_overflow_before_mutating() {
        let load = 0x100usize;
        let region = NulRegionSpec {
            label: "credits",
            start: 0x80,
            end: 0x85,
        };
        let entries = vec![entry(0x80, 0x180, 5, 0x10)];
        let mut decoded = vec![0u8; 0x100];
        decoded[0x80..0x85].copy_from_slice(&[0x82, 0x82, 0x82, 0x82, 0]);
        decoded[0x10..0x12].copy_from_slice(&0x180u16.to_le_bytes());
        let before = decoded.clone();
        let replacements = BTreeMap::from([(0x80usize, vec![0xEB; 6])]);

        let error = repack_pointer_grid_region(&mut decoded, region, load, &entries, &replacements)
            .unwrap_err();

        assert!(error.to_string().contains("need 7 bytes"));
        assert_eq!(decoded, before);
    }

    #[test]
    fn applies_in_place_and_mov_dx_relocation_atomically() {
        const NO_REGIONS: &[NulRegionSpec] = &[];
        const NO_GRIDS: &[crate::cutscene_catalog::PointerGridSpec] = &[];
        let spec = CutsceneResourceSpec {
            character: "test",
            scene: "opening",
            file: "TEST.OVL",
            layout: ResourceLayout::OverlayDialogue {
                extra_nul_regions: NO_REGIONS,
                pointer_grids: NO_GRIDS,
            },
        };
        let mut decoded = vec![0u8; 0x100];
        decoded[0x80..0x85].copy_from_slice(&[0x82, 0xA0, 0x82, 0xA2, 0x24]);
        decoded[0x90..0x93].copy_from_slice(&[0x82, 0xA4, 0x24]);
        decoded[0x20..0x22].copy_from_slice(&0x190u16.to_le_bytes());
        let catalog = CutsceneCatalog {
            spec,
            packed_size: 0,
            decoded_size: decoded.len(),
            load_offset: Some(0x100),
            entries: vec![
                CutsceneEntry {
                    region: "dialogue",
                    slot_decoded_offset: 0x80,
                    string_decoded_offset: 0x80,
                    string_logical_offset: Some(0x180),
                    byte_budget: 5,
                    prefix: Vec::new(),
                    raw: vec![0x82, 0xA0, 0x82, 0xA2],
                    text: "あい".to_owned(),
                    terminator: 0x24,
                    rewrite_sites: Vec::new(),
                    layout_windows: Vec::new(),
                    structural: false,
                    had_decode_errors: false,
                },
                CutsceneEntry {
                    region: "dialogue",
                    slot_decoded_offset: 0x90,
                    string_decoded_offset: 0x90,
                    string_logical_offset: Some(0x190),
                    byte_budget: 3,
                    prefix: Vec::new(),
                    raw: vec![0x82, 0xA4],
                    text: "う".to_owned(),
                    terminator: 0x24,
                    rewrite_sites: vec![CutsceneRewriteSite {
                        site: 0x20,
                        kind: CutsceneRewriteKind::MovDx,
                    }],
                    layout_windows: Vec::new(),
                    structural: false,
                    had_decode_errors: false,
                },
            ],
        };
        let translations =
            BTreeMap::from([(0x80usize, "가".to_owned()), (0x90usize, "가나".to_owned())]);

        let report =
            apply_cutscene_translations(&mut decoded, &catalog, &translations, &gaiji()).unwrap();

        assert_eq!(report.in_place, 1);
        assert_eq!(report.relocated, 1);
        assert_eq!(report.relocation_bytes, 5);
        assert_eq!(report.rewritten_mov_dx_sites, 1);
        assert_eq!(&decoded[0x80..0x85], &[0xEB, 0x9F, 0x24, 0, 0]);
        assert_eq!(&decoded[0x20..0x22], &0x200u16.to_le_bytes());
        assert_eq!(&decoded[0x100..], &[0xEB, 0x9F, 0xEB, 0xA0, 0x24]);
    }

    #[test]
    fn production_reinsertion_failure_does_not_partially_mutate() {
        const REGIONS: &[NulRegionSpec] = &[NulRegionSpec {
            label: "dialogue",
            start: 0x10,
            end: 0x15,
        }];
        let spec = CutsceneResourceSpec {
            character: "test",
            scene: "opening",
            file: "TEST.DAT",
            layout: ResourceLayout::NulRegions(REGIONS),
        };
        let mut decoded = vec![0u8; 0x20];
        decoded[0x10..0x15].copy_from_slice(&[0x82, 0xA0, 0x82, 0xA2, 0]);
        let catalog = CutsceneCatalog {
            spec,
            packed_size: 0,
            decoded_size: decoded.len(),
            load_offset: None,
            entries: vec![CutsceneEntry {
                region: "dialogue",
                slot_decoded_offset: 0x10,
                string_decoded_offset: 0x10,
                string_logical_offset: None,
                byte_budget: 5,
                prefix: Vec::new(),
                raw: vec![0x82, 0xA0, 0x82, 0xA2],
                text: "あい".to_owned(),
                terminator: 0,
                rewrite_sites: Vec::new(),
                layout_windows: Vec::new(),
                structural: false,
                had_decode_errors: false,
            }],
        };
        let before = decoded.clone();
        let translations = BTreeMap::from([(0x10usize, "가나다".to_owned())]);

        assert!(
            apply_cutscene_translations(&mut decoded, &catalog, &translations, &gaiji()).is_err()
        );
        assert_eq!(decoded, before);
    }

    #[test]
    fn reinsertion_preserves_each_source_newline_control() {
        const REGIONS: &[NulRegionSpec] = &[NulRegionSpec {
            label: "dialogue",
            start: 0x10,
            end: 0x19,
        }];
        let spec = CutsceneResourceSpec {
            character: "test",
            scene: "opening",
            file: "TEST.DAT",
            layout: ResourceLayout::NulRegions(REGIONS),
        };
        let mut decoded = vec![0u8; 0x20];
        decoded[0x10..0x19].copy_from_slice(&[0x82, 0xA0, 0x0A, 0x82, 0xA2, 0x0D, 0x82, 0xA4, 0]);
        let catalog = CutsceneCatalog {
            spec,
            packed_size: 0,
            decoded_size: decoded.len(),
            load_offset: None,
            entries: vec![CutsceneEntry {
                region: "dialogue",
                slot_decoded_offset: 0x10,
                string_decoded_offset: 0x10,
                string_logical_offset: None,
                byte_budget: 9,
                prefix: Vec::new(),
                raw: vec![0x82, 0xA0, 0x0A, 0x82, 0xA2, 0x0D, 0x82, 0xA4],
                text: "あ\nい\nう".to_owned(),
                terminator: 0,
                rewrite_sites: Vec::new(),
                layout_windows: Vec::new(),
                structural: false,
                had_decode_errors: false,
            }],
        };
        let translations = BTreeMap::from([(0x10usize, "가\n나\n가".to_owned())]);

        apply_cutscene_translations(&mut decoded, &catalog, &translations, &gaiji()).unwrap();

        assert_eq!(
            &decoded[0x10..0x19],
            &[0xEB, 0x9F, 0x0A, 0xEB, 0xA0, 0x0D, 0xEB, 0x9F, 0]
        );
    }
}

// DEMO 10B0 consumes attribute pairs without drawing; its 114E writer uses
// one half-cell for the custom JIS 29/2A/2B rows, two for ordinary kanji/gaiji.
fn arle_credit_width(bytes: &[u8]) -> Result<usize> {
    let mut i = 0;
    let mut width = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'@' {
            if i + 1 == bytes.len() {
                bail!("truncated Arle credit attribute");
            }
            i += 2;
        } else if (0x81..=0x9F).contains(&b) || (0xE0..=0xFC).contains(&b) {
            let tail = *bytes.get(i + 1).context("truncated Arle credit glyph")?;
            let row = (u16::from(b) - if b < 0xA0 { 0x81 } else { 0xC1 }) * 2
                + 0x21
                + u16::from(tail >= 0x9F);
            width += if (0x29..=0x2B).contains(&row) { 1 } else { 2 };
            i += 2;
        } else if b >= 0x20 && !matches!(b, b'\\' | b'#' | b'$') {
            width += 1;
            i += 1;
        } else {
            bail!("unsupported control in Arle credit row");
        }
    }
    Ok(width)
}
fn pad_arle_credit_row(source: &[u8], bytes: &mut Vec<u8>) -> Result<()> {
    let width = arle_credit_width(source)?;
    if width != 44 {
        bail!("Arle source credit row no longer covers 44 half-cells");
    }
    let used = arle_credit_width(bytes)?;
    if used > width {
        bail!("Arle credit row exceeds its 44-half-cell clearing width");
    }
    bytes.resize(bytes.len() + width - used, b' ');
    Ok(())
}

#[cfg(test)]
mod credit_clearing_tests {
    use super::*;
    #[test]
    fn short_credit_overwrites_prior_suffix_and_preserves_attribute_width() {
        let source = vec![b' '; 44];
        let mut next = b"shiki".to_vec();
        pad_arle_credit_row(&source, &mut next).unwrap();
        let mut screen = b"program   TAKESHI                            ".to_vec();
        screen[..next.len()].copy_from_slice(&next);
        assert_eq!(&screen[..5], b"shiki");
        assert!(screen[5..44].iter().all(|b| *b == b' '));
        let mut voice = vec![0x85, 0x47, b'A', 0x85, 0x48];
        pad_arle_credit_row(&source, &mut voice).unwrap();
        assert_eq!(arle_credit_width(&voice).unwrap(), 44);
        assert_eq!(voice.len(), 46);
        let mut company = b"@aCOMPILE".to_vec();
        pad_arle_credit_row(&source, &mut company).unwrap();
        assert_eq!(arle_credit_width(&company).unwrap(), 44);
    }
}
