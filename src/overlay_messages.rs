//! Unified per-message reinsertion list.
//!
//! `overlay_catalog` finds messages whose pointer is an inline `mov si`
//! immediate; `overlay_ptrtable` finds messages reached through data pointer
//! tables. This module merges both into one entry per string, carrying every
//! 16-bit pointer site that must be rewritten when the string is relocated --
//! the input the batch reinserter consumes. A `mov si` pointer lives at the
//! instruction's immediate (`call + 1`); a table pointer lives at the entry
//! site. Both hold the string's logical offset as a little-endian 16-bit value,
//! so relocation rewrites them identically.

use std::collections::{BTreeMap, HashSet, VecDeque};

use anyhow::{Context, Result, bail};
use encoding_rs::SHIFT_JIS;

use crate::overlay_catalog::{catalog_messages, scan_sjis_runs};
use crate::overlay_ptrtable::{find_pointer_tables, string_start_set};
use crate::overlay_text::collect_all_message_refs;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerKind {
    /// The `mov si, imm16` immediate inside a renderer call site.
    MovSi,
    /// An item-use comparison against the inventory label address.
    ItemIdentity,
    /// An entry of a data pointer table.
    Table,
    /// An immediate stored to, or compared with, a battle status-effect
    /// message field that the renderer later consumes as `SI`.
    StatusMessageField,
}

impl PointerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PointerKind::MovSi => "mov_si",
            PointerKind::ItemIdentity => "item_identity",
            PointerKind::Table => "table",
            PointerKind::StatusMessageField => "status_message_field",
        }
    }
}

/// One 16-bit pointer to this message, and what to rewrite it as on relocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RewriteSite {
    /// Decoded offset of the little-endian 16-bit pointer.
    pub site: usize,
    pub kind: PointerKind,
}

/// One unique string with everything the reinserter needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnifiedMessage {
    pub string_decoded_offset: usize,
    pub string_logical_offset: usize,
    /// In-place slot size: string bytes plus the NUL terminator.
    pub byte_budget: usize,
    pub raw: Vec<u8>,
    pub text: String,
    /// Every pointer to rewrite together when this string is relocated.
    pub rewrite_sites: Vec<RewriteSite>,
}

/// Recover variant `mov si, imm16` renderer consumers that the strict signature
/// does not cover. A bare `0xBE` byte is not an instruction boundary: an address
/// immediate ending in `BE` followed by two unrelated instruction bytes can
/// spell a false `mov si`. Decode a bounded control-flow graph from the
/// candidate, track whether its `SI` value remains live across clobbers and
/// save/restore pairs, and require a transfer to a renderer consumer already
/// proven by the strict catalog or by an exact wrapper/scheduler shape before
/// granting the immediate rewrite ownership.
fn variant_renderer_string_refs(
    decoded: &[u8],
    load_offset: usize,
    starts: &HashSet<usize>,
    text_spans: &[(usize, usize)],
    renderer_targets: &HashSet<usize>,
) -> Vec<(usize, usize)> {
    const MAX_CONSUMER_BYTES: usize = 96;

    let mut refs = Vec::new();
    if decoded.len() < 3 {
        return refs;
    }
    for pos in 0..=decoded.len() - 3 {
        if decoded[pos] != 0xBE {
            continue;
        }
        // `0xBE` is also a valid Shift-JIS trail byte.  In GAME_S, the byte
        // sequence `82 BE 00 82` occurred at three string boundaries and looked
        // like `mov si,0x8200` when scanned without a code/text boundary.  Never
        // trust an instruction candidate whose opcode or immediate overlaps a
        // confirmed text slot.  Strict renderer-call sites are inserted
        // separately below, so this guard only narrows the heuristic broad scan.
        if text_spans
            .iter()
            .any(|&(start, end)| pos < end && start < pos + 3)
        {
            continue;
        }
        let value = u16::from_le_bytes([decoded[pos + 1], decoded[pos + 2]]) as usize;
        let Some(target) = value.checked_sub(load_offset) else {
            continue;
        };
        if !starts.contains(&target) {
            continue;
        }
        let Ok(candidate) = v30::decode_bytes(&decoded[pos..]) else {
            continue;
        };
        if candidate.byte_len != 3 || candidate.source_bytes() != &decoded[pos..pos + 3] {
            continue;
        }

        let first = pos + candidate.byte_len;
        let limit = decoded.len().min(pos + MAX_CONSUMER_BYTES);
        let mut pending = VecDeque::from([(first, true, 0u8)]);
        let mut visited = HashSet::new();
        let mut reaches_renderer = false;
        while let Some((cursor, si_live, saved_si_depth)) = pending.pop_front() {
            if !(first..limit).contains(&cursor)
                || !visited.insert((cursor, si_live, saved_si_depth))
            {
                continue;
            }
            let Ok(instruction) = v30::decode_bytes(&decoded[cursor..limit]) else {
                continue;
            };
            let next = cursor + instruction.byte_len;
            let (next_si_live, next_saved_si_depth) = match &instruction.instruction {
                v30::Instruction::Push {
                    src: v30::Operand::Reg16(v30::Register16::SI),
                } if si_live => (true, saved_si_depth.saturating_add(1)),
                v30::Instruction::Pop {
                    dest: v30::Operand::Reg16(v30::Register16::SI),
                } => (saved_si_depth > 0, saved_si_depth.saturating_sub(1)),
                v30::Instruction::Mov {
                    dest: v30::Operand::Reg16(v30::Register16::SI),
                    ..
                }
                | v30::Instruction::Lea {
                    dest: v30::Register16::SI,
                    ..
                }
                | v30::Instruction::Les {
                    dest: v30::Register16::SI,
                    ..
                }
                | v30::Instruction::Lds {
                    dest: v30::Register16::SI,
                    ..
                } => (false, saved_si_depth),
                _ => (si_live, saved_si_depth),
            };

            let relative_target = |displacement: isize| {
                let target = next as isize + displacement;
                (target >= 0).then_some(target as usize)
            };
            match instruction.instruction {
                v30::Instruction::Call {
                    target: v30::CallTarget::Rel16(displacement),
                } => {
                    let target = relative_target(displacement as isize);
                    if si_live && target.is_some_and(|target| renderer_targets.contains(&target)) {
                        reaches_renderer = true;
                        break;
                    }
                    pending.push_back((next, next_si_live, next_saved_si_depth));
                }
                v30::Instruction::Jmp {
                    target: v30::JmpTarget::Rel8(displacement),
                } => {
                    let target = relative_target(displacement as isize);
                    if si_live && target.is_some_and(|target| renderer_targets.contains(&target)) {
                        reaches_renderer = true;
                        break;
                    }
                    if let Some(target) = target {
                        pending.push_back((target, next_si_live, next_saved_si_depth));
                    }
                }
                v30::Instruction::Jmp {
                    target: v30::JmpTarget::Rel16(displacement),
                } => {
                    let target = relative_target(displacement as isize);
                    if si_live && target.is_some_and(|target| renderer_targets.contains(&target)) {
                        reaches_renderer = true;
                        break;
                    }
                    if let Some(target) = target {
                        pending.push_back((target, next_si_live, next_saved_si_depth));
                    }
                }
                v30::Instruction::Jcc { target, .. } | v30::Instruction::Loop { target, .. } => {
                    if let Some(target) = relative_target(target as isize) {
                        pending.push_back((target, next_si_live, next_saved_si_depth));
                    }
                    pending.push_back((next, next_si_live, next_saved_si_depth));
                }
                v30::Instruction::Ret { .. }
                | v30::Instruction::Retf { .. }
                | v30::Instruction::Iret => {}
                _ => pending.push_back((next, next_si_live, next_saved_si_depth)),
            }
        }
        if reaches_renderer {
            refs.push((target, pos + 1));
        }
    }
    refs
}

