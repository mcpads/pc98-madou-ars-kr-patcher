//! Deterministic font derivation for the production build.
//!
//! The shipping pipeline derives both renderer-hook sheets and phase-local
//! BIOS gaiji tables from the selected translation tree. Generated font files
//! are disposable build products, never an independently edited input.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use fontdue::{Font, FontSettings};
use serde_json::{Value, json};

use crate::cutscene_catalog::{BIOS_GAIJI_CAPACITY, CUTSCENE_RESOURCES};
use crate::media_identity::sha256_hex;

pub const PROFILE_SCHEMA: &str = "pc98_madou_ars.font_profile.v1";
pub const GLYPH_WIDTH: usize = 16;
pub const GLYPH_HEIGHT: usize = 16;
pub const GLYPH_BYTES: usize = 32;
pub const RENDERER_BASE_ROW: u8 = 0x75;
pub const RENDERER_ROWS: usize = 10;
pub const CELLS_PER_ROW: usize = 94;
pub const RENDERER_CAPACITY: usize = RENDERER_ROWS * CELLS_PER_ROW;
pub const RUNTIME_RENDERER_SYLLABLES: [char; 8] = [
    crate::josa::TOPIC_NO_FINAL,
    crate::josa::TOPIC_FINAL,
    crate::josa::OBJECT_NO_FINAL,
    crate::josa::OBJECT_FINAL,
    crate::josa::SUBJECT_NO_FINAL,
    crate::josa::SUBJECT_FINAL,
    crate::josa::WITH_NO_FINAL,
    crate::josa::WITH_FINAL,
];
const FONTDUE_VERSION: &str = "0.9.3";

#[derive(Debug, Clone)]
pub struct FontProfile {
    pub profile_path: PathBuf,
    pub id: String,
    pub font_name: String,
    pub font_path: PathBuf,
    pub font_sha256: String,
    pub font_version: String,
    pub font_size: u16,
    pub baseline_y: u8,
    pub threshold: u8,
    pub license: String,
    pub source: String,
    font_bytes: Vec<u8>,
}

#[derive(Debug)]
pub struct RendererFont {
    pub bytes: Vec<u8>,
    pub metadata: Value,
    pub glyphs: usize,
}

#[derive(Debug)]
pub struct GaijiFont {
    pub metadata: Value,
    pub glyphs: usize,
}

#[derive(Debug)]
pub struct FullBuildFonts {
    pub renderer_bin: PathBuf,
    pub renderer_json: PathBuf,
    pub cutscene_dir: PathBuf,
    pub renderer_glyphs: usize,
    pub cutscene_glyphs: Vec<(String, usize)>,
}

/// Rasterize an explicit set of characters through the same verified 16x16
/// profile used by the renderer and cutscene builders. Graphics-text compilers
/// use this instead of opening or interpreting the font independently.
pub fn rasterize_glyphs(
    profile: &FontProfile,
    characters: &BTreeSet<char>,
) -> Result<std::collections::BTreeMap<char, [u8; GLYPH_BYTES]>> {
    let font = profile.font()?;
    characters
        .iter()
        .copied()
        .filter(|ch| !ch.is_whitespace())
        .map(|ch| Ok((ch, rasterize(&font, profile, ch)?)))
        .collect()
}

impl FontProfile {
    pub fn load(path: &Path) -> Result<Self> {
        let profile_path = path
            .canonicalize()
            .with_context(|| format!("resolve font profile {}", path.display()))?;
        let value: Value = serde_json::from_slice(
            &std::fs::read(&profile_path)
                .with_context(|| format!("read font profile {}", profile_path.display()))?,
        )
        .with_context(|| format!("parse font profile {}", profile_path.display()))?;
        expect_string(&value, "schema", &profile_path).and_then(|schema| {
            if schema == PROFILE_SCHEMA {
                Ok(schema)
            } else {
                bail!(
                    "{}: unsupported schema {schema:?}, expected {PROFILE_SCHEMA:?}",
                    profile_path.display()
                )
            }
        })?;

        let id = expect_string(&value, "id", &profile_path)?.to_owned();
        if id.is_empty()
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            bail!(
                "{}: id must contain only lowercase ASCII letters, digits, and hyphens",
                profile_path.display()
            );
        }

