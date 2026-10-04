//! Project-side assembly conveniences for the shared typed V30 profile.
//!
//! CPU semantics and byte encoding belong to `retro-typed-isa`. This module
//! only keeps the A.R.S patch builders readable while they supply game-specific
//! addresses and values to that shared model.

use anyhow::{Context, Result, bail};
use v30::{
    Assembler, EffectiveAddress, EffectiveAddressBase, EffectiveAddressDisplacement, Instruction,
    InstructionAddress, Operand, OperandSize, Register8, Register16, SegmentRegister,
};

pub(crate) fn assemble_at(assembler: &Assembler, origin: u16, purpose: &str) -> Result<Vec<u8>> {
    assembler
        .assemble(InstructionAddress {
            seg: 0,
            off: origin,
        })
        .with_context(|| format!("assemble typed V30 {purpose} at 0x{origin:04X}"))
        .map(|program| program.into_bytes())
}

pub(crate) const fn reg8(register: Register8) -> Operand {
    Operand::Reg8(register)
}

pub(crate) const fn reg16(register: Register16) -> Operand {
    Operand::Reg16(register)
}

pub(crate) const fn segment(register: SegmentRegister) -> Operand {
    Operand::Sreg(register)
}

pub(crate) const fn imm8(value: u8) -> Operand {
    Operand::Imm8(value)
}

pub(crate) const fn imm16(value: u16) -> Operand {
    Operand::Imm16(value)
}

pub(crate) fn direct_memory(
    segment: Option<SegmentRegister>,
    address: u16,
    size: OperandSize,
) -> Operand {
    Operand::Mem(
        EffectiveAddress::new(
            segment,
            EffectiveAddressBase::Direct,
            EffectiveAddressDisplacement::Absolute(address),
            size,
        )
        .expect("direct V30 address is representable"),
    )
}

pub(crate) fn based_memory(
    segment: Option<SegmentRegister>,
    base: EffectiveAddressBase,
    displacement: i16,
    size: OperandSize,
) -> Operand {
    Operand::Mem(
        EffectiveAddress::new(
            segment,
            base,
            EffectiveAddressDisplacement::Signed(displacement),
            size,
        )
        .expect("based V30 address is representable"),
    )
}

pub(crate) const fn mov(dest: Operand, src: Operand) -> Instruction {
    Instruction::Mov { dest, src }
}

pub(crate) const fn push(src: Operand) -> Instruction {
    Instruction::Push { src }
}

pub(crate) const fn pop(dest: Operand) -> Instruction {
    Instruction::Pop { dest }
}

pub(crate) const fn near_displacement(origin: u16, target: u16) -> i16 {
    target.wrapping_sub(origin.wrapping_add(3)) as i16
}

pub(crate) fn near_jump_patch(
    origin: u16,
    target: u16,
    slot_len: usize,
    purpose: &str,
) -> Result<Vec<u8>> {
    if slot_len < 3 {
        bail!("{purpose} slot is shorter than a V30 near jump");
    }
    let mut assembler = Assembler::new();
    assembler.emit(Instruction::Jmp {
        target: v30::JmpTarget::Rel16(near_displacement(origin, target)),
    });
    for _ in 3..slot_len {
        assembler.emit(Instruction::Nop);
    }
    let patch = assemble_at(&assembler, origin, purpose)?;
    if patch.len() != slot_len {
        bail!("typed {purpose} is not length preserving");
    }
    Ok(patch)
}