/// Find small SI-forwarding renderer wrappers.
///
/// Scenario event handlers commonly call a local routine that sets `DS`, `DI`,
/// and the draw mode before forwarding the caller's unchanged `SI` to a proven
/// renderer. The fixed prefix is the data-flow proof: none of the setup writes
/// `SI`, and the relative call must resolve to a renderer already owned by the
/// strict catalog.
fn renderer_wrapper_targets(decoded: &[u8], renderer_targets: &HashSet<usize>) -> HashSet<usize> {
    const PREFIX: [u8; 8] = [0x8C, 0xC8, 0x8E, 0xD8, 0xBF, 0xFF, 0xFF, 0xB0];
    const CALL_OFFSET: usize = 9;
    const MIN_BYTES: usize = 12;

    let mut wrappers = HashSet::new();
    for start in 0..decoded.len().saturating_sub(MIN_BYTES) {
        if decoded.get(start..start + PREFIX.len()) != Some(PREFIX.as_slice())
            || decoded[start + CALL_OFFSET] != 0xE8
        {
            continue;
        }
        let displacement = i16::from_le_bytes([
            decoded[start + CALL_OFFSET + 1],
            decoded[start + CALL_OFFSET + 2],
        ]) as isize;
        let next = start + CALL_OFFSET + 3;
        let target = (next as isize + displacement) as usize;
        if renderer_targets.contains(&target) {
            wrappers.insert(start);
        }
    }
    wrappers
}

/// Find the shared event-text scheduler used by all three overlays.
///
/// The routine records the caller's `SI` and `DI` in live event state; its
/// callback later consumes those exact values. A call to this routine therefore
/// owns the immediate that loaded `SI`, even though the final renderer call is
/// asynchronous rather than adjacent to the source instruction.
fn scheduled_text_consumer_targets(decoded: &[u8]) -> HashSet<usize> {
    const ENTRY_PREFIX: [u8; 9] = [0x53, 0xB4, 0x01, 0xCD, 0x7B, 0xB4, 0x07, 0xCD, 0x7A];
    const STORE_SOURCE_AND_DESTINATION: [u8; 10] =
        [0x26, 0x89, 0xB7, 0x04, 0x03, 0x26, 0x89, 0xBF, 0x06, 0x03];
    const MAX_PROLOGUE_BYTES: usize = 64;

    let mut consumers = HashSet::new();
    for start in 0..decoded.len().saturating_sub(ENTRY_PREFIX.len()) {
        if decoded.get(start..start + ENTRY_PREFIX.len()) != Some(ENTRY_PREFIX.as_slice()) {
            continue;
        }
        let end = decoded.len().min(start + MAX_PROLOGUE_BYTES);
        if decoded[start + ENTRY_PREFIX.len()..end]
            .windows(STORE_SOURCE_AND_DESTINATION.len())
            .any(|window| window == STORE_SOURCE_AND_DESTINATION)
        {
            consumers.insert(start);
        }
    }
    consumers
}

/// Recover strings selected by the sound-device bit table.  Its records pair
/// one device-control routine with one label pointer, while the consumer walks
/// the label field at a four-byte stride.  A label made only of kanji is absent
/// from the kana-anchored start set, so validating the consumer, every control
/// routine, and all neighbouring label slots is what makes that pointer safe to
/// rewrite.
fn bit_selected_string_refs(
    decoded: &[u8],
    load_offset: usize,
    starts: &HashSet<usize>,
) -> Vec<(usize, usize)> {
    let mut refs = Vec::new();
    let fixed_consumer_bytes = [
        (0, 0xB0),
        (1, 0x02),
        (2, 0xE8),
        (5, 0xE8),
        (8, 0xB4),
        (9, 0x05),
        (10, 0xCD),
        (11, 0x7D),
        (12, 0x33),
        (13, 0xC9),
        (14, 0x8C),
        (15, 0xC8),
        (16, 0x8E),
        (17, 0xD8),
        (18, 0xBE),
        (21, 0x80),
        (22, 0xFA),
        (23, 0x00),
        (24, 0x74),
        (26, 0x83),
        (27, 0xC6),
        (28, 0x04),
        (29, 0xD0),
        (30, 0xEA),
        (31, 0x73),
        (33, 0x41),
        (34, 0x51),
        (35, 0x52),
        (36, 0x56),
        (37, 0x8B),
        (38, 0x34),
    ];

    for consumer_start in 0..decoded.len().saturating_sub(38) {
        if !fixed_consumer_bytes
            .iter()
            .all(|&(relative, byte)| decoded[consumer_start + relative] == byte)
        {
            continue;
        }

        let table_base_logical =
            u16::from_le_bytes([decoded[consumer_start + 19], decoded[consumer_start + 20]])
                as usize;
        let Some(table_base) = table_base_logical.checked_sub(load_offset) else {
            continue;
        };
        let Some(first_label_site) = table_base.checked_add(4) else {
            continue;
        };
        let Some(table_bytes) = consumer_start.checked_sub(first_label_site) else {
            continue;
        };
        if table_bytes % 4 != 0 {
            continue;
        }
        let entry_count = table_bytes / 4;
        if !(4..=16).contains(&entry_count) {
            continue;
        }

        let mut entries = Vec::with_capacity(entry_count);
        let mut anchored = 0usize;
        let mut valid = true;
        for index in 0..entry_count {
            let label_site = first_label_site + index * 4;
            let control_site = label_site - 2;
            let control_logical =
                u16::from_le_bytes([decoded[control_site], decoded[control_site + 1]]) as usize;
            let Some(control_offset) = control_logical.checked_sub(load_offset) else {
                valid = false;
                break;
            };
            if decoded.get(control_offset..control_offset + 4) != Some(&[0xB4, 0x05, 0xCD, 0x7D]) {
                valid = false;
                break;
            }

            let label_logical =
                u16::from_le_bytes([decoded[label_site], decoded[label_site + 1]]) as usize;
            let Some(label_offset) = label_logical.checked_sub(load_offset) else {
                valid = false;
                break;
            };
            let Some(raw) = crate::renderer_control::message_bytes(decoded, label_offset) else {
                valid = false;
                break;
            };
            let (_, _, had_errors) = SHIFT_JIS.decode(raw);
            if raw.is_empty()
                || raw.len() > 80
                || raw.last() != Some(&b'\n')
                || had_errors
                || (label_offset > 0 && decoded[label_offset - 1] != 0)
            {
                valid = false;
                break;
            }
            anchored += usize::from(starts.contains(&label_offset));
            entries.push((label_offset, label_site));
        }
        if valid && anchored > 0 {
            refs.extend(entries);
        }
    }
    refs
}

