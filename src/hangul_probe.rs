//! Minimal PC-98 text-plane Hangul probe for A.R.S.
//!
//! This is deliberately a one-glyph experiment: register JIS 0x7621 as a
//! 16x16 1bpp "ga" bitmap through PC-98 INT 18h AH=1Ah, then make the
//! boot prompt reference the matching Shift-JIS gaiji pair EB 9F.

use anyhow::{Context, Result, bail};
use v30::{
    Assembler, Condition, Instruction, JmpTarget, Register8, Register16, SegmentRegister,
    decode_bytes,
};

use crate::fat12_replace::{ReplaceReport, replace_file_in_place};
use crate::sjis_marker::{encode_sjis, replace_all_exact};
use crate::v30_assembler::{
    assemble_at, imm8, imm16, mov, near_displacement, pop, push, reg8, reg16, segment,
};

pub const FILE_MAIN_COM: &str = "MAIN.COM";
pub const FILE_GAOO_OVL: &str = "GAOO.OVL";
pub const GAIJI_JIS_7621_SJIS: [u8; 2] = [0xEB, 0x9F];
pub const GAIJI_GLYPH_BYTES: usize = 32;
/// PC-98 `INT 18h`, `AH=1Ah` reads a 34-byte pattern buffer. The first
/// two bytes describe a 16x16 double-byte glyph (`0x0202`), followed by the
/// 32-byte, two-bytes-per-row bitmap.
pub const GAIJI_PATTERN_HEADER: [u8; 2] = [0x02, 0x02];
pub const GAIJI_PATTERN_BYTES: usize = GAIJI_PATTERN_HEADER.len() + GAIJI_GLYPH_BYTES;

const COM_ENTRY_IP: u16 = 0x0100;

fn emit_main_entry_prologue(assembler: &mut Assembler) {
    assembler
        .emit(Instruction::Cld)
        .emit(Instruction::Pusha)
        .emit(mov(reg16(Register16::AX), segment(SegmentRegister::CS)))
        .emit(mov(segment(SegmentRegister::DS), reg16(Register16::AX)));
}

fn main_entry_prologue() -> Result<Vec<u8>> {
    let mut assembler = Assembler::new();
    emit_main_entry_prologue(&mut assembler);
    assemble_at(&assembler, COM_ENTRY_IP, "original MAIN.COM entry prologue")
}

fn main_entry_resume_ip() -> Result<u16> {
    let prologue_len = u16::try_from(main_entry_prologue()?.len())
        .context("MAIN.COM entry prologue is too large")?;
    COM_ENTRY_IP
        .checked_add(prologue_len)
        .context("MAIN.COM entry resume address overflow")
}

fn emit_gaiji_registration(assembler: &mut Assembler, glyph_ip: u16, jis: u16) {
    assembler
        .emit(mov(reg16(Register16::CX), imm16(glyph_ip)))
        .emit(mov(reg16(Register16::DX), imm16(jis)))
        .emit(mov(reg8(Register8::AH), imm8(0x1A)))
        .emit(Instruction::Int { vector: 0x18 });
}

fn finish_with_near_jump(
    assembler: &mut Assembler,
    origin: u16,
    target: u16,
    purpose: &str,
) -> Result<Vec<u8>> {
    let prefix = assemble_at(assembler, origin, purpose)?;
    let jump_ip = origin.wrapping_add(prefix.len() as u16);
    assembler.emit(Instruction::Jmp {
        target: JmpTarget::Rel16(near_displacement(jump_ip, target)),
    });
    assemble_at(assembler, origin, purpose)
}

fn patch_main_entry(main_com: &[u8], stub_ip: u16) -> Result<Vec<u8>> {
    let displaced = main_entry_prologue()?;
    if !main_com.starts_with(&displaced) {
        bail!("{FILE_MAIN_COM} entry bytes did not match expected A.R.S prologue");
    }
    let mut assembler = Assembler::new();
    assembler.emit(Instruction::Jmp {
        target: JmpTarget::Rel16(near_displacement(COM_ENTRY_IP, stub_ip)),
    });
    for _ in 3..displaced.len() {
        assembler.emit(Instruction::Nop);
    }
    let hijack = assemble_at(&assembler, COM_ENTRY_IP, "MAIN.COM entry hijack")?;
    if hijack.len() != displaced.len() {
        bail!("typed MAIN.COM entry hijack is not length preserving");
    }
    let mut out = main_com.to_vec();
    out[..hijack.len()].copy_from_slice(&hijack);
    Ok(out)
}

