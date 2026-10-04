//! Isolated playback for received-damage and combo S0 samples.
//!
//! The original A/R/S routes keep the character `*_S0.CNS` bank in GVRAM page
//! 1 and let `BSAMP.COM` consume the selected track from there while the game
//! waits for playback to finish. The Korean patch has interrupt-driven
//! renderer work that does not belong in that shared GVRAM lifetime. For the
//! selected S0 records, this module copies the selected length-prefixed
//! track to a bounded conventional-RAM scratch range with interrupts masked,
//! restores GVRAM page 0, and then starts the same BSAMP payload from RAM.

use anyhow::{Context, Result, bail};
use v30::{
    Assembler, CallTarget, Condition, Instruction, JmpTarget, OperandSize, PortAddress, Register8,
    Register16, SegmentRegister,
};

use crate::{
    hook_geometry::{HOOK_SEG, RendererOverlay, S0_DAMAGE_PLAYER_OFF},
    v30_assembler::{
        assemble_at, based_memory, imm8, imm16, mov, near_displacement, pop, push, reg8, reg16,
        segment,
    },
};

/// Start of the runtime-only copy of one length-prefixed S0 track.
pub const S0_DAMAGE_TRACK_SCRATCH_OFF: u16 = 0xB000;

/// Largest payload selected by the exact A/R/S damage and combo records.
pub const S0_DAMAGE_MAX_SELECTED_PAYLOAD_BYTES: u16 = 5_884;

/// Highest S0 entry selected by the exact damage and combo consumers.
const S0_DAMAGE_MAX_SELECTED_INDEX: u8 = 18;

/// Space reserved in the proven overlay relocation band for the routing stub.
pub const S0_DAMAGE_OVERLAY_STUB_BYTES: usize = 16;

#[derive(Debug, Clone, Copy)]
struct ReceivedDamageRouteProfile {
    label: &'static str,
    indexed_sfx_logical: u16,
    call_decoded_offset: usize,
    call_logical_offset: u16,
    source_call: [u8; 5],
    damage_call_decoded_offset: usize,
    damage_source_call: [u8; 5],
}

