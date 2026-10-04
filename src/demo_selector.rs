//! Shared renderer-image loading at the normal Demo scenario handoff.
//!
//! The stock selector remains resident from the Demo disk and does not execute
//! a character Game disk's patched `MAIN.COM`. Its one A/R/S handoff owns the
//! cross-media responsibility: after the requested Game disk is mounted, load
//! that disk's `KFONT.BIN` before returning to the unchanged overlay-selection
//! sequence.

use anyhow::{Context, Result, bail};
use v30::{
    Assembler, CallTarget, Condition, EffectiveAddressBase, Instruction, OperandSize, Register8,
    Register16, SegmentRegister,
};

use crate::hook_geometry::HOOK_SEG;
use crate::v30_assembler::{
    assemble_at, based_memory, imm16, mov, near_displacement, pop, push, reg8, reg16, segment,
};

pub const SELECTOR_OVERLAY: &str = "MADOMENU.OVL";
pub const SOURCE_DECODED_BYTES: usize = 0x0F66;
pub const HANDOFF_CALL_DECODED_OFFSET: usize = 0x02CF;
pub const OVERLAY_LOAD_OFFSET: usize = 0x0100;
pub const RENDERER_IMAGE_PATH: &[u8] = b"A:KFONT.BIN\0";
const SOURCE_HANDOFF: [u8; 3] = [0x8B, 0x76, 0x02]; // mov si,[bp+2]
const DOS_READ_TO_SEGMENT_END: u16 = 0xFFFF;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectorRendererLoadReport {
    pub call_decoded_offset: usize,
    pub stub_decoded_offset: usize,
    pub path_decoded_offset: usize,
    pub source_decoded_bytes: usize,
    pub product_decoded_bytes: usize,
}

/// Install one typed loader in the exact shared A/R/S selector handoff.
pub fn install_selector_renderer_load(decoded: &mut Vec<u8>) -> Result<SelectorRendererLoadReport> {
    if decoded.len() != SOURCE_DECODED_BYTES {
        bail!(
            "{SELECTOR_OVERLAY} decoded length is 0x{:X}, expected exact source 0x{SOURCE_DECODED_BYTES:X}",
            decoded.len()
        );
    }
    if decoded.get(HANDOFF_CALL_DECODED_OFFSET..HANDOFF_CALL_DECODED_OFFSET + 3)
        != Some(SOURCE_HANDOFF.as_slice())
    {
        bail!(
            "{SELECTOR_OVERLAY} handoff at decoded 0x{HANDOFF_CALL_DECODED_OFFSET:04X} does not match mov si,[bp+2]"
        );
    }

    let stub_decoded_offset = decoded.len();
    let stub_ip = logical_offset(stub_decoded_offset)?;
    let placeholder = assemble_renderer_load_stub(stub_ip, 0)?;
    let path_decoded_offset = stub_decoded_offset
        .checked_add(placeholder.len())
        .context("selector renderer path offset overflow")?;
    let path_ip = logical_offset(path_decoded_offset)?;
    let stub = assemble_renderer_load_stub(stub_ip, path_ip)?;
    if stub.len() != placeholder.len() {
        bail!("selector renderer loader changed size after path binding");
    }

    let call_ip = logical_offset(HANDOFF_CALL_DECODED_OFFSET)?;
    let mut call_assembler = Assembler::new();
    call_assembler.emit(Instruction::Call {
        target: CallTarget::Rel16(near_displacement(call_ip, stub_ip)),
    });
    let call = assemble_at(
        &call_assembler,
        call_ip,
        "normal-selector renderer load call",
    )?;
    if call.len() != SOURCE_HANDOFF.len() {
        bail!("normal-selector renderer load call is not length preserving");
    }

    let mut planned = decoded.clone();
    planned[HANDOFF_CALL_DECODED_OFFSET..HANDOFF_CALL_DECODED_OFFSET + call.len()]
        .copy_from_slice(&call);
    planned.extend_from_slice(&stub);
    planned.extend_from_slice(RENDERER_IMAGE_PATH);

    let expected_target = call_ip.wrapping_add(3).wrapping_add(u16::from_le_bytes([
        planned[HANDOFF_CALL_DECODED_OFFSET + 1],
        planned[HANDOFF_CALL_DECODED_OFFSET + 2],
    ]));
    if expected_target != stub_ip {
        bail!("normal-selector renderer load call does not resolve to its appended stub");
    }
    if planned.get(path_decoded_offset..path_decoded_offset + RENDERER_IMAGE_PATH.len())
        != Some(RENDERER_IMAGE_PATH)
    {
        bail!("normal-selector renderer path postcondition failed");
    }

    let product_decoded_bytes = planned.len();
    *decoded = planned;
    Ok(SelectorRendererLoadReport {
        call_decoded_offset: HANDOFF_CALL_DECODED_OFFSET,
        stub_decoded_offset,
        path_decoded_offset,
        source_decoded_bytes: SOURCE_DECODED_BYTES,
        product_decoded_bytes,
    })
}