        let font_name = expect_string(&value, "font", &profile_path)?.to_owned();
        if Path::new(&font_name)
            .file_name()
            .and_then(|name| name.to_str())
            != Some(font_name.as_str())
        {
            bail!(
                "{}: font must be a file name, got {font_name:?}",
                profile_path.display()
            );
        }
        let profile_dir = profile_path
            .parent()
            .context("font profile has no parent directory")?;
        let font_path = profile_dir.join(&font_name);
        let font_bytes = std::fs::read(&font_path)
            .with_context(|| format!("read font {}", font_path.display()))?;
        let actual_sha256 = sha256_hex(&font_bytes);
        let expected_sha256 = expect_string(&value, "font_sha256", &profile_path)?;
        if actual_sha256 != expected_sha256 {
            bail!(
                "{}: SHA-256 {actual_sha256} does not match profile {expected_sha256}",
                font_path.display()
            );
        }

        let font_size = expect_u64(&value, "font_size", &profile_path)?;
        if !(1..=32).contains(&font_size) {
            bail!("{}: font_size must be in 1..=32", profile_path.display());
        }
        let baseline_y = expect_u64(&value, "baseline_y", &profile_path)?;
        if baseline_y > GLYPH_HEIGHT as u64 {
            bail!(
                "{}: baseline_y must be in 0..={GLYPH_HEIGHT}",
                profile_path.display()
            );
        }
        let threshold = expect_u64(&value, "threshold", &profile_path)?;
        if !(1..=255).contains(&threshold) {
            bail!("{}: threshold must be in 1..=255", profile_path.display());
        }

        let license = expect_string(&value, "license", &profile_path)?.to_owned();
        let license_path = profile_dir.join(&license);
        if !license_path.is_file() {
            bail!(
                "{}: declared font license is missing",
                license_path.display()
            );
        }

        Ok(Self {
            profile_path,
            id,
            font_name,
            font_path,
            font_sha256: actual_sha256,
            font_version: expect_string(&value, "font_version", path)?.to_owned(),
            font_size: font_size as u16,
            baseline_y: baseline_y as u8,
            threshold: threshold as u8,
            license,
            source: expect_string(&value, "source", path)?.to_owned(),
            font_bytes,
        })
    }

    fn font(&self) -> Result<Font> {
        Font::from_bytes(self.font_bytes.clone(), FontSettings::default())
            .map_err(|error| anyhow::anyhow!("parse {}: {error}", self.font_path.display()))
    }
}