/// Recover the first field of the scenario item-record table. Each ten-byte
/// record is `(label, primary_handler, optional_handler, flags, zero)`. The
/// existing generic table scan can split this table when a kanji-only label or
/// the shared empty-label sentinel is not in its kana-anchored start set,
/// leaving later labels with only a second call-site reference. Validate the
/// complete record shape and a run of records before adopting every non-empty
/// label pointer.
fn item_record_string_refs(
    decoded: &[u8],
    load_offset: usize,
    starts: &HashSet<usize>,
) -> Vec<(usize, usize)> {
    const RECORD_BYTES: usize = 10;
    const MIN_RECORDS: usize = 4;
    const MIN_ANCHORED_LABELS: usize = 3;

    let logical_target = |site: usize| -> Option<usize> {
        let value = u16::from_le_bytes([*decoded.get(site)?, *decoded.get(site + 1)?]) as usize;
        value
            .checked_sub(load_offset)
            .filter(|target| *target < decoded.len())
    };
    let valid_label = |target: usize| -> bool {
        if decoded.get(target) == Some(&0) {
            return true;
        }
        if target > 0 && decoded[target - 1] != 0 {
            return false;
        }
        let Some(raw) = crate::renderer_control::message_bytes(decoded, target) else {
            return false;
        };
        if raw.is_empty() || raw.len() > 80 {
            return false;
        }
        let (text, _, had_errors) = SHIFT_JIS.decode(raw);
        !had_errors
            && raw.iter().any(|byte| *byte >= 0x80)
            && text.chars().any(|ch| !ch.is_whitespace())
    };
    let valid_record = |site: usize| -> Option<Option<usize>> {
        let record = decoded.get(site..site + RECORD_BYTES)?;
        let label = logical_target(site)?;
        if !valid_label(label) {
            return None;
        }
        logical_target(site + 2)?;
        let optional_handler = u16::from_le_bytes([record[4], record[5]]) as usize;
        if optional_handler != 0
            && optional_handler
                .checked_sub(load_offset)
                .is_none_or(|target| target >= decoded.len())
        {
            return None;
        }
        if record[7] != 0xFF || record[8] != 0 || record[9] != 0 {
            return None;
        }
        Some((decoded[label] != 0).then_some(label))
    };

    let mut refs = Vec::new();
    let mut site = 0usize;
    while site + RECORD_BYTES <= decoded.len() {
        if valid_record(site).is_none()
            || (site >= RECORD_BYTES && valid_record(site - RECORD_BYTES).is_some())
        {
            site += 1;
            continue;
        }

        let start = site;
        let mut labels = Vec::new();
        while let Some(label) = valid_record(site) {
            if let Some(label) = label {
                labels.push((label, site));
            }
            site += RECORD_BYTES;
        }
        let records = (site - start) / RECORD_BYTES;
        let anchored = labels
            .iter()
            .filter(|(label, _)| starts.contains(label))
            .count();
        if records >= MIN_RECORDS && anchored >= MIN_ANCHORED_LABELS {
            refs.extend(labels);
        }
    }
    refs
}

/// Recover the inventory label field from its actual indexed renderer consumer.
/// The low six item bits select one of 64 ten-byte records. Unlike the other
/// item table, these records contain effect values and prices, not a second
/// handler and zero padding. Only the first word is a string pointer.
fn inventory_label_refs(
    decoded: &[u8],
    load_offset: usize,
    renderer_targets: &HashSet<usize>,
    text_spans: &[(usize, usize)],
) -> Vec<(usize, usize)> {
    use v30::{Instruction as I, Operand as O, Register16 as R, ShiftCount};

    let prefix = [
        I::And {
            dest: O::Reg16(R::AX),
            src: O::Imm16(0x3f),
        },
        I::Mov {
            dest: O::Reg16(R::SI),
            src: O::Reg16(R::AX),
        },
        I::Shl {
            dest: O::Reg16(R::SI),
            count: ShiftCount::One,
        },
        I::Shl {
            dest: O::Reg16(R::SI),
            count: ShiftCount::One,
        },
        I::Add {
            dest: O::Reg16(R::SI),
            src: O::Reg16(R::AX),
        },
        I::Shl {
            dest: O::Reg16(R::SI),
            count: ShiftCount::One,
        },
    ];
    let mut refs = Vec::new();
    for pos in 0..=decoded.len().saturating_sub(30) {
        if decoded.get(pos..pos + 3) != Some(&[0x25, 0x3f, 0x00])
            || text_spans
                .iter()
                .any(|&(start, end)| pos < end && pos + 30 > start)
        {
            continue;
        }
        let mut cursor = pos;
        let mut instructions = Vec::new();
        for _ in 0..11 {
            let Ok(instruction) = v30::decode_bytes(&decoded[cursor..]) else {
                break;
            };
            cursor += instruction.byte_len;
            instructions.push(instruction.instruction);
        }
        if instructions.len() != 11 || instructions[..6] != prefix {
            continue;
        }
        // The list consumer advances its separate item cursor before loading
        // the label; this direct-memory increment leaves the scaled SI intact.
        let I::Add {
            dest: O::Mem(counter),
            src: O::Imm16(2),
        } = instructions[6]
        else {
            continue;
        };
        if counter.base() != v30::EffectiveAddressBase::Direct
            || counter.segment().is_some()
            || counter.size() != v30::OperandSize::Word
        {
            continue;
        }
        let I::Mov {
            dest: O::Reg16(R::SI),
            src: O::Mem(memory),
        } = instructions[7]
        else {
            continue;
        };
        if memory.base() != v30::EffectiveAddressBase::Si
            || memory.segment().is_some()
            || memory.size() != v30::OperandSize::Word
        {
            continue;
        }
        let v30::EffectiveAddressDisplacement::Signed(base) = memory.displacement() else {
            continue;
        };
        if instructions[8]
            != (I::Mov {
                dest: O::Reg16(R::DI),
                src: O::Imm16(0xffff),
            })
            || instructions[9]
                != (I::Mov {
                    dest: O::Reg8(v30::Register8::AL),
                    src: O::Imm8(0),
                })
        {
            continue;
        }
        let I::Call {
            target: v30::CallTarget::Rel16(delta),
        } = instructions[10]
        else {
            continue;
        };
        let renderer = cursor as isize + delta as isize;
        if renderer < 0 || !renderer_targets.contains(&(renderer as usize)) {
            continue;
        }
        let Some(table) = (base as u16 as usize).checked_sub(load_offset) else {
            continue;
        };
        let Some(end) = table
            .checked_add(64 * 10)
            .filter(|&end| end <= decoded.len())
        else {
            continue;
        };
        if text_spans
            .iter()
            .any(|&(start, stop)| table < stop && end > start)
        {
            continue;
        }
        let mut labels = Vec::new();
        let mut valid = true;
        for site in (table..end).step_by(10) {
            let logical = u16::from_le_bytes([decoded[site], decoded[site + 1]]) as usize;
            let Some(target) = logical
                .checked_sub(load_offset)
                .filter(|&target| target < decoded.len())
            else {
                valid = false;
                break;
            };
            if decoded[target] == 0 {
                continue;
            }
            if target == 0 || decoded[target - 1] != 0 || (table..end).contains(&target) {
                valid = false;
                break;
            }
            let Some(raw) = crate::renderer_control::message_bytes(decoded, target) else {
                valid = false;
                break;
            };
            let (text, _, errors) = SHIFT_JIS.decode(raw);
            if errors || raw.is_empty() || raw.len() > 80 {
                valid = false;
                break;
            }
            if text.chars().any(|ch| !ch.is_whitespace()) {
                labels.push((target, site));
            }
        }
        if valid {
            refs.extend(labels);
        }
    }
    refs.sort_unstable();
    refs.dedup();
    refs
}

