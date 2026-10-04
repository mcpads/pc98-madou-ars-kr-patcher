//! Batch reinsertion over the unified message map.
//!
//! Given the decoded overlay, the per-message map (`overlay_messages`), and a
//! translation table, the production path repacks Korean entirely inside the
//! catalog's proven source text slots. Messages that fit stay at their source
//! address where possible; overflow messages and small executable reservations
//! consume recovered tail slack, and every moved message rewrites all of its
//! proven consumers. This keeps the decoded GAME extent source-exact for the
//! normal Demo selector. The older high-band allocator remains only for focused
//! direct-boot diagnostics.
//!
//! The in-place-vs-relocate choice and the pointer rewrite follow
//! `references/strategy/reinsertion.md` 1.1-1.2. A missing glyph mapping is a
//! build error, never a silent skip (`references/strategy/font-strategy.md`).

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result, bail};

use crate::overlay_messages::{UnifiedMessage, shared_tail_groups, validate_unified_catalog};
use crate::overlay_reloc::SheetCodes;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSlotReservation {
    pub offset: usize,
    pub len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSlotBatchReport {
    pub in_place: usize,
    pub relocated: usize,
    pub untranslated: usize,
    pub slot_bytes_used: usize,
    pub slot_capacity: usize,
    /// Zero-filled ranges reserved for callers that install executable stubs.
    /// Entries retain the same order as the requested reservation lengths.
    pub reservations: Vec<SourceSlotReservation>,
    /// Placement of every translated catalog entry, sorted in the same order
    /// as `messages`. Keeping the source and destination together lets callers
    /// audit the complete consumer surface of every address that moved.
    pub placements: Vec<SourceSlotMessagePlacement>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSlotMessagePlacement {
    pub source_offset: usize,
    pub destination_offset: usize,
    pub source_budget: usize,
    pub payload_len: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnclassifiedSourceAddressOccurrence {
    pub source_offset: usize,
    pub destination_offset: usize,
    pub site: usize,
}

/// Find little-endian source-address byte pairs outside verified rewrite sites
/// and confirmed source text slots. These are classification candidates, not
/// pointers by themselves: an occurrence may cross an instruction boundary or
/// be incidental data. A structurally proven consumer must be promoted into
/// the unified catalog before its source slot can move.
pub fn unclassified_relocated_source_address_occurrences(
    source: &[u8],
    load_offset: usize,
    messages: &[UnifiedMessage],
    placements: &[SourceSlotMessagePlacement],
) -> Result<Vec<UnclassifiedSourceAddressOccurrence>> {
    validate_unified_catalog(source, messages).context("unsafe unified message catalog")?;

    let by_source = messages
        .iter()
        .map(|message| (message.string_decoded_offset, message))
        .collect::<HashMap<_, _>>();
    let text_spans = messages
        .iter()
        .map(|message| {
            (
                message.string_decoded_offset,
                message.string_decoded_offset + message.byte_budget,
            )
        })
        .collect::<Vec<_>>();
    let mut seen_sources = HashSet::new();
    let mut occurrences = Vec::new();

    for placement in placements {
        if !seen_sources.insert(placement.source_offset) {
            bail!(
                "source-slot placement repeats message 0x{:04X}",
                placement.source_offset
            );
        }
        let message = by_source.get(&placement.source_offset).with_context(|| {
            format!(
                "source-slot placement names unknown message 0x{:04X}",
                placement.source_offset
            )
        })?;
        if placement.source_budget != message.byte_budget {
            bail!(
                "source-slot placement budget {} for 0x{:04X} does not match catalog budget {}",
                placement.source_budget,
                placement.source_offset,
                message.byte_budget
            );
        }
        if placement.source_offset == placement.destination_offset {
            continue;
        }

        let logical = placement
            .source_offset
            .checked_add(load_offset)
            .context("source message logical offset overflow")?;
        let logical = u16::try_from(logical).with_context(|| {
            format!(
                "source message 0x{:04X} logical offset exceeds 16 bits",
                placement.source_offset
            )
        })?;
        let expected = logical.to_le_bytes();
        let owned_sites = message
            .rewrite_sites
            .iter()
            .map(|rewrite| rewrite.site)
            .collect::<HashSet<_>>();

        for (site, bytes) in source.windows(2).enumerate() {
            if bytes != expected || owned_sites.contains(&site) {
                continue;
            }
            let site_end = site + 2;
            if text_spans
                .iter()
                .any(|&(start, end)| site < end && start < site_end)
            {
                continue;
            }
            occurrences.push(UnclassifiedSourceAddressOccurrence {
                source_offset: placement.source_offset,
                destination_offset: placement.destination_offset,
                site,
            });
        }
    }

    occurrences.sort_by_key(|occurrence| (occurrence.source_offset, occurrence.site));
    Ok(occurrences)
}

#[derive(Debug, Clone, Copy)]
struct FreeRange {
    start: usize,
    end: usize,
}

impl FreeRange {
    fn len(self) -> usize {
        self.end - self.start
    }
}

#[derive(Debug, Clone, Copy)]
enum SourceSlotItem {
    Message(usize),
    Reservation(usize),
}

#[derive(Debug, Clone, Copy)]
struct PendingSourceSlotItem {
    item: SourceSlotItem,
    len: usize,
}

#[derive(Debug)]
struct SourceSlotLayout {
    message_destinations: Vec<Option<usize>>,
    reservation_offsets: Vec<Option<usize>>,
}

/// Repack translated strings entirely inside the catalog's proven source text
/// slots, keeping the decoded overlay length unchanged. A translation that fits
/// its own source slot stays at its source address; only that translation's
/// zero-padded tail becomes reusable. An overflowing translation contributes
/// its whole source slot and is placed with the optional executable
/// reservations in the resulting free ranges. This preserves any consumer not
/// represented by the catalog whenever relocation is unnecessary.
pub fn apply_translations_in_source_slots(
    decoded: &mut [u8],
    load_offset: usize,
    messages: &[UnifiedMessage],
    translations: &HashMap<usize, String>,
    sheet: &SheetCodes,
    reservation_lengths: &[usize],
) -> Result<SourceSlotBatchReport> {
    validate_unified_catalog(decoded, messages).context("unsafe unified message catalog")?;
    let known_offsets = messages
        .iter()
        .map(|message| message.string_decoded_offset)
        .collect::<HashSet<_>>();
    if let Some(unknown) = translations
        .keys()
        .find(|offset| !known_offsets.contains(offset))
    {
        bail!("translation targets unknown message 0x{unknown:04X}");
    }
    if let Some(zero_index) = reservation_lengths.iter().position(|len| *len == 0) {
        bail!("source-slot reservation {zero_index} has zero length");
    }

    // A suffix addressed by its own consumers shares the head's source
    // terminator. The group's bytes are counted once, every member must be
    // translated together, and only the head may stay at its source address.
    let groups = shared_tail_groups(messages);
    let mut is_tail = vec![false; messages.len()];
    for (head, members) in &groups {
        let translated = members
            .iter()
            .filter(|&&index| translations.contains_key(&messages[index].string_decoded_offset))
            .count();
        if translated != 0 && translated != members.len() {
            bail!(
                "shared-tail group at 0x{:04X} is only partly translated",
                messages[*head].string_decoded_offset
            );
        }
        for &index in members {
            is_tail[index] = index != *head;
        }
    }

    let mut encoded = Vec::with_capacity(messages.len());
    let mut untranslated = 0usize;
    let mut pinned = vec![false; messages.len()];
    let mut slot_capacity = 0usize;
    let mut slot_bytes_used = 0usize;

    for (index, message) in messages.iter().enumerate() {
        if !is_tail[index] {
            slot_capacity = slot_capacity
                .checked_add(message.byte_budget)
                .context("source text-slot capacity overflow")?;
        }
        let Some(korean) = translations.get(&message.string_decoded_offset) else {
            untranslated += 1;
            if !is_tail[index] {
                slot_bytes_used = slot_bytes_used
                    .checked_add(message.byte_budget)
                    .context("source text-slot use overflow")?;
            }
            encoded.push(None);
            continue;
        };
        let mut payload = sheet.encode_line(korean).map_err(|err| {
            err.context(format!("string 0x{:04X}", message.string_decoded_offset))
        })?;
        payload.push(0);
        slot_bytes_used = slot_bytes_used
            .checked_add(payload.len())
            .context("source text-slot use overflow")?;
        if payload.len() <= message.byte_budget && !is_tail[index] {
            pinned[index] = true;
        }
        encoded.push(Some(payload));
    }

    for len in reservation_lengths.iter().copied() {
        slot_bytes_used = slot_bytes_used
            .checked_add(len)
            .context("source text-slot reservation use overflow")?;
    }
    if slot_bytes_used > slot_capacity {
        bail!(
            "translated messages and reservations need {slot_bytes_used} source-slot bytes, but only {slot_capacity} are proven"
        );
    }

    // Start with every fitting message pinned. If fragmented tail slack cannot
    // place the overflow/reservations, release the easiest fitting source
    // strings one at a time. This is a deterministic fallback, not the default:
    // it minimizes the exposed consumer surface before considering more moves.
    let mut pin_release_order = pinned
        .iter()
        .enumerate()
        .filter_map(|(index, is_pinned)| is_pinned.then_some(index))
        .collect::<Vec<_>>();
    pin_release_order.sort_by(|left, right| {
        let left_len = encoded[*left].as_ref().map_or(0, Vec::len);
        let right_len = encoded[*right].as_ref().map_or(0, Vec::len);
        left_len
            .cmp(&right_len)
            .then_with(|| {
                messages[*right]
                    .byte_budget
                    .cmp(&messages[*left].byte_budget)
            })
            .then_with(|| {
                messages[*left]
                    .string_decoded_offset
                    .cmp(&messages[*right].string_decoded_offset)
            })
    });
    let mut released = 0usize;
    let layout = loop {
        match try_plan_source_slot_layout(
            messages,
            &encoded,
            &pinned,
            &is_tail,
            reservation_lengths,
        ) {
            Ok(layout) => break layout,
            Err((item_len, largest)) => {
                let Some(index) = pin_release_order.get(released).copied() else {
                    bail!(
                        "source text slots cannot place a {item_len}-byte item; largest remaining contiguous range is {largest} bytes"
                    );
                };
                pinned[index] = false;
                released += 1;
            }
        }
    };
    let message_destinations = layout.message_destinations;
    let reservation_offsets = layout.reservation_offsets;

    let mut planned = decoded.to_vec();
    for (index, message) in messages.iter().enumerate() {
        if encoded[index].is_some() {
            planned[message.string_decoded_offset
                ..message.string_decoded_offset + message.byte_budget]
                .fill(0);
        }
    }
    for (index, message) in messages.iter().enumerate() {
        let Some(payload) = &encoded[index] else {
            continue;
        };
        let destination =
            message_destinations[index].context("translated message has no placement")?;
        let destination_end = destination + payload.len();
        planned[destination..destination_end].copy_from_slice(payload);
        let logical = destination
            .checked_add(load_offset)
            .context("source-slot message logical offset overflow")?;
        let logical = u16::try_from(logical).with_context(|| {
            format!(
                "source-slot message 0x{:04X} destination exceeds 16 bits",
                message.string_decoded_offset
            )
        })?;
        for site in &message.rewrite_sites {
            planned[site.site..site.site + 2].copy_from_slice(&logical.to_le_bytes());
        }
    }

    let reservations = reservation_offsets
        .into_iter()
        .zip(reservation_lengths.iter().copied())
        .enumerate()
        .map(|(index, (offset, len))| {
            Ok(SourceSlotReservation {
                offset: offset.with_context(|| format!("reservation {index} has no placement"))?,
                len,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let placements = messages
        .iter()
        .enumerate()
        .filter_map(|(index, message)| {
            let payload = encoded[index].as_ref()?;
            Some(SourceSlotMessagePlacement {
                source_offset: message.string_decoded_offset,
                destination_offset: message_destinations[index]
                    .expect("translated message placement was checked above"),
                source_budget: message.byte_budget,
                payload_len: payload.len(),
            })
        })
        .collect::<Vec<_>>();

    let in_place = messages
        .iter()
        .enumerate()
        .filter(|(index, message)| {
            encoded[*index].is_some()
                && message_destinations[*index] == Some(message.string_decoded_offset)
        })
        .count();

    for (index, message) in messages.iter().enumerate() {
        let Some(payload) = &encoded[index] else {
            continue;
        };
        let destination =
            message_destinations[index].context("translated message has no placement")?;
        if planned.get(destination..destination + payload.len()) != Some(payload.as_slice()) {
            bail!(
                "source-slot payload postcondition failed for message 0x{:04X}",
                message.string_decoded_offset
            );
        }
        let expected = ((destination + load_offset) as u16).to_le_bytes();
        for site in &message.rewrite_sites {
            if planned.get(site.site..site.site + 2) != Some(expected.as_slice()) {
                bail!(
                    "source-slot pointer postcondition failed at 0x{:04X} for message 0x{:04X}",
                    site.site,
                    message.string_decoded_offset
                );
            }
        }
    }
    for reservation in &reservations {
        if !planned[reservation.offset..reservation.offset + reservation.len]
            .iter()
            .all(|byte| *byte == 0)
        {
            bail!(
                "source-slot reservation 0x{:04X}..0x{:04X} is not empty",
                reservation.offset,
                reservation.offset + reservation.len
            );
        }
    }

    decoded.copy_from_slice(&planned);
    Ok(SourceSlotBatchReport {
        in_place,
        relocated: translations.len() - in_place,
        untranslated,
        slot_bytes_used,
        slot_capacity,
        reservations,
        placements,
    })
}

fn try_plan_source_slot_layout(
    messages: &[UnifiedMessage],
    encoded: &[Option<Vec<u8>>],
    pinned: &[bool],
    is_tail: &[bool],
    reservation_lengths: &[usize],
) -> std::result::Result<SourceSlotLayout, (usize, usize)> {
    let mut free_ranges = Vec::new();
    let mut pending = Vec::new();
    let mut message_destinations = vec![None; messages.len()];
    for (index, message) in messages.iter().enumerate() {
        let Some(payload) = &encoded[index] else {
            continue;
        };
        let source_end = message.string_decoded_offset + message.byte_budget;
        if is_tail[index] {
            // The group head's range already contributes these bytes.
            pending.push(PendingSourceSlotItem {
                item: SourceSlotItem::Message(index),
                len: payload.len(),
            });
        } else if pinned[index] {
            message_destinations[index] = Some(message.string_decoded_offset);
            if payload.len() < message.byte_budget {
                free_ranges.push(FreeRange {
                    start: message.string_decoded_offset + payload.len(),
                    end: source_end,
                });
            }
        } else {
            free_ranges.push(FreeRange {
                start: message.string_decoded_offset,
                end: source_end,
            });
            pending.push(PendingSourceSlotItem {
                item: SourceSlotItem::Message(index),
                len: payload.len(),
            });
        }
    }
    for (index, len) in reservation_lengths.iter().copied().enumerate() {
        pending.push(PendingSourceSlotItem {
            item: SourceSlotItem::Reservation(index),
            len,
        });
    }

    free_ranges.sort_by_key(|range| range.start);
    let mut merged_ranges: Vec<FreeRange> = Vec::with_capacity(free_ranges.len());
    for range in free_ranges {
        if let Some(previous) = merged_ranges.last_mut()
            && previous.end == range.start
        {
            previous.end = range.end;
        } else {
            merged_ranges.push(range);
        }
    }

    // Best-fit decreasing avoids consuming the one large source slot with a
    // small payload while remaining deterministic across hosts.
    pending.sort_by(|left, right| {
        right.len.cmp(&left.len).then_with(|| {
            source_slot_item_order(left.item).cmp(&source_slot_item_order(right.item))
        })
    });
    let mut reservation_offsets = vec![None; reservation_lengths.len()];
    for item in pending {
        let Some((range_index, _)) = merged_ranges
            .iter()
            .enumerate()
            .filter(|(_, range)| range.len() >= item.len)
            .min_by_key(|(_, range)| (range.len(), range.start))
        else {
            let largest = merged_ranges
                .iter()
                .map(|range| range.len())
                .max()
                .unwrap_or(0);
            return Err((item.len, largest));
        };
        let destination = merged_ranges[range_index].start;
        merged_ranges[range_index].start += item.len;
        match item.item {
            SourceSlotItem::Message(index) => message_destinations[index] = Some(destination),
            SourceSlotItem::Reservation(index) => reservation_offsets[index] = Some(destination),
        }
    }
    Ok(SourceSlotLayout {
        message_destinations,
        reservation_offsets,
    })
}

fn source_slot_item_order(item: SourceSlotItem) -> (u8, usize) {
    match item {
        SourceSlotItem::Message(index) => (0, index),
        SourceSlotItem::Reservation(index) => (1, index),
    }
}

/// Runtime-proven free band in the GAME_A/R/S renderer segment.  Decoded
/// 0xBFC0 maps to physical 0x2A000; 0xDFC0 maps to the first byte of the
/// resident-data boundary at physical 0x2C000, so the end is exclusive.
pub const PROVEN_GAME_RELOC_BASE: usize = 0xBFC0;
pub const PROVEN_GAME_RELOC_END: usize = 0xDFC0;

/// Production callers may narrow the proven band but may not expand it into
/// renderer scratch or the resident-data region without a new runtime proof.
pub fn validate_proven_relocation_band(reloc_base: usize, reloc_end: usize) -> Result<()> {
    if reloc_base >= reloc_end {
        bail!("relocation band is empty or reversed: 0x{reloc_base:04X}..0x{reloc_end:04X}");
    }
    if reloc_base < PROVEN_GAME_RELOC_BASE || reloc_end > PROVEN_GAME_RELOC_END {
        bail!(
            "relocation band 0x{reloc_base:04X}..0x{reloc_end:04X} exceeds the runtime-proven range 0x{PROVEN_GAME_RELOC_BASE:04X}..0x{PROVEN_GAME_RELOC_END:04X}"
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BatchReport {
    pub in_place: usize,
    pub relocated: usize,
    /// Bytes consumed in the relocation band (appended strings plus NULs).
    pub reloc_bytes_used: usize,
    /// Cataloged messages with no translation entry.
    pub untranslated: usize,
}

/// Apply `translations` (keyed by a message's decoded string offset) to the
/// decoded overlay. `reloc_base`/`reloc_end` bound the free band that relocated
/// strings are appended into (decoded offsets). Fails on an unmappable glyph, a
/// pointer past the 16-bit space, or an exhausted band -- never silently.
pub fn apply_translations(
    decoded: &mut Vec<u8>,
    load_offset: usize,
    messages: &[UnifiedMessage],
    translations: &HashMap<usize, String>,
    sheet: &SheetCodes,
    reloc_base: usize,
    reloc_end: usize,
) -> Result<BatchReport> {
    validate_unified_catalog(decoded, messages).context("unsafe unified message catalog")?;
    if let Some((head, _)) = shared_tail_groups(messages)
        .into_iter()
        .find(|(_, members)| members.len() > 1)
    {
        bail!(
            "diagnostic high-band allocator does not support the shared-tail group at 0x{:04X}; use the source-slot builder",
            messages[head].string_decoded_offset
        );
    }
    if reloc_base >= reloc_end {
        bail!("relocation band is empty or reversed: 0x{reloc_base:04X}..0x{reloc_end:04X}");
    }
    let logical_end = reloc_end
        .checked_add(load_offset)
        .context("relocation-band logical end overflow")?;
    if logical_end > 0x10000 {
        bail!(
            "relocation band end 0x{reloc_end:04X} + load 0x{load_offset:04X} exceeds the 16-bit segment"
        );
    }
    let known_offsets = messages
        .iter()
        .map(|message| message.string_decoded_offset)
        .collect::<HashSet<_>>();
    if let Some(unknown) = translations
        .keys()
        .find(|offset| !known_offsets.contains(offset))
    {
        bail!("translation targets unknown message 0x{unknown:04X}");
    }

    // 1. Encode every translation first (so an unmappable glyph fails before any
    //    byte is written) and split into in-place vs relocate by byte budget.
    let mut in_place: Vec<(usize, Vec<u8>, usize)> = Vec::new();
    let mut relocate: Vec<(&UnifiedMessage, Vec<u8>)> = Vec::new();
    let mut untranslated = 0usize;
    for message in messages {
        let Some(korean) = translations.get(&message.string_decoded_offset) else {
            untranslated += 1;
            continue;
        };
        let encoded = sheet.encode_line(korean).map_err(|err| {
            err.context(format!("string 0x{:04X}", message.string_decoded_offset))
        })?;
        if encoded.len() < message.byte_budget {
            in_place.push((message.string_decoded_offset, encoded, message.byte_budget));
        } else {
            relocate.push((message, encoded));
        }
    }

    // Plan every byte on a clone.  Any later capacity/pointer/postcondition
    // failure leaves the caller's decoded image untouched.
    let mut planned = decoded.clone();

    // 2. In-place edits: overwrite the slot, NUL-pad the rest so the renderer
    //    stops at the shorter Korean. These do not move any string.
    for (offset, bytes, budget) in &in_place {
        planned[*offset..*offset + bytes.len()].copy_from_slice(bytes);
        for pad in planned[*offset + bytes.len()..*offset + budget].iter_mut() {
            *pad = 0;
        }
    }

    // 3. Relocations: pad up to the band, then append each string and rewrite
    //    all of its pointer sites to the new logical offset.
    if !relocate.is_empty() {
        if planned.len() > reloc_base {
            bail!(
                "decoded overlay ends at 0x{:04X}, inside relocation band starting at 0x{reloc_base:04X}",
                planned.len()
            );
        }
        planned.resize(reloc_base, 0);
    }
    let mut placements = Vec::with_capacity(relocate.len());
    for (message, bytes) in &relocate {
        let new_decoded = planned.len();
        let new_logical = new_decoded
            .checked_add(load_offset)
            .context("relocated logical offset overflow")?;
        // The whole string (not just its start) must live below 0x10000, or the
        // renderer's 16-bit `si` wraps to 0 mid-string.
        let new_logical_end = new_logical
            .checked_add(bytes.len() + 1)
            .context("relocated string logical range overflow")?;
        if new_logical_end > 0x10000 {
            bail!(
                "relocated string 0x{:04X} (start 0x{new_logical:X}, +{} bytes) crosses the 16-bit pointer space",
                message.string_decoded_offset,
                bytes.len() + 1
            );
        }
        let new_decoded_end = new_decoded
            .checked_add(bytes.len() + 1)
            .context("relocated string decoded range overflow")?;
        if new_decoded_end > reloc_end {
            bail!("relocation band exhausted at 0x{new_decoded:04X} (end 0x{reloc_end:04X})");
        }
        planned.extend_from_slice(bytes);
        planned.push(0);
        let immediate = (new_logical as u16).to_le_bytes();
        for site in &message.rewrite_sites {
            planned[site.site..site.site + 2].copy_from_slice(&immediate);
        }
        placements.push((*message, bytes.as_slice(), new_decoded, new_logical));
    }

    // 4. Semantic postconditions: verify the bytes at every destination and the
    // complete rewrite-site set, not only the later LZ/FAT byte round-trip.
    for (offset, bytes, budget) in &in_place {
        if planned.get(*offset..*offset + bytes.len()) != Some(bytes.as_slice())
            || planned.get(*offset + bytes.len()) != Some(&0)
            || !planned[*offset + bytes.len()..*offset + budget]
                .iter()
                .all(|byte| *byte == 0)
        {
            bail!("in-place postcondition failed for message 0x{offset:04X}");
        }
    }
    for (message, bytes, new_decoded, new_logical) in &placements {
        let payload_end = *new_decoded + bytes.len();
        if planned.get(*new_decoded..payload_end) != Some(*bytes)
            || planned.get(payload_end) != Some(&0)
        {
            bail!(
                "relocated payload postcondition failed for message 0x{:04X}",
                message.string_decoded_offset
            );
        }
        let expected = (*new_logical as u16).to_le_bytes();
        for site in &message.rewrite_sites {
            if planned.get(site.site..site.site + 2) != Some(expected.as_slice()) {
                bail!(
                    "relocated pointer postcondition failed at 0x{:04X} for message 0x{:04X}",
                    site.site,
                    message.string_decoded_offset
                );
            }
        }
        let original_end = message.string_decoded_offset + message.raw.len();
        if planned.get(message.string_decoded_offset..original_end) != Some(message.raw.as_slice())
        {
            bail!(
                "relocation overwrote the original slot for message 0x{:04X}",
                message.string_decoded_offset
            );
        }
    }

    let reloc_bytes_used = if placements.is_empty() {
        0
    } else {
        planned.len() - reloc_base
    };
    *decoded = planned;
    Ok(BatchReport {
        in_place: in_place.len(),
        relocated: relocate.len(),
        reloc_bytes_used,
        untranslated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay_messages::unified_catalog;

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

    // 가 -> eba7, 나 -> eba1 (hook codes, 2 bytes each).
    fn sheet() -> SheetCodes {
        SheetCodes::from_json_str(
            r#"{"glyphs":[{"char":"가","sjis":"eba7"},{"char":"나","sjis":"eba1"}]}"#,
        )
        .unwrap()
    }

    fn build() -> (Vec<u8>, usize, Vec<UnifiedMessage>) {
        let load = 0x100usize;
        let mut decoded = vec![0u8; 0x400];
        decoded[0x300] = 0xC3;
        // mov-si message: あいうえ (8 bytes) + NUL = budget 9, at 0x200, one call.
        decoded[0x200..0x209]
            .copy_from_slice(&[0x82, 0xA0, 0x82, 0xA2, 0x82, 0xA4, 0x82, 0xA6, 0x00]);
        write_call(&mut decoded, 0x10, 0x300, 0x300);
        // Table message: かき (4 bytes) + NUL = budget 5, at 0x220, table of 4.
        for (i, off) in [0x220usize, 0x230, 0x240, 0x250].into_iter().enumerate() {
            decoded[off..off + 5].copy_from_slice(&[0x82, 0xA9, 0x82, 0xAB, 0x00]);
            let logical = (off + load) as u16;
            decoded[0x120 + i * 2..0x120 + i * 2 + 2].copy_from_slice(&logical.to_le_bytes());
        }
        let messages = unified_catalog(&decoded, load, 4, &[2, 4, 8]).unwrap();
        (decoded, load, messages)
    }

    #[test]
    fn source_slot_repack_pins_fitting_messages_and_uses_only_their_tail() {
        let load = 0x100usize;
        let mut decoded = vec![0u8; 0x100];
        decoded[0x40..0x48].copy_from_slice(b"abcdefg\0");
        decoded[0x48..0x4C].copy_from_slice(b"xyz\0");
        decoded[0x10..0x12].copy_from_slice(&0x140u16.to_le_bytes());
        decoded[0x20..0x22].copy_from_slice(&0x148u16.to_le_bytes());
        let messages = vec![
            UnifiedMessage {
                string_decoded_offset: 0x40,
                string_logical_offset: 0x140,
                byte_budget: 8,
                raw: b"abcdefg".to_vec(),
                text: "abcdefg".to_string(),
                rewrite_sites: vec![crate::overlay_messages::RewriteSite {
                    site: 0x10,
                    kind: crate::overlay_messages::PointerKind::Table,
                }],
            },
            UnifiedMessage {
                string_decoded_offset: 0x48,
                string_logical_offset: 0x148,
                byte_budget: 4,
                raw: b"xyz".to_vec(),
                text: "xyz".to_string(),
                rewrite_sites: vec![crate::overlay_messages::RewriteSite {
                    site: 0x20,
                    kind: crate::overlay_messages::PointerKind::Table,
                }],
            },
        ];
        let translations = HashMap::from([
            (0x40usize, "가".to_string()),
            (0x48usize, "가나가".to_string()),
        ]);
        let source_len = decoded.len();
        let report = apply_translations_in_source_slots(
            &mut decoded,
            load,
            &messages,
            &translations,
            &sheet(),
            &[2],
        )
        .unwrap();

        assert_eq!(decoded.len(), source_len);
        assert_eq!(report.in_place, 1);
        assert_eq!(report.relocated, 1);
        assert_eq!(report.slot_bytes_used, report.slot_capacity);
        assert_eq!(&decoded[0x40..0x43], &[0xEB, 0xA7, 0]);
        assert_eq!(u16::from_le_bytes([decoded[0x10], decoded[0x11]]), 0x140);
        assert_eq!(
            &decoded[0x43..0x4A],
            &[0xEB, 0xA7, 0xEB, 0xA1, 0xEB, 0xA7, 0]
        );
        assert_eq!(u16::from_le_bytes([decoded[0x20], decoded[0x21]]), 0x143);
        assert_eq!(
            report.reservations[0],
            SourceSlotReservation {
                offset: 0x4A,
                len: 2
            }
        );
        assert_eq!(&decoded[0x4A..0x4C], &[0, 0]);
    }

    fn shared_tail_messages() -> (Vec<u8>, Vec<UnifiedMessage>) {
        let mut decoded = vec![0u8; 0x100];
        // あいうえお: the head renders all five kana; a separate consumer
        // renders only the えお suffix before the same terminator.
        decoded[0x40..0x4B].copy_from_slice(&[
            0x82, 0xA0, 0x82, 0xA2, 0x82, 0xA4, 0x82, 0xA6, 0x82, 0xA8, 0x00,
        ]);
        decoded[0x10..0x12].copy_from_slice(&0x140u16.to_le_bytes());
        decoded[0x20..0x22].copy_from_slice(&0x146u16.to_le_bytes());
        let site = |site| crate::overlay_messages::RewriteSite {
            site,
            kind: crate::overlay_messages::PointerKind::Table,
        };
        let messages = vec![
            UnifiedMessage {
                string_decoded_offset: 0x40,
                string_logical_offset: 0x140,
                byte_budget: 11,
                raw: decoded[0x40..0x4A].to_vec(),
                text: "あいうえお".to_string(),
                rewrite_sites: vec![site(0x10)],
            },
            UnifiedMessage {
                string_decoded_offset: 0x46,
                string_logical_offset: 0x146,
                byte_budget: 5,
                raw: decoded[0x46..0x4A].to_vec(),
                text: "えお".to_string(),
                rewrite_sites: vec![site(0x20)],
            },
        ];
        (decoded, messages)
    }

    #[test]
    fn shared_tail_suffix_moves_without_reusing_bytes_the_head_renders() {
        let (mut decoded, messages) = shared_tail_messages();
        let translations = HashMap::from([
            (0x40usize, "가".to_string()),
            (0x46usize, "가나".to_string()),
        ]);
        let report = apply_translations_in_source_slots(
            &mut decoded,
            0x100,
            &messages,
            &translations,
            &sheet(),
            &[],
        )
        .unwrap();

        // The shared bytes are counted once.
        assert_eq!(report.slot_capacity, 11);
        assert_eq!(report.in_place, 1);
        assert_eq!(report.relocated, 1);
        assert_eq!(u16::from_le_bytes([decoded[0x10], decoded[0x11]]), 0x140);
        assert_eq!(&decoded[0x40..0x43], &[0xEB, 0xA7, 0]);
        let suffix = u16::from_le_bytes([decoded[0x20], decoded[0x21]]) as usize - 0x100;
        assert_ne!(suffix, 0x46);
        assert_eq!(&decoded[suffix..suffix + 5], &[0xEB, 0xA7, 0xEB, 0xA1, 0]);
    }

    #[test]
    fn shared_tail_group_rejects_partial_translation() {
        let (mut decoded, messages) = shared_tail_messages();
        let before = decoded.clone();
        let translations = HashMap::from([(0x46usize, "가".to_string())]);
        let error = apply_translations_in_source_slots(
            &mut decoded,
            0x100,
            &messages,
            &translations,
            &sheet(),
            &[],
        )
        .unwrap_err();
        assert!(error.to_string().contains("partly translated"));
        assert_eq!(decoded, before);
    }

    #[test]
    fn suffix_of_a_referenced_uncataloged_head_is_rejected() {
        let (decoded, messages) = shared_tail_messages();
        let error = validate_unified_catalog(&decoded, &messages[1..]).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("suffix of uncataloged source text at 0x0040")
        );

        // With no occurrence of the head address, the head is unreachable.
        let mut unreachable = decoded.clone();
        unreachable[0x10..0x12].fill(0);
        validate_unified_catalog(&unreachable, &messages[1..]).unwrap();
    }

    #[test]
    fn source_slot_repack_failure_is_transactional() {
        let (mut decoded, load, messages) = build();
        let before = decoded.clone();
        let translations = HashMap::from([(0x200usize, "가나".to_string())]);
        let error = apply_translations_in_source_slots(
            &mut decoded,
            load,
            &messages,
            &translations,
            &sheet(),
            &[0x100],
        )
        .unwrap_err();
        assert!(error.to_string().contains("source-slot bytes"));
        assert_eq!(decoded, before);
    }

    #[test]
    fn in_place_when_korean_fits_the_budget() {
        let (mut decoded, load, messages) = build();
        // 가나 = 4 bytes, budget 9 -> in place at 0x200.
        let translations = HashMap::from([(0x200usize, "가나".to_string())]);
        let report = apply_translations(
            &mut decoded,
            load,
            &messages,
            &translations,
            &sheet(),
            0x380,
            0x400,
        )
        .unwrap();
        assert_eq!(report.in_place, 1);
        assert_eq!(report.relocated, 0);
        assert_eq!(&decoded[0x200..0x204], &[0xEB, 0xA7, 0xEB, 0xA1]);
        assert_eq!(decoded[0x204], 0); // NUL pad, renderer stops here
        // The call-site immediate is untouched (still points at 0x300).
        assert_eq!(&decoded[0x11..0x13], &0x300u16.to_le_bytes());
    }

    #[test]
    fn relocates_a_table_message_and_rewrites_its_table_entry() {
        let (mut decoded, load, messages) = build();
        // Translate the table string at 0x220 with 가나가나가 (10 bytes) -> over
        // its 5-byte budget, so it relocates and the table entry is rewritten.
        let translations = HashMap::from([(0x220usize, "가나가나가".to_string())]);
        let report = apply_translations(
            &mut decoded,
            load,
            &messages,
            &translations,
            &sheet(),
            0x400,
            0x600,
        )
        .unwrap();
        assert_eq!(report.relocated, 1);
        // New string sits at the band base (0x400), logical 0x500.
        assert_eq!(
            &decoded[0x400..0x40A],
            &[0xEB, 0xA7, 0xEB, 0xA1, 0xEB, 0xA7, 0xEB, 0xA1, 0xEB, 0xA7]
        );
        assert_eq!(decoded[0x40A], 0);
        // The table entry (site 0x120) now points at the new logical offset.
        assert_eq!(&decoded[0x120..0x122], &0x500u16.to_le_bytes());
    }

    #[test]
    fn untranslated_messages_are_left_unchanged_and_counted() {
        let (mut decoded, load, messages) = build();
        let before = decoded.clone();
        let report = apply_translations(
            &mut decoded,
            load,
            &messages,
            &HashMap::new(),
            &sheet(),
            0x380,
            0x400,
        )
        .unwrap();
        assert_eq!(report.in_place, 0);
        assert_eq!(report.relocated, 0);
        assert!(report.untranslated >= 2);
        assert_eq!(decoded, before); // nothing touched
    }

    #[test]
    fn unmappable_glyph_is_a_build_error() {
        let (mut decoded, load, messages) = build();
        // 다 is not in the sheet and not plain Shift-JIS-encodable here.
        let translations = HashMap::from([(0x200usize, "가다".to_string())]);
        let err = apply_translations(
            &mut decoded,
            load,
            &messages,
            &translations,
            &sheet(),
            0x380,
            0x400,
        )
        .unwrap_err();
        assert!(err.to_string().contains("0x0200") || format!("{err:#}").contains("다"));
    }

    #[test]
    fn relocation_failure_does_not_partially_mutate() {
        let (mut decoded, load, messages) = build();
        let before = decoded.clone();
        let translations = HashMap::from([(0x220usize, "가나가나가".to_string())]);
        let error = apply_translations(
            &mut decoded,
            load,
            &messages,
            &translations,
            &sheet(),
            0x400,
            0x405,
        )
        .unwrap_err();
        assert!(error.to_string().contains("relocation band exhausted"));
        assert_eq!(decoded, before);
    }

    #[test]
    fn production_band_must_stay_inside_the_runtime_proof() {
        validate_proven_relocation_band(PROVEN_GAME_RELOC_BASE, PROVEN_GAME_RELOC_END).unwrap();
        validate_proven_relocation_band(PROVEN_GAME_RELOC_BASE + 0x100, PROVEN_GAME_RELOC_END)
            .unwrap();
        assert!(
            validate_proven_relocation_band(PROVEN_GAME_RELOC_BASE - 1, PROVEN_GAME_RELOC_END)
                .is_err()
        );
        assert!(
            validate_proven_relocation_band(PROVEN_GAME_RELOC_BASE, PROVEN_GAME_RELOC_END + 1)
                .is_err()
        );
    }
}