pub fn build_renderer_font(profile: &FontProfile, demand: &BTreeSet<char>) -> Result<RendererFont> {
    if demand.is_empty() {
        bail!("renderer translation demand has no Hangul syllables");
    }
    if demand.len() > RENDERER_CAPACITY {
        bail!(
            "renderer translation demand has {} Hangul syllables, exceeding capacity {RENDERER_CAPACITY}",
            demand.len(),
        );
    }
    let font = profile.font()?;
    let mut bytes = vec![0; RENDERER_CAPACITY * GLYPH_BYTES];
    let mut glyphs = Vec::with_capacity(demand.len());
    let mut topic_no_final_jis = None;
    let mut topic_final_jis = None;
    let mut object_no_final_jis = None;
    let mut object_final_jis = None;
    let mut subject_no_final_jis = None;
    let mut subject_final_jis = None;
    let mut with_no_final_jis = None;
    let mut with_final_jis = None;
    for (slot, ch) in demand.iter().copied().enumerate() {
        let jis = renderer_jis(slot)?;
        let sjis = jis_to_sjis(jis)?;
        let bitmap = rasterize(&font, profile, ch)?;
        let start = slot * GLYPH_BYTES;
        bytes[start..start + GLYPH_BYTES].copy_from_slice(&bitmap);
        if ch == crate::josa::TOPIC_NO_FINAL {
            topic_no_final_jis = Some(jis);
        } else if ch == crate::josa::TOPIC_FINAL {
            topic_final_jis = Some(jis);
        } else if ch == crate::josa::OBJECT_NO_FINAL {
            object_no_final_jis = Some(jis);
        } else if ch == crate::josa::OBJECT_FINAL {
            object_final_jis = Some(jis);
        } else if ch == crate::josa::SUBJECT_NO_FINAL {
            subject_no_final_jis = Some(jis);
        } else if ch == crate::josa::SUBJECT_FINAL {
            subject_final_jis = Some(jis);
        } else if ch == crate::josa::WITH_NO_FINAL {
            with_no_final_jis = Some(jis);
        } else if ch == crate::josa::WITH_FINAL {
            with_final_jis = Some(jis);
        }
        glyphs.push(json!({
            "char": ch.to_string(),
            "codepoint": format!("U+{:04X}", ch as u32),
            "slot": slot,
            "jis": format!("0x{jis:04X}"),
            "sjis": format!("{:02x}{:02x}", sjis[0], sjis[1]),
            "index": slot,
        }));
    }
    let topic_no_final_jis =
        topic_no_final_jis.context("renderer demand lacks runtime topic particle `는`")?;
    let topic_final_jis =
        topic_final_jis.context("renderer demand lacks runtime topic particle `은`")?;
    let object_no_final_jis =
        object_no_final_jis.context("renderer demand lacks runtime object particle `를`")?;
    let object_final_jis =
        object_final_jis.context("renderer demand lacks runtime object particle `을`")?;
    let subject_no_final_jis =
        subject_no_final_jis.context("renderer demand lacks runtime subject particle `가`")?;
    let subject_final_jis =
        subject_final_jis.context("renderer demand lacks runtime subject particle `이`")?;
    let with_no_final_jis =
        with_no_final_jis.context("renderer demand lacks runtime with particle `와`")?;
    let with_final_jis =
        with_final_jis.context("renderer demand lacks runtime with particle `과`")?;
    let metadata = json!({
        "schema": "pc98_madou_ars.glyph_sheet.v2",
        "profile": profile.id,
        "font": profile.font_name,
        "font_sha256": profile.font_sha256,
        "font_version": profile.font_version,
        "font_license": profile.license,
        "font_source": profile.source,
        "font_size": profile.font_size,
        "baseline_y": profile.baseline_y,
        "threshold": profile.threshold,
        "rasterizer": format!("fontdue {FONTDUE_VERSION}"),
        "glyph_format": glyph_format(),
        "index_formula": "(jis_high - 0x75) * 94 + (jis_low - 0x21)",
        "base_row": "0x75",
        "rows": (RENDERER_BASE_ROW..RENDERER_BASE_ROW + RENDERER_ROWS as u8)
            .map(|row| format!("0x{row:02X}"))
            .collect::<Vec<_>>(),
        "capacity": RENDERER_CAPACITY,
        "used": demand.len(),
        "sheet_bytes": bytes.len(),
        "josa": {
            "schema": "pc98_madou_ars.runtime_josa.v2",
            "class_encoding": {
                "no_final": crate::josa::JongseongClass::NoFinal as u8,
                "rieul": crate::josa::JongseongClass::Rieul as u8,
                "other_final": crate::josa::JongseongClass::OtherFinal as u8,
            },
            "non_hangul_fallback": "no_final",
            "object": {
                "marker": {
                    "char": crate::josa::OBJECT_MARKER.to_string(),
                    "jis": format!("0x{:04X}", crate::josa::PARTICLE_MARKER_FIRST_JIS),
                    "sjis": "f040",
                },
                "no_final": {
                    "char": crate::josa::OBJECT_NO_FINAL.to_string(),
                    "jis": format!("0x{object_no_final_jis:04X}"),
                },
                "final": {
                    "char": crate::josa::OBJECT_FINAL.to_string(),
                    "jis": format!("0x{object_final_jis:04X}"),
                },
            },
            "subject": {
                "marker": {
                    "char": crate::josa::SUBJECT_MARKER.to_string(),
                    "jis": format!("0x{:04X}", crate::josa::PARTICLE_MARKER_FIRST_JIS + 1),
                    "sjis": "f041",
                },
                "no_final": {
                    "char": crate::josa::SUBJECT_NO_FINAL.to_string(),
                    "jis": format!("0x{subject_no_final_jis:04X}"),
                },
                "final": {
                    "char": crate::josa::SUBJECT_FINAL.to_string(),
                    "jis": format!("0x{subject_final_jis:04X}"),
                },
            },
            "topic": {
                "marker": {
                    "char": crate::josa::TOPIC_MARKER.to_string(),
                    "jis": format!("0x{:04X}", crate::josa::PARTICLE_MARKER_FIRST_JIS + 2),
                    "sjis": "f042",
                },
                "no_final": {
                    "char": crate::josa::TOPIC_NO_FINAL.to_string(),
                    "jis": format!("0x{topic_no_final_jis:04X}"),
                },
                "final": {
                    "char": crate::josa::TOPIC_FINAL.to_string(),
                    "jis": format!("0x{topic_final_jis:04X}"),
                },
            },
            "with": {
                "marker": {
                    "char": crate::josa::WITH_MARKER.to_string(),
                    "jis": format!("0x{:04X}", crate::josa::PARTICLE_MARKER_FIRST_JIS + 3),
                    "sjis": "f043",
                },
                "no_final": {
                    "char": crate::josa::WITH_NO_FINAL.to_string(),
                    "jis": format!("0x{with_no_final_jis:04X}"),
                },
                "final": {
                    "char": crate::josa::WITH_FINAL.to_string(),
                    "jis": format!("0x{with_final_jis:04X}"),
                },
            },
            "runtime_data_offset": format!("0x{:04X}", crate::josa::RUNTIME_DATA_OFF),
            "class_table_offset": format!("0x{:04X}", crate::josa::CLASS_TABLE_OFF),
            "class_table_len": crate::josa::CLASS_TABLE_LEN,
        },
        "glyphs": glyphs,
    });
    Ok(RendererFont {
        bytes,
        metadata,
        glyphs: demand.len(),
    })
}