fn assemble_gaiji_registration_code(
    stub_ip: u16,
    glyph_base_ip: u16,
    glyphs: &[GaijiGlyph],
) -> Result<Vec<u8>> {
    let mut assembler = Assembler::new();
    assembler
        .emit(push(segment(SegmentRegister::CS)))
        .emit(pop(segment(SegmentRegister::DS)))
        .emit(push(segment(SegmentRegister::CS)))
        .emit(pop(reg16(Register16::BX)));
    for (index, glyph) in glyphs.iter().enumerate() {
        let glyph_ip = glyph_base_ip.wrapping_add((index * GAIJI_PATTERN_BYTES) as u16);
        emit_gaiji_registration(&mut assembler, glyph_ip, glyph.jis);
    }
    emit_main_entry_prologue(&mut assembler);
    finish_with_near_jump(
        &mut assembler,
        stub_ip,
        main_entry_resume_ip()?,
        "MAIN.COM gaiji registration stub",
    )
}

fn assemble_embedded_hook_loader_code(
    stub_ip: u16,
    sheet_ip: u16,
    hook_ip: u16,
    sheet_len: u16,
    hook_len: u16,
) -> Result<Vec<u8>> {
    let mut assembler = Assembler::new();
    assembler
        .emit(push(segment(SegmentRegister::ES)))
        .emit(push(segment(SegmentRegister::DS)))
        .emit(mov(
            reg16(Register16::AX),
            imm16(crate::hook_geometry::HOOK_SEG),
        ))
        .emit(mov(segment(SegmentRegister::ES), reg16(Register16::AX)))
        .emit(push(segment(SegmentRegister::CS)))
        .emit(pop(segment(SegmentRegister::DS)))
        .emit(mov(reg16(Register16::SI), imm16(sheet_ip)))
        .emit(Instruction::Xor {
            dest: reg16(Register16::DI),
            src: reg16(Register16::DI),
        })
        .emit(mov(reg16(Register16::CX), imm16(sheet_len)))
        .emit(Instruction::Cld)
        .emit(Instruction::Rep(Box::new(Instruction::Movsb)))
        .emit(mov(reg16(Register16::SI), imm16(hook_ip)))
        .emit(mov(
            reg16(Register16::DI),
            imm16(crate::hook_geometry::HOOK_ENTRY_OFF),
        ))
        .emit(mov(reg16(Register16::CX), imm16(hook_len)))
        .emit(Instruction::Rep(Box::new(Instruction::Movsb)))
        .emit(pop(segment(SegmentRegister::DS)))
        .emit(pop(segment(SegmentRegister::ES)));
    emit_main_entry_prologue(&mut assembler);
    finish_with_near_jump(
        &mut assembler,
        stub_ip,
        main_entry_resume_ip()?,
        "MAIN.COM embedded renderer-hook loader",
    )
}

fn assemble_external_sheet_loader_code(
    stub_ip: u16,
    name_ip: u16,
    hook_ip: u16,
    sheet_size: u16,
    hook_len: u16,
) -> Result<Vec<u8>> {
    let mut assembler = Assembler::new();
    assembler
        .emit(push(segment(SegmentRegister::DS)))
        .emit(push(segment(SegmentRegister::ES)))
        .emit(push(segment(SegmentRegister::CS)))
        .emit(pop(segment(SegmentRegister::DS)))
        .emit(mov(reg16(Register16::DX), imm16(name_ip)))
        .emit(mov(reg16(Register16::AX), imm16(0x3D00)))
        .emit(Instruction::Int { vector: 0x21 })
        .emit_branch(Condition::B, "copy_hook")
        .emit(mov(reg16(Register16::BX), reg16(Register16::AX)))
        .emit(push(segment(SegmentRegister::DS)))
        .emit(mov(
            reg16(Register16::AX),
            imm16(crate::hook_geometry::HOOK_SEG),
        ))
        .emit(mov(segment(SegmentRegister::DS), reg16(Register16::AX)))
        .emit(Instruction::Xor {
            dest: reg16(Register16::DX),
            src: reg16(Register16::DX),
        })
        .emit(mov(reg16(Register16::CX), imm16(sheet_size)))
        .emit(mov(reg8(Register8::AH), imm8(0x3F)))
        .emit(Instruction::Int { vector: 0x21 })
        .emit(pop(segment(SegmentRegister::DS)))
        .emit(mov(reg8(Register8::AH), imm8(0x3E)))
        .emit(Instruction::Int { vector: 0x21 })
        .label("copy_hook")
        .emit(mov(
            reg16(Register16::AX),
            imm16(crate::hook_geometry::HOOK_SEG),
        ))
        .emit(mov(segment(SegmentRegister::ES), reg16(Register16::AX)))
        .emit(mov(reg16(Register16::SI), imm16(hook_ip)))
        .emit(mov(
            reg16(Register16::DI),
            imm16(crate::hook_geometry::HOOK_ENTRY_OFF),
        ))
        .emit(mov(reg16(Register16::CX), imm16(hook_len)))
        .emit(Instruction::Cld)
        .emit(Instruction::Rep(Box::new(Instruction::Movsb)))
        .emit(pop(segment(SegmentRegister::ES)))
        .emit(pop(segment(SegmentRegister::DS)));
    emit_main_entry_prologue(&mut assembler);
    finish_with_near_jump(
        &mut assembler,
        stub_ip,
        main_entry_resume_ip()?,
        "MAIN.COM external sheet loader",
    )
}

