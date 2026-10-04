//! Runtime Korean-particle selection for dynamically prefixed renderer text.
//!
//! Translation strings may encode one-cell markers for `을/를`, `이/가`,
//! `은/는`, and `와/과`. The renderer entry recognizes a marker before it
//! replaces the saved JIS code of the preceding dynamic name, classifies that
//! name through build-generated jongseong metadata, and substitutes the real
//! particle JIS code. The two assembly-level enemy-status `は` sites use the
//! same selector through a small overlay stub because no translatable marker
//! string exists at those call sites.

use anyhow::{Context, Result, bail};
use serde_json::Value;
use v30::{CallTarget, Instruction, encode_bytes};

use crate::font_build::{CELLS_PER_ROW, RENDERER_BASE_ROW, RENDERER_CAPACITY, RENDERER_ROWS};

pub const TOPIC_NO_FINAL: char = '는';
pub const TOPIC_FINAL: char = '은';
pub const OBJECT_NO_FINAL: char = '를';
pub const OBJECT_FINAL: char = '을';
pub const SUBJECT_NO_FINAL: char = '가';
pub const SUBJECT_FINAL: char = '이';
pub const WITH_NO_FINAL: char = '와';
pub const WITH_FINAL: char = '과';
pub const OBJECT_MARKER: char = '\u{E000}';
pub const SUBJECT_MARKER: char = '\u{E001}';
pub const TOPIC_MARKER: char = '\u{E002}';
pub const WITH_MARKER: char = '\u{E003}';
pub const PARTICLE_MARKERS: [char; 4] = [OBJECT_MARKER, SUBJECT_MARKER, TOPIC_MARKER, WITH_MARKER];

/// Runtime-only particle markers use CP932's first user-defined lead byte. The
/// game converts `F0 40..43` to JIS `0x7F21..0x7F24`, one row above the
/// renderer sheet, so the entry hook can dispatch them before GRCG drawing
/// without consuming real glyph cells.
pub const PARTICLE_MARKER_FIRST_JIS: u16 = 0x7F21;
pub const PARTICLE_MARKER_FIRST_SJIS: [u8; 2] = [0xF0, 0x40];

pub fn particle_marker_sjis(index: usize) -> Result<[u8; 2]> {
    if index >= PARTICLE_MARKERS.len() {
        bail!("runtime particle marker index {index} is out of range");
    }
    Ok([
        PARTICLE_MARKER_FIRST_SJIS[0],
        PARTICLE_MARKER_FIRST_SJIS[1] + index as u8,
    ])
}

/// The 940-glyph renderer sheet occupies exactly `0x0000..0x7580`.
pub const RUNTIME_DATA_OFF: usize = 0x7580;
pub const RUNTIME_MAGIC: &[u8; 4] = b"JOSA";
pub const OBJECT_NO_FINAL_CODE_OFF: usize = RUNTIME_DATA_OFF + 0x04;
pub const OBJECT_FINAL_CODE_OFF: usize = RUNTIME_DATA_OFF + 0x06;
pub const SUBJECT_NO_FINAL_CODE_OFF: usize = RUNTIME_DATA_OFF + 0x08;
pub const SUBJECT_FINAL_CODE_OFF: usize = RUNTIME_DATA_OFF + 0x0A;
pub const TOPIC_NO_FINAL_CODE_OFF: usize = RUNTIME_DATA_OFF + 0x0C;
pub const TOPIC_FINAL_CODE_OFF: usize = RUNTIME_DATA_OFF + 0x0E;
pub const WITH_NO_FINAL_CODE_OFF: usize = RUNTIME_DATA_OFF + 0x10;
pub const WITH_FINAL_CODE_OFF: usize = RUNTIME_DATA_OFF + 0x12;
pub const CLASS_TABLE_OFF: usize = RUNTIME_DATA_OFF + 0x20;
pub const CLASS_TABLE_LEN: usize = RENDERER_CAPACITY;
pub const RUNTIME_DATA_LEN: usize = 0x20 + CLASS_TABLE_LEN;

/// `call far 0x8800:topic_selector` + `ret`.
pub const OVERLAY_STUB_LEN: usize = 8;
pub const EXPECTED_TOPIC_SITES: usize = 2;