/// Equipment inventory records point at an eight-byte behavior record. Its
/// final word is consumed as SI by the equip/remove feedback renderer.
fn equipment_label_refs(
    decoded: &[u8],
    load_offset: usize,
    inventory_labels: &[(usize, usize)],
    renderer_targets: &HashSet<usize>,
) -> Vec<(usize, usize)> {
    let mut refs = Vec::new();
    for &(_, record) in inventory_labels {
        let target = |site: usize| {
            let bytes = decoded.get(site..site + 2)?;
            usize::from(u16::from_le_bytes([bytes[0], bytes[1]]))
                .checked_sub(load_offset)
                .filter(|&offset| offset < decoded.len())
        };
        let Some(handler) = target(record + 2) else {
            continue;
        };
        // mov ax,cs; mov ds,ax; mov si,[bx+4]: load equipment behavior record.
        if decoded.get(handler..handler + 7) != Some(&[0x8c, 0xc8, 0x8e, 0xd8, 0x8b, 0x77, 0x04]) {
            continue;
        }
        // The same handler renders [si+6] after applying the equipment action.
        let consumer = (handler..decoded.len().min(handler + 180)).find(|&site| {
            let Some(bytes) = decoded.get(site..site + 11) else {
                return false;
            };
            if bytes[..9] != [0x8b, 0x74, 0x06, 0xbf, 0xff, 0xff, 0xb0, 0x01, 0xe8] {
                return false;
            }
            let displacement = i16::from_le_bytes([bytes[9], bytes[10]]);
            let destination = (site + 11).wrapping_add_signed(displacement as isize);
            renderer_targets.contains(&destination)
        });
        if consumer.is_none() {
            continue;
        }
        let Some(equipment) = target(record + 4) else {
            continue;
        };
        if (0..3).any(|i| target(equipment + i * 2).is_none()) {
            continue;
        }
        let Some(label) = target(equipment + 6) else {
            continue;
        };
        if inventory_labels.iter().any(|&(known, _)| known == label) {
            refs.push((label, equipment + 6));
        }
    }
    refs.sort_unstable();
    refs.dedup();
    refs
}

/// Item-use handlers compare CS:[BX], the selected record's name word, and
/// return carry clear on mismatch. Only the observed complete guard and a
/// renderer-proven inventory label grant ownership of its immediate operand.
fn inventory_identity_refs(
    decoded: &[u8],
    load_offset: usize,
    labels: &[(usize, usize)],
    text_spans: &[(usize, usize)],
) -> Vec<(usize, usize)> {
    let targets = labels
        .iter()
        .map(|&(target, _)| target)
        .collect::<HashSet<_>>();
    let mut refs = Vec::new();
    for (pos, guard) in decoded.windows(9).enumerate() {
        if guard[..3] != [0x2e, 0x81, 0x3f]
            || guard[5..] != [0x74, 0x02, 0xf8, 0xc3]
            || text_spans
                .iter()
                .any(|&(start, end)| pos < end && pos + 9 > start)
        {
            continue;
        }
        let Ok(instruction) = v30::decode_bytes(guard) else {
            continue;
        };
        let v30::Instruction::Cmp {
            a: v30::Operand::Mem(memory),
            b: v30::Operand::Imm16(logical),
        } = instruction.instruction
        else {
            continue;
        };
        if instruction.byte_len != 5
            || memory.base() != v30::EffectiveAddressBase::Bx
            || memory.segment() != Some(v30::SegmentRegister::CS)
            || memory.size() != v30::OperandSize::Word
            || memory.displacement() != v30::EffectiveAddressDisplacement::Signed(0)
        {
            continue;
        }
        if let Some(target) = (logical as usize).checked_sub(load_offset)
            && targets.contains(&target)
        {
            refs.push((target, pos + 3));
        }
    }
    refs
}

/// Recover immediates owned by battle status-effect message fields.
///
/// Each scenario overlay records a status effect's activation and expiry
/// messages in fields of the combatant record, later rendered by
/// `mov si,es:[bx+disp16]; mov di,0xFFFF; mov al,imm8; call renderer`. The
/// casting routine writes those fields with `mov word es:[bx+disp16],imm16`
/// and may test an active effect with `cmp word es:[bx+disp16],imm16`. An
/// expiry message can be the head of a shared source tail (`NAMEの　効果が切れた`)
/// whose suffix has separate `mov si` consumers, so the field immediates must
/// move together with the complete head string.
///
/// Only displacements proven by the exact renderer read are accepted, and a
/// writer or comparison must decode as one seven-byte instruction whose
/// immediate names a terminator-delimited text start outside confirmed text.
fn status_message_field_refs(
    decoded: &[u8],
    load_offset: usize,
    renderer_targets: &HashSet<usize>,
    text_spans: &[(usize, usize)],
) -> Vec<(usize, usize)> {
    const READER_PREFIX: [u8; 3] = [0x26, 0x8B, 0xB7];
    const READER_RENDER: [u8; 4] = [0xBF, 0xFF, 0xFF, 0xB0];
    const WRITE_PREFIX: [u8; 3] = [0x26, 0xC7, 0x87];
    const COMPARE_PREFIX: [u8; 3] = [0x26, 0x81, 0xBF];

    let overlaps_text = |start: usize, len: usize| {
        text_spans
            .iter()
            .any(|&(span_start, span_end)| start < span_end && span_start < start + len)
    };

    let mut fields = HashSet::new();
    for pos in 0..decoded.len().saturating_sub(13) {
        let bytes = &decoded[pos..pos + 13];
        if bytes[..3] != READER_PREFIX || bytes[5..9] != READER_RENDER || bytes[10] != 0xE8 {
            continue;
        }
        if overlaps_text(pos, 13) {
            continue;
        }
        let displacement = i16::from_le_bytes([bytes[11], bytes[12]]) as isize;
        let destination = (pos + 13).wrapping_add_signed(displacement);
        if renderer_targets.contains(&destination) {
            fields.insert([bytes[3], bytes[4]]);
        }
    }

    let mut refs = Vec::new();
    for pos in 0..decoded.len().saturating_sub(7) {
        let bytes = &decoded[pos..pos + 7];
        if (bytes[..3] != WRITE_PREFIX && bytes[..3] != COMPARE_PREFIX)
            || !fields.contains(&[bytes[3], bytes[4]])
            || overlaps_text(pos, 7)
        {
            continue;
        }
        let Ok(instruction) = v30::decode_bytes(bytes) else {
            continue;
        };
        if instruction.byte_len != 7 || instruction.source_bytes() != bytes {
            continue;
        }
        let logical = u16::from_le_bytes([bytes[5], bytes[6]]) as usize;
        let Some(target) = logical.checked_sub(load_offset) else {
            continue;
        };
        if target == 0 || target >= decoded.len() {
            continue;
        }
        if !crate::renderer_control::is_terminator(decoded[target - 1]) {
            continue;
        }
        let Some(raw) = crate::renderer_control::message_bytes(decoded, target) else {
            continue;
        };
        let (_, _, had_errors) = SHIFT_JIS.decode(raw);
        if raw.is_empty() || had_errors {
            continue;
        }
        refs.push((target, pos + 5));
    }
    refs.sort_unstable();
    refs.dedup();
    refs
}

/// Byte ranges known to be renderer text, including their terminator.  The
/// strict call catalog protects ASCII-only strings; NUL-delimited kana strings
/// protect the broader text population used to anchor heuristic pointer scans.
fn confirmed_text_spans(
    decoded: &[u8],
    catalog: &[crate::overlay_catalog::CatalogedMessage],
) -> Vec<(usize, usize)> {
    let mut spans = catalog
        .iter()
        .map(|message| {
            (
                message.string_decoded_offset,
                message.string_decoded_offset + message.byte_budget,
            )
        })
        .collect::<Vec<_>>();
    for start in crate::overlay_catalog::nul_delimited_kana_starts(decoded, 1) {
        if let Some(raw) = crate::renderer_control::message_bytes(decoded, start) {
            spans.push((start, start + raw.len() + 1));
        }
    }
    spans.sort_unstable();
    spans.dedup();
    spans
}

fn read_raw_and_text(decoded: &[u8], offset: usize) -> (Vec<u8>, String) {
    let raw = crate::renderer_control::message_bytes(decoded, offset)
        .unwrap_or_else(|| decoded.get(offset..).unwrap_or_default())
        .to_vec();
    let text = SHIFT_JIS.decode(&raw).0.into_owned();
    (raw, text)
}