pub fn build_gaiji_font(profile: &FontProfile, demand: &BTreeSet<char>) -> Result<GaijiFont> {
    build_gaiji_font_after_reserved(profile, demand, 0)
}

/// Allocate around picture cells that the original phase registers after boot.
pub fn build_resource_gaiji_font(
    profile: &FontProfile,
    demand: &BTreeSet<char>,
    resource: &str,
) -> Result<GaijiFont> {
    build_gaiji_font_after_reserved(
        profile,
        demand,
        crate::cutscene_catalog::reserved_gaiji_slots(resource),
    )
}

fn build_gaiji_font_after_reserved(
    profile: &FontProfile,
    demand: &BTreeSet<char>,
    reserved: usize,
) -> Result<GaijiFont> {
    if demand.is_empty() {
        bail!("cutscene phase translation demand has no Hangul syllables");
    }
    if demand.len().saturating_add(reserved) > BIOS_GAIJI_CAPACITY {
        bail!(
            "cutscene phase translation demand has {} Hangul syllables, exceeding BIOS gaiji capacity {BIOS_GAIJI_CAPACITY} with {reserved} reserved cells",
            demand.len()
        );
    }
    let font = profile.font()?;
    let mut glyphs = Vec::with_capacity(demand.len());
    for (slot, ch) in demand.iter().copied().enumerate() {
        let slot = slot + reserved;
        let jis = gaiji_jis(slot)?;
        let sjis = jis_to_sjis(jis)?;
        let bitmap = rasterize(&font, profile, ch)?;
        glyphs.push(json!({
            "char": ch.to_string(),
            "codepoint": format!("U+{:04X}", ch as u32),
            "slot": slot,
            "jis": format!("0x{jis:04X}"),
            "sjis": format!("{:02x}{:02x}", sjis[0], sjis[1]),
            "glyph_hex": encode_hex(&bitmap),
        }));
    }
    let metadata = json!({
        "schema": "pc98_madou_ars.gaiji_table.v2",
        "profile": profile.id,
        "font": profile.font_name,
        "font_sha256": profile.font_sha256,
        "font_version": profile.font_version,
        "font_license": profile.license,
        "font_source": profile.source,
        "font_size": profile.font_size,
        "baseline_y": profile.baseline_y,
        "threshold": profile.threshold,
        "rasterizer": format!("fontdue {FONTDUE_VERSION}"),
        "glyph_format": glyph_format(),
        "gaiji_jis_first": format!("0x{:04X}", gaiji_jis(reserved)?),
        "glyphs": glyphs,
    });
    Ok(GaijiFont {
        metadata,
        glyphs: demand.len(),
    })
}

