//! Retire Rulue's action-specific failure text when the watering can succeeds.
//!
//! Failure already consumes MAIN+2FE. Success must retire the same override
//! before it can leak into an unrelated attack or a later encounter.
use anyhow::{Context, Result, bail};
use v30::{
    Assembler, CallTarget, EffectiveAddressBase, Instruction, JmpTarget, OperandSize, Register8,
    SegmentRegister,
};

use crate::v30_assembler::{
    assemble_at, based_memory, imm8, imm16, mov, near_displacement, pop, push, reg8, segment,
};

pub const STUB_BYTES: usize = 22;
const CALL_OFFSET: usize = 0xABFB;
const CALL_IP: u16 = 0xACFB;
const EFFECT_IP: u16 = 0x367C;

pub fn install(decoded: &mut [u8], stub_offset: usize) -> Result<()> {
    // The setup producer and success-only call must both still be the proven
    // source. Check everything before modifying the candidate.
    if decoded.get(CALL_OFFSET..CALL_OFFSET + 3) != Some(&[0xE8, 0x7E, 0x89])
        || decoded.get(0xAC0E..0xAC1E)
            != Some(&[
                0x8C, 0xC8, 0x8E, 0xD8, 0xB4, 0x01, 0xCD, 0x7B, 0xB8, 0x86, 0xBD, 0x26, 0x89, 0x87,
                0xFE, 0x02,
            ])
    {
        bail!("Rulue failure-message producer/success consumer differs from source");
    }
    let end = stub_offset
        .checked_add(STUB_BYTES)
        .context("failure-message stub overflow")?;
    let slot = decoded
        .get(stub_offset..end)
        .context("failure-message stub outside overlay")?;
    if slot.iter().any(|byte| *byte != 0) {
        bail!("failure-message stub reservation is not empty");
    }
    let ip = u16::try_from(
        stub_offset
            .checked_add(0x100)
            .context("stub address overflow")?,
    )?;
    let mut a = Assembler::new();
    a.emit(Instruction::Pushf)
        .emit(Instruction::Pusha)
        .emit(push(segment(SegmentRegister::DS)))
        .emit(push(segment(SegmentRegister::ES)))
        .emit(mov(reg8(Register8::AH), imm8(1)))
        .emit(Instruction::Int { vector: 0x7B })
        .emit(mov(
            based_memory(
                Some(SegmentRegister::ES),
                EffectiveAddressBase::Bx,
                0x2FE,
                OperandSize::Word,
            ),
            imm16(0),
        ))
        .emit(pop(segment(SegmentRegister::ES)))
        .emit(pop(segment(SegmentRegister::DS)))
        .emit(Instruction::Popa)
        .emit(Instruction::Popf);
    let prefix = assemble_at(&a, ip, "failure-message retirement")?;
    let jump_ip = ip
        .checked_add(u16::try_from(prefix.len())?)
        .context("stub jump overflow")?;
    a.emit(Instruction::Jmp {
        target: JmpTarget::Rel16(near_displacement(jump_ip, EFFECT_IP)),
    });
    let stub = assemble_at(&a, ip, "failure-message retirement and original effect")?;
    if stub.len() != STUB_BYTES {
        bail!("failure-message stub size changed: {}", stub.len());
    }
    let mut call = Assembler::new();
    call.emit(Instruction::Call {
        target: CallTarget::Rel16(near_displacement(CALL_IP, ip)),
    });
    let call = assemble_at(&call, CALL_IP, "watering-can success redirection")?;
    if call.len() != 3 {
        bail!("failure-message call extent changed");
    }
    decoded[stub_offset..end].copy_from_slice(&stub);
    decoded[CALL_OFFSET..CALL_OFFSET + 3].copy_from_slice(&call);
    Ok(())
}
