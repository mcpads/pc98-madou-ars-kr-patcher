//! Preserve the full ten-byte item-record offset in Rulue's sale path.
use anyhow::{Result, bail};
use v30::{
    Assembler, EffectiveAddressBase, Instruction, OperandSize, Register8, Register16,
    SegmentRegister, SignedImmediate,
};

use crate::{
    hook_geometry::RendererOverlay,
    v30_assembler::{assemble_at, based_memory, imm16, mov, reg8, reg16},
};

pub fn install(decoded: &mut [u8], overlay: RendererOverlay) -> Result<()> {
    if overlay != RendererOverlay::Rulue {
        return Ok(());
    }
    const OFFSET: usize = 0x7E07;
    // Guard the index calculation and both consumers, leaving relocated record
    // contents alone. AAD truncates index 26 * 10 to 4 before these reads.
    const SOURCE: &[u8] = &[
        0x26, 0x8A, 0x25, 0x25, 0x00, 0x3F, 0xD5, 0x0A, 0x8B, 0xF0, 0x2E, 0x8B, 0x84, 0x74, 0x5E,
        0x2E, 0xFF, 0xB4, 0x6C, 0x5E,
    ];
    if decoded.get(OFFSET..OFFSET + SOURCE.len()) != Some(SOURCE) {
        bail!("Rulue shop sale index/consumers differ from source");
    }
    let mut a = Assembler::new();
    a.emit(mov(
        reg8(Register8::AL),
        based_memory(
            Some(SegmentRegister::ES),
            EffectiveAddressBase::Di,
            0,
            OperandSize::Byte,
        ),
    ))
    .emit(Instruction::And {
        dest: reg16(Register16::AX),
        src: imm16(0x3F),
    })
    .emit(Instruction::ImulImmediate {
        dest: Register16::SI,
        src: reg16(Register16::AX),
        immediate: SignedImmediate::Byte(10),
    })
    .emit(Instruction::Nop);
    let patch = assemble_at(&a, 0x7F07, "Rulue shop sale record index")?;
    if patch.len() != 10 {
        bail!("Rulue shop sale patch extent changed");
    }
    decoded[OFFSET..OFFSET + 10].copy_from_slice(&patch);
    Ok(())
}