const ORIGINAL_TOPIC_MOV: [u8; 3] = [0xB8, 0x4F, 0x24]; // mov ax, JIS 0x244F (は)
const APPROVED_DIRECT_DRAW_TARGETS: [u16; 3] = [0x53F0, 0x5430, 0x53C0];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum JongseongClass {
    NoFinal = 0,
    Rieul = 1,
    OtherFinal = 2,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopicPatchReport {
    pub sites: Vec<usize>,
    pub stub_decoded_offset: usize,
    pub stub_logical_offset: usize,
    pub direct_draw_target: u16,
}

pub fn jongseong_class(ch: char) -> Result<JongseongClass> {
    if !('가'..='힣').contains(&ch) {
        bail!("particle class requires a precomposed Hangul syllable, got {ch:?}");
    }
    let jongseong = (ch as u32 - '가' as u32) % 28;
    Ok(match jongseong {
        0 => JongseongClass::NoFinal,
        8 => JongseongClass::Rieul,
        _ => JongseongClass::OtherFinal,
    })
}

/// Build the boot-loaded selector header and one-byte class table from the
/// generated glyph-sheet metadata. The table is derived, never hand-maintained.
pub fn build_runtime_data_from_json(text: &str) -> Result<Vec<u8>> {
    let value: Value = serde_json::from_str(text).context("parse glyph-sheet JSON")?;
    let glyphs = value
        .get("glyphs")
        .and_then(Value::as_array)
        .context("glyph sheet missing `glyphs` array")?;

    let mut classes = vec![JongseongClass::NoFinal as u8; CLASS_TABLE_LEN];
    let mut occupied = vec![false; CLASS_TABLE_LEN];
    let mut topic_no_final = None;
    let mut topic_final = None;
    let mut object_no_final = None;
    let mut object_final = None;
    let mut subject_no_final = None;
    let mut subject_final = None;
    let mut with_no_final = None;
    let mut with_final = None;

    for (index, glyph) in glyphs.iter().enumerate() {
        let text = glyph
            .get("char")
            .and_then(Value::as_str)
            .with_context(|| format!("glyph {index} missing `char`"))?;
        let mut chars = text.chars();
        let ch = chars
            .next()
            .with_context(|| format!("glyph {index} has an empty `char`"))?;
        if chars.next().is_some() {
            bail!("glyph {index} `char` must contain exactly one scalar");
        }
        let slot = glyph
            .get("slot")
            .and_then(Value::as_u64)
            .with_context(|| format!("glyph {index} missing integer `slot`"))?
            as usize;
        if slot >= CLASS_TABLE_LEN {
            bail!("glyph {index} slot {slot} exceeds renderer capacity");
        }
        if std::mem::replace(&mut occupied[slot], true) {
            bail!("glyph {index} duplicates renderer slot {slot}");
        }

        let jis_text = glyph
            .get("jis")
            .and_then(Value::as_str)
            .with_context(|| format!("glyph {index} missing `jis`"))?;
        let jis = u16::from_str_radix(jis_text.trim_start_matches("0x"), 16)
            .with_context(|| format!("glyph {index} has bad JIS code {jis_text:?}"))?;
        let expected_jis = renderer_jis(slot)?;
        if jis != expected_jis {
            bail!(
                "glyph {index} slot {slot} maps to JIS 0x{expected_jis:04X}, metadata says 0x{jis:04X}"
            );
        }

        classes[slot] = jongseong_class(ch)? as u8;
        if ch == TOPIC_NO_FINAL {
            topic_no_final = Some(jis);
        } else if ch == TOPIC_FINAL {
            topic_final = Some(jis);
        } else if ch == OBJECT_NO_FINAL {
            object_no_final = Some(jis);
        } else if ch == OBJECT_FINAL {
            object_final = Some(jis);
        } else if ch == SUBJECT_NO_FINAL {
            subject_no_final = Some(jis);
        } else if ch == SUBJECT_FINAL {
            subject_final = Some(jis);
        } else if ch == WITH_NO_FINAL {
            with_no_final = Some(jis);
        } else if ch == WITH_FINAL {
            with_final = Some(jis);
        }
    }

    let topic_no_final = topic_no_final.context("glyph sheet lacks runtime topic particle `는`")?;
    let topic_final = topic_final.context("glyph sheet lacks runtime topic particle `은`")?;
    let object_no_final =
        object_no_final.context("glyph sheet lacks runtime object particle `를`")?;
    let object_final = object_final.context("glyph sheet lacks runtime object particle `을`")?;
    let subject_no_final =
        subject_no_final.context("glyph sheet lacks runtime subject particle `가`")?;
    let subject_final = subject_final.context("glyph sheet lacks runtime subject particle `이`")?;
    let with_no_final = with_no_final.context("glyph sheet lacks runtime with particle `와`")?;
    let with_final = with_final.context("glyph sheet lacks runtime with particle `과`")?;
    let mut data = vec![0u8; RUNTIME_DATA_LEN];
    data[..RUNTIME_MAGIC.len()].copy_from_slice(RUNTIME_MAGIC);
    data[TOPIC_NO_FINAL_CODE_OFF - RUNTIME_DATA_OFF
        ..TOPIC_NO_FINAL_CODE_OFF - RUNTIME_DATA_OFF + 2]
        .copy_from_slice(&topic_no_final.to_le_bytes());
    data[TOPIC_FINAL_CODE_OFF - RUNTIME_DATA_OFF..TOPIC_FINAL_CODE_OFF - RUNTIME_DATA_OFF + 2]
        .copy_from_slice(&topic_final.to_le_bytes());
    data[OBJECT_NO_FINAL_CODE_OFF - RUNTIME_DATA_OFF
        ..OBJECT_NO_FINAL_CODE_OFF - RUNTIME_DATA_OFF + 2]
        .copy_from_slice(&object_no_final.to_le_bytes());
    data[OBJECT_FINAL_CODE_OFF - RUNTIME_DATA_OFF..OBJECT_FINAL_CODE_OFF - RUNTIME_DATA_OFF + 2]
        .copy_from_slice(&object_final.to_le_bytes());
    data[SUBJECT_NO_FINAL_CODE_OFF - RUNTIME_DATA_OFF
        ..SUBJECT_NO_FINAL_CODE_OFF - RUNTIME_DATA_OFF + 2]
        .copy_from_slice(&subject_no_final.to_le_bytes());
    data[SUBJECT_FINAL_CODE_OFF - RUNTIME_DATA_OFF..SUBJECT_FINAL_CODE_OFF - RUNTIME_DATA_OFF + 2]
        .copy_from_slice(&subject_final.to_le_bytes());
    data[WITH_NO_FINAL_CODE_OFF - RUNTIME_DATA_OFF..WITH_NO_FINAL_CODE_OFF - RUNTIME_DATA_OFF + 2]
        .copy_from_slice(&with_no_final.to_le_bytes());
    data[WITH_FINAL_CODE_OFF - RUNTIME_DATA_OFF..WITH_FINAL_CODE_OFF - RUNTIME_DATA_OFF + 2]
        .copy_from_slice(&with_final.to_le_bytes());
    data[CLASS_TABLE_OFF - RUNTIME_DATA_OFF..].copy_from_slice(&classes);
    Ok(data)
}

/// Reserve room for the overlay-side selector stub at the end of the caller's
/// requested relocation range. Translated strings may use the returned end.
pub fn translation_reloc_end(reloc_base: usize, reloc_end: usize) -> Result<usize> {
    let end = reloc_end
        .checked_sub(OVERLAY_STUB_LEN)
        .context("relocation end is smaller than the particle stub")?;
    if end <= reloc_base {
        bail!(
            "relocation band 0x{reloc_base:04X}..0x{reloc_end:04X} has no room for the particle stub"
        );
    }
    Ok(end)
}

/// Patch the two generic enemy-condition `は` sites and append their shared
/// near-call stub inside the proven relocation band.
pub fn install_topic_particle_selector(
    decoded: &mut Vec<u8>,
    load_offset: usize,
    reloc_base: usize,
    reloc_end: usize,
) -> Result<TopicPatchReport> {
    let stub_decoded_offset = decoded.len().max(reloc_base);
    let stub_end = stub_decoded_offset
        .checked_add(OVERLAY_STUB_LEN)
        .context("particle stub range overflow")?;
    if stub_end > reloc_end {
        bail!(
            "particle stub 0x{stub_decoded_offset:04X}..0x{stub_end:04X} exceeds relocation end 0x{reloc_end:04X}"
        );
    }
    decoded.resize(stub_end, 0);
    install_topic_particle_selector_at(decoded, load_offset, stub_decoded_offset)
}

/// Patch the two generic enemy-condition topic sites through an explicitly
/// owned, already allocated source-overlay slot.
pub fn install_topic_particle_selector_at(
    decoded: &mut [u8],
    load_offset: usize,
    stub_decoded_offset: usize,
) -> Result<TopicPatchReport> {
    let sites = decoded
        .windows(6)
        .enumerate()
        .filter(|(_, bytes)| bytes[..3] == ORIGINAL_TOPIC_MOV && bytes[3] == 0xE8)
        .map(|(offset, _)| offset)
        .collect::<Vec<_>>();
    if sites.len() != EXPECTED_TOPIC_SITES {
        bail!(
            "dynamic topic-particle pattern has {} sites, expected {EXPECTED_TOPIC_SITES}",
            sites.len()
        );
    }

    let direct_targets = sites
        .iter()
        .map(|site| near_call_target(decoded, *site + 3, load_offset))
        .collect::<Result<Vec<_>>>()?;
    if !direct_targets
        .iter()
        .all(|target| *target == direct_targets[0])
    {
        bail!("dynamic topic-particle sites call different direct-glyph renderers");
    }
    let direct_draw_target = direct_targets[0];
    if !APPROVED_DIRECT_DRAW_TARGETS.contains(&direct_draw_target) {
        bail!(
            "dynamic topic-particle direct-glyph target 0x{direct_draw_target:04X} is not an approved GAME_A/R/S renderer"
        );
    }

    let stub = crate::hook_geometry::topic_selector_stub();
    if stub.len() != OVERLAY_STUB_LEN {
        bail!(
            "particle stub is {} bytes, expected {OVERLAY_STUB_LEN}",
            stub.len()
        );
    }
    let stub_end = stub_decoded_offset
        .checked_add(stub.len())
        .context("particle stub range overflow")?;
    let target = decoded
        .get(stub_decoded_offset..stub_end)
        .context("particle stub source-slot reservation lies outside the overlay")?;
    if !target.iter().all(|byte| *byte == 0) {
        bail!(
            "particle stub source-slot reservation 0x{stub_decoded_offset:04X}..0x{stub_end:04X} is not empty"
        );
    }
    let stub_logical_offset = stub_decoded_offset
        .checked_add(load_offset)
        .context("particle stub logical offset overflow")?;
    if stub_logical_offset > u16::MAX as usize {
        bail!("particle stub logical offset 0x{stub_logical_offset:X} exceeds 16 bits");
    }

    let mut planned = decoded.to_vec();
    planned[stub_decoded_offset..stub_end].copy_from_slice(&stub);
    for site in &sites {
        let call_logical_end = site
            .checked_add(load_offset + 3)
            .context("particle call logical offset overflow")?;
        // 8086 rel16 calls wrap inside the current 64 KiB code segment. The
        // proven stub band is high enough that the shortest representation from
        // these low call sites crosses offset zero, so compute it modulo 16 bits.
        let displacement =
            (stub_logical_offset as u16).wrapping_sub(call_logical_end as u16) as i16;
        let call = encode_bytes(&Instruction::Call {
            target: CallTarget::Rel16(displacement),
        })
        .context("emit particle near call")?;
        if call.len() != ORIGINAL_TOPIC_MOV.len() {
            bail!("particle near call is not length-preserving");
        }
        planned[*site..*site + call.len()].copy_from_slice(&call);
    }

    for site in &sites {
        if near_call_target(&planned, *site, load_offset)? != stub_logical_offset as u16
            || near_call_target(&planned, *site + 3, load_offset)? != direct_draw_target
        {
            bail!("particle selector postcondition failed at decoded 0x{site:04X}");
        }
    }
    decoded.copy_from_slice(&planned);

    Ok(TopicPatchReport {
        sites,
        stub_decoded_offset,
        stub_logical_offset,
        direct_draw_target,
    })
}

fn renderer_jis(slot: usize) -> Result<u16> {
    if slot >= RENDERER_CAPACITY {
        bail!("renderer glyph slot {slot} exceeds capacity");
    }
    let row = RENDERER_BASE_ROW + (slot / CELLS_PER_ROW) as u8;
    let cell = 0x21 + (slot % CELLS_PER_ROW) as u8;
    if row >= RENDERER_BASE_ROW + RENDERER_ROWS as u8 {
        bail!("renderer glyph slot {slot} exceeds configured rows");
    }
    Ok(u16::from(row) << 8 | u16::from(cell))
}

fn near_call_target(decoded: &[u8], call_decoded_offset: usize, load_offset: usize) -> Result<u16> {
    let call = decoded
        .get(call_decoded_offset..call_decoded_offset + 3)
        .with_context(|| format!("near call at 0x{call_decoded_offset:04X} is truncated"))?;
    if call[0] != 0xE8 {
        bail!("expected near call at decoded 0x{call_decoded_offset:04X}");
    }
    let relative = i16::from_le_bytes([call[1], call[2]]);
    let next = call_decoded_offset
        .checked_add(load_offset + 3)
        .context("near-call next offset overflow")? as u16;
    Ok(next.wrapping_add_signed(relative))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_topic_site(decoded: &mut [u8], site: usize, load: usize, target: u16) {
        decoded[site..site + 3].copy_from_slice(&ORIGINAL_TOPIC_MOV);
        decoded[site + 3] = 0xE8;
        let next = (site + load + 6) as u16;
        let relative = target.wrapping_sub(next) as i16;
        decoded[site + 4..site + 6].copy_from_slice(&relative.to_le_bytes());
    }

    #[test]
    fn classifies_none_rieul_and_other_final() {
        assert_eq!(jongseong_class('가').unwrap(), JongseongClass::NoFinal);
        assert_eq!(jongseong_class('갈').unwrap(), JongseongClass::Rieul);
        assert_eq!(jongseong_class('각').unwrap(), JongseongClass::OtherFinal);
        assert!(jongseong_class('A').is_err());
    }

    #[test]
    fn runtime_data_contains_particle_codes_and_classes() {
        let json = r#"{
            "glyphs": [
                {"char":"가","slot":0,"jis":"0x7521"},
                {"char":"각","slot":1,"jis":"0x7522"},
                {"char":"갈","slot":2,"jis":"0x7523"},
                {"char":"는","slot":3,"jis":"0x7524"},
                {"char":"은","slot":4,"jis":"0x7525"},
                {"char":"를","slot":5,"jis":"0x7526"},
                {"char":"을","slot":6,"jis":"0x7527"},
                {"char":"이","slot":7,"jis":"0x7528"},
                {"char":"와","slot":8,"jis":"0x7529"},
                {"char":"과","slot":9,"jis":"0x752A"}
            ]
        }"#;
        let data = build_runtime_data_from_json(json).unwrap();
        assert_eq!(&data[..4], b"JOSA");
        assert_eq!(&data[4..6], &0x7526u16.to_le_bytes());
        assert_eq!(&data[6..8], &0x7527u16.to_le_bytes());
        assert_eq!(&data[8..10], &0x7521u16.to_le_bytes());
        assert_eq!(&data[10..12], &0x7528u16.to_le_bytes());
        assert_eq!(&data[12..14], &0x7524u16.to_le_bytes());
        assert_eq!(&data[14..16], &0x7525u16.to_le_bytes());
        assert_eq!(&data[16..18], &0x7529u16.to_le_bytes());
        assert_eq!(&data[18..20], &0x752Au16.to_le_bytes());
        assert_eq!(
            &data[CLASS_TABLE_OFF - RUNTIME_DATA_OFF..CLASS_TABLE_OFF - RUNTIME_DATA_OFF + 5],
            &[0, 2, 1, 2, 2]
        );
    }

    #[test]
    fn particle_markers_use_the_first_cp932_user_defined_row() {
        for index in 0..PARTICLE_MARKERS.len() {
            assert_eq!(
                particle_marker_sjis(index).unwrap(),
                [0xF0, 0x40 + index as u8]
            );
            assert_eq!(
                PARTICLE_MARKER_FIRST_JIS + index as u16,
                0x7F21 + index as u16
            );
        }
        assert!(particle_marker_sjis(PARTICLE_MARKERS.len()).is_err());
    }

    #[test]
    fn patches_exactly_two_topic_sites_and_preserves_direct_draw_calls() {
        let load = 0x100;
        let mut decoded = vec![0; 0x200];
        write_topic_site(&mut decoded, 0x20, load, 0x53F0);
        write_topic_site(&mut decoded, 0x80, load, 0x53F0);
        let report = install_topic_particle_selector(&mut decoded, load, 0x200, 0x300).unwrap();
        assert_eq!(report.sites, vec![0x20, 0x80]);
        assert_eq!(report.stub_decoded_offset, 0x200);
        assert_eq!(report.stub_logical_offset, 0x300);
        assert_eq!(report.direct_draw_target, 0x53F0);
        assert_eq!(
            &decoded[0x200..0x200 + OVERLAY_STUB_LEN],
            crate::hook_geometry::topic_selector_stub()
        );
        for site in report.sites {
            assert_eq!(near_call_target(&decoded, site, load).unwrap(), 0x300);
            assert_eq!(near_call_target(&decoded, site + 3, load).unwrap(), 0x53F0);
        }
    }

    #[test]
    fn refuses_partial_topic_site_coverage() {
        let load = 0x100;
        let mut decoded = vec![0; 0x200];
        write_topic_site(&mut decoded, 0x20, load, 0x53F0);
        let error = install_topic_particle_selector(&mut decoded, load, 0x200, 0x300).unwrap_err();
        assert!(error.to_string().contains("1 sites, expected 2"));
    }
}
