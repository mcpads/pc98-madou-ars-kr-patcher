//! Preserve the encounter parameter held by the one-step post-load callback.
//!
//! An immediate native save contains rate=0 and its saved rate at record +3F1.
//! The original word-sized initialization at +3F0 destroys that saved rate on
//! reload. Initialize only the callback type; its existing exchange/retirement
//! routine can then restore the saved rate. No encounter rate is synthesized.
use anyhow::{Result, bail};
use v30::{Assembler, EffectiveAddressBase, Instruction, OperandSize, SegmentRegister};

use crate::{
    hook_geometry::RendererOverlay,
    v30_assembler::{assemble_at, based_memory, imm8, mov},
};

const INITIALIZER: usize = 0x15C;
const SOURCE_INITIALIZER: &[u8] = &[0x26, 0xC7, 0x87, 0xF0, 0x03, 0x51, 0x00];
const SOURCE_EXCHANGE: &[u8] = &[
    0xB4, 0x01, 0xCD, 0x7B, 0x26, 0x8A, 0x87, 0xF1, 0x03, 0x26, 0x86, 0x47, 0x3E, 0x26, 0x88, 0x87,
    0xF1, 0x03, 0x3C, 0x00, 0x75, 0x0C, 0x26, 0x88, 0x87, 0xF0, 0x03, 0x26, 0xC7, 0x87, 0xF2, 0x03,
    0x00, 0x00, 0xF8, 0xC3,
];

pub fn install(decoded: &mut [u8], overlay: RendererOverlay) -> Result<()> {
    let callback = match overlay {
        RendererOverlay::Arle => 0x293D,
        RendererOverlay::Rulue => 0x2943,
        RendererOverlay::Schezo => 0x293C,
    };
    let logical = (callback + 0x100_u16).to_le_bytes();
    let callback_writer = [0x26, 0xC7, 0x87, 0xF2, 0x03, logical[0], logical[1]];
    if decoded.get(INITIALIZER..INITIALIZER + SOURCE_INITIALIZER.len()) != Some(SOURCE_INITIALIZER)
        || decoded.get(0x163..0x16A) != Some(callback_writer.as_slice())
        || decoded.get(usize::from(callback)..usize::from(callback) + SOURCE_EXCHANGE.len())
            != Some(SOURCE_EXCHANGE)
    {
        bail!("native-save encounter initializer/callback differs from the proven source");
    }
    let mut assembler = Assembler::new();
    assembler.emit(mov(
        based_memory(
            Some(SegmentRegister::ES),
            EffectiveAddressBase::Bx,
            0x3F0,
            OperandSize::Byte,
        ),
        imm8(0x51),
    ));
    assembler.emit(Instruction::Nop);
    let patch = assemble_at(&assembler, 0x25C, "native-save encounter initialization")?;
    if patch.len() != SOURCE_INITIALIZER.len() {
        bail!("native-save encounter initialization changed instruction extent");
    }
    decoded[INITIALIZER..INITIALIZER + patch.len()].copy_from_slice(&patch);
    Ok(())
}