/// Merge the `mov si` catalog and the detected pointer tables into one list,
/// one entry per string offset, sorted by offset (address order), each with its
/// full set of rewrite sites.
pub fn unified_catalog(
    decoded: &[u8],
    load_offset: usize,
    min_entries: usize,
    strides: &[usize],
) -> Result<Vec<UnifiedMessage>> {
    let mut by_string: BTreeMap<usize, UnifiedMessage> = BTreeMap::new();

    // Confirmed string starts anchor both scans: the strict catalog gives the
    // ASCII/kanji-only messages the kana scan would skip; the runs and NUL-
    // delimited starts give the rest.
    let strict_refs = collect_all_message_refs(decoded, load_offset);
    let renderer_targets = strict_refs
        .iter()
        .map(|entry| entry.renderer_decoded_offset)
        .collect::<HashSet<_>>();
    let mut renderer_consumers = renderer_targets.clone();
    renderer_consumers.extend(renderer_wrapper_targets(decoded, &renderer_targets));
    renderer_consumers.extend(scheduled_text_consumer_targets(decoded));
    let catalog = catalog_messages(decoded, load_offset);
    let runs = scan_sjis_runs(decoded, 2);
    let starts = string_start_set(decoded, &catalog, &runs);
    let text_spans = confirmed_text_spans(decoded, &catalog);

    let insert = |by_string: &mut BTreeMap<usize, UnifiedMessage>,
                  target: usize,
                  site: usize,
                  kind: PointerKind| {
        let logical = target + load_offset;
        let message = by_string.entry(target).or_insert_with(|| {
            let (raw, text) = read_raw_and_text(decoded, target);
            UnifiedMessage {
                string_decoded_offset: target,
                string_logical_offset: logical,
                byte_budget: raw.len() + 1,
                raw,
                text,
                rewrite_sites: Vec::new(),
            }
        });
        message.rewrite_sites.push(RewriteSite { site, kind });
    };

    // Full renderer-call signatures are the high-confidence baseline.  Insert
    // them explicitly so the broad scan below can reject all text-overlapping
    // `0xBE` candidates without risking a false negative on an already-proven
    // call site.
    for message in &catalog {
        for &call in &message.call_sites {
            insert(
                &mut by_string,
                message.string_decoded_offset,
                call + 1,
                PointerKind::MovSi,
            );
        }
    }

    // Other renderer-consumer variants, gated by a confirmed string start,
    // code/text non-overlap, typed instruction decoding, and a transfer to a
    // renderer target proven by the strict catalog.
    for (target, site) in variant_renderer_string_refs(
        decoded,
        load_offset,
        &starts,
        &text_spans,
        &renderer_consumers,
    ) {
        insert(&mut by_string, target, site, PointerKind::MovSi);
    }

    let tables = find_pointer_tables(decoded, load_offset, &starts, min_entries, strides);
    for table in &tables {
        for entry in &table.entries {
            insert(
                &mut by_string,
                entry.target_decoded,
                entry.site,
                PointerKind::Table,
            );
        }
    }

    for (target, site) in bit_selected_string_refs(decoded, load_offset, &starts) {
        insert(&mut by_string, target, site, PointerKind::Table);
    }

    for (target, site) in item_record_string_refs(decoded, load_offset, &starts) {
        insert(&mut by_string, target, site, PointerKind::Table);
    }

    let inventory_labels =
        inventory_label_refs(decoded, load_offset, &renderer_targets, &text_spans);
    for &(target, site) in &inventory_labels {
        insert(&mut by_string, target, site, PointerKind::Table);
    }

    for (target, site) in
        equipment_label_refs(decoded, load_offset, &inventory_labels, &renderer_targets)
    {
        insert(&mut by_string, target, site, PointerKind::Table);
    }

    for (target, site) in
        inventory_identity_refs(decoded, load_offset, &inventory_labels, &text_spans)
    {
        insert(&mut by_string, target, site, PointerKind::ItemIdentity);
    }

    for (target, site) in
        status_message_field_refs(decoded, load_offset, &renderer_targets, &text_spans)
    {
        insert(
            &mut by_string,
            target,
            site,
            PointerKind::StatusMessageField,
        );
    }

    let mut out: Vec<UnifiedMessage> = by_string.into_values().collect();
    for message in &mut out {
        message.rewrite_sites.sort_by_key(|site| site.site);
        message.rewrite_sites.dedup_by_key(|site| site.site);
    }
    validate_unified_catalog(decoded, &out)?;
    Ok(out)
}

/// Validate the safety properties required before any pointer rewrite is
/// allowed.  In particular, distinct translation units may not overlap and no
/// rewrite site may live inside a text slot.  A true mid-string pointer needs an
/// explicit alias/structure model; treating it as an independent relocatable
/// string would make either in-place patch order or relocation ambiguous.
/// Group cataloged messages that share one source terminator. Each group is
/// returned as `(head_index, member_indices)` in source order; a message that
/// shares nothing forms a one-member group.
pub fn shared_tail_groups(messages: &[UnifiedMessage]) -> Vec<(usize, Vec<usize>)> {
    let mut order = (0..messages.len()).collect::<Vec<_>>();
    order.sort_by_key(|&index| messages[index].string_decoded_offset);
    let mut groups: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut group_end = 0usize;
    for index in order {
        let message = &messages[index];
        let end = message.string_decoded_offset + message.byte_budget;
        if let Some((_, members)) = groups.last_mut()
            && message.string_decoded_offset < group_end
            && end == group_end
        {
            members.push(index);
            continue;
        }
        groups.push((index, vec![index]));
        group_end = end;
    }
    groups
}

/// Reject a moved suffix whose source head is still reachable but uncataloged.
///
/// A cataloged message preceded by Shift-JIS text rather than a terminator is
/// the suffix of a longer source string. Relocating that suffix frees bytes
/// the longer string still renders through. The head is safe only when it is
/// cataloged itself, or when no 16-bit occurrence of its logical address
/// exists anywhere in the source image.
fn validate_shared_tail_heads(
    decoded: &[u8],
    messages: &[UnifiedMessage],
    spans: &[(usize, usize)],
) -> Result<()> {
    let starts = messages
        .iter()
        .map(|message| message.string_decoded_offset)
        .collect::<HashSet<_>>();
    for message in messages {
        let start = message.string_decoded_offset;
        let load_offset = message.string_logical_offset - start;
        let mut head = start;
        while head >= 2
            && is_double_byte_text(decoded[head - 2], decoded[head - 1])
            && (head == start || !starts.contains(&head))
        {
            head -= 2;
        }
        if head == start || head == 0 || !crate::renderer_control::is_terminator(decoded[head - 1])
        {
            continue;
        }
        if starts.contains(&head) {
            continue;
        }
        let logical = u16::try_from(head + load_offset)
            .with_context(|| format!("shared-tail head 0x{head:04X} exceeds 16 bits"))?
            .to_le_bytes();
        if let Some(site) = decoded
            .windows(2)
            .enumerate()
            .filter(|(_, bytes)| *bytes == logical)
            .map(|(site, _)| site)
            .find(|&site| {
                !spans
                    .iter()
                    .any(|&(span_start, span_end)| site < span_end && span_start < site + 2)
            })
        {
            bail!(
                "message 0x{start:04X} is the suffix of uncataloged source text at 0x{head:04X}, whose address occurs at 0x{site:04X}",
            );
        }
    }
    Ok(())
}

fn is_double_byte_text(lead: u8, trail: u8) -> bool {
    matches!(lead, 0x81..=0x9F | 0xE0..=0xEF) && matches!(trail, 0x40..=0x7E | 0x80..=0xFC)
}

