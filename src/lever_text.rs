//! Schezo's lever substitutes a two-byte state into two independently placed
//! messages. Both the values and the write destinations must follow translation.
use crate::{
    overlay_batch::SourceSlotMessagePlacement,
    overlay_reloc::SheetCodes,
    v30_assembler::{assemble_at, direct_memory, mov, reg16},
};
use anyhow::{Context, Result, bail};
use std::path::Path;
use v30::{Assembler, OperandSize, Register16, SegmentRegister};

pub fn state_catalog(root: &Path) -> Result<serde_json::Value> {
    let path = root.join("game_s_all.json");
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
    value
        .get("lever_states")
        .cloned()
        .context("Schezo translation catalog lacks lever_states")
}

pub fn install(
    decoded: &mut [u8],
    placements: &[SourceSlotMessagePlacement],
    sheet: &SheetCodes,
    catalog: &serde_json::Value,
    allow_needs_review: bool,
) -> Result<()> {
    let entries = catalog["entries"]
        .as_array()
        .context("lever states missing entries")?;
    if entries.len() != 2 {
        bail!("lever state table needs exactly two words");
    }
    let mut words = Vec::new();
    for (entry, expected) in entries.iter().zip(["上", "下"]) {
        if entry["text"] != expected {
            bail!("lever state source order changed");
        }
        let status = entry["status"].as_str().unwrap_or("");
        if status != "complete" && !(allow_needs_review && status == "needs_review") {
            bail!("lever state translation is not approved");
        }
        let ko = entry["ko"].as_str().context("lever state lacks ko")?;
        let word = sheet.encode_line(ko)?;
        if word.len() != 2 || !ko.chars().all(|c| ('가'..='힣').contains(&c)) {
            bail!("lever state must encode as one Hangul word");
        }
        words.extend(word);
    }
    if decoded.get(0x6FA5..0x6FA9) != Some(&[0x8F, 0xE3, 0x89, 0xBA][..]) {
        bail!("lever source state table differs");
    }
    let base = placements
        .iter()
        .find(|p| p.source_offset == 0x6FC6)
        .context("lever current-state message has no placement")?
        .destination_offset;
    let marker = sheet.encode_line("＊")?;
    let mut patches = Vec::new();
    for (base, site, old_target, register) in [
        (base, 0x6ED7, 0x70D4, Register16::AX),
        (0x7000, 0x6EDB, 0x710A, Register16::DX),
    ] {
        let end = decoded
            .get(base..)
            .context("lever message outside overlay")?
            .iter()
            .position(|b| *b == 0)
            .context("lever message unterminated")?
            + base;
        let positions = decoded[base..end]
            .windows(2)
            .enumerate()
            .filter_map(|(i, b)| (b == marker).then_some(i))
            .collect::<Vec<_>>();
        if positions.len() != 1 {
            bail!("lever message must contain exactly one substitution marker");
        }
        let target = u16::try_from(base + positions[0] + 0x100)?;
        let assemble = |address| -> Result<Vec<u8>> {
            let mut assembler = Assembler::new();
            assembler.emit(mov(
                direct_memory(Some(SegmentRegister::CS), address, OperandSize::Word),
                reg16(register),
            ));
            assemble_at(&assembler, (site + 0x100) as u16, "lever state write")
        };
        let source = assemble(old_target)?;
        if decoded.get(site..site + source.len()) != Some(source.as_slice()) {
            bail!("lever state writer signature differs at {site:04X}");
        }
        let patch = assemble(target)?;
        if patch.len() != source.len() {
            bail!("lever state writer changed instruction length");
        }
        patches.push((site, patch));
    }
    // Validate every writer before applying this unit; do not modify event state.
    decoded[0x6FA5..0x6FA9].copy_from_slice(&words);
    for (site, patch) in patches {
        decoded[site..site + patch.len()].copy_from_slice(&patch);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn both_states_follow_translated_placeholder_positions() {
        let sheet = SheetCodes::from_json_str(
            r#"{"glyphs":[{"char":"위","sjis":"eb9f"},{"char":"밑","sjis":"eba0"}]}"#,
        )
        .unwrap();
        let catalog = serde_json::json!({"entries":[{"text":"上","ko":"위","status":"needs_review"},{"text":"下","ko":"밑","status":"needs_review"}]});
        let mut bytes = vec![0; 0x8000];
        bytes[0x6FA5..0x6FA9].copy_from_slice(&[0x8F, 0xE3, 0x89, 0xBA]);
        bytes[0x6ED7..0x6EE0]
            .copy_from_slice(&[0x2E, 0xA3, 0xD4, 0x70, 0x2E, 0x89, 0x16, 0x0A, 0x71]);
        let text = sheet.encode_line("ABC ＊ DEF").unwrap();
        for base in [0x7400, 0x7000] {
            bytes[base..base + text.len()].copy_from_slice(&text);
        }
        let placements = [SourceSlotMessagePlacement {
            source_offset: 0x6FC6,
            destination_offset: 0x7400,
            source_budget: 34,
            payload_len: text.len(),
        }];
        let before = bytes.clone();
        let mut missing = bytes.clone();
        missing[0x7404] = 0;
        let unchanged = missing.clone();
        assert!(install(&mut missing, &placements, &sheet, &catalog, true).is_err());
        assert_eq!(missing, unchanged);
        install(&mut bytes, &placements, &sheet, &catalog, true).unwrap();
        let current =
            u16::from_le_bytes(bytes[0x6ED9..0x6EDB].try_into().unwrap()) as usize - 0x100;
        let alternate =
            u16::from_le_bytes(bytes[0x6EDE..0x6EE0].try_into().unwrap()) as usize - 0x100;
        assert_eq!(current, 0x7404);
        assert_eq!(alternate, 0x7004);
        for flip in [false, true] {
            let mut runtime = bytes.clone();
            let upper = bytes[0x6FA5..0x6FA7].to_vec();
            let lower = bytes[0x6FA7..0x6FA9].to_vec();
            runtime[current..current + 2].copy_from_slice(if flip { &lower } else { &upper });
            runtime[alternate..alternate + 2].copy_from_slice(if flip { &upper } else { &lower });
            let first = sheet
                .encode_line(if flip { "ABC 밑 DEF" } else { "ABC 위 DEF" })
                .unwrap();
            let next = sheet
                .encode_line(if flip { "ABC 위 DEF" } else { "ABC 밑 DEF" })
                .unwrap();
            assert_eq!(&runtime[0x7400..0x7400 + first.len()], first);
            assert_eq!(&runtime[0x7000..0x7000 + next.len()], next);
        }
        for i in 0..bytes.len() {
            if !(0x6FA5..0x6FA9).contains(&i) && !(0x6ED7..0x6EE0).contains(&i) {
                assert_eq!(before[i], bytes[i]);
            }
        }
    }
}
