//! Text-plane cutscene probes for the character opening overlays.
//!
//! Unlike the in-game `GAME_*.OVL` renderer, the opening overlay consumes SJIS
//! and writes converted character codes directly to PC-98 TVRAM.  A BIOS gaiji
//! is therefore the smallest Hangul visibility probe for this path.

use std::collections::BTreeSet;

use anyhow::{Context, Result, bail};
use v30::{
    Assembler, Condition, EffectiveAddressBase, Instruction, JmpTarget, OperandSize, Register8,
    Register16, SegmentRegister, decode_bytes, encode_bytes,
};

use crate::cutscene_catalog::BIOS_GAIJI_CAPACITY;
use crate::hangul_probe::{
    GAIJI_GLYPH_BYTES, GAIJI_JIS_7621_SJIS, GAIJI_PATTERN_BYTES, GAIJI_PATTERN_HEADER, GaijiGlyph,
};
use crate::sjis_marker::{encode_sjis, replace_first_exact};
use crate::v30_assembler::{
    assemble_at, based_memory, direct_memory, imm8, imm16, mov, near_displacement, near_jump_patch,
    pop, push, reg8, reg16, segment,
};

pub const SHEZO_OPENING_OVERLAY: &str = "SHEZO_OP.OVL";
const SHEZO_OPENING_PREFIX: &str = "「みなさーん　ここが勇者様と";
const SHEZO_OPENING_LEADING_PUNCTUATION: &str = "「";
const OVERLAY_LOAD_OFFSET: usize = 0x100;
const STACK_RESERVE: usize = 0x2400;
const DEMO_STACK_TOP: usize = 0x2800;
const DEMO_MIN_STACK_RESERVE: usize = 0x1000;
pub const DAT_GAIJI_MAGIC: [u8; 8] = *b"KRGJ1ARS";
pub const DAT_GAIJI_SEGMENT: u16 = 0x8800;
pub const INTEGRATED_DAT_GAIJI_ALIGNMENT: usize = 0x100;
pub const ARLE_OP_PHASE_KEY: u16 = 0x375E;
pub const ARLE_CD_PHASE_KEY: u16 = 0x2AE8;
pub const ARLE_ED_PHASE_KEY: u16 = 0x337C;
const DEMO_PSP_PHASE_NAME: u16 = 0x0084;
const DEMO_PSP_PHASE_SUFFIX: u16 = 0x0086;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayGaijiStubReport {
    pub stub_decoded_offset: usize,
    pub prologue_decoded_offset: usize,
    pub consumer_decoded_offset: usize,
    pub static_end_logical: usize,
    pub stack_top: usize,
    pub glyphs: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatGaijiPhaseReport {
    pub selector: u16,
    pub table_offset: u16,
    pub glyphs: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatGaijiBlobReport {
    pub bytes: Vec<u8>,
    pub phases: Vec<DatGaijiPhaseReport>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegratedDatGaijiLayout {
    pub blob_offset: usize,
    pub blob_segment: u16,
    pub load_size: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DemoGaijiStubReport {
    pub stub_decoded_offset: usize,
    pub consumer_decoded_offset: usize,
    pub video_reset_decoded_offset: usize,
    pub bitmap_scratch_logical: u16,
    pub selector_scratch_logical: u16,
    pub table_segment: u16,
    pub static_end_logical: usize,
    pub stack_reserve: usize,
}

fn validate_gaiji_glyphs(glyphs: &[GaijiGlyph]) -> Result<()> {
    if glyphs.is_empty() {
        bail!("no phase-local gaiji glyphs to register");
    }
    if glyphs.len() > BIOS_GAIJI_CAPACITY {
        bail!(
            "phase-local set has {} glyphs, exceeding the BIOS capacity {BIOS_GAIJI_CAPACITY}",
            glyphs.len(),
        );
    }
    let mut slots = BTreeSet::new();
    for glyph in glyphs {
        let row = glyph.jis >> 8;
        let cell = glyph.jis & 0xFF;
        if !(0x76..=0x77).contains(&row) || !(0x21..=0x7E).contains(&cell) {
            bail!(
                "phase-local gaiji JIS 0x{:04X} is outside rows 0x76/0x77",
                glyph.jis
            );
        }
        if !slots.insert(glyph.jis) {
            bail!("phase-local gaiji JIS 0x{:04X} is duplicated", glyph.jis);
        }
    }
    Ok(())
}

/// Build one boot-loaded table file for the three Arle cutscene DAT phases.
/// Each descriptor uses the measured first-script offset as a stable phase key
/// and points to `(JIS, 32-byte bitmap)` entries in the selected table segment.
pub fn build_dat_gaiji_blob(phases: &[(u16, Vec<GaijiGlyph>)]) -> Result<DatGaijiBlobReport> {
    if phases.is_empty() {
        bail!("no Arle DAT gaiji phases");
    }
    let mut selectors = BTreeSet::new();
    for (selector, glyphs) in phases {
        if !selectors.insert(*selector) {
            bail!("duplicate Arle DAT phase selector 0x{selector:04X}");
        }
        validate_gaiji_glyphs(glyphs)?;
    }
    let descriptor_bytes = phases
        .len()
        .checked_mul(6)
        .context("Arle DAT phase descriptor size overflow")?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&DAT_GAIJI_MAGIC);
    bytes.extend_from_slice(&(phases.len() as u16).to_le_bytes());
    bytes.resize(bytes.len() + descriptor_bytes, 0);

    let mut reports = Vec::with_capacity(phases.len());
    for (index, (selector, glyphs)) in phases.iter().enumerate() {
        let table_offset = u16::try_from(bytes.len())
            .context("Arle DAT gaiji table starts outside the 16-bit load segment")?;
        for glyph in glyphs {
            bytes.extend_from_slice(&glyph.jis.to_le_bytes());
            bytes.extend_from_slice(&glyph.bitmap);
        }
        let descriptor = DAT_GAIJI_MAGIC.len() + 2 + index * 6;
        bytes[descriptor..descriptor + 2].copy_from_slice(&selector.to_le_bytes());
        bytes[descriptor + 2..descriptor + 4].copy_from_slice(&(glyphs.len() as u16).to_le_bytes());
        bytes[descriptor + 4..descriptor + 6].copy_from_slice(&table_offset.to_le_bytes());
        reports.push(DatGaijiPhaseReport {
            selector: *selector,
            table_offset,
            glyphs: glyphs.len(),
        });
    }
    u16::try_from(bytes.len()).context("Arle DAT gaiji blob exceeds 64 KiB")?;
    Ok(DatGaijiBlobReport {
        bytes,
        phases: reports,
    })
}

/// Place the Arle cutscene gaiji blob after the renderer sheet+hook image while
/// keeping both inside the runtime-proven conventional-RAM block. The blob is
/// paragraph-addressable through a segment alias, so the existing zero-based
/// descriptor offsets remain valid.
pub fn plan_integrated_dat_gaiji_layout(
    renderer_image_len: usize,
    dat_blob_len: usize,
) -> Result<IntegratedDatGaijiLayout> {
    use crate::hook_geometry::{
        HOOK_BLOCK_PHYSICAL_BASE, HOOK_BLOCK_PHYSICAL_END, HOOK_ENTRY_OFF, HOOK_SEG,
    };
    use crate::received_damage_sfx::S0_DAMAGE_TRACK_SCRATCH_OFF;

    if renderer_image_len <= HOOK_ENTRY_OFF as usize {
        bail!(
            "renderer image ends at 0x{renderer_image_len:04X}, before the hook payload at 0x{HOOK_ENTRY_OFF:04X}"
        );
    }
    if dat_blob_len == 0 {
        bail!("Arle DAT gaiji blob is empty");
    }
    let blob_offset = renderer_image_len
        .checked_add(INTEGRATED_DAT_GAIJI_ALIGNMENT - 1)
        .context("integrated gaiji alignment overflow")?
        & !(INTEGRATED_DAT_GAIJI_ALIGNMENT - 1);
    let total = blob_offset
        .checked_add(dat_blob_len)
        .context("integrated KFONT size overflow")?;
    let load_size = u16::try_from(total).context("integrated KFONT exceeds one DOS read")?;
    if total > usize::from(S0_DAMAGE_TRACK_SCRATCH_OFF) {
        bail!(
            "integrated KFONT reaches offset 0x{total:04X}, overlapping the received-damage S0 scratch at 0x{S0_DAMAGE_TRACK_SCRATCH_OFF:04X}"
        );
    }
    let paragraphs = u16::try_from(blob_offset / 16)
        .context("integrated gaiji segment displacement overflow")?;
    let blob_segment = HOOK_SEG
        .checked_add(paragraphs)
        .context("integrated gaiji segment overflow")?;
    let physical_end = HOOK_BLOCK_PHYSICAL_BASE
        .checked_add(total)
        .context("integrated gaiji physical range overflow")?;
    if physical_end > HOOK_BLOCK_PHYSICAL_END {
        bail!(
            "integrated KFONT reaches physical 0x{physical_end:05X}, beyond the proven block end 0x{HOOK_BLOCK_PHYSICAL_END:05X}"
        );
    }

    Ok(IntegratedDatGaijiLayout {
        blob_offset,
        blob_segment,
        load_size,
    })
}

fn locate_demo_dat_consumer(decoded: &[u8]) -> Result<(usize, [u8; 4])> {
    const SIGNATURE: [u8; 20] = [
        0xFC, 0xB8, 0x00, 0xA0, 0x8E, 0xC0, 0x2E, 0x8A, 0x1E, 0xD0, 0x12, 0xAC, 0x3C, 0x81, 0x72,
        0x0C, 0x3C, 0xA0, 0x72, 0x6A,
    ];
    let matches = decoded
        .windows(SIGNATURE.len())
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == SIGNATURE).then_some(offset))
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [offset] => Ok((*offset, SIGNATURE[..4].try_into().unwrap())),
        _ => bail!(
            "expected one DEMO.OVL DAT text consumer, found {}",
            matches.len()
        ),
    }
}