fn assemble_combined_font_loader_code(
    stub_ip: u16,
    name_ip: u16,
    file_size: u16,
) -> Result<Vec<u8>> {
    let mut assembler = Assembler::new();
    assembler
        .emit(push(segment(SegmentRegister::DS)))
        .emit(push(segment(SegmentRegister::CS)))
        .emit(pop(segment(SegmentRegister::DS)))
        .emit(mov(reg16(Register16::DX), imm16(name_ip)))
        .emit(mov(reg16(Register16::AX), imm16(0x3D00)))
        .emit(Instruction::Int { vector: 0x21 })
        .emit_branch(Condition::B, "restore_ds")
        .emit(mov(reg16(Register16::BX), reg16(Register16::AX)))
        .emit(mov(
            reg16(Register16::AX),
            imm16(crate::hook_geometry::HOOK_SEG),
        ))
        .emit(mov(segment(SegmentRegister::DS), reg16(Register16::AX)))
        .emit(Instruction::Xor {
            dest: reg16(Register16::DX),
            src: reg16(Register16::DX),
        })
        .emit(mov(reg16(Register16::CX), imm16(file_size)))
        .emit(mov(reg8(Register8::AH), imm8(0x3F)))
        .emit(Instruction::Int { vector: 0x21 })
        .emit(mov(reg8(Register8::AH), imm8(0x3E)))
        .emit(Instruction::Int { vector: 0x21 })
        .label("restore_ds")
        .emit(pop(segment(SegmentRegister::DS)));
    emit_main_entry_prologue(&mut assembler);
    finish_with_near_jump(
        &mut assembler,
        stub_ip,
        main_entry_resume_ip()?,
        "MAIN.COM combined font loader",
    )
}

/// One 16x16 1bpp gaiji glyph (32 bytes, MSB-first, 2 bytes/row) and the PC-98
/// external-character JIS code it registers at.
#[derive(Debug, Clone)]
pub struct GaijiGlyph {
    pub jis: u16,
    pub bitmap: [u8; GAIJI_GLYPH_BYTES],
}