impl ReceivedDamageRouteProfile {
    fn for_overlay(overlay: RendererOverlay) -> Self {
        match overlay {
            RendererOverlay::Arle => Self {
                label: "Arle",
                indexed_sfx_logical: 0x042B,
                call_decoded_offset: 0x348A,
                damage_call_decoded_offset: 0x0F4A,
                damage_source_call: [0xB1, 0x01, 0xE8, 0xDC, 0xF3],
                call_logical_offset: 0x358A,
                source_call: [0xB1, 0x00, 0xE8, 0x9C, 0xCE],
            },
            RendererOverlay::Rulue => Self {
                label: "Rulue",
                indexed_sfx_logical: 0x0444,
                call_decoded_offset: 0x3483,
                damage_call_decoded_offset: 0x0F8C,
                damage_source_call: [0xB1, 0x01, 0xE8, 0xB3, 0xF3],
                call_logical_offset: 0x3583,
                source_call: [0xB1, 0x00, 0xE8, 0xBC, 0xCE],
            },
            RendererOverlay::Schezo => Self {
                label: "Schezo",
                indexed_sfx_logical: 0x0439,
                call_decoded_offset: 0x342A,
                damage_call_decoded_offset: 0x0F6B,
                damage_source_call: [0xB1, 0x01, 0xE8, 0xC9, 0xF3],
                call_logical_offset: 0x352A,
                source_call: [0xB1, 0x00, 0xE8, 0x0A, 0xCF],
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S0DamageRoutePatch {
    /// Start of the combo CL setup and call.
    pub call_decoded_offset: usize,
    /// Start of the received-damage CL setup and call.
    pub damage_call_decoded_offset: usize,
    pub stub_decoded_offset: usize,
    pub stub_logical_offset: u16,
}

/// Reserve the routing stub after translated strings and the topic-particle
/// stub. The returned end is the upper bound available to earlier consumers.
pub fn reserve_s0_damage_stub(reloc_base: usize, reloc_end: usize) -> Result<usize> {
    let end = reloc_end
        .checked_sub(S0_DAMAGE_OVERLAY_STUB_BYTES)
        .context("relocation end is smaller than the S0 damage routing stub")?;
    if end <= reloc_base {
        bail!(
            "relocation band 0x{reloc_base:04X}..0x{reloc_end:04X} has no room for the S0 damage routing stub"
        );
    }
    Ok(end)
}

/// Redirect exact damage and combo calls through one near stub. S0
/// selectors enter the RAM-isolated player; alternate-bank and silent
/// selectors retain that overlay's source routine.
pub fn install_s0_damage_route(
    decoded: &mut Vec<u8>,
    overlay: RendererOverlay,
    load_offset: usize,
    reloc_base: usize,
    reloc_end: usize,
) -> Result<S0DamageRoutePatch> {
    let stub_decoded_offset = decoded.len().max(reloc_base);
    let stub_end = stub_decoded_offset
        .checked_add(S0_DAMAGE_OVERLAY_STUB_BYTES)
        .context("S0 damage routing stub range overflow")?;
    if stub_end > reloc_end {
        bail!(
            "S0 damage routing stub 0x{stub_decoded_offset:04X}..0x{stub_end:04X} exceeds relocation end 0x{reloc_end:04X}"
        );
    }
    decoded.resize(stub_end, 0);
    install_s0_damage_route_at(decoded, overlay, load_offset, stub_decoded_offset)
}

/// Redirect the received-damage route through an explicitly owned,
/// already-allocated source-overlay slot.
pub fn install_s0_damage_route_at(
    decoded: &mut [u8],
    overlay: RendererOverlay,
    load_offset: usize,
    stub_decoded_offset: usize,
) -> Result<S0DamageRoutePatch> {
    let profile = ReceivedDamageRouteProfile::for_overlay(overlay);
    let calls = [
        (profile.call_decoded_offset, profile.source_call),
        (
            profile.damage_call_decoded_offset,
            profile.damage_source_call,
        ),
    ];
    for (offset, expected) in calls {
        let source = decoded
            .get(offset..offset + expected.len())
            .with_context(|| format!("{} S0 call is truncated", profile.label))?;
        if source != expected {
            bail!(
                "{} S0 call at decoded 0x{offset:04X} does not match the source",
                profile.label
            );
        }
    }
    if profile.call_decoded_offset + load_offset != usize::from(profile.call_logical_offset) {
        bail!("{} S0 call load origin drifted", profile.label);
    }

    let stub_logical_offset = stub_decoded_offset
        .checked_add(load_offset)
        .context("S0 damage routing stub logical offset overflow")?;
    let stub_logical_offset =
        u16::try_from(stub_logical_offset).context("S0 damage routing stub exceeds 16 bits")?;
    let stub = assemble_s0_damage_overlay_stub(stub_logical_offset, profile)?;
    if stub.len() != S0_DAMAGE_OVERLAY_STUB_BYTES {
        bail!(
            "S0 damage routing stub is {} bytes, expected {S0_DAMAGE_OVERLAY_STUB_BYTES}",
            stub.len()
        );
    }
    let stub_end = stub_decoded_offset
        .checked_add(stub.len())
        .context("S0 damage routing stub range overflow")?;
    let target = decoded
        .get(stub_decoded_offset..stub_end)
        .context("S0 damage routing source-slot reservation lies outside the overlay")?;
    if !target.iter().all(|byte| *byte == 0) {
        bail!(
            "S0 damage routing source-slot reservation 0x{stub_decoded_offset:04X}..0x{stub_end:04X} is not empty"
        );
    }

    let mut planned = decoded.to_vec();
    planned[stub_decoded_offset..stub_end].copy_from_slice(&stub);
    for (offset, _) in calls {
        let call_ip = u16::try_from(offset + load_offset + 2).context("S0 call exceeds 16 bits")?;
        let mut call_assembler = Assembler::new();
        call_assembler.emit(Instruction::Call {
            target: CallTarget::Rel16(near_displacement(call_ip, stub_logical_offset)),
        });
        let call = assemble_at(&call_assembler, call_ip, "S0 routing call")?;
        if call.len() != 3 {
            bail!("S0 routing call is not length preserving");
        }
        planned[offset + 2..offset + 5].copy_from_slice(&call);
        if near_call_target(&planned, offset + 2, load_offset)? != stub_logical_offset {
            bail!("S0 routing call did not resolve to its stub");
        }
    }
    decoded.copy_from_slice(&planned);

    Ok(S0DamageRoutePatch {
        call_decoded_offset: profile.call_decoded_offset,
        damage_call_decoded_offset: profile.damage_call_decoded_offset,
        stub_decoded_offset,
        stub_logical_offset,
    })
}

fn assemble_s0_damage_overlay_stub(
    stub_ip: u16,
    profile: ReceivedDamageRouteProfile,
) -> Result<Vec<u8>> {
    let mut assembler = Assembler::new();
    assembler
        .emit(Instruction::Test {
            a: reg8(Register8::CH),
            b: imm8(0x40),
        })
        .emit_branch(Condition::Ne, "source_route")
        .emit(Instruction::Call {
            target: CallTarget::Far {
                seg: HOOK_SEG,
                off: S0_DAMAGE_PLAYER_OFF,
            },
        })
        .emit(Instruction::Ret { pop: 0 })
        .label("source_route");
    let prefix = assemble_at(
        &assembler,
        stub_ip,
        &format!("{} S0 damage routing stub prefix", profile.label),
    )?;
    let jump_ip = stub_ip.wrapping_add(prefix.len() as u16);
    assembler.emit(Instruction::Jmp {
        target: JmpTarget::Rel16(near_displacement(jump_ip, profile.indexed_sfx_logical)),
    });
    let assembled = assemble_at(
        &assembler,
        stub_ip,
        &format!("{} S0 damage routing stub", profile.label),
    )?;
    if assembled.len() > S0_DAMAGE_OVERLAY_STUB_BYTES {
        bail!("typed S0 damage routing stub exceeds its reserved slot");
    }
    for _ in assembled.len()..S0_DAMAGE_OVERLAY_STUB_BYTES {
        assembler.emit(Instruction::Nop);
    }
    assemble_at(
        &assembler,
        stub_ip,
        &format!("padded {} S0 damage routing stub", profile.label),
    )
}

/// Assemble the external player shared by all three character hook images.
pub(crate) fn assemble_s0_damage_player() -> Result<Vec<u8>> {
    let mut assembler = Assembler::new();
    assembler
        .emit(Instruction::Pushf)
        .emit(Instruction::Pusha)
        .emit(push(segment(SegmentRegister::DS)))
        .emit(push(segment(SegmentRegister::ES)))
        // Preserve the source sound-availability guard.
        .emit(mov(reg8(Register8::AH), imm8(5)))
        .emit(Instruction::Int { vector: 0x7D })
        .emit(Instruction::Test {
            a: reg8(Register8::AL),
            b: imm8(4),
        })
        .emit_branch(Condition::Ne, "sound_ready")
        .emit(Instruction::Test {
            a: reg8(Register8::AH),
            b: imm8(4),
        })
        .emit_branch(Condition::E, "done")
        .label("sound_ready")
        .emit(mov(reg8(Register8::AH), imm8(5)))
        .emit(Instruction::Int { vector: 0x7A })
        // Damage selects through 18 (A/S); combo selects through 14.
        // FF remains silent, and unsupported indices never access a bank.
        .emit(Instruction::Cmp {
            a: reg8(Register8::CH),
            b: imm8(S0_DAMAGE_MAX_SELECTED_INDEX),
        })
        .emit_branch(Condition::A, "done")
        .emit(Instruction::Xor {
            dest: reg16(Register16::BP),
            src: reg16(Register16::BP),
        })
        // Hold page 1 only for the bounded copy. No interrupt-driven renderer
        // can observe or write through the temporary page selection.
        .emit(Instruction::Pushf)
        .emit(Instruction::Cli)
        .emit(mov(reg16(Register16::AX), imm16(1)))
        .emit(Instruction::OutAl {
            port: PortAddress::Imm8(0xA6),
        })
        .emit(mov(reg8(Register8::AL), reg8(Register8::AH)))
        .emit(Instruction::OutAl {
            port: PortAddress::Imm8(0xA4),
        })
        .emit(mov(reg16(Register16::AX), imm16(0xA800)))
        .emit(mov(segment(SegmentRegister::DS), reg16(Register16::AX)))
        .emit(mov(reg8(Register8::AL), reg8(Register8::CH)))
        .emit(Instruction::Xor {
            dest: reg8(Register8::AH),
            src: reg8(Register8::AH),
        })
        .emit(Instruction::Shl {
            dest: reg16(Register16::AX),
            count: v30::ShiftCount::One,
        })
        .emit(Instruction::Shl {
            dest: reg16(Register16::AX),
            count: v30::ShiftCount::One,
        })
        .emit(mov(reg16(Register16::SI), reg16(Register16::AX)))
        .emit(mov(
            reg16(Register16::BX),
            based_memory(None, v30::EffectiveAddressBase::Si, 0, OperandSize::Word),
        ))
        .emit(Instruction::Test {
            a: reg16(Register16::BX),
            b: reg16(Register16::BX),
        })
        .emit_branch(Condition::E, "restore_page")
        .emit(mov(
            reg16(Register16::CX),
            based_memory(None, v30::EffectiveAddressBase::Bx, 0, OperandSize::Word),
        ))
        .emit(Instruction::Cmp {
            a: reg16(Register16::CX),
            b: imm16(S0_DAMAGE_MAX_SELECTED_PAYLOAD_BYTES),
        })
        .emit_branch(Condition::A, "restore_page")
        .emit(Instruction::Add {
            dest: reg16(Register16::CX),
            src: imm16(2),
        })
        .emit(mov(reg16(Register16::SI), reg16(Register16::BX)))
        .emit(mov(reg16(Register16::AX), imm16(HOOK_SEG)))
        .emit(mov(segment(SegmentRegister::ES), reg16(Register16::AX)))
        .emit(mov(
            reg16(Register16::DI),
            imm16(S0_DAMAGE_TRACK_SCRATCH_OFF),
        ))
        .emit(Instruction::Cld)
        .emit(Instruction::Rep(Box::new(Instruction::Movsb)))
        .emit(mov(reg16(Register16::BP), imm16(1)))
        .label("restore_page")
        .emit(Instruction::Xor {
            dest: reg16(Register16::AX),
            src: reg16(Register16::AX),
        })
        .emit(Instruction::OutAl {
            port: PortAddress::Imm8(0xA6),
        })
        .emit(Instruction::OutAl {
            port: PortAddress::Imm8(0xA4),
        })
        .emit(Instruction::Popf)
        .emit(Instruction::Test {
            a: reg16(Register16::BP),
            b: reg16(Register16::BP),
        })
        .emit_branch(Condition::E, "done")
        // Start the copied track through the resident INT 7Dh/AH=4 wrapper,
        // as the source consumer does. BSAMP.COM has no separate stop
        // service: INT 7Eh/AH=0 replaces the current sample object, while
        // INT 7Eh/AH=4 with CL=0 stores AL as the player's output mode. Never
        // call the mode service here; it would discard the player's option.
        .emit(mov(reg16(Register16::AX), imm16(HOOK_SEG)))
        .emit(mov(segment(SegmentRegister::ES), reg16(Register16::AX)))
        .emit(mov(
            reg16(Register16::BX),
            imm16(S0_DAMAGE_TRACK_SCRATCH_OFF),
        ))
        .emit(Instruction::Xor {
            dest: reg16(Register16::DX),
            src: reg16(Register16::DX),
        })
        .emit(mov(reg8(Register8::AH), imm8(4)))
        .emit(Instruction::Int { vector: 0x7D })
        .emit(mov(reg8(Register8::AH), imm8(8)))
        .emit(Instruction::Int { vector: 0x7D })
        .emit(mov(reg8(Register8::AH), imm8(4)))
        .emit(Instruction::Int { vector: 0x7A })
        .label("done")
        .emit(pop(segment(SegmentRegister::ES)))
        .emit(pop(segment(SegmentRegister::DS)))
        .emit(Instruction::Popa)
        .emit(Instruction::Popf)
        .emit(Instruction::Retf { pop: 0 });
    assemble_at(
        &assembler,
        S0_DAMAGE_PLAYER_OFF,
        "isolated S0 damage player",
    )
}

fn near_call_target(decoded: &[u8], call_decoded_offset: usize, load_offset: usize) -> Result<u16> {
    let call = decoded
        .get(call_decoded_offset..call_decoded_offset + 3)
        .context("S0 damage near call is truncated")?;
    if call[0] != 0xE8 {
        bail!("S0 damage route no longer contains a near call");
    }
    let relative = i16::from_le_bytes([call[1], call[2]]);
    let next = (call_decoded_offset + load_offset + 3) as u16;
    Ok(next.wrapping_add_signed(relative))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_stub_sends_s0_to_ram_player_and_other_selectors_to_source_consumer() {
        for overlay in [
            RendererOverlay::Arle,
            RendererOverlay::Rulue,
            RendererOverlay::Schezo,
        ] {
            let profile = ReceivedDamageRouteProfile::for_overlay(overlay);
            let mut decoded = vec![0; 0x3600];
            decoded[profile.call_decoded_offset..profile.call_decoded_offset + 5]
                .copy_from_slice(&profile.source_call);
            decoded[profile.damage_call_decoded_offset..profile.damage_call_decoded_offset + 5]
                .copy_from_slice(&profile.damage_source_call);
            let report =
                install_s0_damage_route(&mut decoded, overlay, 0x100, 0x4000, 0x4100).unwrap();
            assert_eq!(report.call_decoded_offset, profile.call_decoded_offset);
            assert_eq!(
                near_call_target(&decoded, profile.damage_call_decoded_offset + 2, 0x100).unwrap(),
                0x4100
            );
            assert_eq!(decoded[profile.damage_call_decoded_offset + 1], 1);
            assert_eq!(report.stub_decoded_offset, 0x4000);
            assert_eq!(report.stub_logical_offset, 0x4100);
            assert_eq!(
                near_call_target(&decoded, profile.call_decoded_offset + 2, 0x100).unwrap(),
                0x4100
            );

            let stub = &decoded[0x4000..0x4000 + S0_DAMAGE_OVERLAY_STUB_BYTES];
            let far_call = v30::encode_bytes(&Instruction::Call {
                target: CallTarget::Far {
                    seg: HOOK_SEG,
                    off: S0_DAMAGE_PLAYER_OFF,
                },
            })
            .unwrap();
            assert!(stub.windows(far_call.len()).any(|bytes| bytes == far_call));
            let source_jump = v30::encode_bytes(&Instruction::Jmp {
                target: JmpTarget::Rel16(near_displacement(0x410B, profile.indexed_sfx_logical)),
            })
            .unwrap();
            assert!(
                stub.windows(source_jump.len())
                    .any(|bytes| bytes == source_jump)
            );
        }
    }

    #[test]
    fn player_never_calls_the_bsamp_output_mode_service() {
        // BSAMP.COM INT 7Eh/AH=4 with CL=0 stores AL as the output mode, so a
        // direct INT 7Eh would overwrite the player's sampling option.
        let player = assemble_s0_damage_player().unwrap();
        let direct_bsamp = v30::encode_bytes(&Instruction::Int { vector: 0x7E }).unwrap();
        assert!(
            !player
                .windows(direct_bsamp.len())
                .any(|bytes| bytes == direct_bsamp)
        );
    }

    #[test]
    fn player_bounds_copy_and_restores_page_before_bsamp_start() {
        let player = assemble_s0_damage_player().unwrap();
        let page_one = v30::encode_bytes(&mov(reg16(Register16::AX), imm16(1))).unwrap();
        let copy = v30::encode_bytes(&Instruction::Rep(Box::new(Instruction::Movsb))).unwrap();
        let page_zero = v30::encode_bytes(&Instruction::Xor {
            dest: reg16(Register16::AX),
            src: reg16(Register16::AX),
        })
        .unwrap();
        let scratch = v30::encode_bytes(&mov(
            reg16(Register16::DI),
            imm16(S0_DAMAGE_TRACK_SCRATCH_OFF),
        ))
        .unwrap();
        let start = v30::encode_bytes(&mov(reg8(Register8::AH), imm8(4))).unwrap();

        let page_one_at = player
            .windows(page_one.len())
            .position(|bytes| bytes == page_one)
            .unwrap();
        let scratch_at = player
            .windows(scratch.len())
            .position(|bytes| bytes == scratch)
            .unwrap();
        let copy_at = player
            .windows(copy.len())
            .position(|bytes| bytes == copy)
            .unwrap();
        let page_zero_at = player
            .windows(page_zero.len())
            .position(|bytes| bytes == page_zero)
            .unwrap();
        let start_at = player
            .windows(start.len())
            .rposition(|bytes| bytes == start)
            .unwrap();
        assert!(page_one_at < scratch_at && scratch_at < copy_at);
        assert!(copy_at < page_zero_at && page_zero_at < start_at);
        assert_eq!(
            S0_DAMAGE_TRACK_SCRATCH_OFF + S0_DAMAGE_MAX_SELECTED_PAYLOAD_BYTES + 2,
            0xC6FE
        );
        assert!(
            crate::hook_geometry::HOOK_BLOCK_PHYSICAL_BASE
                + usize::from(
                    S0_DAMAGE_TRACK_SCRATCH_OFF + S0_DAMAGE_MAX_SELECTED_PAYLOAD_BYTES + 2,
                )
                <= crate::hook_geometry::HOOK_BLOCK_PHYSICAL_END
        );
    }
}