fn locate_demo_video_reset(decoded: &[u8]) -> Result<usize> {
    const SIGNATURE: [u8; 13] = [
        0xB0, 0x41, 0xE6, 0x6A, 0xB4, 0x12, 0xCD, 0x18, 0xC3, 0xB0, 0x0C, 0xE6, 0x62,
    ];
    let matches = decoded
        .windows(SIGNATURE.len())
        .enumerate()
        .filter_map(|(offset, bytes)| (bytes == SIGNATURE).then_some(offset + 4))
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [offset] => Ok(*offset),
        _ => bail!(
            "expected one DEMO.OVL video-reset routine, found {}",
            matches.len()
        ),
    }
}

fn assemble_demo_dat_gaiji_code(
    stub_ip: u16,
    table_segment: u16,
    bitmap_scratch: u16,
    selector_scratch: u16,
) -> Result<Vec<u8>> {
    let mut assembler = Assembler::new();
    assembler
        .emit(mov(reg8(Register8::AH), imm8(0x12)))
        .emit(Instruction::Int { vector: 0x18 })
        .emit(Instruction::Pushf)
        .emit(Instruction::Pusha)
        .emit(push(segment(SegmentRegister::DS)))
        .emit(push(segment(SegmentRegister::ES)))
        .emit(Instruction::Cmp {
            a: direct_memory(
                Some(SegmentRegister::CS),
                DEMO_PSP_PHASE_SUFFIX,
                OperandSize::Word,
            ),
            b: imm16(0x415F),
        })
        .emit_branch(Condition::Ne, "invalid_phase")
        .emit(Instruction::Cmp {
            a: direct_memory(
                Some(SegmentRegister::CS),
                DEMO_PSP_PHASE_NAME,
                OperandSize::Word,
            ),
            b: imm16(0x504F),
        })
        .emit_branch(Condition::Ne, "check_cd")
        .emit(mov(reg16(Register16::DX), imm16(ARLE_OP_PHASE_KEY)))
        .emit_jump_short("phase_ready")
        .label("check_cd")
        .emit(Instruction::Cmp {
            a: direct_memory(
                Some(SegmentRegister::CS),
                DEMO_PSP_PHASE_NAME,
                OperandSize::Word,
            ),
            b: imm16(0x4443),
        })
        .emit_branch(Condition::Ne, "check_ed")
        .emit(mov(reg16(Register16::DX), imm16(ARLE_CD_PHASE_KEY)))
        .emit_jump_short("phase_ready")
        .label("check_ed")
        .emit(Instruction::Cmp {
            a: direct_memory(
                Some(SegmentRegister::CS),
                DEMO_PSP_PHASE_NAME,
                OperandSize::Word,
            ),
            b: imm16(0x4445),
        })
        .emit_branch(Condition::Ne, "invalid_phase")
        .emit(mov(reg16(Register16::DX), imm16(ARLE_ED_PHASE_KEY)))
        .emit_jump_short("phase_ready")
        .label("invalid_phase")
        .emit_jump_near("done")
        .label("phase_ready")
        .emit(mov(
            direct_memory(
                Some(SegmentRegister::CS),
                selector_scratch,
                OperandSize::Word,
            ),
            reg16(Register16::DX),
        ))
        .emit(mov(reg16(Register16::AX), imm16(table_segment)))
        .emit(mov(segment(SegmentRegister::DS), reg16(Register16::AX)));
    for (index, word) in DAT_GAIJI_MAGIC
        .as_chunks::<2>()
        .0
        .iter()
        .map(|bytes| u16::from_le_bytes(*bytes))
        .enumerate()
    {
        assembler
            .emit(Instruction::Cmp {
                a: direct_memory(None, (index * 2) as u16, OperandSize::Word),
                b: imm16(word),
            })
            .emit_branch(Condition::Ne, "done");
    }
    assembler
        .emit(mov(
            reg16(Register16::BP),
            direct_memory(None, 8, OperandSize::Word),
        ))
        .emit(mov(reg16(Register16::SI), imm16(10)))
        .label("find_phase")
        .emit(Instruction::Test {
            a: reg16(Register16::BP),
            b: reg16(Register16::BP),
        })
        .emit_branch(Condition::E, "done")
        .emit(Instruction::Cmp {
            a: reg16(Register16::DX),
            b: based_memory(None, EffectiveAddressBase::Si, 0, OperandSize::Word),
        })
        .emit_branch(Condition::E, "found")
        .emit(Instruction::Add {
            dest: reg16(Register16::SI),
            src: imm16(6),
        })
        .emit(Instruction::Dec {
            dest: reg16(Register16::BP),
        })
        .emit_jump_short("find_phase")
        .label("found")
        .emit(mov(
            direct_memory(
                Some(SegmentRegister::CS),
                selector_scratch.wrapping_add(2),
                OperandSize::Word,
            ),
            reg16(Register16::DX),
        ))
        .emit(mov(
            reg16(Register16::BP),
            based_memory(None, EffectiveAddressBase::Si, 2, OperandSize::Word),
        ))
        .emit(mov(
            reg16(Register16::SI),
            based_memory(None, EffectiveAddressBase::Si, 4, OperandSize::Word),
        ))
        .emit(Instruction::Test {
            a: reg16(Register16::BP),
            b: reg16(Register16::BP),
        })
        .emit_branch(Condition::E, "done")
        .label("register")
        .emit(push(reg16(Register16::SI)))
        .emit(push(reg16(Register16::BP)))
        .emit(mov(
            reg16(Register16::DX),
            based_memory(None, EffectiveAddressBase::Si, 0, OperandSize::Word),
        ))
        .emit(push(reg16(Register16::DX)))
        .emit(Instruction::Add {
            dest: reg16(Register16::SI),
            src: imm16(2),
        })
        .emit(push(segment(SegmentRegister::CS)))
        .emit(pop(segment(SegmentRegister::ES)))
        .emit(mov(
            reg16(Register16::DI),
            imm16(bitmap_scratch.wrapping_add(GAIJI_PATTERN_HEADER.len() as u16)),
        ))
        .emit(mov(reg16(Register16::CX), imm16(16)))
        .emit(Instruction::Cld)
        .emit(Instruction::Rep(Box::new(Instruction::Movsw)))
        .emit(pop(reg16(Register16::DX)))
        .emit(push(segment(SegmentRegister::CS)))
        .emit(pop(reg16(Register16::BX)))
        .emit(mov(reg16(Register16::CX), imm16(bitmap_scratch)))
        .emit(mov(reg8(Register8::AH), imm8(0x1A)))
        .emit(Instruction::Int { vector: 0x18 })
        .emit(pop(reg16(Register16::BP)))
        .emit(pop(reg16(Register16::SI)))
        .emit(mov(reg16(Register16::AX), imm16(table_segment)))
        .emit(mov(segment(SegmentRegister::DS), reg16(Register16::AX)))
        .emit(Instruction::Add {
            dest: reg16(Register16::SI),
            src: imm16(GAIJI_PATTERN_BYTES as u16),
        })
        .emit(Instruction::Dec {
            dest: reg16(Register16::BP),
        })
        .emit_branch(Condition::Ne, "register")
        .label("done")
        .emit(pop(segment(SegmentRegister::ES)))
        .emit(pop(segment(SegmentRegister::DS)))
        .emit(Instruction::Popa)
        .emit(Instruction::Popf)
        .emit(Instruction::Ret { pop: 0 });
    assemble_at(&assembler, stub_ip, "DEMO.OVL DAT gaiji hook")
}