pub fn validate_unified_catalog(decoded: &[u8], messages: &[UnifiedMessage]) -> Result<()> {
    let mut spans = Vec::with_capacity(messages.len());
    for message in messages {
        if message.rewrite_sites.is_empty() {
            bail!(
                "message 0x{:04X} has no verified rewrite site",
                message.string_decoded_offset
            );
        }
        let raw_end = message
            .string_decoded_offset
            .checked_add(message.raw.len())
            .context("message range overflow")?;
        let slot_end = message
            .string_decoded_offset
            .checked_add(message.byte_budget)
            .context("message slot overflow")?;
        if message.byte_budget != message.raw.len() + 1 {
            bail!(
                "message 0x{:04X} budget {} does not equal raw {} + terminator",
                message.string_decoded_offset,
                message.byte_budget,
                message.raw.len()
            );
        }
        if decoded.get(message.string_decoded_offset..raw_end) != Some(message.raw.as_slice()) {
            bail!(
                "message 0x{:04X} raw bytes do not match the decoded overlay",
                message.string_decoded_offset
            );
        }
        let Some(&terminator) = decoded.get(raw_end) else {
            bail!(
                "message 0x{:04X} terminator lies outside the decoded overlay",
                message.string_decoded_offset
            );
        };
        if !crate::renderer_control::is_terminator(terminator) {
            bail!(
                "message 0x{:04X} ends with non-terminator 0x{terminator:02X}",
                message.string_decoded_offset
            );
        }
        u16::try_from(message.string_logical_offset).with_context(|| {
            format!(
                "message 0x{:04X} logical offset exceeds 16 bits",
                message.string_decoded_offset
            )
        })?;
        spans.push((message.string_decoded_offset, slot_end));
    }

    spans.sort_unstable();
    for pair in spans.windows(2) {
        // A consumer may address the suffix of another string. Such a suffix
        // shares the complete source terminator; any other overlap would let
        // two independent slots claim different byte ranges.
        if pair[1].0 < pair[0].1 && pair[1].1 != pair[0].1 {
            bail!(
                "relocatable message slots overlap at 0x{:04X} and 0x{:04X}",
                pair[0].0,
                pair[1].0
            );
        }
    }
    validate_shared_tail_heads(decoded, messages, &spans)?;

    let mut site_owners = BTreeMap::<usize, usize>::new();
    for message in messages {
        let expected = (message.string_logical_offset as u16).to_le_bytes();
        for site in &message.rewrite_sites {
            let site_end = site
                .site
                .checked_add(2)
                .context("rewrite-site range overflow")?;
            if decoded.get(site.site..site_end) != Some(expected.as_slice()) {
                bail!(
                    "rewrite site 0x{:04X} for message 0x{:04X} does not hold 0x{:04X}",
                    site.site,
                    message.string_decoded_offset,
                    message.string_logical_offset
                );
            }
            if let Some(&(start, _)) = spans
                .iter()
                .find(|&&(start, end)| site.site < end && start < site_end)
            {
                bail!(
                    "rewrite site 0x{:04X} for message 0x{:04X} overlaps text slot 0x{start:04X}",
                    site.site,
                    message.string_decoded_offset
                );
            }
            if let Some(owner) = site_owners.insert(site.site, message.string_decoded_offset)
                && owner != message.string_decoded_offset
            {
                bail!(
                    "rewrite site 0x{:04X} is owned by both 0x{owner:04X} and 0x{:04X}",
                    site.site,
                    message.string_decoded_offset
                );
            }
        }
    }
    Ok(())
}

/// Every rewrite site must actually hold this message's logical offset as a
/// little-endian 16-bit value; otherwise relocation would corrupt an unrelated
/// byte. Returns the first inconsistent site, if any -- the reinserter refuses
/// to run when this is non-empty.
pub fn first_inconsistent_site(decoded: &[u8], message: &UnifiedMessage) -> Option<RewriteSite> {
    let expected = (message.string_logical_offset as u16).to_le_bytes();
    message
        .rewrite_sites
        .iter()
        .find(|site| decoded.get(site.site..site.site + 2) != Some(&expected[..]))
        .copied()
}

#[cfg(test)]
mod tests {

    #[test]
    fn item_identity_requires_inventory_label_and_complete_use_guard() {
        let guard = [0x2e, 0x81, 0x3f, 0x96, 0x69, 0x74, 0x02, 0xf8, 0xc3];
        let labels = [(0x6896, 0x5e42)];
        assert_eq!(
            inventory_identity_refs(&guard, 0x100, &labels, &[]),
            vec![(0x6896, 3)]
        );
        assert!(inventory_identity_refs(&guard, 0x100, &[], &[]).is_empty());
        assert!(inventory_identity_refs(&guard, 0x100, &labels, &[(0, 9)]).is_empty());
        for (site, value) in [(0, 0x26), (2, 0x3e), (5, 0x75), (6, 0x03), (7, 0xf9)] {
            let mut other = guard;
            other[site] = value;
            assert!(inventory_identity_refs(&other, 0x100, &labels, &[]).is_empty());
        }
    }

    // Inventory names made only of kanji must be relocatable through the
    // indexed renderer, while a different stride, call target, or interior
    // label pointer must never authorize rewriting a table.
    #[test]
    fn inventory_consumer_proves_kanji_labels_and_rejects_invalid_tables() {
        let mut source = vec![0u8; 0x900];
        let consumer = 0x20;
        let table = 0x200;
        let renderer = 0x100;
        let target = 0x600;
        let fixture = [
            0x25, 0x3f, 0x00, 0x8b, 0xf0, 0xd1, 0xe6, 0xd1, 0xe6, 0x03, 0xf0, 0xd1, 0xe6, 0x83,
            0x06, 0x00, 0x08, 0x02, 0x8b, 0xb4, 0x00, 0x02, 0xbf, 0xff, 0xff, 0xb0, 0x00, 0xe8,
            0xc2, 0x00,
        ];
        source[consumer..consumer + fixture.len()].copy_from_slice(&fixture);
        let name = crate::sjis_marker::encode_sjis("犬顎菊").unwrap();
        source[target..target + name.len()].copy_from_slice(&name);
        for record in 0..64 {
            let site = table + record * 10;
            source[site..site + 2].copy_from_slice(&(target as u16).to_le_bytes());
            // Other fields can be price/effect values, not pointer words.
            source[site + 2..site + 10].fill(0xff);
        }
        let targets = HashSet::from([renderer]);
        let refs = inventory_label_refs(&source, 0, &targets, &[]);
        assert_eq!(
            refs,
            (0..64)
                .map(|i| (target, table + i * 10))
                .collect::<Vec<_>>()
        );
        assert!(inventory_label_refs(&source, 0, &HashSet::from([renderer + 1]), &[]).is_empty());
        assert!(
            inventory_label_refs(
                &source,
                0,
                &targets,
                &[(consumer, consumer + fixture.len())]
            )
            .is_empty()
        );
        for (offset, replacement) in [(1, 0x1f), (12, 0xee)] {
            let mut broken = source.clone();
            broken[consumer + offset] = replacement;
            assert!(inventory_label_refs(&broken, 0, &targets, &[]).is_empty());
        }
        let mut broken = source.clone();
        broken[table + 63 * 10..table + 63 * 10 + 2].copy_from_slice(&0xffffu16.to_le_bytes());
        assert!(inventory_label_refs(&broken, 0, &targets, &[]).is_empty());
        source[table..table + 2].copy_from_slice(&((target + 2) as u16).to_le_bytes());
        assert!(inventory_label_refs(&source, 0, &targets, &[]).is_empty());
    }
    use super::*;

    fn write_call(buf: &mut [u8], at: usize, si: u16, target: usize) {
        buf[at] = 0xBE;
        buf[at + 1..at + 3].copy_from_slice(&si.to_le_bytes());
        buf[at + 3] = 0xBF;
        buf[at + 4] = 0xFF;
        buf[at + 5] = 0xFF;
        buf[at + 6] = 0xB0;
        buf[at + 7] = 0x01;
        buf[at + 8] = 0xE8;
        let rel = target as isize - (at as isize + 11);
        buf[at + 9..at + 11].copy_from_slice(&(rel as i16).to_le_bytes());
    }

