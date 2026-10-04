//! Keep dancer thunder's attribute writes in MAIN and its status backup.
use crate::hook_geometry::RendererOverlay;
use crate::v30_assembler::{assemble_at, based_memory, imm8, mov, near_displacement, reg8, reg16};
use anyhow::{Context, Result, bail};
use v30::{
    Assembler, CallTarget, EffectiveAddressBase, Instruction, OperandSize, Register8, Register16,
    SegmentRegister,
};

pub const STUB_BYTES: usize = 8;

pub fn install(decoded: &mut [u8], overlay: RendererOverlay, slot: usize) -> Result<()> {
    let (call_ip, effect_ip): (u16, u16) = match overlay {
        RendererOverlay::Arle => (0x97A8, 0x3683),
        RendererOverlay::Rulue => (0xA3CB, 0x367C),
        RendererOverlay::Schezo => (0x8B4A, 0x3623),
    };
    let offset = usize::from(call_ip) - 0x100;
    let mut old_call = Assembler::new();
    old_call.emit(Instruction::Call {
        target: CallTarget::Rel16(near_displacement(call_ip, effect_ip)),
    });
    let old_call = assemble_at(&old_call, call_ip, "source thunder setup call")?;
    const ACCESS: &[u8] = &[
        0x26, 0x8B, 0x87, 0x10, 0x01, 0x26, 0x89, 0x87, 0x28, 0x03, 0xD0, 0xE8, 0x14, 0, 0xD0,
        0xEC, 0x80, 0xD4, 0, 0x26, 0x89, 0x87, 0x10, 0x01,
    ];
    if decoded.get(offset..offset + 3) != Some(old_call.as_slice())
        || decoded.get(offset + 3..offset + 27) != Some(ACCESS)
    {
        bail!("dancer thunder setup/access differs from source");
    }
    // The original helper preserves the caller's item-record BX and restores
    // previous status attributes from MAIN+308..30B. Guard that provenance.
    let effect = usize::from(effect_ip) - 0x100;
    if decoded.get(effect..effect + 5) != Some(&[0x53, 0xB4, 1, 0xCD, 0x7B])
        || decoded.get(effect + 47..effect + 52) != Some(&[0x26, 0x86, 0x87, 0x08, 0x03])
        || decoded.get(effect + 109..effect + 111) != Some(&[0x5B, 0xC3])
    {
        bail!("dancer thunder status helper differs from source");
    }
    let end = slot
        .checked_add(STUB_BYTES)
        .context("thunder stub overflow")?;
    if decoded
        .get(slot..end)
        .context("thunder stub outside overlay")?
        .iter()
        .any(|b| *b != 0)
    {
        bail!("thunder reservation is not empty");
    }
    let ip = u16::try_from(slot.checked_add(0x100).context("thunder IP overflow")?)?;
    let mut a = Assembler::new();
    a.emit(Instruction::Call {
        target: CallTarget::Rel16(near_displacement(ip, effect_ip)),
    })
    .emit(mov(reg8(Register8::AH), imm8(1)))
    .emit(Instruction::Int { vector: 0x7B })
    .emit(Instruction::Ret { pop: 0 });
    let stub = assemble_at(&a, ip, "thunder MAIN base refresh")?;
    let mut call = Assembler::new();
    call.emit(Instruction::Call {
        target: CallTarget::Rel16(near_displacement(call_ip, ip)),
    });
    let call = assemble_at(&call, call_ip, "thunder setup redirection")?;
    let mut backup = Assembler::new();
    backup.emit(mov(
        based_memory(
            Some(SegmentRegister::ES),
            EffectiveAddressBase::Bx,
            0x308,
            OperandSize::Word,
        ),
        reg16(Register16::AX),
    ));
    let backup = assemble_at(&backup, call_ip + 8, "thunder status attribute backup")?;
    if stub.len() != STUB_BYTES || call.len() != 3 || backup.len() != 5 {
        bail!("thunder patch extent changed");
    }
    decoded[slot..end].copy_from_slice(&stub);
    decoded[offset..offset + 3].copy_from_slice(&call);
    decoded[offset + 8..offset + 13].copy_from_slice(&backup);
    Ok(())
}