/// Redirect the shared Arle `DEMO.OVL` video-reset routine through a gaiji
/// registration stub. Registering inside the DAT consumer stalls the DEMO
/// task after its first character, so the hook replays `INT 18h, AH=12h` and
/// performs registration before returning from video setup instead.
///
/// The DOS PSP command tail at `CS:0x84` holds `OP_A`, `CD_A`, or `ED_A`.
/// That stable phase tag selects a descriptor from the boot-loaded
/// [`DAT_GAIJI_MAGIC`] table segment. Each bitmap is copied to a local `CS`
/// buffer before the BIOS call, matching the proven overlay call shape.
pub fn install_demo_dat_gaiji_registration(decoded: &mut Vec<u8>) -> Result<DemoGaijiStubReport> {
    install_demo_dat_gaiji_registration_from_segment(decoded, DAT_GAIJI_SEGMENT)
}

pub fn install_demo_dat_gaiji_registration_from_segment(
    decoded: &mut Vec<u8>,
    table_segment: u16,
) -> Result<DemoGaijiStubReport> {
    if table_segment >= 0xA000 {
        bail!("Arle DAT gaiji table segment 0x{table_segment:04X} is outside conventional RAM");
    }
    let (consumer_decoded_offset, _) = locate_demo_dat_consumer(decoded)?;
    let video_reset_decoded_offset = locate_demo_video_reset(decoded)?;
    let video_reset_ip = u16::try_from(video_reset_decoded_offset + OVERLAY_LOAD_OFFSET)
        .context("DEMO.OVL video-reset offset exceeds the 16-bit segment")?;
    let stub_decoded_offset = decoded.len();
    let stub_ip = u16::try_from(stub_decoded_offset + OVERLAY_LOAD_OFFSET)
        .context("DEMO.OVL gaiji stub starts outside the 16-bit segment")?;

    let provisional = assemble_demo_dat_gaiji_code(stub_ip, table_segment, 0, 0)?;
    let bitmap_scratch_logical = stub_ip.wrapping_add(provisional.len() as u16);
    let selector_scratch_logical = bitmap_scratch_logical.wrapping_add(GAIJI_PATTERN_BYTES as u16);
    let mut stub = assemble_demo_dat_gaiji_code(
        stub_ip,
        table_segment,
        bitmap_scratch_logical,
        selector_scratch_logical,
    )?;
    if stub.len() != provisional.len() {
        bail!("typed DEMO.OVL DAT gaiji hook changed length after scratch placement");
    }
    stub.extend_from_slice(&GAIJI_PATTERN_HEADER);
    stub.resize(stub.len() + GAIJI_GLYPH_BYTES, 0);
    stub.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
    let static_end_logical = stub_decoded_offset + stub.len() + OVERLAY_LOAD_OFFSET;
    let stack_reserve = DEMO_STACK_TOP
        .checked_sub(static_end_logical)
        .context("DEMO.OVL gaiji stub crosses its fixed stack top")?;
    if stack_reserve < DEMO_MIN_STACK_RESERVE {
        bail!("DEMO.OVL gaiji stub leaves only 0x{stack_reserve:04X} bytes below its fixed stack");
    }

    let hijack = near_jump_patch(video_reset_ip, stub_ip, 5, "DEMO.OVL video-reset hijack")?;
    decoded[video_reset_decoded_offset..video_reset_decoded_offset + hijack.len()]
        .copy_from_slice(&hijack);
    decoded.extend_from_slice(&stub);

    Ok(DemoGaijiStubReport {
        stub_decoded_offset,
        consumer_decoded_offset,
        video_reset_decoded_offset,
        bitmap_scratch_logical,
        selector_scratch_logical,
        table_segment,
        static_end_logical,
        stack_reserve,
    })
}