    #[test]
    fn item_record_table_recovers_labels_across_an_unanchored_kanji_entry() {
        let load = 0x100usize;
        let mut decoded = vec![0u8; 0x500];
        let labels = [0x200usize, 0x210, 0x220, 0x230];
        for &offset in &labels[..3] {
            decoded[offset..offset + 3].copy_from_slice(&[0x82, 0xA0, 0]);
        }
        decoded[labels[3]..labels[3] + 3].copy_from_slice(&[0x89, 0x5F, 0]);
        decoded[0x300] = 0xC3;

        for (index, &label) in labels.iter().enumerate() {
            let site = 0x100 + index * 10;
            decoded[site..site + 2].copy_from_slice(&((label + load) as u16).to_le_bytes());
            decoded[site + 2..site + 4].copy_from_slice(&0x400u16.to_le_bytes());
            decoded[site + 6..site + 8].copy_from_slice(&0xFF20u16.to_le_bytes());
        }
        let starts = HashSet::from([labels[0], labels[1], labels[2]]);

        assert_eq!(
            item_record_string_refs(&decoded, load, &starts),
            vec![
                (labels[0], 0x100),
                (labels[1], 0x10A),
                (labels[2], 0x114),
                (labels[3], 0x11E),
            ]
        );
    }

    #[test]
    fn merges_mov_si_and_table_pointers_with_verified_sites() {
        let load = 0x100usize;
        let mut decoded = vec![0u8; 0x400];
        decoded[0x300] = 0xC3; // renderer body

        // mov si message: string at 0x200 (logical 0x300), two call sites.
        decoded[0x200..0x203].copy_from_slice(b"hi\0");
        write_call(&mut decoded, 0x10, 0x300, 0x300);
        write_call(&mut decoded, 0x40, 0x300, 0x300);

        // Table-indexed block: four kana strings at 0x220,0x230,0x240,0x250 with
        // a 2-byte pointer table at 0x120. Two kana each so the run scan (which
        // wants >= 2 double-byte chars) confirms them as string starts.
        let kana = [0x82u8, 0xA0, 0x82, 0xA2, 0x00]; // あい\0
        for (i, off) in [0x220usize, 0x230, 0x240, 0x250].into_iter().enumerate() {
            decoded[off..off + 5].copy_from_slice(&kana);
            let logical = (off + load) as u16;
            decoded[0x120 + i * 2..0x120 + i * 2 + 2].copy_from_slice(&logical.to_le_bytes());
        }

        let messages = unified_catalog(&decoded, load, 4, &[2, 4, 8]).unwrap();

        // The mov si message carries both call-site immediates (call + 1).
        let hi = messages
            .iter()
            .find(|m| m.string_decoded_offset == 0x200)
            .unwrap();
        assert_eq!(hi.text, "hi");
        let sites: Vec<_> = hi.rewrite_sites.iter().map(|s| (s.site, s.kind)).collect();
        assert_eq!(
            sites,
            vec![(0x11, PointerKind::MovSi), (0x41, PointerKind::MovSi)]
        );

        // Each table string carries its table entry as the rewrite site.
        let first_table_msg = messages
            .iter()
            .find(|m| m.string_decoded_offset == 0x220)
            .unwrap();
        assert_eq!(first_table_msg.text, "あい");
        assert_eq!(first_table_msg.rewrite_sites.len(), 1);
        assert_eq!(first_table_msg.rewrite_sites[0].kind, PointerKind::Table);
        assert_eq!(first_table_msg.rewrite_sites[0].site, 0x120);

        // Every site verifies against the string's logical offset.
        for message in &messages {
            assert!(first_inconsistent_site(&decoded, message).is_none());
        }
    }

    #[test]
    fn broad_scan_rejects_sjis_trail_plus_boundary_as_mov_si() {
        let load = 0usize;
        let mut decoded = vec![0u8; 0x8300];

        // Real Japanese text contains SJIS `だ` (82 BE), then NUL, then another
        // SJIS lead.  Starting at the trail byte this spells `BE 00 82`, which a
        // byte-only scan would misread as `mov si,0x8200`.
        decoded[0x200..0x206].copy_from_slice(&[0x82, 0xA0, 0x82, 0xBE, 0x00, 0x82]);
        decoded[0x206..0x209].copy_from_slice(&[0xA2, 0x82, 0xA4]);
        decoded[0x209] = 0;
        decoded[0x8200..0x8205].copy_from_slice(&[0x82, 0xA0, 0x82, 0xA2, 0]);

        let messages = unified_catalog(&decoded, load, 4, &[2, 4, 8]).unwrap();
        assert!(
            messages
                .iter()
                .all(|message| message.string_decoded_offset != 0x8200)
        );
        assert!(
            messages
                .iter()
                .all(|message| { message.rewrite_sites.iter().all(|site| site.site != 0x204) })
        );
    }

    #[test]
    fn recovers_kanji_only_name_from_bit_selected_string_table() {
        let load = 0x100usize;
        let mut decoded = vec![0u8; 0x500];
        let label_offsets = [0x200usize, 0x220, 0x240, 0x260];
        decoded[label_offsets[0]..label_offsets[0] + 6]
            .copy_from_slice(&[0x82, 0xA0, 0x82, 0xA2, b'\n', 0x00]);
        for &offset in &[label_offsets[1], label_offsets[3]] {
            decoded[offset..offset + 4].copy_from_slice(b"ABC\n");
        }
        decoded[label_offsets[2]..label_offsets[2] + 6]
            .copy_from_slice(&[0x89, 0xB9, 0x95, 0x50, b'\n', 0x00]); // 音姫\n\0

        let consumer_start = 0x90usize;
        let table_base = 0x7Cusize;
        let control_offsets = [0x300usize, 0x310, 0x320, 0x330];
        for (index, (&label, &control)) in
            label_offsets.iter().zip(control_offsets.iter()).enumerate()
        {
            decoded[control..control + 4].copy_from_slice(&[0xB4, 0x05, 0xCD, 0x7D]);
            let label_site = table_base + 4 + index * 4;
            decoded[label_site - 2..label_site]
                .copy_from_slice(&((control + load) as u16).to_le_bytes());
            decoded[label_site..label_site + 2]
                .copy_from_slice(&((label + load) as u16).to_le_bytes());
        }

        let consumer = [
            0xB0, 0x02, 0xE8, 0x00, 0x00, 0xE8, 0x00, 0x00, 0xB4, 0x05, 0xCD, 0x7D, 0x33, 0xC9,
            0x8C, 0xC8, 0x8E, 0xD8, 0xBE, 0x7C, 0x01, 0x80, 0xFA, 0x00, 0x74, 0x1A, 0x83, 0xC6,
            0x04, 0xD0, 0xEA, 0x73, 0xF4, 0x41, 0x51, 0x52, 0x56, 0x8B, 0x34,
        ];
        decoded[consumer_start..consumer_start + consumer.len()].copy_from_slice(&consumer);

        let messages = unified_catalog(&decoded, load, 4, &[2, 4, 8]).unwrap();
        let kanji_only = messages
            .iter()
            .find(|message| message.string_decoded_offset == label_offsets[2])
            .unwrap();
        assert_eq!(kanji_only.text, "音姫\n");
        assert_eq!(
            kanji_only.rewrite_sites,
            vec![RewriteSite {
                site: table_base + 12,
                kind: PointerKind::Table,
            }]
        );
    }