pub fn generate_full_build_fonts(
    profile_path: &Path,
    translations_dir: &Path,
    character: &str,
    allow_needs_review: bool,
    output_dir: &Path,
) -> Result<FullBuildFonts> {
    crate::dialogue_layout::validate_translation_tree(translations_dir)
        .context("validate static dialogue layout")?;
    let profile = FontProfile::load(profile_path)?;
    let renderer_demand = collect_renderer_demand(translations_dir, character, allow_needs_review)?;
    let renderer = build_renderer_font(&profile, &renderer_demand)?;

    let renderer_bin = output_dir.join("kfont.bin");
    let renderer_json = output_dir.join("kfont.json");
    let cutscene_dir = output_dir.join("cutscene");
    std::fs::create_dir_all(&cutscene_dir)
        .with_context(|| format!("create {}", cutscene_dir.display()))?;

    let mut cutscene_outputs = Vec::new();
    for spec in CUTSCENE_RESOURCES
        .iter()
        .filter(|spec| spec.character == character)
    {
        let stem = spec
            .file
            .rsplit_once('.')
            .map(|(stem, _)| stem.to_ascii_lowercase())
            .context("cutscene resource lacks an extension")?;
        let catalog_path = translations_dir
            .join("cutscene")
            .join(format!("{stem}.json"));
        let catalog: Value = serde_json::from_slice(
            &std::fs::read(&catalog_path)
                .with_context(|| format!("read {}", catalog_path.display()))?,
        )
        .with_context(|| format!("parse {}", catalog_path.display()))?;
        let demand = collect_catalog_hangul(
            &catalog,
            allow_needs_review,
            &catalog_path.display().to_string(),
        )?;
        let gaiji = build_resource_gaiji_font(&profile, &demand, spec.file)
            .with_context(|| format!("build cutscene font for {}", spec.file))?;
        cutscene_outputs.push((
            cutscene_dir.join(format!("{stem}.json")),
            gaiji.metadata,
            gaiji.glyphs,
        ));
    }
    if cutscene_outputs.len() != 3 {
        bail!(
            "{character} font build planned {} cutscene phases, expected 3",
            cutscene_outputs.len()
        );
    }

    std::fs::create_dir_all(output_dir)
        .with_context(|| format!("create {}", output_dir.display()))?;
    std::fs::write(&renderer_bin, &renderer.bytes)
        .with_context(|| format!("write {}", renderer_bin.display()))?;
    write_json(&renderer_json, &renderer.metadata)?;
    let mut cutscene_glyphs = Vec::new();
    for (path, metadata, count) in cutscene_outputs {
        write_json(&path, &metadata)?;
        cutscene_glyphs.push((
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("?")
                .to_owned(),
            count,
        ));
    }

    Ok(FullBuildFonts {
        renderer_bin,
        renderer_json,
        cutscene_dir,
        renderer_glyphs: renderer.glyphs,
        cutscene_glyphs,
    })
}