fn locate_overlay_prologue(decoded: &[u8]) -> Result<usize> {
    const PREFIX: [u8; 5] = [0x8C, 0xC8, 0x8E, 0xD8, 0xBB];
    const SUFFIX: [u8; 10] = [0x83, 0xC3, 0x0F, 0x83, 0xE3, 0xF0, 0x81, 0xC3, 0x00, 0x24];
    let matches = decoded
        .windows(17)
        .enumerate()
        .filter_map(|(offset, bytes)| {
            (bytes[..5] == PREFIX && bytes[7..] == SUFFIX).then_some(offset)
        })
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [offset] => Ok(*offset),
        _ => bail!(
            "expected one cutscene overlay entry/stack prologue, found {}",
            matches.len()
        ),
    }
}

fn locate_text_consumer(decoded: &[u8]) -> Result<(usize, [u8; 4])> {
    let matches = decoded
        .windows(13)
        .enumerate()
        .filter(|(_, bytes)| {
            bytes[..2] == [0x8B, 0x36]
                && bytes[4..9] == [0xAC, 0x8A, 0xE0, 0x89, 0x36]
                && bytes[2..4] == bytes[9..11]
                && bytes[11..13] == [0x3C, 0x24]
        })
        .map(|(offset, bytes)| (offset, [bytes[0], bytes[1], bytes[2], bytes[3]]))
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [found] => Ok(*found),
        _ => bail!(
            "expected one cutscene decoded-SJIS consumer, found {}",
            matches.len()
        ),
    }
}

