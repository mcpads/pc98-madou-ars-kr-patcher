//! Single source of truth for the renderer-hook geometry in the `0x8800`
//! segment, and typed trampoline generation via the shared `retro-typed-isa`
//! V30 profile.
//!
//! Per `references/strategy/reinsertion.md` §198/§209, hook glue is not
//! hand-coded opcode bytes: the far jumps are emitted from the instruction type,
//! and the one hook offset lives here so moving the hook (e.g. to make room for a
//! bigger glyph sheet) is a single-constant change, not scattered magic numbers
//! across the patcher and loaders (§194 "hardcoded addresses drift
//! when the glyph count changes").

use anyhow::{Context, Result, bail};
use v30::{
    Assembler, CallTarget, Condition, EffectiveAddressBase, Instruction, JmpTarget, LoopCondition,
    OperandSize, PortAddress, Register8, Register16, SegmentRegister, ShiftCount, encode_bytes,
};

use crate::v30_assembler::{
    assemble_at, based_memory, direct_memory, imm8, imm16, mov, pop, push, reg8, reg16, segment,
};

/// Segment holding the glyph sheet (at offset 0) and the hook code.
pub const HOOK_SEG: u16 = 0x8800;

/// Runtime-proven all-zero conventional-RAM block used by the renderer supply.
/// The lower bound is the physical base of [`HOOK_SEG`]; the upper bound is
/// exclusive. Any additional boot-loaded payload must remain inside this block.
pub const HOOK_BLOCK_PHYSICAL_BASE: usize = HOOK_SEG as usize * 16;
pub const HOOK_BLOCK_PHYSICAL_END: usize = 0x9F000;

/// Hook entry offset. Moved above the sheet so the sheet (`0x0..`) can hold the
/// full ten-row gaiji glyph set (rung 2: 940 glyphs = `0x7580` bytes).
pub const HOOK_ENTRY_OFF: u16 = 0x8000;

/// Saved-JIS scratch word the hook writes at runtime. It sits just below the
/// hook, so the glyph sheet must end at or before this — a sheet that reaches
/// `SCRATCH_OFF` gets its tail glyph clobbered by the entry hook. This, not
/// `HOOK_ENTRY_OFF`, is the true sheet ceiling (max 1023 glyphs).
pub const SCRATCH_OFF: u16 = 0x7FFE;

/// One-shot validity byte for [`SCRATCH_OFF`]. Some battle-animation paths call
/// the full-width draw helper directly, without passing through the JIS fetch
/// entry. They must use the original font path instead of reusing the last
/// Hangul code left in `SCRATCH_OFF`.
const SCRATCH_ACTIVE_OFF: u16 = SCRATCH_OFF - 2;

/// Draw hook keeps its runtime-proven `0x20` placement. Runtime-particle marker
/// dispatch fits inside the entry range and jumps to a profile-specific tail.
pub const HOOK_DRAW_OFF: u16 = HOOK_ENTRY_OFF + 0x20;

/// Runtime topic-particle selector. The assembly pads the existing renderer hook
/// to this offset so an overlay-side near stub can far-call it.
pub const HOOK_TOPIC_SELECTOR_OFF: u16 = HOOK_ENTRY_OFF + 0xB0;

/// Shared player that isolates one low-tier damage track from GVRAM before
/// handing it to BSAMP.COM.
pub const S0_DAMAGE_PLAYER_OFF: u16 = HOOK_ENTRY_OFF + 0x100;

/// Marker-only resolver. Ordinary glyphs never enter this tail; this preserves
/// the proven per-character entry/draw path that battle animation depends on.
pub const PARTICLE_MARKER_RESOLVER_OFF: u16 = HOOK_ENTRY_OFF + 0x190;

const RENDERER_SEG: u16 = 0x1DF4;
const JOSA_DATA: i16 = crate::josa::RUNTIME_DATA_OFF as i16;
const JOSA_TOPIC_NONE_INDEX: u8 =
    (crate::josa::TOPIC_NO_FINAL_CODE_OFF - crate::josa::RUNTIME_DATA_OFF) as u8;