fn logical_offset(decoded_offset: usize) -> Result<u16> {
    let logical = decoded_offset
        .checked_add(OVERLAY_LOAD_OFFSET)
        .context("selector logical offset overflow")?;
    u16::try_from(logical).context("selector logical offset exceeds 16 bits")
}

fn assemble_renderer_load_stub(stub_ip: u16, path_ip: u16) -> Result<Vec<u8>> {
    let mut assembler = Assembler::new();
    assembler
        .emit(Instruction::Pushf)
        .emit(Instruction::Pusha)
        .emit(push(segment(SegmentRegister::DS)))
        .emit(push(segment(SegmentRegister::ES)))
        .emit(push(segment(SegmentRegister::CS)))
        .emit(pop(segment(SegmentRegister::DS)))
        .emit(mov(reg16(Register16::DX), imm16(path_ip)))
        .emit(mov(reg16(Register16::AX), imm16(0x3D00)))
        .emit(Instruction::Int { vector: 0x21 })
        .emit_branch(Condition::B, "restore")
        .emit(mov(reg16(Register16::BX), reg16(Register16::AX)))
        .emit(mov(reg16(Register16::AX), imm16(HOOK_SEG)))
        .emit(mov(segment(SegmentRegister::DS), reg16(Register16::AX)))
        .emit(Instruction::Xor {
            dest: reg16(Register16::DX),
            src: reg16(Register16::DX),
        })
        // DOS returns the bytes available at EOF. Requesting the segment limit
        // removes character-specific file-size constants while preserving the
        // 64 KiB destination boundary.
        .emit(mov(reg16(Register16::CX), imm16(DOS_READ_TO_SEGMENT_END)))
        .emit(mov(reg8(Register8::AH), crate::v30_assembler::imm8(0x3F)))
        .emit(Instruction::Int { vector: 0x21 })
        .emit(mov(reg8(Register8::AH), crate::v30_assembler::imm8(0x3E)))
        .emit(Instruction::Int { vector: 0x21 })
        .label("restore")
        .emit(pop(segment(SegmentRegister::ES)))
        .emit(pop(segment(SegmentRegister::DS)))
        .emit(Instruction::Popa)
        .emit(Instruction::Popf)
        .emit(mov(
            reg16(Register16::SI),
            based_memory(None, EffectiveAddressBase::Bp, 2, OperandSize::Word),
        ))
        .emit(Instruction::Ret { pop: 0 });
    assemble_at(
        &assembler,
        stub_ip,
        "normal Demo selector renderer-image loader",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_loader_preserves_the_source_handoff_and_uses_one_bounded_read() {
        let mut decoded = vec![0u8; SOURCE_DECODED_BYTES];
        decoded[HANDOFF_CALL_DECODED_OFFSET..HANDOFF_CALL_DECODED_OFFSET + 3]
            .copy_from_slice(&SOURCE_HANDOFF);
        let report = install_selector_renderer_load(&mut decoded).unwrap();

        assert_eq!(report.stub_decoded_offset, SOURCE_DECODED_BYTES);
        assert!(report.product_decoded_bytes > report.source_decoded_bytes);
        assert_eq!(
            &decoded[report.path_decoded_offset
                ..report.path_decoded_offset + RENDERER_IMAGE_PATH.len()],
            RENDERER_IMAGE_PATH
        );
        assert!(
            decoded[report.stub_decoded_offset..report.path_decoded_offset]
                .windows([0xB9, 0xFF, 0xFF].len())
                .any(|bytes| bytes == [0xB9, 0xFF, 0xFF])
        );
        assert!(
            decoded[report.stub_decoded_offset..report.path_decoded_offset].ends_with(
                &SOURCE_HANDOFF
                    .iter()
                    .copied()
                    .chain([0xC3])
                    .collect::<Vec<_>>()
            )
        );
    }

    #[test]
    fn selector_loader_rejects_a_non_source_overlay_transactionally() {
        let mut decoded = vec![0u8; SOURCE_DECODED_BYTES];
        let before = decoded.clone();
        assert!(install_selector_renderer_load(&mut decoded).is_err());
        assert_eq!(decoded, before);
    }
}