fn assemble_overlay_gaiji_code(
    stub_ip: u16,
    glyph_base_ip: u16,
    glyphs: &[GaijiGlyph],
    consumer_ip: u16,
    consumer_displaced: &[u8; 4],
) -> Result<Vec<u8>> {
    let mut assembler = Assembler::new();
    assembler
        .emit(Instruction::Pushf)
        .emit(Instruction::Pusha)
        .emit(push(segment(SegmentRegister::DS)))
        .emit(push(segment(SegmentRegister::ES)))
        .emit(push(segment(SegmentRegister::CS)))
        .emit(pop(segment(SegmentRegister::DS)))
        .emit(push(segment(SegmentRegister::CS)))
        .emit(pop(reg16(Register16::BX)));
    for (address, bytes) in [
        (consumer_ip, &consumer_displaced[..2]),
        (consumer_ip + 2, &consumer_displaced[2..]),
    ] {
        assembler.emit(mov(
            direct_memory(None, address, OperandSize::Word),
            imm16(u16::from_le_bytes(
                bytes.try_into().expect("two displaced bytes"),
            )),
        ));
    }
    for (index, glyph) in glyphs.iter().enumerate() {
        let glyph_ip = glyph_base_ip.wrapping_add((index * GAIJI_PATTERN_BYTES) as u16);
        assembler
            .emit(mov(reg16(Register16::CX), imm16(glyph_ip)))
            .emit(mov(reg16(Register16::DX), imm16(glyph.jis)))
            .emit(mov(reg8(Register8::AH), imm8(0x1A)))
            .emit(Instruction::Int { vector: 0x18 });
    }
    assembler
        .emit(pop(segment(SegmentRegister::ES)))
        .emit(pop(segment(SegmentRegister::DS)))
        .emit(Instruction::Popa)
        .emit(Instruction::Popf);

    let displaced = decode_bytes(consumer_displaced)
        .context("decode displaced cutscene consumer instruction")?;
    if displaced.byte_len != consumer_displaced.len()
        || displaced.source_bytes() != consumer_displaced
    {
        bail!("cutscene consumer displacement is not one canonical V30 instruction");
    }
    assembler.emit(displaced.instruction);
    let prefix = assemble_at(&assembler, stub_ip, "cutscene overlay gaiji hook")?;
    let resume_jump_ip = stub_ip.wrapping_add(prefix.len() as u16);
    assembler.emit(Instruction::Jmp {
        target: JmpTarget::Rel16(near_displacement(
            resume_jump_ip,
            consumer_ip + consumer_displaced.len() as u16,
        )),
    });
    assemble_at(&assembler, stub_ip, "cutscene overlay gaiji hook")
}

fn patch_overlay_static_end(decoded: &mut [u8], prologue_offset: usize, value: u16) -> Result<()> {
    let instruction_offset = prologue_offset + 4;
    let original = decode_bytes(&decoded[instruction_offset..])
        .context("decode cutscene overlay static-end instruction")?;
    if !matches!(
        original.instruction,
        Instruction::Mov {
            dest: v30::Operand::Reg16(Register16::BX),
            src: v30::Operand::Imm16(_),
        }
    ) {
        bail!("cutscene overlay static-end instruction is not mov bx,imm16");
    }
    let replacement = encode_bytes(&mov(reg16(Register16::BX), imm16(value)))
        .context("encode cutscene overlay static-end instruction")?;
    if replacement.len() != original.byte_len {
        bail!("cutscene overlay static-end replacement changed instruction length");
    }
    decoded[instruction_offset..instruction_offset + replacement.len()]
        .copy_from_slice(&replacement);
    Ok(())
}

/// Wait for the last battle sample before ENDING_R asks MAIN to resume music.
/// MAIN's AH=2/INT 7Ah skips resume while a sample is still active; the ending
/// never retries it. AH=8/INT 7Dh waits for both sample and timer-hook completion.
pub fn install_rulue_ending_sample_wait(decoded: &mut Vec<u8>) -> Result<usize> {
    if decoded.get(..7) != Some(&[0xB4, 0x02, 0xCD, 0x7A, 0xBA, 0xFE, 0x01]) {
        bail!("Rulue ending music-resume entry changed");
    }
    let prologue = locate_overlay_prologue(decoded)?;
    if prologue != 0x23 {
        bail!("Rulue ending stack prologue changed");
    }
    let offset = decoded.len();
    let stub_ip = u16::try_from(offset + OVERLAY_LOAD_OFFSET)
        .context("Rulue ending sample-wait stub exceeds segment")?;
    let mut assembler = Assembler::new();
    assembler
        .emit(mov(reg8(Register8::AH), imm8(8)))
        .emit(Instruction::Int { vector: 0x7D })
        .emit(mov(reg8(Register8::AH), imm8(2)))
        .emit(Instruction::Int { vector: 0x7A });
    let prefix = assemble_at(&assembler, stub_ip, "Rulue ending sample wait")?;
    let jump_ip = stub_ip
        .checked_add(prefix.len() as u16)
        .context("Rulue ending resume jump exceeds segment")?;
    assembler.emit(Instruction::Jmp {
        target: JmpTarget::Rel16(near_displacement(jump_ip, 0x104)),
    });
    let stub = assemble_at(&assembler, stub_ip, "Rulue ending sample wait")?;
    let end = offset + OVERLAY_LOAD_OFFSET + stub.len();
    if ((end + 0x0F) & !0x0F) + STACK_RESERVE > u16::MAX as usize {
        bail!("Rulue ending sample wait leaves no room for the overlay stack");
    }
    let hijack = near_jump_patch(0x100, stub_ip, 4, "Rulue ending entry")?;
    patch_overlay_static_end(decoded, prologue, end as u16)?;
    decoded[..4].copy_from_slice(&hijack);
    decoded.extend_from_slice(&stub);
    Ok(offset)
}