    #[test]
    fn variant_scan_accepts_a_typed_renderer_consumer_outside_text() {
        let load = 0x100usize;
        let mut decoded = vec![0u8; 0x400];
        decoded[0x200..0x205].copy_from_slice(&[0x82, 0xA0, 0x82, 0xA2, 0]);
        decoded[0x220..0x225].copy_from_slice(&[0x82, 0xA0, 0x82, 0xA4, 0]);
        decoded[0x300] = 0xC3;
        write_call(&mut decoded, 0x10, 0x300, 0x300);

        // Variant: mov si,string; xor ax,ax; call the already-proven renderer.
        decoded[0x40] = 0xBE;
        decoded[0x41..0x43].copy_from_slice(&0x320u16.to_le_bytes());
        decoded[0x43..0x45].copy_from_slice(&[0x31, 0xC0]);
        decoded[0x45] = 0xE8;
        let displacement = 0x300isize - 0x48isize;
        decoded[0x46..0x48].copy_from_slice(&(displacement as i16).to_le_bytes());

        let messages = unified_catalog(&decoded, load, 4, &[2, 4, 8]).unwrap();
        let message = messages
            .iter()
            .find(|message| message.string_decoded_offset == 0x220)
            .unwrap();
        assert_eq!(
            message.rewrite_sites,
            vec![RewriteSite {
                site: 0x41,
                kind: PointerKind::MovSi,
            }]
        );
    }

    #[test]
    fn variant_scan_accepts_an_si_forwarding_renderer_wrapper() {
        let load = 0x100usize;
        let mut decoded = vec![0u8; 0x500];
        decoded[0x200..0x205].copy_from_slice(&[0x82, 0xA0, 0x82, 0xA2, 0]);
        decoded[0x220..0x225].copy_from_slice(&[0x82, 0xA0, 0x82, 0xA4, 0]);
        decoded[0x300] = 0xC3;
        write_call(&mut decoded, 0x10, 0x300, 0x300);

        decoded[0x340..0x34C].copy_from_slice(&[
            0x8C, 0xC8, 0x8E, 0xD8, 0xBF, 0xFF, 0xFF, 0xB0, 0x02, 0xE8, 0x00, 0x00,
        ]);
        let displacement = 0x300isize - 0x34Cisize;
        decoded[0x34A..0x34C].copy_from_slice(&(displacement as i16).to_le_bytes());
        decoded[0x34C] = 0xC3;

        decoded[0x40] = 0xBE;
        decoded[0x41..0x43].copy_from_slice(&0x320u16.to_le_bytes());
        decoded[0x43] = 0xE8;
        let displacement = 0x340isize - 0x46isize;
        decoded[0x44..0x46].copy_from_slice(&(displacement as i16).to_le_bytes());

        let messages = unified_catalog(&decoded, load, 4, &[2, 4, 8]).unwrap();
        let message = messages
            .iter()
            .find(|message| message.string_decoded_offset == 0x220)
            .unwrap();
        assert_eq!(message.rewrite_sites[0].site, 0x41);
    }

    #[test]
    fn variant_scan_accepts_the_event_text_scheduler() {
        let load = 0x100usize;
        let mut decoded = vec![0u8; 0x500];
        decoded[0x200..0x205].copy_from_slice(&[0x82, 0xA0, 0x82, 0xA2, 0]);
        decoded[0x220..0x225].copy_from_slice(&[0x82, 0xA0, 0x82, 0xA4, 0]);
        decoded[0x300] = 0xC3;
        write_call(&mut decoded, 0x10, 0x300, 0x300);

        decoded[0x340..0x349]
            .copy_from_slice(&[0x53, 0xB4, 0x01, 0xCD, 0x7B, 0xB4, 0x07, 0xCD, 0x7A]);
        decoded[0x360..0x36A]
            .copy_from_slice(&[0x26, 0x89, 0xB7, 0x04, 0x03, 0x26, 0x89, 0xBF, 0x06, 0x03]);
        decoded[0x40] = 0xBE;
        decoded[0x41..0x43].copy_from_slice(&0x320u16.to_le_bytes());
        decoded[0x43] = 0xE8;
        let displacement = 0x340isize - 0x46isize;
        decoded[0x44..0x46].copy_from_slice(&(displacement as i16).to_le_bytes());

        let messages = unified_catalog(&decoded, load, 4, &[2, 4, 8]).unwrap();
        let message = messages
            .iter()
            .find(|message| message.string_decoded_offset == 0x220)
            .unwrap();
        assert_eq!(message.rewrite_sites[0].site, 0x41);
    }

    #[test]
    fn variant_scan_follows_a_branch_to_the_renderer() {
        let load = 0x100usize;
        let mut decoded = vec![0u8; 0x500];
        decoded[0x200..0x205].copy_from_slice(&[0x82, 0xA0, 0x82, 0xA2, 0]);
        decoded[0x220..0x225].copy_from_slice(&[0x82, 0xA0, 0x82, 0xA4, 0]);
        decoded[0x300] = 0xC3;
        write_call(&mut decoded, 0x10, 0x300, 0x300);

        decoded[0x40] = 0xBE;
        decoded[0x41..0x43].copy_from_slice(&0x320u16.to_le_bytes());
        decoded[0x43..0x45].copy_from_slice(&[0xEB, 0x3B]);
        decoded[0x80] = 0xE8;
        let displacement = 0x300isize - 0x83isize;
        decoded[0x81..0x83].copy_from_slice(&(displacement as i16).to_le_bytes());

        let messages = unified_catalog(&decoded, load, 4, &[2, 4, 8]).unwrap();
        let message = messages
            .iter()
            .find(|message| message.string_decoded_offset == 0x220)
            .unwrap();
        assert_eq!(message.rewrite_sites[0].site, 0x41);
    }

    #[test]
    fn variant_scan_rejects_an_unaligned_be_crossing_real_instructions() {
        let load = 0x100usize;
        let mut decoded = vec![0u8; 0xBD00];
        decoded[0x200..0x205].copy_from_slice(&[0x82, 0xA0, 0x82, 0xA2, 0]);
        decoded[0xBC57..0xBC5C].copy_from_slice(&[0x82, 0xA0, 0x82, 0xA4, 0]);
        decoded[0x300] = 0xC3;
        write_call(&mut decoded, 0x10, 0x300, 0x300);

        // Exact shape from GAME_R: `BE` is the high byte of
        // mov cs:[BE02],cx. The following 57 BD are push di and the mov-bp
        // opcode, not a mov-si immediate.
        decoded[0x80..0x94].copy_from_slice(&[
            0x2E, 0x89, 0x0E, 0x02, 0xBE, 0x57, 0xBD, 0x6E, 0xBE, 0x88, 0x66, 0x00, 0xC7, 0x46,
            0x02, 0x70, 0x00, 0xB4, 0x05, 0xCD,
        ]);

        let messages = unified_catalog(&decoded, load, 4, &[2, 4, 8]).unwrap();
        assert!(
            messages
                .iter()
                .all(|message| { message.rewrite_sites.iter().all(|site| site.site != 0x85) })
        );
    }

    #[test]
    fn catalog_safety_rejects_a_rewrite_site_inside_text() {
        let mut decoded = vec![0u8; 0x40];
        // The raw bytes happen to contain the message's logical value at 0x11,
        // but a pointer write there would overwrite the text itself.
        decoded[0x10..0x15].copy_from_slice(&[b'A', 0x42, 0x41, b'B', 0]);
        let messages = vec![UnifiedMessage {
            string_decoded_offset: 0x10,
            string_logical_offset: 0x4142,
            byte_budget: 5,
            raw: decoded[0x10..0x14].to_vec(),
            text: "synthetic".to_owned(),
            rewrite_sites: vec![RewriteSite {
                site: 0x11,
                kind: PointerKind::Table,
            }],
        }];

        let error = validate_unified_catalog(&decoded, &messages).unwrap_err();
        assert!(error.to_string().contains("overlaps text slot"));
    }
}
