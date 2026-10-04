//! Resolve temporary post-load encounter suppression before casting Illusion.
use crate::{
    hook_geometry::RendererOverlay,
    v30_assembler::{assemble_at, based_memory, imm8, imm16, mov, near_displacement, reg8},
};
use anyhow::{Context, Result, bail};
use v30::{
    Assembler, CallTarget, Condition, EffectiveAddressBase, Instruction, OperandSize, Register8,
    SegmentRegister,
};

pub const STUB_BYTES: usize = 43;

pub fn install(decoded: &mut [u8], overlay: RendererOverlay, slot: usize) -> Result<()> {
    let (offset, source): (usize, &[u8]) = match overlay {
        RendererOverlay::Arle => (
            0x9277,
            &[
                0xb4, 1, 0xcd, 0x7b, 0x26, 0xc6, 0x87, 0, 2, 1, 0x26, 0xc7, 0x87, 2, 2, 0xb0, 0x93,
                0x26, 0x83, 0x87, 4, 2, 0x32, 0x26, 0x8b, 0x47, 0x3e, 0x0b, 0xc0, 0x74, 0x0b, 0x26,
                0x89, 0x87, 6, 2, 0x26, 0xc7, 0x47, 0x3e, 0, 0,
            ],
        ),
        RendererOverlay::Schezo => (
            0x85f2,
            &[
                0xb4, 1, 0xcd, 0x7b, 0x26, 0xc6, 0x87, 0, 2, 0x41, 0x26, 0xc7, 0x87, 2, 2, 0x2d,
                0x87, 0x26, 0x83, 0x87, 4, 2, 0x32, 0x26, 0x8b, 0x47, 0x3e, 0x0b, 0xc0, 0x74, 0x0d,
                0x26, 0x89, 0x87, 6, 2, 0x33, 0xc0, 0x26, 0xc7, 0x47, 0x3e, 0, 0,
            ],
        ),
        RendererOverlay::Rulue => bail!("Rulue has no ordinary Illusion handler"),
    };
    if decoded.get(offset..offset + source.len()) != Some(source) {
        bail!("Illusion setup differs from source");
    }
    let end = slot
        .checked_add(STUB_BYTES)
        .context("Illusion stub overflow")?;
    if (slot..end).contains(&offset)
        || (offset..offset + source.len()).contains(&slot)
        || decoded
            .get(slot..end)
            .context("Illusion stub outside overlay")?
            .iter()
            .any(|b| *b != 0)
    {
        bail!("Illusion reservation is not empty or overlaps setup");
    }
    let ip = u16::try_from(slot.checked_add(0x100).context("Illusion IP overflow")?)?;
    let mem = |off, size| {
        based_memory(
            Some(SegmentRegister::ES),
            EffectiveAddressBase::Bx,
            off,
            size,
        )
    };
    let mut a = Assembler::new();
    a.emit(mov(reg8(Register8::AH), imm8(1)))
        .emit(Instruction::Int { vector: 0x7b })
        .emit(Instruction::Cmp {
            a: mem(0x3f0, OperandSize::Byte),
            b: imm8(0x51),
        })
        .emit_branch(Condition::Ne, "done")
        .emit(Instruction::Cmp {
            a: mem(0x3e, OperandSize::Word),
            b: imm16(0),
        })
        .emit_branch(Condition::Ne, "retire")
        .emit(mov(reg8(Register8::AL), mem(0x3f1, OperandSize::Byte)))
        .emit(mov(mem(0x3e, OperandSize::Byte), reg8(Register8::AL)))
        .label("retire")
        .emit(mov(mem(0x3f0, OperandSize::Word), imm16(0)))
        .emit(mov(mem(0x3f2, OperandSize::Word), imm16(0)))
        .label("done")
        .emit(Instruction::Ret { pop: 0 });
    let stub = assemble_at(&a, ip, "Illusion post-load encounter resolution")?;
    let mut hook = Assembler::new();
    hook.emit(Instruction::Call {
        target: CallTarget::Rel16(near_displacement((offset + 0x100) as u16, ip)),
    })
    .emit(Instruction::Nop);
    let hook = assemble_at(&hook, (offset + 0x100) as u16, "Illusion MAIN lookup hook")?;
    if stub.len() != STUB_BYTES || hook.len() != 4 {
        bail!("Illusion patch extent changed: {}", stub.len());
    }
    decoded[slot..end].copy_from_slice(&stub);
    decoded[offset..offset + 4].copy_from_slice(&hook);
    Ok(())
}