pub fn collect_renderer_demand(
    translations_dir: &Path,
    character: &str,
    allow_needs_review: bool,
) -> Result<BTreeSet<char>> {
    let (overlay, dat_character) = match character {
        "arle" => ("GAME_A.OVL", "A"),
        "rulue" => ("GAME_R.OVL", "R"),
        "schezo" => ("GAME_S.OVL", "S"),
        _ => bail!("unknown build character {character:?}"),
    };
    let mut demand = BTreeSet::new();

    let mut paths = json_files(translations_dir)?;
    for path in paths.drain(..) {
        let value = read_json(&path)?;
        if value.get("overlay").and_then(Value::as_str) == Some(overlay) {
            demand.extend(collect_catalog_hangul(
                &value,
                allow_needs_review,
                &path.display().to_string(),
            )?);
        }
    }

    if character == "schezo" {
        demand.extend(collect_catalog_hangul(
            &crate::lever_text::state_catalog(translations_dir)?,
            allow_needs_review,
            "Schezo lever states",
        )?);
    }
    let dat_dir = translations_dir.join("dat");
    for path in json_files(&dat_dir)? {
        let value = read_json(&path)?;
        let value =
            crate::translation_binding::resolve_bound_translations(&value, translations_dir)
                .with_context(|| {
                    format!("resolve renderer demand bindings in {}", path.display())
                })?;
        if value.get("char").and_then(Value::as_str) == Some(dat_character) {
            demand.extend(collect_catalog_hangul(
                &value,
                allow_needs_review,
                &path.display().to_string(),
            )?);
        }
    }
    demand.extend(RUNTIME_RENDERER_SYLLABLES);
    if demand.is_empty() {
        bail!(
            "{character} renderer demand is empty in {}",
            translations_dir.display()
        );
    }
    Ok(demand)
}

fn collect_catalog_hangul(
    catalog: &Value,
    allow_needs_review: bool,
    label: &str,
) -> Result<BTreeSet<char>> {
    let entries = catalog
        .get("entries")
        .and_then(Value::as_array)
        .with_context(|| format!("{label}: missing entries array"))?;
    let mut demand = BTreeSet::new();
    for (index, entry) in entries.iter().enumerate() {
        let structural = entry
            .get("flags")
            .and_then(Value::as_array)
            .is_some_and(|flags| flags.iter().any(|flag| flag.as_str() == Some("structural")));
        if structural {
            continue;
        }
        let status = entry
            .get("status")
            .and_then(Value::as_str)
            .with_context(|| format!("{label}: entry {index} missing status"))?;
        match status {
            "complete" => {}
            "needs_review" | "translated" if allow_needs_review => {}
            "needs_review" | "translated" => bail!(
                "{label}: entry {index} has status={status}; shipping build requires complete"
            ),
            _ => bail!("{label}: entry {index} has unsupported status {status:?}"),
        }
        let text = entry
            .get("ko")
            .and_then(Value::as_str)
            .with_context(|| format!("{label}: entry {index} missing ko"))?;
        if text.is_empty() {
            bail!("{label}: entry {index} has an empty translation");
        }
        demand.extend(text.chars().filter(|ch| is_hangul_syllable(*ch)));
    }
    Ok(demand)
}