const JOSA_PARTICLE_FIRST_INDEX: u8 =
    (crate::josa::OBJECT_NO_FINAL_CODE_OFF - crate::josa::RUNTIME_DATA_OFF) as u8;
const JOSA_CLASS: i16 = crate::josa::CLASS_TABLE_OFF as i16;

/// Original renderer bytes replaced by the entry trampoline.
pub const ENTRY_TRAMPOLINE_SOURCE: [u8; 5] = [0xFC, 0x1E, 0x56, 0x8B, 0xD8];

/// Original renderer bytes replaced by the draw trampoline. The external draw
/// hook reproduces this GRCG-on setup before selecting the Hangul or source-font
/// path.
pub const DRAW_SETUP_TRAMPOLINE_SOURCE: [u8; 5] = [0xB0, 0xC0, 0xE6, 0x7C, 0xB9];

/// Unique decoded-renderer anchor beginning at
/// [`DRAW_SETUP_TRAMPOLINE_SOURCE`]. The short prefix alone also occurs in an
/// unrelated renderer path, so decoded-overlay discovery must validate the row
/// count and source-font loop prologue too.
pub const DRAW_SETUP_TRAMPOLINE_ANCHOR: [u8; 12] = [
    0xB0, 0xC0, 0xE6, 0x7C, 0xB9, 0x10, 0x00, 0xBB, 0x00, 0x20, 0x8A, 0xC7,
];

/// Character overlay whose renderer receives the shared hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RendererOverlay {
    Arle,
    Rulue,
    Schezo,
}

impl RendererOverlay {
    fn rejoin_offsets(self) -> (u16, u16, u16, u16) {
        match self {
            Self::Arle => (0x53F5, 0x5469, 0x5491, 0x5448),
            Self::Rulue => (0x5435, 0x54A9, 0x54D1, 0x5488),
            Self::Schezo => (0x53C5, 0x5439, 0x5461, 0x5418),
        }
    }
}

fn far_jmp(off: u16) -> Vec<u8> {
    encode_bytes(&Instruction::Jmp {
        target: JmpTarget::Far { seg: HOOK_SEG, off },
    })
    .expect("emit far jmp")
}

/// The packed-overlay entry trampoline: `jmp HOOK_SEG:HOOK_ENTRY_OFF`.
pub fn entry_trampoline() -> Vec<u8> {
    far_jmp(HOOK_ENTRY_OFF)
}

/// The renderer draw-setup trampoline: `jmp HOOK_SEG:HOOK_DRAW_OFF`.
pub fn draw_trampoline() -> Vec<u8> {
    far_jmp(HOOK_DRAW_OFF)
}

/// Overlay-side shared stub: far-call the external selector, then near-return to
/// the original code. The original direct-glyph renderer call follows the
/// patched three-byte `mov ax,0x244F` and remains untouched.
pub fn topic_selector_stub() -> Vec<u8> {
    let mut bytes = encode_bytes(&mov(reg8(Register8::AL), imm8(JOSA_TOPIC_NONE_INDEX)))
        .expect("emit topic-particle selector index");
    bytes.extend(
        encode_bytes(&Instruction::Call {
            target: CallTarget::Far {
                seg: HOOK_SEG,
                off: HOOK_TOPIC_SELECTOR_OFF,
            },
        })
        .expect("emit far call"),
    );
    bytes.extend(
        encode_bytes(&Instruction::Ret { pop: 0 }).expect("emit near return for particle stub"),
    );
    bytes
}

