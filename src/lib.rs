//! PC-98 Madou Monogatari A.R.S Korean patch tooling.
//!
//! The crate owns the A.R.S media, extraction, reinsertion, and build paths.

use anyhow::{Context, Result};

pub mod approved_terminology;
pub mod arle_ending_graphics;
pub mod arle_opening_graphics;
pub mod bps;
pub mod bsamp_resource;
pub mod character_build;
pub mod cutscene_catalog;
pub mod cutscene_reinsert;
pub mod cutscene_text;
pub mod cutscene_translation;
pub mod dancer_thunder;
pub mod dat_catalog;
pub mod dat_repack;
pub mod demo_compositor;
pub mod demo_menu;
pub mod demo_selector;
pub mod demo_title;
pub mod dialogue_layout;
pub mod enemy_text;
pub mod failure_message;
pub mod fat12;
pub mod fat12_add;
pub mod fat12_replace;
pub mod font_build;
pub mod gaiji_table;
pub mod glyph_stats;
pub mod graphics_resource;
pub mod hangul_probe;
pub mod hdm;
pub mod hook_geometry;
pub mod josa;
pub mod lever_text;
pub mod media_identity;
pub mod overlay_batch;
pub mod overlay_catalog;
pub mod overlay_lz;
pub mod overlay_messages;
pub mod overlay_ptrtable;
pub mod overlay_reloc;
pub mod overlay_text;
pub mod protocol_patch;
pub mod received_damage_sfx;
pub mod release_patch;
pub mod renderer_control;
pub mod resource_audit;
pub mod rulue_credit_graphics;
pub mod rulue_interlude_graphics;
pub mod save_encounter;
pub mod scenario_title;
pub mod shop_sale;
pub mod sjis_marker;
pub mod sjis_sweep;
pub mod translation_binding;
pub mod two_disk_boot;
pub(crate) mod v30_assembler;

pub fn read_fat12_file_from_hdm(disk: &[u8], file_name: &str) -> Result<Vec<u8>> {
    let hdm = hdm::HdmFile::parse(disk).with_context(|| "parse HDM sector-linear image")?;
    let volume = fat12::Fat12Volume::open(hdm.as_bytes()).with_context(|| "open FAT12 volume")?;
    volume
        .read_file(file_name)
        .with_context(|| format!("read {file_name} from FAT12 image"))
}

pub mod illusion;