fn rasterize(font: &Font, profile: &FontProfile, ch: char) -> Result<[u8; GLYPH_BYTES]> {
    let (metrics, coverage) = font.rasterize(ch, f32::from(profile.font_size));
    if metrics.width == 0 || metrics.height == 0 || coverage.is_empty() {
        bail!("font rendered no pixels for {ch:?} (U+{:04X})", ch as u32);
    }
    if metrics.width > GLYPH_WIDTH || metrics.height > GLYPH_HEIGHT {
        bail!(
            "glyph {ch:?} (U+{:04X}) is {}x{} and does not fit {GLYPH_WIDTH}x{GLYPH_HEIGHT}",
            ch as u32,
            metrics.width,
            metrics.height
        );
    }

    let left = (GLYPH_WIDTH - metrics.width) / 2;
    // fontdue reports ymin in a baseline-relative, y-up coordinate system.
    let top = i32::from(profile.baseline_y) - (metrics.ymin + metrics.height as i32);
    let bottom = top + metrics.height as i32;
    if top < 0 || bottom > GLYPH_HEIGHT as i32 {
        bail!(
            "glyph {ch:?} (U+{:04X}) target y {top}..{bottom} does not fit at baseline {}",
            ch as u32,
            profile.baseline_y
        );
    }

    let mut out = [0u8; GLYPH_BYTES];
    for source_y in 0..metrics.height {
        for source_x in 0..metrics.width {
            let coverage_value = coverage[source_y * metrics.width + source_x];
            if coverage_value >= profile.threshold {
                let x = left + source_x;
                let y = top as usize + source_y;
                out[y * 2 + x / 8] |= 1 << (7 - (x % 8));
            }
        }
    }
    if out.iter().all(|byte| *byte == 0) {
        bail!(
            "font rendered an empty 1bpp glyph for {ch:?} (U+{:04X}) at threshold {}",
            ch as u32,
            profile.threshold
        );
    }
    Ok(out)
}

fn renderer_jis(slot: usize) -> Result<u16> {
    if slot >= RENDERER_CAPACITY {
        bail!("renderer glyph slot {slot} exceeds capacity {RENDERER_CAPACITY}");
    }
    let row = RENDERER_BASE_ROW + (slot / CELLS_PER_ROW) as u8;
    let cell = 0x21 + (slot % CELLS_PER_ROW) as u8;
    Ok(u16::from(row) << 8 | u16::from(cell))
}

fn gaiji_jis(slot: usize) -> Result<u16> {
    if slot >= BIOS_GAIJI_CAPACITY {
        bail!("BIOS gaiji slot {slot} exceeds capacity {BIOS_GAIJI_CAPACITY}");
    }
    let row = 0x76 + (slot / CELLS_PER_ROW) as u8;
    let cell = 0x21 + (slot % CELLS_PER_ROW) as u8;
    Ok(u16::from(row) << 8 | u16::from(cell))
}

fn jis_to_sjis(jis: u16) -> Result<[u8; 2]> {
    let row = (jis >> 8) as u8;
    let cell = jis as u8;
    if !(0x21..=0x7E).contains(&row) || !(0x21..=0x7E).contains(&cell) {
        bail!("JIS 0x{jis:04X} is outside 0x21..=0x7E");
    }
    let mut lead = ((row - 0x21) >> 1) + 0x81;
    if lead > 0x9F {
        lead += 0x40;
    }
    let trail = if row % 2 == 1 {
        cell + 0x1F + u8::from(cell > 0x5F)
    } else {
        cell + 0x7E
    };
    if trail == 0x7F {
        bail!("JIS 0x{jis:04X} produced forbidden Shift-JIS trail 0x7F");
    }
    Ok([lead, trail])
}

fn glyph_format() -> Value {
    json!({
        "width": GLYPH_WIDTH,
        "height": GLYPH_HEIGHT,
        "bpp": 1,
        "bytes_per_glyph": GLYPH_BYTES,
        "row_layout": "2 bytes per row, MSB-first",
    })
}

fn json_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = std::fs::read_dir(dir)
        .with_context(|| format!("read directory {}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

fn read_json(path: &Path) -> Result<Value> {
    serde_json::from_slice(
        &std::fs::read(path).with_context(|| format!("read {}", path.display()))?,
    )
    .with_context(|| format!("parse {}", path.display()))
}

fn write_json(path: &Path, value: &Value) -> Result<()> {
    std::fs::write(path, serde_json::to_vec_pretty(value)?)
        .with_context(|| format!("write {}", path.display()))
}

fn expect_string<'a>(value: &'a Value, field: &str, path: &Path) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .with_context(|| format!("{}: missing string field {field:?}", path.display()))
}