/// Build and verify the complete renderer hook for one overlay.
///
/// The hook body, its internal branches, and all executable padding boundaries
/// are derived in Rust from the shared typed V30 instruction model. Game-specific
/// rejoin addresses remain here as A.R.S profile data.
pub fn renderer_hook(overlay: RendererOverlay) -> Result<Vec<u8>> {
    let (entry_rejoin, original_font_loop, grcg_rejoin, text_draw_return) =
        overlay.rejoin_offsets();
    let mut bytes = renderer_entry(entry_rejoin)?;
    let draw_offset = usize::from(HOOK_DRAW_OFF - HOOK_ENTRY_OFF);
    if bytes.len() > draw_offset {
        bail!(
            "typed renderer entry is {} bytes, crossing draw hook",
            bytes.len()
        );
    }
    bytes.resize(draw_offset, 0);
    bytes.extend(renderer_draw(
        original_font_loop,
        grcg_rejoin,
        text_draw_return,
    )?);

    let topic_offset = usize::from(HOOK_TOPIC_SELECTOR_OFF - HOOK_ENTRY_OFF);
    if bytes.len() > topic_offset {
        bail!(
            "typed renderer draw is {} bytes, crossing topic selector",
            bytes.len()
        );
    }
    bytes.resize(topic_offset, 0);
    bytes.extend(renderer_topic_selector()?);
    let player_offset = usize::from(S0_DAMAGE_PLAYER_OFF - HOOK_ENTRY_OFF);
    if bytes.len() > player_offset {
        bail!(
            "typed renderer extras are {} bytes, crossing S0 damage player",
            bytes.len()
        );
    }
    bytes.resize(player_offset, 0);
    bytes.extend(crate::received_damage_sfx::assemble_s0_damage_player()?);

    let resolver_offset = usize::from(PARTICLE_MARKER_RESOLVER_OFF - HOOK_ENTRY_OFF);
    if bytes.len() > resolver_offset {
        bail!(
            "typed renderer extras are {} bytes, crossing particle marker resolver",
            bytes.len()
        );
    }
    bytes.resize(resolver_offset, 0);
    bytes.extend(renderer_particle_marker_resolver(entry_rejoin)?);
    if usize::from(HOOK_ENTRY_OFF) + bytes.len() > 0x8200 {
        bail!("renderer hook and marker resolver cross cutscene alignment 0x8200");
    }

    Ok(bytes)
}

fn renderer_entry(entry_rejoin: u16) -> Result<Vec<u8>> {
    let mut assembler = Assembler::new();
    assembler
        // Preserve the caller's flags while recognizing the dedicated JIS
        // marker row. Ordinary glyphs take no call and no external detour.
        .emit(Instruction::Pushf)
        .emit(Instruction::Cmp {
            a: reg8(Register8::AH),
            b: imm8((crate::josa::PARTICLE_MARKER_FIRST_JIS >> 8) as u8),
        })
        .emit_branch(Condition::Ne, "ordinary")
        .emit(Instruction::Jmp {
            target: JmpTarget::Far {
                seg: HOOK_SEG,
                off: PARTICLE_MARKER_RESOLVER_OFF,
            },
        })
        .label("ordinary")
        .emit(Instruction::Popf)
        .emit(mov(
            direct_memory(Some(SegmentRegister::CS), SCRATCH_OFF, OperandSize::Word),
            reg16(Register16::AX),
        ))
        .emit(mov(
            direct_memory(
                Some(SegmentRegister::CS),
                SCRATCH_ACTIVE_OFF,
                OperandSize::Byte,
            ),
            imm8(1),
        ))
        .emit(Instruction::Cld)
        .emit(push(segment(SegmentRegister::DS)))
        .emit(push(reg16(Register16::SI)))
        .emit(mov(reg16(Register16::BX), reg16(Register16::AX)))
        .emit(Instruction::Jmp {
            target: JmpTarget::Far {
                seg: RENDERER_SEG,
                off: entry_rejoin,
            },
        });
    assemble_at(&assembler, HOOK_ENTRY_OFF, "renderer entry hook")
}