/// Append a one-shot, phase-local BIOS-gaiji registration stub to an executable
/// cutscene overlay and redirect the decoded-SJIS consumer through it. The
/// overlay invokes `INT 18h, AH=12h` during video setup, so registering at entry
/// is too early: the first text-consumer call is the stable post-setup boundary.
/// The stub restores the four displaced consumer bytes before registering, then
/// preserves the caller's registers and continues at the original instruction.
///
/// The verified overlay prologue computes SP from `mov bx,static_end`; its
/// immediate is raised to the end of the appended code and bitmaps before the
/// original alignment and `+0x2400` stack setup.
pub fn install_overlay_gaiji_registration(
    decoded: &mut Vec<u8>,
    glyphs: &[GaijiGlyph],
) -> Result<OverlayGaijiStubReport> {
    validate_gaiji_glyphs(glyphs)?;
    let prologue_decoded_offset = locate_overlay_prologue(decoded)?;
    let (consumer_decoded_offset, consumer_displaced) = locate_text_consumer(decoded)?;

    let stub_decoded_offset = decoded.len();
    let stub_logical = stub_decoded_offset
        .checked_add(OVERLAY_LOAD_OFFSET)
        .context("overlay stub logical offset overflow")?;
    let stub_ip = u16::try_from(stub_logical)
        .context("overlay gaiji stub starts outside the 16-bit segment")?;
    let consumer_ip = u16::try_from(consumer_decoded_offset + OVERLAY_LOAD_OFFSET)
        .context("cutscene consumer offset exceeds the 16-bit segment")?;
    let provisional =
        assemble_overlay_gaiji_code(stub_ip, 0, glyphs, consumer_ip, &consumer_displaced)?;
    let glyph_base_ip = stub_ip.wrapping_add(provisional.len() as u16);
    let mut stub = assemble_overlay_gaiji_code(
        stub_ip,
        glyph_base_ip,
        glyphs,
        consumer_ip,
        &consumer_displaced,
    )?;
    if stub.len() != provisional.len() {
        bail!("typed cutscene overlay gaiji hook changed length after glyph placement");
    }
    for glyph in glyphs {
        stub.extend_from_slice(&GAIJI_PATTERN_HEADER);
        stub.extend_from_slice(&glyph.bitmap);
    }
    let static_end_logical = stub_logical
        .checked_add(stub.len())
        .context("overlay gaiji blob size overflow")?;
    let static_end = u16::try_from(static_end_logical)
        .context("overlay gaiji blob ends outside the 16-bit segment")?;
    let stack_top = static_end_logical
        .checked_add(0x0F)
        .map(|end| end & !0x0F)
        .and_then(|end| end.checked_add(STACK_RESERVE))
        .context("overlay stack-top calculation overflow")?;
    if stack_top > u16::MAX as usize {
        bail!(
            "phase-local gaiji blob ends at 0x{static_end_logical:04X}; the overlay's +0x{STACK_RESERVE:04X} stack would exceed the segment"
        );
    }

    patch_overlay_static_end(decoded, prologue_decoded_offset, static_end)?;
    let hijack = near_jump_patch(
        consumer_ip,
        stub_ip,
        consumer_displaced.len(),
        "cutscene text-consumer hijack",
    )?;
    decoded[consumer_decoded_offset..consumer_decoded_offset + hijack.len()]
        .copy_from_slice(&hijack);
    decoded.extend_from_slice(&stub);

    Ok(OverlayGaijiStubReport {
        stub_decoded_offset,
        prologue_decoded_offset,
        consumer_decoded_offset,
        static_end_logical,
        stack_top,
        glyphs: glyphs.len(),
    })
}

/// Replace the first visible `み` in Schezo's opening narration with the gaiji
/// code registered as Hangul `가` by [`crate::hangul_probe`].  The replacement
/// is exactly two bytes, so the script delimiters (`\r` and `$`) and every
/// following byte retain their original offsets.
///
/// Returns the decoded-overlay offset of the replaced two-byte glyph.
pub fn patch_shezo_opening_one_gaiji(decoded: &mut [u8]) -> Result<usize> {
    let from = encode_sjis(SHEZO_OPENING_PREFIX)?;
    let mut to = from.clone();
    let glyph_in_prefix = encode_sjis(SHEZO_OPENING_LEADING_PUNCTUATION)?.len();
    let glyph_end = glyph_in_prefix + GAIJI_JIS_7621_SJIS.len();
    if glyph_end > to.len() {
        bail!("Schezo opening probe prefix is shorter than its gaiji slot");
    }
    to[glyph_in_prefix..glyph_end].copy_from_slice(&GAIJI_JIS_7621_SJIS);

    let matches = decoded.windows(from.len()).filter(|w| *w == from).count();
    if matches != 1 {
        bail!("expected one Schezo opening prefix, found {matches}");
    }
    let prefix_offset = replace_first_exact(decoded, &from, &to)?;
    Ok(prefix_offset + glyph_in_prefix)
}