fn expect_u64(value: &Value, field: &str, path: &Path) -> Result<u64> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .with_context(|| format!("{}: missing integer field {field:?}", path.display()))
}

fn is_hangul_syllable(ch: char) -> bool {
    ('가'..='힣').contains(&ch)
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}").expect("write to String cannot fail");
    }
    output
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::Path;

    use super::{
        FontProfile, RENDERER_CAPACITY, build_gaiji_font, build_renderer_font, jis_to_sjis,
    };
    use crate::cutscene_catalog::BIOS_GAIJI_CAPACITY;

    fn hangul_demand(count: usize) -> BTreeSet<char> {
        (0..count)
            .map(|index| char::from_u32('가' as u32 + index as u32).unwrap())
            .collect()
    }

    #[test]
    fn jis_conversion_skips_forbidden_trail() {
        assert_eq!(jis_to_sjis(0x775F).unwrap(), [0xEC, 0x7E]);
        assert_eq!(jis_to_sjis(0x7760).unwrap(), [0xEC, 0x80]);
        for cell in 0x21..=0x7E {
            assert_ne!(jis_to_sjis(0x7700 | cell).unwrap()[1], 0x7F);
        }
    }

    #[test]
    #[ignore = "requires Galmuri14.ttf and Galmuri-OFL.txt in assets/fonts/"]
    fn galmuri_profile_builds_both_font_paths() {
        let profile = FontProfile::load(Path::new("assets/fonts/font_profile.json")).unwrap();
        let demand = ['가', '뿌', '요']
            .into_iter()
            .chain(super::RUNTIME_RENDERER_SYLLABLES)
            .collect::<BTreeSet<_>>();
        let renderer = build_renderer_font(&profile, &demand).unwrap();
        let gaiji = build_gaiji_font(&profile, &demand).unwrap();
        assert_eq!(renderer.bytes.len(), 940 * 32);
        assert_eq!(renderer.glyphs, demand.len());
        assert_eq!(gaiji.glyphs, demand.len());
        assert_eq!(renderer.metadata["glyphs"][0]["char"], "가");
        assert_eq!(gaiji.metadata["glyphs"][0]["char"], "가");
        assert_eq!(
            &renderer.bytes[..32],
            &[
                0x00, 0x00, 0x7F, 0x08, 0x01, 0x08, 0x01, 0x08, 0x01, 0x08, 0x01, 0x08, 0x02, 0x08,
                0x02, 0x0E, 0x04, 0x08, 0x04, 0x08, 0x08, 0x08, 0x10, 0x08, 0x60, 0x08, 0x00, 0x08,
                0x00, 0x08, 0x00, 0x00,
            ],
            "Galmuri14 15px/baseline15/threshold128 must retain the reviewed Pillow-era 가 bitmap",
        );
    }

    #[test]
    #[ignore = "requires Galmuri14.ttf and Galmuri-OFL.txt in assets/fonts/"]
    fn renderer_font_rejects_one_glyph_past_the_sheet_capacity() {
        let profile = FontProfile::load(Path::new("assets/fonts/font_profile.json")).unwrap();
        let demand = hangul_demand(RENDERER_CAPACITY + 1);

        let error = build_renderer_font(&profile, &demand)
            .unwrap_err()
            .to_string();

        assert!(error.contains("941 Hangul syllables"), "{error}");
        assert!(error.contains("capacity 940"), "{error}");
    }

    #[test]
    #[ignore = "requires Galmuri14.ttf and Galmuri-OFL.txt in assets/fonts/"]
    fn cutscene_font_rejects_one_glyph_past_the_bios_capacity() {
        let profile = FontProfile::load(Path::new("assets/fonts/font_profile.json")).unwrap();
        let demand = hangul_demand(BIOS_GAIJI_CAPACITY + 1);

        let error = build_gaiji_font(&profile, &demand).unwrap_err().to_string();

        assert!(error.contains("189 Hangul syllables"), "{error}");
        assert!(error.contains("capacity 188"), "{error}");
    }
}