fn renderer_particle_marker_resolver(entry_rejoin: u16) -> Result<Vec<u8>> {
    let mut assembler = Assembler::new();
    assembler
        .emit(Instruction::Cmp {
            a: reg8(Register8::AL),
            b: imm8(crate::josa::PARTICLE_MARKER_FIRST_JIS as u8),
        })
        .emit_branch(Condition::B, "capture")
        .emit(Instruction::Cmp {
            a: reg8(Register8::AL),
            b: imm8(
                crate::josa::PARTICLE_MARKER_FIRST_JIS as u8
                    + (crate::josa::PARTICLE_MARKERS.len() - 1) as u8,
            ),
        })
        .emit_branch(Condition::A, "capture")
        .emit(Instruction::Sub {
            dest: reg8(Register8::AL),
            src: imm8(crate::josa::PARTICLE_MARKER_FIRST_JIS as u8),
        })
        .emit(Instruction::Shl {
            dest: reg8(Register8::AL),
            count: ShiftCount::One,
        })
        .emit(Instruction::Shl {
            dest: reg8(Register8::AL),
            count: ShiftCount::One,
        })
        .emit(Instruction::Add {
            dest: reg8(Register8::AL),
            src: imm8(JOSA_PARTICLE_FIRST_INDEX),
        })
        .emit(Instruction::Call {
            target: CallTarget::Far {
                seg: HOOK_SEG,
                off: HOOK_TOPIC_SELECTOR_OFF,
            },
        })
        .label("capture")
        .emit(Instruction::Popf)
        .emit(mov(
            direct_memory(Some(SegmentRegister::CS), SCRATCH_OFF, OperandSize::Word),
            reg16(Register16::AX),
        ))
        .emit(mov(
            direct_memory(
                Some(SegmentRegister::CS),
                SCRATCH_ACTIVE_OFF,
                OperandSize::Byte,
            ),
            imm8(1),
        ))
        .emit(Instruction::Cld)
        .emit(push(segment(SegmentRegister::DS)))
        .emit(push(reg16(Register16::SI)))
        .emit(mov(reg16(Register16::BX), reg16(Register16::AX)))
        .emit(Instruction::Jmp {
            target: JmpTarget::Far {
                seg: RENDERER_SEG,
                off: entry_rejoin,
            },
        });
    assemble_at(
        &assembler,
        PARTICLE_MARKER_RESOLVER_OFF,
        "renderer particle marker resolver",
    )
}