#[cfg(test)]
mod tests {
    use super::{
        DAT_GAIJI_MAGIC, DAT_GAIJI_SEGMENT, SHEZO_OPENING_PREFIX, build_dat_gaiji_blob,
        install_demo_dat_gaiji_registration, install_demo_dat_gaiji_registration_from_segment,
        install_overlay_gaiji_registration, patch_shezo_opening_one_gaiji,
        plan_integrated_dat_gaiji_layout,
    };
    use crate::hangul_probe::{
        GAIJI_GLYPH_BYTES, GAIJI_JIS_7621_SJIS, GAIJI_PATTERN_BYTES, GAIJI_PATTERN_HEADER,
        GaijiGlyph,
    };
    use crate::sjis_marker::encode_sjis;

    #[test]
    fn cutscene_probe_replaces_only_the_first_visible_glyph() {
        let prefix = encode_sjis(SHEZO_OPENING_PREFIX).unwrap();
        let mut decoded = vec![0xCC; 7];
        decoded.extend_from_slice(&prefix);
        decoded.extend_from_slice(&[0x0D, 0x24, 0xAA]);
        let before_len = decoded.len();

        let glyph_offset = patch_shezo_opening_one_gaiji(&mut decoded).unwrap();

        assert_eq!(glyph_offset, 7 + encode_sjis("「").unwrap().len());
        assert_eq!(
            &decoded[glyph_offset..glyph_offset + 2],
            &GAIJI_JIS_7621_SJIS
        );
        assert_eq!(decoded.len(), before_len);
        assert_eq!(&decoded[decoded.len() - 3..], &[0x0D, 0x24, 0xAA]);
    }

    #[test]
    fn cutscene_probe_rejects_an_ambiguous_prefix() {
        let prefix = encode_sjis(SHEZO_OPENING_PREFIX).unwrap();
        let mut decoded = prefix.clone();
        decoded.extend_from_slice(&prefix);
        assert!(patch_shezo_opening_one_gaiji(&mut decoded).is_err());
    }

    fn executable_overlay(size: usize) -> Vec<u8> {
        const CONSUMER: usize = 0x4B4F;
        const DISPLACED: [u8; 4] = [0x8B, 0x36, 0x82, 0x4F];
        const SUFFIX: [u8; 9] = [0xAC, 0x8A, 0xE0, 0x89, 0x36, 0x82, 0x4F, 0x3C, 0x24];
        let mut decoded = vec![0u8; size];
        decoded[..17].copy_from_slice(&[
            0x8C, 0xC8, 0x8E, 0xD8, 0xBB, 0x00, 0x20, 0x83, 0xC3, 0x0F, 0x83, 0xE3, 0xF0, 0x81,
            0xC3, 0x00, 0x24,
        ]);
        decoded[CONSUMER..CONSUMER + 4].copy_from_slice(&DISPLACED);
        decoded[CONSUMER + 4..CONSUMER + 13].copy_from_slice(&SUFFIX);
        decoded
    }

    #[test]
    fn overlay_stub_registers_glyph_and_moves_stack_above_appended_data() {
        let mut decoded = executable_overlay(0x5000);
        let original_len = decoded.len();
        let glyph = GaijiGlyph {
            jis: 0x7621,
            bitmap: [0xA5; GAIJI_GLYPH_BYTES],
        };

        let report = install_overlay_gaiji_registration(&mut decoded, &[glyph]).unwrap();

        assert_eq!(report.stub_decoded_offset, original_len);
        assert_eq!(report.prologue_decoded_offset, 0);
        assert_eq!(report.consumer_decoded_offset, 0x4B4F);
        assert_eq!(report.glyphs, 1);
        assert!(report.stack_top > report.static_end_logical);
        assert_eq!(decoded[0], 0x8C);
        assert_eq!(
            u16::from_le_bytes([decoded[5], decoded[6]]) as usize,
            report.static_end_logical
        );
        assert_eq!(decoded[report.consumer_decoded_offset], 0xE9);
        assert_eq!(decoded[report.consumer_decoded_offset + 3], 0x90);
        let stub = &decoded[original_len..];
        assert_eq!(
            &stub[..8],
            &[0x9C, 0x60, 0x1E, 0x06, 0x0E, 0x1F, 0x0E, 0x5B]
        );
        assert!(
            stub.windows(4)
                .any(|bytes| { bytes == [0x8B, 0x36, 0x82, 0x4F] })
        );
        assert_eq!(
            &decoded[decoded.len() - GAIJI_PATTERN_BYTES..],
            &[&GAIJI_PATTERN_HEADER[..], &[0xA5; GAIJI_GLYPH_BYTES]].concat()
        );
    }

    #[test]
    fn overlay_stub_rejects_a_stack_that_would_cross_the_segment() {
        let mut decoded = executable_overlay(0xDC00);
        let glyph = GaijiGlyph {
            jis: 0x7621,
            bitmap: [0; GAIJI_GLYPH_BYTES],
        };

        let error = install_overlay_gaiji_registration(&mut decoded, &[glyph]).unwrap_err();

        assert!(error.to_string().contains("stack would exceed"));
    }