// 16x16 1bpp "가" glyph. Format is 16 rows x 2 bytes, MSB-left.
pub const HANGUL_GA_GLYPH_16X16_1BPP: [u8; 32] = [
    0x00, 0x00, 0x00, 0x00, 0x7C, 0x30, 0x0C, 0x30, 0x04, 0x30, 0x04, 0x30, 0x04, 0x30, 0x04, 0x3C,
    0x04, 0x30, 0x04, 0x30, 0x0C, 0x30, 0x38, 0x30, 0x00, 0x30, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

#[derive(Debug)]
pub struct HangulProbeReport {
    pub main_report: ReplaceReport,
    pub gaoo_report: ReplaceReport,
    pub gaoo_offsets: Vec<usize>,
}

pub fn patch_disk_for_hangul_probe(disk: &mut [u8]) -> Result<HangulProbeReport> {
    let main_com = crate::read_fat12_file_from_hdm(disk, FILE_MAIN_COM)?;
    let patched_main = patch_main_com_register_gaiji(&main_com)?;
    let main_report = replace_file_in_place(disk, FILE_MAIN_COM, &patched_main)
        .with_context(|| format!("replace {FILE_MAIN_COM}"))?;

    let mut gaoo = crate::read_fat12_file_from_hdm(disk, FILE_GAOO_OVL)?;
    let gaoo_offsets = patch_gaoo_boot_prompt_hangul_probe(&mut gaoo)?;
    let gaoo_report = replace_file_in_place(disk, FILE_GAOO_OVL, &gaoo)
        .with_context(|| format!("replace {FILE_GAOO_OVL}"))?;

    Ok(HangulProbeReport {
        main_report,
        gaoo_report,
        gaoo_offsets,
    })
}

pub fn patch_gaoo_boot_prompt_hangul_probe(gaoo: &mut [u8]) -> Result<Vec<usize>> {
    let from = encode_sjis("好きなドライブ")?;
    let mut to = encode_sjis("テストテストテ")?;
    if to.len() < GAIJI_JIS_7621_SJIS.len() {
        bail!("replacement marker is unexpectedly short");
    }
    to[..GAIJI_JIS_7621_SJIS.len()].copy_from_slice(&GAIJI_JIS_7621_SJIS);
    replace_all_exact(gaoo, &from, &to)
}

pub fn patch_main_com_register_gaiji(main_com: &[u8]) -> Result<Vec<u8>> {
    patch_main_com_register_gaiji_multi(
        main_com,
        &[GaijiGlyph {
            jis: 0x7621,
            bitmap: HANGUL_GA_GLYPH_16X16_1BPP,
        }],
    )
}

/// Append a boot stub that registers every glyph in `glyphs` through PC-98 BIOS
/// `INT 18h`, `AH=1Ah`, then resumes the original `MAIN.COM` entry. Each glyph
/// is registered at its own external-character JIS code, so a translated string
/// can reference many distinct gaiji slots.
pub fn patch_main_com_register_gaiji_multi(
    main_com: &[u8],
    glyphs: &[GaijiGlyph],
) -> Result<Vec<u8>> {
    if glyphs.is_empty() {
        bail!("no gaiji glyphs to register");
    }
    let stub_file_offset = u16::try_from(main_com.len())
        .with_context(|| format!("{FILE_MAIN_COM} is too large for a .COM near jump"))?;
    let stub_ip = COM_ENTRY_IP.wrapping_add(stub_file_offset);

    let provisional = assemble_gaiji_registration_code(stub_ip, 0, glyphs)?;
    let glyph_base_ip = stub_ip.wrapping_add(provisional.len() as u16);
    let mut stub = assemble_gaiji_registration_code(stub_ip, glyph_base_ip, glyphs)?;
    if stub.len() != provisional.len() {
        bail!("typed gaiji registration changed length after data placement");
    }
    for glyph in glyphs {
        stub.extend_from_slice(&GAIJI_PATTERN_HEADER);
        stub.extend_from_slice(&glyph.bitmap);
    }

    let mut out = patch_main_entry(main_com, stub_ip)?;
    out.extend_from_slice(&stub);
    Ok(out)
}

/// Append a boot stub that copies the renderer-hook code to `0x8800:0x8000` and a
/// glyph sheet to `0x8800:0x0000`, then resumes the original `MAIN.COM` entry.
/// The packed-overlay trampolines far-jmp into the copied hook at render time.
pub fn patch_main_com_install_hook(main_com: &[u8], hook: &[u8], sheet: &[u8]) -> Result<Vec<u8>> {
    let sheet_len = u16::try_from(sheet.len()).context("sheet too large")?;
    let hook_len = u16::try_from(hook.len()).context("hook too large")?;

    let stub_file_offset = u16::try_from(main_com.len())
        .with_context(|| format!("{FILE_MAIN_COM} is too large for a .COM near jump"))?;
    let stub_ip = COM_ENTRY_IP.wrapping_add(stub_file_offset);

    let provisional = assemble_embedded_hook_loader_code(stub_ip, 0, 0, sheet_len, hook_len)?;
    let sheet_ip = stub_ip.wrapping_add(provisional.len() as u16);
    let hook_ip = sheet_ip.wrapping_add(sheet_len);

    let mut stub =
        assemble_embedded_hook_loader_code(stub_ip, sheet_ip, hook_ip, sheet_len, hook_len)?;
    if stub.len() != provisional.len() {
        bail!("typed embedded hook loader changed length after data placement");
    }
    stub.extend_from_slice(sheet);
    stub.extend_from_slice(hook);

    let mut out = patch_main_entry(main_com, stub_ip)?;
    out.extend_from_slice(&stub);
    Ok(out)
}

/// Append a boot stub that loads a glyph sheet file into `0x8800:0` via the DOS
/// `INT 21h` file API (the same path MAIN.COM uses for its own overlays), copies
/// the renderer hook to `0x8800:0x8000`, then resumes the original entry. Unlike
/// `patch_main_com_install_hook`, the sheet is read from disk (`filename`), so a
/// full glyph sheet ships as a separate file instead of being embedded.
pub fn patch_main_com_load_sheet_hook(
    main_com: &[u8],
    hook: &[u8],
    sheet_size: u16,
    filename: &str,
) -> Result<Vec<u8>> {
    let hook_len = u16::try_from(hook.len()).context("hook too large")?;
    let mut name = filename.as_bytes().to_vec();
    name.push(0); // ASCIIZ for INT 21h AH=3D
    let name_len = u16::try_from(name.len()).context("filename too long")?;

    let stub_file_offset = u16::try_from(main_com.len())
        .with_context(|| format!("{FILE_MAIN_COM} is too large for a .COM near jump"))?;
    let stub_ip = COM_ENTRY_IP.wrapping_add(stub_file_offset);

    let provisional = assemble_external_sheet_loader_code(stub_ip, 0, 0, sheet_size, hook_len)?;
    let name_ip = stub_ip.wrapping_add(provisional.len() as u16);
    let hook_ip = name_ip.wrapping_add(name_len);

    let mut stub =
        assemble_external_sheet_loader_code(stub_ip, name_ip, hook_ip, sheet_size, hook_len)?;
    if stub.len() != provisional.len() {
        bail!("typed external sheet loader changed length after data placement");
    }
    stub.extend_from_slice(&name);
    stub.extend_from_slice(hook);

    let mut out = patch_main_entry(main_com, stub_ip)?;
    out.extend_from_slice(&stub);
    Ok(out)
}

/// Append a boot stub that loads a combined sheet+hooks file (`filename`,
/// `file_size` bytes) into `0x8800:0` via the DOS `INT 21h` file API, then
/// resumes. The file already places the sheet at offset 0 and the per-overlay
/// hook blobs at their `0x8000`+ offsets, so no separate hook copy is needed --
/// this is the multi-character build where each overlay's trampolines point at
/// its own hook blob inside the loaded image.
pub fn patch_main_com_load_combined(
    main_com: &[u8],
    file_size: u16,
    filename: &str,
) -> Result<Vec<u8>> {
    let mut name = filename.as_bytes().to_vec();
    name.push(0);
    u16::try_from(name.len()).context("filename too long")?;

    let stub_file_offset = u16::try_from(main_com.len())
        .with_context(|| format!("{FILE_MAIN_COM} is too large for a .COM near jump"))?;
    let stub_ip = COM_ENTRY_IP.wrapping_add(stub_file_offset);

    let provisional = assemble_combined_font_loader_code(stub_ip, 0, file_size)?;
    let name_ip = stub_ip.wrapping_add(provisional.len() as u16);

    let mut stub = assemble_combined_font_loader_code(stub_ip, name_ip, file_size)?;
    if stub.len() != provisional.len() {
        bail!("typed combined font loader changed length after data placement");
    }
    stub.extend_from_slice(&name);

    let mut out = patch_main_entry(main_com, stub_ip)?;
    out.extend_from_slice(&stub);
    Ok(out)
}

/// Retarget a loader previously emitted by [`patch_main_com_load_combined`].
///
/// The existing image is reconstructed and compared byte-for-byte before the
/// size changes. This avoids a loose signature patch accidentally treating an
/// unrelated or manually modified `MAIN.COM` as the production loader.
pub fn retarget_main_com_load_combined(
    main_com: &[u8],
    expected_file_size: u16,
    new_file_size: u16,
    filename: &str,
) -> Result<Vec<u8>> {
    let entry = decode_bytes(main_com).context("decode combined-loader MAIN.COM entry")?;
    let Instruction::Jmp {
        target: JmpTarget::Rel16(displacement),
    } = entry.instruction
    else {
        bail!("{FILE_MAIN_COM} does not start with a typed near combined-loader jump");
    };
    let target_ip = COM_ENTRY_IP as i32 + entry.byte_len as i32 + i32::from(displacement);
    if !(COM_ENTRY_IP as i32..=u16::MAX as i32).contains(&target_ip) {
        bail!("{FILE_MAIN_COM} combined-loader jump target is outside the .COM image");
    }
    let stub_offset = target_ip as usize - COM_ENTRY_IP as usize;
    let prologue = main_entry_prologue()?;
    if stub_offset < prologue.len() || stub_offset >= main_com.len() {
        bail!("{FILE_MAIN_COM} combined-loader stub offset is outside the file");
    }

    let mut original = main_com[..stub_offset].to_vec();
    original[..prologue.len()].copy_from_slice(&prologue);
    let expected = patch_main_com_load_combined(&original, expected_file_size, filename)?;
    if expected != main_com {
        bail!(
            "{FILE_MAIN_COM} is not the expected {filename} combined loader for {expected_file_size} bytes"
        );
    }
    patch_main_com_load_combined(&original, new_file_size, filename)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_main_com() -> Vec<u8> {
        let mut main = main_entry_prologue().unwrap();
        main.extend(std::iter::repeat_n(0x90, 100));
        main
    }

    #[test]
    fn single_glyph_is_the_n1_case() {
        let main = fake_main_com();
        let single = patch_main_com_register_gaiji(&main).unwrap();
        let multi = patch_main_com_register_gaiji_multi(
            &main,
            &[GaijiGlyph {
                jis: 0x7621,
                bitmap: HANGUL_GA_GLYPH_16X16_1BPP,
            }],
        )
        .unwrap();
        assert_eq!(single, multi);
    }

    #[test]
    fn multi_glyph_stub_hijacks_entry_and_appends_all_bios_patterns() {
        let main = fake_main_com();
        let glyphs = [
            GaijiGlyph {
                jis: 0x7621,
                bitmap: [0xAA; GAIJI_GLYPH_BYTES],
            },
            GaijiGlyph {
                jis: 0x7622,
                bitmap: [0xBB; GAIJI_GLYPH_BYTES],
            },
            GaijiGlyph {
                jis: 0x7623,
                bitmap: [0xCC; GAIJI_GLYPH_BYTES],
            },
        ];
        let out = patch_main_com_register_gaiji_multi(&main, &glyphs).unwrap();

        let entry = decode_bytes(&out).unwrap();
        let Instruction::Jmp {
            target: JmpTarget::Rel16(displacement),
        } = entry.instruction
        else {
            panic!("MAIN.COM entry is not a typed near jump");
        };
        let target_ip = COM_ENTRY_IP as i32 + entry.byte_len as i32 + i32::from(displacement);
        assert_eq!(target_ip as usize - COM_ENTRY_IP as usize, main.len());
        let prologue = main_entry_prologue().unwrap();
        assert!(
            out[entry.byte_len..prologue.len()]
                .iter()
                .all(|byte| *byte == 0x90)
        );

        // The displaced prologue is preserved inside the stub so boot resumes.
        assert!(
            out[main.len()..]
                .windows(prologue.len())
                .any(|window| window == prologue)
        );

        let patterns = glyphs
            .iter()
            .flat_map(|glyph| {
                GAIJI_PATTERN_HEADER
                    .iter()
                    .chain(glyph.bitmap.iter())
                    .copied()
            })
            .collect::<Vec<_>>();
        assert!(out.ends_with(&patterns));
    }

    #[test]
    fn combined_loader_retargets_only_an_exact_prior_build() {
        let main = fake_main_com();
        let old = patch_main_com_load_combined(&main, 0x8096, "KFONT.BIN").unwrap();

        let updated = retarget_main_com_load_combined(&old, 0x8096, 0xA1C8, "KFONT.BIN").unwrap();

        assert_eq!(
            updated,
            patch_main_com_load_combined(&main, 0xA1C8, "KFONT.BIN").unwrap()
        );
        assert!(retarget_main_com_load_combined(&old, 0x8095, 0xA1C8, "KFONT.BIN").is_err());
        let mut corrupt = old;
        *corrupt.last_mut().unwrap() ^= 0xFF;
        assert!(retarget_main_com_load_combined(&corrupt, 0x8096, 0xA1C8, "KFONT.BIN").is_err());
    }

    #[test]
    fn rejects_wrong_prologue() {
        let main = vec![0x00; 64];
        assert!(patch_main_com_register_gaiji(&main).is_err());
    }
}