fn renderer_draw(
    original_font_loop: u16,
    grcg_rejoin: u16,
    text_draw_return: u16,
) -> Result<Vec<u8>> {
    let mut assembler = Assembler::new();
    assembler
        // The trampoline replaces the renderer's GRCG-on and row-count setup.
        // Reproduce those instructions before selecting either draw path.
        .emit(mov(reg8(Register8::AL), imm8(0xC0)))
        .emit(Instruction::OutAl {
            port: PortAddress::Imm8(0x7C),
        })
        .emit(mov(reg16(Register16::CX), imm16(0x10)))
        // Both ordinary text branches reach this draw helper with the same
        // profile-specific return address at SS:SP: a near call pushes it, and
        // the alternate branch explicitly pushes it before jumping. Battle
        // animation and field redraw callers have different return addresses.
        // Pair this provenance check with the one-shot capture flag so an
        // entry-only measurement cannot send a later direct redraw through the
        // Hangul sheet path.
        .emit(push(reg16(Register16::BP)))
        .emit(mov(reg16(Register16::BP), reg16(Register16::SP)))
        .emit(Instruction::Cmp {
            a: based_memory(
                Some(SegmentRegister::SS),
                EffectiveAddressBase::Bp,
                2,
                OperandSize::Word,
            ),
            b: imm16(text_draw_return),
        })
        .emit(pop(reg16(Register16::BP)))
        .emit_branch(Condition::Ne, "direct_draw")
        .emit(Instruction::Cmp {
            a: direct_memory(
                Some(SegmentRegister::CS),
                SCRATCH_ACTIVE_OFF,
                OperandSize::Byte,
            ),
            b: imm8(1),
        })
        .emit_branch(Condition::Ne, "original_font")
        .emit(mov(
            direct_memory(
                Some(SegmentRegister::CS),
                SCRATCH_ACTIVE_OFF,
                OperandSize::Byte,
            ),
            imm8(0),
        ))
        .emit(mov(
            reg16(Register16::BX),
            direct_memory(Some(SegmentRegister::CS), SCRATCH_OFF, OperandSize::Word),
        ))
        .emit(mov(reg8(Register8::AL), reg8(Register8::BH)))
        .emit(Instruction::Cmp {
            a: reg8(Register8::AL),
            b: imm8(0x75),
        })
        .emit_branch(Condition::B, "original_font")
        .emit(Instruction::Cmp {
            a: reg8(Register8::AL),
            b: imm8(0x7E),
        })
        .emit_branch(Condition::A, "original_font")
        .emit(push(segment(SegmentRegister::ES)))
        .emit(Instruction::Sub {
            dest: reg8(Register8::AL),
            src: imm8(0x75),
        })
        .emit(mov(reg8(Register8::AH), imm8(94)))
        .emit(Instruction::Mul {
            src: reg8(Register8::AH),
        })
        .emit(mov(reg8(Register8::DL), reg8(Register8::BL)))
        .emit(Instruction::Sub {
            dest: reg8(Register8::DL),
            src: imm8(0x21),
        })
        .emit(Instruction::Xor {
            dest: reg8(Register8::DH),
            src: reg8(Register8::DH),
        })
        .emit(Instruction::Add {
            dest: reg16(Register16::AX),
            src: reg16(Register16::DX),
        });
    for _ in 0..5 {
        assembler.emit(Instruction::Shl {
            dest: reg16(Register16::AX),
            count: ShiftCount::One,
        });
    }
    assembler
        .emit(mov(reg16(Register16::BX), reg16(Register16::AX)))
        .emit(mov(reg16(Register16::AX), imm16(HOOK_SEG)))
        .emit(mov(segment(SegmentRegister::ES), reg16(Register16::AX)))
        .label("hangul_row")
        .emit(mov(
            reg8(Register8::AL),
            based_memory(
                Some(SegmentRegister::ES),
                EffectiveAddressBase::Bx,
                0,
                OperandSize::Byte,
            ),
        ))
        .emit(mov(
            reg8(Register8::AH),
            based_memory(
                Some(SegmentRegister::ES),
                EffectiveAddressBase::Bx,
                1,
                OperandSize::Byte,
            ),
        ))
        .emit(Instruction::Xor {
            dest: reg16(Register16::AX),
            src: reg16(Register16::SI),
        })
        .emit(mov(
            based_memory(None, EffectiveAddressBase::Di, 0, OperandSize::Word),
            reg16(Register16::AX),
        ))
        .emit(Instruction::Add {
            dest: reg16(Register16::DI),
            src: imm16(0x50),
        })
        .emit(Instruction::Add {
            dest: reg16(Register16::BX),
            src: imm16(2),
        })
        .emit_loop(LoopCondition::Always, "hangul_row")
        .emit(pop(segment(SegmentRegister::ES)))
        .emit(Instruction::Jmp {
            target: JmpTarget::Far {
                seg: RENDERER_SEG,
                off: grcg_rejoin,
            },
        })
        .label("direct_draw")
        .emit(mov(
            direct_memory(
                Some(SegmentRegister::CS),
                SCRATCH_ACTIVE_OFF,
                OperandSize::Byte,
            ),
            imm8(0),
        ))
        .label("original_font")
        // Direct battle/field redraw calls did not pass through the JIS entry.
        // Keep their CG-ROM loop in the original renderer segment instead of
        // reimplementing the hardware-font path in the external hook segment.
        .emit(Instruction::Jmp {
            target: JmpTarget::Far {
                seg: RENDERER_SEG,
                off: original_font_loop,
            },
        });
    assemble_at(&assembler, HOOK_DRAW_OFF, "renderer draw hook")
}