    #[test]
    fn dat_blob_serializes_phase_descriptor_jis_and_bitmap_entries() {
        let glyphs = [
            GaijiGlyph {
                jis: 0x7621,
                bitmap: [0x11; GAIJI_GLYPH_BYTES],
            },
            GaijiGlyph {
                jis: 0x7622,
                bitmap: [0x22; GAIJI_GLYPH_BYTES],
            },
        ];

        let report = build_dat_gaiji_blob(&[(0x375E, glyphs.to_vec())]).unwrap();

        assert_eq!(report.phases.len(), 1);
        assert_eq!(report.phases[0].selector, 0x375E);
        assert_eq!(report.phases[0].table_offset, 0x0010);
        assert_eq!(report.phases[0].glyphs, 2);
        assert_eq!(&report.bytes[..8], &DAT_GAIJI_MAGIC);
        assert_eq!(&report.bytes[8..10], &1u16.to_le_bytes());
        assert_eq!(&report.bytes[10..12], &0x375Eu16.to_le_bytes());
        assert_eq!(&report.bytes[12..14], &2u16.to_le_bytes());
        assert_eq!(&report.bytes[14..16], &0x0010u16.to_le_bytes());
        assert_eq!(&report.bytes[16..18], &0x7621u16.to_le_bytes());
        assert_eq!(&report.bytes[18..50], &[0x11; GAIJI_GLYPH_BYTES]);
        assert_eq!(&report.bytes[50..52], &0x7622u16.to_le_bytes());
        assert_eq!(&report.bytes[52..], &[0x22; GAIJI_GLYPH_BYTES]);
    }

    #[test]
    fn integrated_dat_blob_starts_after_the_renderer_hook_in_the_proven_block() {
        let layout = plan_integrated_dat_gaiji_layout(0x8096, 0x20C8).unwrap();

        assert_eq!(layout.blob_offset, 0x8100);
        assert_eq!(layout.blob_segment, 0x9010);
        assert_eq!(layout.load_size, 0xA1C8);
        assert!(plan_integrated_dat_gaiji_layout(0x9000, 0x3000).is_err());
        assert!(plan_integrated_dat_gaiji_layout(0xF000, 0x2000).is_err());
    }

    fn executable_demo(size: usize) -> Vec<u8> {
        const CONSUMER: usize = 0x023F;
        const VIDEO_RESET_SIGNATURE: usize = 0x108F;
        const SIGNATURE: [u8; 20] = [
            0xFC, 0xB8, 0x00, 0xA0, 0x8E, 0xC0, 0x2E, 0x8A, 0x1E, 0xD0, 0x12, 0xAC, 0x3C, 0x81,
            0x72, 0x0C, 0x3C, 0xA0, 0x72, 0x6A,
        ];
        let mut decoded = vec![0u8; size];
        decoded[CONSUMER..CONSUMER + SIGNATURE.len()].copy_from_slice(&SIGNATURE);
        decoded[VIDEO_RESET_SIGNATURE..VIDEO_RESET_SIGNATURE + 13].copy_from_slice(&[
            0xB0, 0x41, 0xE6, 0x6A, 0xB4, 0x12, 0xCD, 0x18, 0xC3, 0xB0, 0x0C, 0xE6, 0x62,
        ]);
        decoded
    }

    #[test]
    fn demo_stub_hooks_video_reset_before_the_dat_consumer_with_stack_headroom() {
        let mut decoded = executable_demo(0x1600);
        let original_len = decoded.len();

        let report = install_demo_dat_gaiji_registration(&mut decoded).unwrap();

        assert_eq!(report.stub_decoded_offset, original_len);
        assert_eq!(report.consumer_decoded_offset, 0x023F);
        assert_eq!(report.video_reset_decoded_offset, 0x1093);
        assert!(report.stack_reserve >= 0x1000);
        assert_eq!(decoded[report.consumer_decoded_offset], 0xFC);
        assert_eq!(decoded[report.video_reset_decoded_offset], 0xE9);
        assert_eq!(
            &decoded[report.video_reset_decoded_offset + 3..report.video_reset_decoded_offset + 5],
            &[0x90, 0x90]
        );
        let stub = &decoded[report.stub_decoded_offset..];
        assert_eq!(
            &stub[..8],
            &[0xB4, 0x12, 0xCD, 0x18, 0x9C, 0x60, 0x1E, 0x06]
        );
        assert_eq!(
            report.selector_scratch_logical,
            report.bitmap_scratch_logical + GAIJI_PATTERN_BYTES as u16
        );
        assert_eq!(report.table_segment, DAT_GAIJI_SEGMENT);
        assert_eq!(
            &decoded[report.bitmap_scratch_logical as usize - super::OVERLAY_LOAD_OFFSET
                ..report.selector_scratch_logical as usize - super::OVERLAY_LOAD_OFFSET],
            &[&GAIJI_PATTERN_HEADER[..], &[0; GAIJI_GLYPH_BYTES]].concat()
        );
        assert!(stub.windows(2).any(|bytes| bytes == [0xF3, 0xA5]));
        assert!(
            stub.windows(2)
                .any(|bytes| bytes == 0x524Bu16.to_le_bytes())
        );
    }

    #[test]
    fn demo_stub_can_read_an_integrated_table_through_a_segment_alias() {
        let mut decoded = executable_demo(0x1600);

        let report =
            install_demo_dat_gaiji_registration_from_segment(&mut decoded, 0x9010).unwrap();

        assert_eq!(report.table_segment, 0x9010);
        let stub = &decoded[report.stub_decoded_offset..];
        assert_eq!(
            stub.windows(3)
                .filter(|bytes| *bytes == [0xB8, 0x10, 0x90])
                .count(),
            2
        );
    }

    #[test]
    fn demo_stub_rejects_an_ambiguous_consumer_without_mutating() {
        let mut decoded = executable_demo(0x1600);
        let signature = decoded[0x023F..0x0253].to_vec();
        decoded[0x0800..0x0814].copy_from_slice(&signature);
        let before = decoded.clone();

        let error = install_demo_dat_gaiji_registration(&mut decoded).unwrap_err();

        assert!(error.to_string().contains("found 2"));
        assert_eq!(decoded, before);
    }
}