fn renderer_topic_selector() -> Result<Vec<u8>> {
    let mut assembler = Assembler::new();
    assembler
        .emit(push(reg16(Register16::BX)))
        .emit(push(reg16(Register16::DX)))
        .emit(push(reg16(Register16::SI)))
        .emit(push(reg16(Register16::CX)))
        .emit(mov(reg8(Register8::CL), reg8(Register8::AL)))
        .emit(mov(
            reg16(Register16::BX),
            direct_memory(Some(SegmentRegister::CS), SCRATCH_OFF, OperandSize::Word),
        ))
        .emit(mov(reg8(Register8::DL), reg8(Register8::BH)))
        .emit(Instruction::Cmp {
            a: reg8(Register8::DL),
            b: imm8(0x75),
        })
        .emit_branch(Condition::B, "particle_none")
        .emit(Instruction::Cmp {
            a: reg8(Register8::DL),
            b: imm8(0x7E),
        })
        .emit_branch(Condition::A, "particle_none")
        .emit(Instruction::Cmp {
            a: reg8(Register8::BL),
            b: imm8(0x21),
        })
        .emit_branch(Condition::B, "particle_none")
        .emit(Instruction::Cmp {
            a: reg8(Register8::BL),
            b: imm8(0x7E),
        })
        .emit_branch(Condition::A, "particle_none")
        .emit(mov(reg8(Register8::AL), reg8(Register8::DL)))
        .emit(Instruction::Sub {
            dest: reg8(Register8::AL),
            src: imm8(0x75),
        })
        .emit(mov(reg8(Register8::AH), imm8(94)))
        .emit(Instruction::Mul {
            src: reg8(Register8::AH),
        })
        .emit(mov(reg8(Register8::DL), reg8(Register8::BL)))
        .emit(Instruction::Sub {
            dest: reg8(Register8::DL),
            src: imm8(0x21),
        })
        .emit(Instruction::Xor {
            dest: reg8(Register8::DH),
            src: reg8(Register8::DH),
        })
        .emit(Instruction::Add {
            dest: reg16(Register16::AX),
            src: reg16(Register16::DX),
        })
        .emit(mov(reg16(Register16::SI), reg16(Register16::AX)))
        .emit(Instruction::Cmp {
            a: based_memory(
                Some(SegmentRegister::CS),
                EffectiveAddressBase::Si,
                JOSA_CLASS,
                OperandSize::Byte,
            ),
            b: imm8(0),
        })
        .emit_branch(Condition::E, "particle_none")
        .emit(Instruction::Add {
            dest: reg8(Register8::CL),
            src: imm8(2),
        })
        .label("particle_none")
        .emit(Instruction::Xor {
            dest: reg8(Register8::CH),
            src: reg8(Register8::CH),
        })
        .emit(mov(reg16(Register16::BX), reg16(Register16::CX)))
        .emit(mov(
            reg16(Register16::AX),
            based_memory(
                Some(SegmentRegister::CS),
                EffectiveAddressBase::Bx,
                JOSA_DATA,
                OperandSize::Word,
            ),
        ))
        .emit(pop(reg16(Register16::CX)))
        .emit(pop(reg16(Register16::SI)))
        .emit(pop(reg16(Register16::DX)))
        .emit(pop(reg16(Register16::BX)))
        .emit(Instruction::Retf { pop: 0 });
    assemble_at(
        &assembler,
        HOOK_TOPIC_SELECTOR_OFF,
        "renderer topic selector",
    )
    .context("build renderer topic selector")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trampolines_are_far_jmps_to_the_hook() {
        // EA <off_lo> <off_hi> <seg_lo> <seg_hi>
        assert_eq!(entry_trampoline(), vec![0xEA, 0x00, 0x80, 0x00, 0x88]);
        assert_eq!(draw_trampoline(), vec![0xEA, 0x20, 0x80, 0x00, 0x88]);
    }

    #[test]
    fn topic_selector_stub_far_calls_hook_then_near_returns() {
        assert_eq!(
            topic_selector_stub(),
            vec![0xB0, 0x0C, 0x9A, 0xB0, 0x80, 0x00, 0x88, 0xC3]
        );
    }

    #[test]
    fn renderer_hooks_fit_fixed_entry_points_and_use_profile_rejoins() {
        let arle = renderer_hook(RendererOverlay::Arle).unwrap();
        let rulue = renderer_hook(RendererOverlay::Rulue).unwrap();
        let schezo = renderer_hook(RendererOverlay::Schezo).unwrap();
        assert_eq!(schezo.len(), rulue.len());
        assert_eq!(arle.len(), rulue.len());
        assert!(arle.len() > usize::from(PARTICLE_MARKER_RESOLVER_OFF - HOOK_ENTRY_OFF));
        let entry_len = usize::from(HOOK_DRAW_OFF - HOOK_ENTRY_OFF);
        for (hook, rejoin) in [(&arle, 0x53F5), (&rulue, 0x5435), (&schezo, 0x53C5)] {
            let expected = encode_bytes(&Instruction::Jmp {
                target: JmpTarget::Far {
                    seg: RENDERER_SEG,
                    off: rejoin,
                },
            })
            .unwrap();
            assert!(
                hook[..entry_len]
                    .windows(expected.len())
                    .any(|bytes| bytes == expected),
                "entry hook must rejoin the selected renderer profile"
            );
        }
        let topic_offset = usize::from(HOOK_TOPIC_SELECTOR_OFF - HOOK_ENTRY_OFF);
        let resolver_offset = usize::from(PARTICLE_MARKER_RESOLVER_OFF - HOOK_ENTRY_OFF);
        assert_eq!(
            &arle[topic_offset..resolver_offset],
            &rulue[topic_offset..resolver_offset]
        );
        assert_eq!(
            &rulue[topic_offset..resolver_offset],
            &schezo[topic_offset..resolver_offset]
        );
    }

    #[test]
    fn ordinary_entry_avoids_the_marker_resolver_call_and_keeps_draw_unchanged() {
        let hook = renderer_hook(RendererOverlay::Arle).unwrap();
        let entry = &hook[..usize::from(HOOK_DRAW_OFF - HOOK_ENTRY_OFF)];
        assert_eq!(entry.first(), Some(&0x9C)); // pushf
        assert!(!entry.contains(&0x9A)); // no far call on the common path
        let resolver_jump = encode_bytes(&Instruction::Jmp {
            target: JmpTarget::Far {
                seg: HOOK_SEG,
                off: PARTICLE_MARKER_RESOLVER_OFF,
            },
        })
        .unwrap();
        assert!(
            entry
                .windows(resolver_jump.len())
                .any(|bytes| bytes == resolver_jump)
        );

        let draw = &hook[usize::from(HOOK_DRAW_OFF - HOOK_ENTRY_OFF)
            ..usize::from(HOOK_TOPIC_SELECTOR_OFF - HOOK_ENTRY_OFF)];
        let selector_call = encode_bytes(&Instruction::Call {
            target: CallTarget::Far {
                seg: HOOK_SEG,
                off: HOOK_TOPIC_SELECTOR_OFF,
            },
        })
        .unwrap();
        assert!(
            !draw
                .windows(selector_call.len())
                .any(|bytes| bytes == selector_call)
        );
    }

    #[test]
    fn marker_row_dispatches_before_grcg_drawing_and_rejoins_the_profile() {
        let hook = renderer_hook(RendererOverlay::Arle).unwrap();
        let resolver = &hook[usize::from(PARTICLE_MARKER_RESOLVER_OFF - HOOK_ENTRY_OFF)..];
        let selector_call = encode_bytes(&Instruction::Call {
            target: CallTarget::Far {
                seg: HOOK_SEG,
                off: HOOK_TOPIC_SELECTOR_OFF,
            },
        })
        .unwrap();
        assert!(
            resolver
                .windows(selector_call.len())
                .any(|bytes| bytes == selector_call)
        );
        let rejoin = encode_bytes(&Instruction::Jmp {
            target: JmpTarget::Far {
                seg: RENDERER_SEG,
                off: 0x53F5,
            },
        })
        .unwrap();
        assert!(resolver.ends_with(&rejoin));
    }

    #[test]
    fn draw_hook_requires_the_profile_text_return_address() {
        for (overlay, text_draw_return) in [
            (RendererOverlay::Arle, 0x5448),
            (RendererOverlay::Rulue, 0x5488),
            (RendererOverlay::Schezo, 0x5418),
        ] {
            let hook = renderer_hook(overlay).unwrap();
            let draw = &hook[usize::from(HOOK_DRAW_OFF - HOOK_ENTRY_OFF)
                ..usize::from(HOOK_TOPIC_SELECTOR_OFF - HOOK_ENTRY_OFF)];
            let provenance = encode_bytes(&Instruction::Cmp {
                a: based_memory(
                    Some(SegmentRegister::SS),
                    EffectiveAddressBase::Bp,
                    2,
                    OperandSize::Word,
                ),
                b: imm16(text_draw_return),
            })
            .unwrap();
            assert!(
                draw.windows(provenance.len())
                    .any(|bytes| bytes == provenance),
                "draw hook must reject stale captures from a non-text caller"
            );
        }
    }

    #[test]
    fn saved_jis_is_a_one_shot_draw_input() {
        let hook = renderer_hook(RendererOverlay::Arle).unwrap();
        let draw_requires_freshness = encode_bytes(&Instruction::Cmp {
            a: direct_memory(
                Some(SegmentRegister::CS),
                SCRATCH_ACTIVE_OFF,
                OperandSize::Byte,
            ),
            b: imm8(1),
        })
        .unwrap();
        let entry_marks_fresh = encode_bytes(&mov(
            direct_memory(
                Some(SegmentRegister::CS),
                SCRATCH_ACTIVE_OFF,
                OperandSize::Byte,
            ),
            imm8(1),
        ))
        .unwrap();
        let draw_consumes_freshness = encode_bytes(&mov(
            direct_memory(
                Some(SegmentRegister::CS),
                SCRATCH_ACTIVE_OFF,
                OperandSize::Byte,
            ),
            imm8(0),
        ))
        .unwrap();
        let draw_loads_saved_jis = encode_bytes(&mov(
            reg16(Register16::BX),
            direct_memory(Some(SegmentRegister::CS), SCRATCH_OFF, OperandSize::Word),
        ))
        .unwrap();

        assert!(
            hook[..usize::from(HOOK_DRAW_OFF - HOOK_ENTRY_OFF)]
                .windows(entry_marks_fresh.len())
                .any(|bytes| bytes == entry_marks_fresh),
            "the JIS fetch entry must mark the captured code fresh"
        );

        let draw = &hook[usize::from(HOOK_DRAW_OFF - HOOK_ENTRY_OFF)..];
        let freshness_check_at = draw
            .windows(draw_requires_freshness.len())
            .position(|bytes| bytes == draw_requires_freshness)
            .expect("the draw hook must test JIS freshness");
        assert!(
            draw.get(freshness_check_at + draw_requires_freshness.len()) == Some(&0x75),
            "a draw without a fresh JIS capture must branch to the original font path"
        );
        let consumed_at = draw
            .windows(draw_consumes_freshness.len())
            .position(|bytes| bytes == draw_consumes_freshness)
            .expect("the draw hook must consume JIS freshness");
        let loaded_at = draw
            .windows(draw_loads_saved_jis.len())
            .position(|bytes| bytes == draw_loads_saved_jis)
            .expect("the fresh draw path must load the captured JIS code");
        assert!(
            consumed_at < loaded_at,
            "freshness must be consumed before the captured JIS code is used"
        );
    }

    #[test]
    fn direct_draws_rejoin_each_original_font_loop() {
        let setup = [
            encode_bytes(&mov(reg8(Register8::AL), imm8(0xC0))).unwrap(),
            encode_bytes(&Instruction::OutAl {
                port: PortAddress::Imm8(0x7C),
            })
            .unwrap(),
            encode_bytes(&mov(reg16(Register16::CX), imm16(0x10))).unwrap(),
        ]
        .concat();

        for (overlay, original_font_loop) in [
            (RendererOverlay::Arle, 0x5469),
            (RendererOverlay::Rulue, 0x54A9),
            (RendererOverlay::Schezo, 0x5439),
        ] {
            let hook = renderer_hook(overlay).unwrap();
            let draw = &hook[usize::from(HOOK_DRAW_OFF - HOOK_ENTRY_OFF)
                ..usize::from(HOOK_TOPIC_SELECTOR_OFF - HOOK_ENTRY_OFF)];
            assert!(
                draw.starts_with(&setup),
                "draw hook must reproduce the overwritten renderer setup"
            );

            let rejoin = encode_bytes(&Instruction::Jmp {
                target: JmpTarget::Far {
                    seg: RENDERER_SEG,
                    off: original_font_loop,
                },
            })
            .unwrap();
            assert!(
                draw.windows(rejoin.len()).any(|bytes| bytes == rejoin),
                "direct draws must return to the selected overlay's source-font loop"
            );
        }
    }
}
