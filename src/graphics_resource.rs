//! Conservative renderer for the screen and tile-atlas resource shapes proven
//! in A.R.S.
//!
//! The Demo overlay expands selected `OP*.CNS` files into four buffers and
//! copies them to the PC-98 B/R/G/I graphics-VRAM planes in that order. Some
//! streams carry a 128-byte tail after the 640x400 plane; the renderer preserves
//! that fact in its report and never interprets the tail as pixels. `TC.CNS`
//! and `MU.CNS` instead use the resident MEGDOS AH=5 column-major transpose.
//! `NOCOPY.CNS` uses the ordinary four-plane path but deliberately represents
//! its blank B/R planes as two empty leading streams.
//! Their second streams are length-prefixed `BSAMP.COM` sample banks, reported
//! independently and never interpreted as pixels. Named `MG_*` map graphics
//! and character `F`/`B` resources expose 16x16 B/R/G/I tiles in 0x8000-byte
//! banks; their diagnostic atlases preserve the consumer address space but do
//! not claim to reconstruct a map, character frame, or animation. `ISHI_*`
//! combines an initial plane-major tile bank with row-interleaved sprite
//! regions, rendered according to its separate direct-blitter consumers. The
//! Rulue `RO*.DAT` opening, `RTU*.DAT` interlude, and `RE*.DAT` ending resources
//! use variable-width plane-major scene sheets and consumer-selected subframes.
//! RO and the late RE21 resources, plus Schezo `S*.DAT`/`ST*.DAT`/`SE*.DAT`,
//! additionally use row-interleaved plane tuples for effects, sprites, and
//! wipe-transition sheets.

use anyhow::{Context, Result, bail};

use crate::{
    bsamp_resource::{BsampBank, parse_named_bsamp_companion},
    overlay_lz::decode_overlay_lz,
};

pub const SCREEN_WIDTH: usize = 640;
pub const SCREEN_HEIGHT: usize = 400;
pub const PLANE_BYTES: usize = SCREEN_WIDTH * SCREEN_HEIGHT / 8;
pub const DEMO_LINEAR_WIDTH: usize = 320;
pub const DEMO_LINEAR_HEIGHT: usize = 200;
pub const DEMO_LINEAR_PLANE_BYTES: usize = DEMO_LINEAR_WIDTH * DEMO_LINEAR_HEIGHT / 8;
const DEMO_LINEAR_DEST_X: usize = 160;
const DEMO_LINEAR_DEST_Y: usize = 96;
const TILE_WIDTH: usize = 16;
const TILE_HEIGHT: usize = 16;
const TILE_PLANE_BYTES: usize = TILE_WIDTH * TILE_HEIGHT / 8;
const TILE_BYTES: usize = TILE_PLANE_BYTES * 4;
const TILE_BANK_BYTES: usize = 0x8000;
const ISHI_DECODED_BYTES: usize = 0x2C00;
const ISHI_TILE_END: usize = 0x1100;
const ISHI_SMALL_END: usize = 0x1600;
const ISHI_MEDIUM_END: usize = 0x1E00;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenLayout {
    /// One 640x400 1bpp image stream.
    Monochrome,
    /// Four streams copied to B/R/G/I graphics VRAM respectively.
    BrgiPlanes,
    /// One stream containing four contiguous 320x200 B/R/G/I planes. The Demo
    /// direct-copy consumer advances SI across all four planes and places the
    /// image at (160, 96) on the 640x400 graphics screen.
    LinearBrgiPlanes,
    /// One 128,000-byte stream whose four B/R/G/I planes are stored as
    /// 80 columns of 400 bytes and transposed by MEGDOS `INT 7Ch / AH=5`.
    ColumnMajorBrgiPlanes,
    /// A 640x400 diagnostic atlas of 16x16 B/R/G/I tiles stored in 0x8000-byte
    /// source banks. This is a source-sheet view, not a composed game screen.
    BrgiTileAtlas,
    /// A 640x400 diagnostic atlas for the mixed ISHI contract: plane-major
    /// tiles plus row-interleaved 16x16, 32x32, and 64x112 sprites.
    BrgiMixedSpriteAtlas,
    /// A 640x400 diagnostic atlas of consumer-proven plane-major or
    /// row-interleaved scene-DAT regions. This is a source-sheet view, not a
    /// composed cutscene frame.
    BrgiSceneAtlas,
}

impl ScreenLayout {
    pub fn label(self) -> &'static str {
        match self {
            Self::Monochrome => "640x400 1bpp",
            Self::BrgiPlanes => "640x400 B/R/G/I planes",
            Self::LinearBrgiPlanes => {
                "320x200 contiguous B/R/G/I planes (Demo direct-copy destination 160,96)"
            }
            Self::ColumnMajorBrgiPlanes => "640x400 B/R/G/I planes (AH=5 column-major source)",
            Self::BrgiTileAtlas => "640x400 atlas of 16x16 B/R/G/I source tiles",
            Self::BrgiMixedSpriteAtlas => {
                "640x400 atlas of mixed tile and row-interleaved B/R/G/I sprites"
            }
            Self::BrgiSceneAtlas => "640x400 atlas of consumer-proven scene-DAT B/R/G/I regions",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodedRange {
    pub offset: usize,
    pub bytes: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PlaneMajorRegion {
    source_offset: usize,
    source_row_bytes: usize,
    plane_stride: usize,
    width: usize,
    height: usize,
    destination_x: usize,
    destination_y: usize,
}

impl PlaneMajorRegion {
    const fn new(
        source_offset: usize,
        source_row_bytes: usize,
        plane_stride: usize,
        width: usize,
        height: usize,
        destination_x: usize,
        destination_y: usize,
    ) -> Self {
        Self {
            source_offset,
            source_row_bytes,
            plane_stride,
            width,
            height,
            destination_x,
            destination_y,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedScreen {
    pub layout: ScreenLayout,
    pub stream_sizes: Vec<usize>,
    pub metadata_tail_bytes: Vec<usize>,
    /// Decoded LZ streams that the proven screen layout does not consume as
    /// pixels. TC/MU retain their second, independently consumed stream here.
    pub companion_stream_sizes: Vec<usize>,
    /// Consumer-proven `BSAMP.COM` sample-bank profile for the companion.
    pub companion_audio: Option<BsampBank>,
    /// Decoded bytes deliberately excluded because no matching pixel consumer
    /// was proven. These are not silently labeled metadata or graphics.
    pub unrendered_ranges: Vec<DecodedRange>,
    /// Row-major RGB24 pixels, top to bottom.
    pub rgb: Vec<u8>,
}

/// Decode a packed resource and render only the two statically bounded screen
/// shapes: one plane, or four independently packed B/R/G/I planes. Any packed
/// tail, unexpected stream count, or stream size other than 32,000/32,128 bytes
/// fails closed.
pub fn render_screen_resource(packed: &[u8]) -> Result<RenderedScreen> {
    let streams = decode_exact_nonempty_streams(packed)?;
    let stream_sizes = streams.iter().map(Vec::len).collect::<Vec<_>>();
    if !stream_sizes
        .iter()
        .all(|size| *size == PLANE_BYTES || *size == PLANE_BYTES + 128)
    {
        bail!("unsupported decoded stream sizes {stream_sizes:?}; expected 32000 or 32128 bytes");
    }
    let metadata_tail_bytes = stream_sizes
        .iter()
        .map(|size| size - PLANE_BYTES)
        .collect::<Vec<_>>();
    let (layout, rgb) = match streams.as_slice() {
        [plane] => (ScreenLayout::Monochrome, render_monochrome(plane)),
        [blue, red, green, intensity] => (
            ScreenLayout::BrgiPlanes,
            render_brgi([
                &blue[..PLANE_BYTES],
                &red[..PLANE_BYTES],
                &green[..PLANE_BYTES],
                &intensity[..PLANE_BYTES],
            ]),
        ),
        _ => bail!(
            "unsupported decoded stream count {}; expected one monochrome stream or four B/R/G/I streams",
            streams.len()
        ),
    };
    Ok(RenderedScreen {
        layout,
        stream_sizes,
        metadata_tail_bytes,
        companion_stream_sizes: Vec::new(),
        companion_audio: None,
        unrendered_ranges: Vec::new(),
        rgb,
    })
}

/// Render a resource by its consumer-proven A.R.S name. TC/MU and the three
/// character WAKU resources and the shared ARS_RX title use the MEGDOS AH=5
/// column-major four-plane path; NOCOPY uses the ordinary four-plane path with
/// two explicitly empty leading planes;
/// MG and character F/B resources use named tile-atlas contracts; RTU/RE/ST
/// use their cutscene consumers' scene regions. All other names stay on the
/// conservative one-/four-stream screen renderer above.
pub fn render_named_screen_resource(name: &str, packed: &[u8]) -> Result<RenderedScreen> {
    match name.to_ascii_uppercase().as_str() {
        "OP17.CNS" | "OP18.CNS" | "OP20.CNS" => render_demo_linear_brgi_resource(packed),
        "NOCOPY.CNS" => render_nocopy_resource(packed),
        "TC.CNS" => render_column_major_brgi_resource(name, packed, 4_740),
        "MU.CNS" => render_column_major_brgi_resource(name, packed, 12_163),
        "ARS_RX.CS" | "WAKU_A.CS" | "WAKU_R.CNS" | "WAKU_S.CS" => {
            render_single_column_major_brgi_resource(packed)
        }
        "MG_A1.CNS" => render_mg_tile_atlas(packed, 0x09F0, 105_456),
        "MG_A2.CNS" => render_mg_tile_atlas(packed, 0x0A90, 85_904),
        "MG_R1.CNS" => render_mg_tile_atlas(packed, 0x0E10, 114_576),
        "MG_S1.CNS" => render_mg_tile_atlas(packed, 0x1250, 114_256),
        "ARURU_F.CNS" => render_character_tile_atlas(packed, 0x0180, 31_872),
        "ARURU_B.CNS" => render_character_tile_atlas(packed, 0x03D0, 36_176),
        "RURUU_F.CNS" => render_character_tile_atlas(packed, 0x0150, 30_416),
        "RURUU_B.CNS" => render_character_tile_atlas(packed, 0x0410, 33_680),
        "RURUU_FC.CNS" => render_character_tile_atlas(packed, 0x0150, 29_776),
        "RURUU_BC.CNS" => render_character_tile_atlas(packed, 0x03D0, 29_264),
        "SHEZO_F.CNS" => render_character_tile_atlas(packed, 0x0160, 30_816),
        "SHEZO_B.CNS" => render_character_tile_atlas(packed, 0x04E0, 44_000),
        "ISHI_A.CNS" | "ISHI_R.CNS" | "ISHI_S.CNS" => render_ishi_sprite_atlas(packed),
        "RTU1.DAT" => render_rtu_scene_atlas(name, packed, 28_672),
        "RTU2.DAT" => render_rtu_scene_atlas(name, packed, 39_936),
        "RTU3.DAT" => render_rtu_scene_atlas(name, packed, 57_600),
        "RTU4.DAT" => render_rtu_scene_atlas(name, packed, 30_912),
        "RO1.DAT" => render_ro_scene_atlas(name, packed, 49_152),
        "RO2.DAT" => render_ro_scene_atlas(name, packed, 27_648),
        "RO3.DAT" => render_ro_scene_atlas(name, packed, 55_296),
        "RO5.DAT" => render_ro_scene_atlas(name, packed, 55_456),
        "RO6.DAT" => render_ro_scene_atlas(name, packed, 44_736),
        "RO9.DAT" => render_ro_scene_atlas(name, packed, 27_648),
        "RO10.DAT" => render_ro_scene_atlas(name, packed, 63_648),
        "RO11.DAT" => render_ro_scene_atlas(name, packed, 33_792),
        "RO12.DAT" => render_ro_scene_atlas(name, packed, 51_072),
        "RO15.DAT" => render_ro_scene_atlas(name, packed, 55_296),
        "RO15_1.DAT" => render_ro_scene_atlas(name, packed, 32_160),
        "RO18.DAT" => render_ro_scene_atlas(name, packed, 35_328),
        "RO19.DAT" => render_ro_scene_atlas(name, packed, 21_856),
        "RO20.DAT" => render_ro_scene_atlas(name, packed, 36_864),
        "RO21.DAT" => render_ro_scene_atlas(name, packed, 55_808),
        "RO22.DAT" => render_ro_scene_atlas(name, packed, 32_352),
        "RO23.DAT" => render_ro_scene_atlas(name, packed, 55_296),
        "RO23_1.DAT" => render_ro_scene_atlas(name, packed, 27_648),
        "RO24.DAT" => render_ro_scene_atlas(name, packed, 45_376),
        "RO30.DAT" => render_ro_scene_atlas(name, packed, 39_616),
        "RO31.DAT" => render_ro_scene_atlas(name, packed, 26_752),
        "S1.DAT" => render_s_scene_atlas(name, packed, 62_976),
        "S2.DAT" => render_s_scene_atlas(name, packed, 53_760),
        "S3.DAT" => render_s_scene_atlas(name, packed, 47_360),
        "S7.DAT" => render_s_scene_atlas(name, packed, 57_408),
        "S8.DAT" => render_s_scene_atlas(name, packed, 29_952),
        "S9.DAT" => render_s_scene_atlas(name, packed, 59_136),
        "S11_1.DAT" => render_s_scene_atlas(name, packed, 63_488),
        "S11_2.DAT" => render_s_scene_atlas(name, packed, 60_672),
        "S13.DAT" => render_s_scene_atlas(name, packed, 21_120),
        "S15.DAT" => render_s_scene_atlas(name, packed, 42_432),
        "S16.DAT" => render_s_scene_atlas(name, packed, 21_024),
        "S18.DAT" => render_s_scene_atlas(name, packed, 56_544),
        "S20.DAT" => render_s_scene_atlas(name, packed, 56_704),
        "S21.DAT" => render_s_scene_atlas(name, packed, 8_960),
        "S1316.DAT" => render_s_scene_atlas(name, packed, 39_936),
        "ST1.DAT" => render_st_scene_atlas(name, packed, 36_544),
        "ST2.DAT" => render_st_scene_atlas(name, packed, 57_440),
        "ST4.DAT" => render_st_scene_atlas(name, packed, 37_664),
        "ST6.DAT" => render_st_scene_atlas(name, packed, 30_784),
        "ST8.DAT" => render_st_scene_atlas(name, packed, 24_320),
        "ST8_1.DAT" => render_st_scene_atlas(name, packed, 44_160),
        "ST10.DAT" => render_st_scene_atlas(name, packed, 55_296),
        "ST10_1.DAT" => render_st_scene_atlas(name, packed, 27_776),
        "ST11.DAT" => render_st_scene_atlas(name, packed, 27_456),
        "RE1.DAT" => render_re_scene_atlas(name, packed, 52_800),
        "RE2.DAT" => render_re_scene_atlas(name, packed, 28_416),
        "RE3.DAT" => render_re_scene_atlas(name, packed, 58_624),
        "RE6.DAT" => render_re_scene_atlas(name, packed, 29_376),
        "RE7.DAT" => render_re_scene_atlas(name, packed, 51_456),
        "RE9.DAT" => render_re_scene_atlas(name, packed, 31_808),
        "RE11.DAT" => render_re_scene_atlas(name, packed, 28_576),
        "RE15.DAT" => render_re_scene_atlas(name, packed, 36_672),
        "RE17.DAT" => render_re_scene_atlas(name, packed, 61_440),
        "RE17_1.DAT" => render_re_scene_atlas(name, packed, 17_952),
        "RE19.DAT" => render_re_scene_atlas(name, packed, 27_648),
        "RE21_1.DAT" => render_re_scene_atlas(name, packed, 57_216),
        "RE21_2.DAT" => render_re_scene_atlas(name, packed, 16_384),
        "SE1.DAT" => render_se_scene_atlas(name, packed, 63_936),
        "SE3.DAT" => render_se_scene_atlas(name, packed, 43_776),
        "SE4.DAT" => render_se_scene_atlas(name, packed, 43_136),
        "SE5.DAT" => render_se_scene_atlas(name, packed, 42_272),
        "SE7.DAT" => render_se_scene_atlas(name, packed, 55_296),
        "SE7_1.DAT" => render_se_scene_atlas(name, packed, 4_640),
        "SE10.DAT" => render_se_scene_atlas(name, packed, 27_648),
        "SE10_1.DAT" => render_se_scene_atlas(name, packed, 40_064),
        "SE12.DAT" => render_se_scene_atlas(name, packed, 30_592),
        "SE13.DAT" => render_se_scene_atlas(name, packed, 14_976),
        "SE17.DAT" => render_se_scene_atlas(name, packed, 54_528),
        "SE18.DAT" => render_se_scene_atlas(name, packed, 50_048),
        "SE19.DAT" => render_se_scene_atlas(name, packed, 28_608),
        "SE23.DAT" => render_se_scene_atlas(name, packed, 47_808),
        _ => render_screen_resource(packed),
    }
}

fn render_nocopy_resource(packed: &[u8]) -> Result<RenderedScreen> {
    let streams = decode_exact_streams(packed)?;
    let stream_sizes = streams.iter().map(Vec::len).collect::<Vec<_>>();
    if stream_sizes != [0, 0, PLANE_BYTES, PLANE_BYTES] {
        bail!(
            "unsupported NOCOPY stream sizes {stream_sizes:?}; expected [0, 0, {PLANE_BYTES}, {PLANE_BYTES}]"
        );
    }
    let blank = vec![0u8; PLANE_BYTES];
    Ok(RenderedScreen {
        layout: ScreenLayout::BrgiPlanes,
        stream_sizes,
        metadata_tail_bytes: vec![0; 4],
        companion_stream_sizes: Vec::new(),
        companion_audio: None,
        unrendered_ranges: Vec::new(),
        rgb: render_brgi([&blank, &blank, &streams[2], &streams[3]]),
    })
}

fn render_demo_linear_brgi_resource(packed: &[u8]) -> Result<RenderedScreen> {
    let streams = decode_exact_nonempty_streams(packed)?;
    let [decoded] = streams.as_slice() else {
        bail!(
            "unsupported Demo direct-copy stream count {}; expected one stream",
            streams.len()
        );
    };
    let pixel_bytes = DEMO_LINEAR_PLANE_BYTES * 4;
    if decoded.len() != pixel_bytes && decoded.len() != pixel_bytes + 128 {
        bail!(
            "unsupported Demo direct-copy decoded size {}; expected {} or {}",
            decoded.len(),
            pixel_bytes,
            pixel_bytes + 128
        );
    }
    let metadata_tail_bytes = decoded.len() - pixel_bytes;

    let planes = decoded[..pixel_bytes]
        .as_chunks::<DEMO_LINEAR_PLANE_BYTES>()
        .0;
    let source_rgb = render_brgi_dimensions(
        [&planes[0], &planes[1], &planes[2], &planes[3]],
        DEMO_LINEAR_WIDTH,
        DEMO_LINEAR_HEIGHT,
    );
    let mut rgb = vec![0u8; SCREEN_WIDTH * SCREEN_HEIGHT * 3];
    blit_demo_linear_rgb24(&source_rgb, &mut rgb)?;

    Ok(RenderedScreen {
        layout: ScreenLayout::LinearBrgiPlanes,
        stream_sizes: vec![decoded.len()],
        metadata_tail_bytes: vec![metadata_tail_bytes],
        companion_stream_sizes: Vec::new(),
        companion_audio: None,
        unrendered_ranges: Vec::new(),
        rgb,
    })
}

fn render_rtu_scene_atlas(
    name: &str,
    packed: &[u8],
    expected_decoded_size: usize,
) -> Result<RenderedScreen> {
    let streams = decode_exact_nonempty_streams(packed)?;
    let [decoded] = streams.as_slice() else {
        bail!(
            "unsupported RTU stream count {}; expected one stream",
            streams.len()
        );
    };
    if decoded.len() != expected_decoded_size {
        bail!(
            "unsupported {name} decoded size {}; expected {expected_decoded_size}",
            decoded.len()
        );
    }

    let mut rgb = vec![0u8; SCREEN_WIDTH * SCREEN_HEIGHT * 3];
    let mut unrendered_ranges = Vec::new();
    match name.to_ascii_uppercase().as_str() {
        "RTU1.DAT" => {
            render_plane_major_brgi_region(
                decoded,
                PlaneMajorRegion::new(0, 36, 0x1B00, 288, 192, 0, 0),
                &mut rgb,
            )?;
            // TYUKAN_R's complete segment-reference inventory proves that
            // all three RTU1 consumers end at or before 0x6C00.
            unrendered_ranges.push(DecodedRange {
                offset: 0x6C00,
                bytes: 0x400,
            });
        }
        "RTU2.DAT" => {
            render_plane_major_brgi_region(
                decoded,
                PlaneMajorRegion::new(0, 52, 0x2700, 416, 192, 0, 0),
                &mut rgb,
            )?;
        }
        "RTU3.DAT" => {
            render_plane_major_brgi_region(
                decoded,
                PlaneMajorRegion::new(0, 36, 0x3840, 288, 400, 0, 0),
                &mut rgb,
            )?;
        }
        "RTU4.DAT" => {
            for region in [
                PlaneMajorRegion::new(0, 36, 0x1B00, 288, 192, 0, 0),
                PlaneMajorRegion::new(0x6C00, 9, 0x168, 72, 40, 0, 208),
                PlaneMajorRegion::new(0x71A0, 9, 0x168, 72, 40, 80, 208),
                PlaneMajorRegion::new(0x7740, 2, 0x30, 16, 24, 160, 208),
                PlaneMajorRegion::new(0x7800, 2, 0x30, 16, 24, 184, 208),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
        }
        _ => unreachable!("named RTU dispatcher passed {name}"),
    }

    Ok(RenderedScreen {
        layout: ScreenLayout::BrgiSceneAtlas,
        stream_sizes: vec![decoded.len()],
        metadata_tail_bytes: Vec::new(),
        companion_stream_sizes: Vec::new(),
        companion_audio: None,
        unrendered_ranges,
        rgb,
    })
}

fn render_row_interleaved_brgi_region(
    decoded: &[u8],
    offset: usize,
    width: usize,
    height: usize,
    destination_x: usize,
    destination_y: usize,
    rgb: &mut [u8],
) -> Result<()> {
    if !width.is_multiple_of(8) {
        bail!("row-interleaved width {width} is not byte aligned");
    }
    let bytes = width
        .checked_div(8)
        .and_then(|row_bytes| row_bytes.checked_mul(4))
        .and_then(|row_bytes| row_bytes.checked_mul(height))
        .context("row-interleaved region size overflow")?;
    let end = offset
        .checked_add(bytes)
        .context("row-interleaved region end overflow")?;
    let region = decoded.get(offset..end).with_context(|| {
        format!(
            "row-interleaved region 0x{offset:X}..0x{end:X} exceeds decoded size 0x{:X}",
            decoded.len()
        )
    })?;
    render_row_interleaved_brgi(region, width, height, destination_x, destination_y, rgb)
}

fn render_ro_scene_atlas(
    name: &str,
    packed: &[u8],
    expected_decoded_size: usize,
) -> Result<RenderedScreen> {
    let streams = decode_exact_nonempty_streams(packed)?;
    let [decoded] = streams.as_slice() else {
        bail!(
            "unsupported RO stream count {}; expected one stream",
            streams.len()
        );
    };
    if decoded.len() != expected_decoded_size {
        bail!(
            "unsupported {name} decoded size {}; expected {expected_decoded_size}",
            decoded.len()
        );
    }

    let mut rgb = vec![0u8; SCREEN_WIDTH * SCREEN_HEIGHT * 3];
    let mut unrendered_ranges = Vec::new();
    match name.to_ascii_uppercase().as_str() {
        "RO1.DAT" => {
            for (offset, destination_x) in [
                (0x0000, 0),
                (0x0080, 24),
                (0x0100, 48),
                (0x0180, 72),
                (0x0200, 96),
                (0x0280, 120),
                (0x0300, 144),
                (0x0380, 168),
                (0x0400, 192),
                (0x0480, 216),
                (0x0500, 240),
            ] {
                render_plane_major_brgi_region(
                    decoded,
                    PlaneMajorRegion::new(offset, 2, 0x0020, 16, 16, destination_x, 0),
                    &mut rgb,
                )?;
            }
            for region in [
                PlaneMajorRegion::new(0x0580, 4, 0x0040, 32, 16, 264, 0),
                PlaneMajorRegion::new(0x0680, 4, 0x0080, 32, 32, 304, 0),
                PlaneMajorRegion::new(0x0880, 10, 0x01E0, 80, 48, 344, 0),
                PlaneMajorRegion::new(0x1000, 4, 0x0100, 32, 64, 432, 0),
                PlaneMajorRegion::new(0x1400, 6, 0x01E0, 48, 80, 472, 0),
                PlaneMajorRegion::new(0x1B80, 4, 0x01C0, 32, 112, 528, 0),
                PlaneMajorRegion::new(0x2280, 4, 0x0240, 32, 144, 568, 0),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
            for (offset, width, height, destination_x, destination_y) in [
                (0x4C80, 176, 96, 0, 152),
                (0x6D80, 176, 96, 184, 152),
                (0x8E80, 128, 64, 368, 152),
                (0x9E80, 128, 64, 504, 152),
                (0xBE80, 16, 16, 0, 256),
                (0xBF00, 16, 16, 24, 256),
                (0xBF80, 16, 16, 48, 256),
            ] {
                render_row_interleaved_brgi_region(
                    decoded,
                    offset,
                    width,
                    height,
                    destination_x,
                    destination_y,
                    &mut rgb,
                )?;
            }
            // OPENINGR's exhaustive owning-phase source inventory proves
            // these gaps are not consumed by RO1. Keep them explicit rather
            // than assigning pixel geometry from alignment alone.
            unrendered_ranges.extend([
                DecodedRange {
                    offset: 0x2B80,
                    bytes: 0x2100,
                },
                DecodedRange {
                    offset: 0xAE80,
                    bytes: 0x1000,
                },
            ]);
        }
        "RO2.DAT" | "RO9.DAT" => {
            render_plane_major_brgi_region(
                decoded,
                PlaneMajorRegion::new(0x0000, 36, 0x1B00, 288, 192, 0, 0),
                &mut rgb,
            )?;
        }
        "RO3.DAT" => {
            for (offset, destination_x) in [(0x0000, 0), (0x6C00, 296)] {
                render_plane_major_brgi_region(
                    decoded,
                    PlaneMajorRegion::new(offset, 36, 0x1B00, 288, 192, destination_x, 0),
                    &mut rgb,
                )?;
            }
        }
        "RO5.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 36, 0x1B00, 288, 192, 0, 0),
                PlaneMajorRegion::new(0x6C00, 21, 0x07E0, 168, 96, 296, 0),
                PlaneMajorRegion::new(0x8B80, 21, 0x0B28, 168, 136, 296, 104),
                PlaneMajorRegion::new(0xB820, 14, 0x0770, 112, 136, 472, 104),
                PlaneMajorRegion::new(0xD5E0, 11, 0x00B0, 88, 16, 296, 248),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
        }
        "RO6.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 36, 0x1B00, 288, 192, 0, 0),
                PlaneMajorRegion::new(0x6C00, 2, 0x0010, 16, 8, 296, 0),
                PlaneMajorRegion::new(0x6C40, 3, 0x0060, 24, 32, 320, 0),
                PlaneMajorRegion::new(0x6DC0, 3, 0x0060, 24, 32, 352, 0),
                PlaneMajorRegion::new(0x6F40, 3, 0x0060, 24, 32, 384, 0),
                PlaneMajorRegion::new(0x70C0, 3, 0x0060, 24, 32, 416, 0),
                PlaneMajorRegion::new(0x7240, 3, 0x0060, 24, 32, 448, 0),
                PlaneMajorRegion::new(0x73C0, 3, 0x0060, 24, 32, 480, 0),
                PlaneMajorRegion::new(0x7540, 3, 0x0060, 24, 32, 512, 0),
                PlaneMajorRegion::new(0x76C0, 5, 0x00A0, 40, 32, 544, 0),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
            for (offset, width, height, destination_x, destination_y) in [
                (0x7940, 16, 64, 296, 48),
                (0x7B40, 96, 144, 320, 48),
                (0x9640, 192, 32, 424, 48),
                (0xA240, 16, 128, 424, 88),
                (0xA640, 16, 96, 448, 88),
                (0xA940, 16, 80, 472, 88),
                (0xABC0, 16, 48, 496, 88),
                (0xAD40, 16, 32, 520, 88),
                (0xAE40, 16, 16, 544, 88),
            ] {
                render_row_interleaved_brgi_region(
                    decoded,
                    offset,
                    width,
                    height,
                    destination_x,
                    destination_y,
                    &mut rgb,
                )?;
            }
        }
        "RO10.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 36, 0x1B00, 288, 192, 0, 0),
                PlaneMajorRegion::new(0x6C00, 6, 0x0300, 48, 128, 296, 0),
                PlaneMajorRegion::new(0x7800, 9, 0x0558, 72, 152, 352, 0),
                PlaneMajorRegion::new(0x8D60, 2, 0x0090, 16, 72, 432, 0),
                PlaneMajorRegion::new(0x8FA0, 10, 0x04B0, 80, 120, 456, 0),
                PlaneMajorRegion::new(0xA260, 9, 0x0558, 72, 152, 544, 0),
                PlaneMajorRegion::new(0xB7C0, 8, 0x0240, 64, 72, 296, 160),
                PlaneMajorRegion::new(0xC0C0, 4, 0x01A0, 32, 104, 368, 160),
                PlaneMajorRegion::new(0xC740, 9, 0x0558, 72, 152, 408, 160),
                PlaneMajorRegion::new(0xDCA0, 1, 0x0008, 8, 8, 488, 160),
                PlaneMajorRegion::new(0xDCC0, 4, 0x01A0, 32, 104, 504, 160),
                PlaneMajorRegion::new(0xE340, 9, 0x0558, 72, 152, 544, 160),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
        }
        "RO11.DAT" => {
            render_plane_major_brgi_region(
                decoded,
                PlaneMajorRegion::new(0x0000, 44, 0x2100, 352, 192, 0, 0),
                &mut rgb,
            )?;
        }
        "RO12.DAT" => {
            render_plane_major_brgi_region(
                decoded,
                PlaneMajorRegion::new(0x0000, 36, 0x22E0, 288, 248, 0, 0),
                &mut rgb,
            )?;
            for (offset, destination_x) in
                [(0x8B80, 296), (0x8CA0, 328), (0x8DC0, 360), (0x8EE0, 392)]
            {
                render_plane_major_brgi_region(
                    decoded,
                    PlaneMajorRegion::new(offset, 3, 0x0048, 24, 24, destination_x, 0),
                    &mut rgb,
                )?;
            }
            for (offset, destination_x) in [(0x9000, 424), (0x9180, 464)] {
                render_plane_major_brgi_region(
                    decoded,
                    PlaneMajorRegion::new(offset, 4, 0x0060, 32, 24, destination_x, 0),
                    &mut rgb,
                )?;
            }
            for (offset, width, height, destination_x, destination_y) in [
                (0x9300, 24, 16, 504, 0),
                (0x93C0, 104, 16, 536, 0),
                (0x9700, 120, 32, 296, 32),
                (0x9E80, 16, 80, 424, 32),
                (0xA100, 136, 64, 448, 32),
                (0xB200, 152, 16, 296, 120),
                (0xB6C0, 168, 16, 456, 120),
                (0xBC00, 184, 32, 296, 144),
            ] {
                render_row_interleaved_brgi_region(
                    decoded,
                    offset,
                    width,
                    height,
                    destination_x,
                    destination_y,
                    &mut rgb,
                )?;
            }
        }
        "RO15.DAT" | "RO23.DAT" => {
            if name.eq_ignore_ascii_case("RO15.DAT") {
                render_plane_major_brgi_region(
                    decoded,
                    PlaneMajorRegion::new(0x0000, 36, 0x3600, 288, 384, 0, 0),
                    &mut rgb,
                )?;
            } else {
                render_row_interleaved_brgi_region(decoded, 0x0000, 288, 192, 0, 0, &mut rgb)?;
                render_row_interleaved_brgi_region(decoded, 0x6C00, 288, 192, 296, 0, &mut rgb)?;
            }
        }
        "RO15_1.DAT" => {
            for (offset, width, height, destination_x, destination_y) in [
                (0x0000, 176, 64, 0, 0),
                (0x1600, 160, 88, 184, 0),
                (0x3180, 144, 96, 352, 0),
                (0x4C80, 120, 112, 504, 0),
                (0x66C0, 72, 120, 0, 120),
                (0x77A0, 24, 128, 80, 120),
            ] {
                render_row_interleaved_brgi_region(
                    decoded,
                    offset,
                    width,
                    height,
                    destination_x,
                    destination_y,
                    &mut rgb,
                )?;
            }
        }
        "RO18.DAT" => {
            render_plane_major_brgi_region(
                decoded,
                PlaneMajorRegion::new(0x0000, 46, 0x2280, 368, 192, 0, 0),
                &mut rgb,
            )?;
        }
        "RO19.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 16, 0x0980, 128, 152, 0, 0),
                PlaneMajorRegion::new(0x2600, 7, 0x0118, 56, 40, 136, 0),
                PlaneMajorRegion::new(0x2A60, 7, 0x0118, 56, 40, 200, 0),
                PlaneMajorRegion::new(0x2EC0, 7, 0x0118, 56, 40, 264, 0),
                PlaneMajorRegion::new(0x3320, 9, 0x01B0, 72, 48, 328, 0),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
            for (offset, destination_x, destination_y) in [
                (0x39E0, 408, 0),
                (0x40C0, 504, 0),
                (0x47A0, 408, 48),
                (0x4E80, 504, 48),
            ] {
                render_row_interleaved_brgi_region(
                    decoded,
                    offset,
                    88,
                    40,
                    destination_x,
                    destination_y,
                    &mut rgb,
                )?;
            }
        }
        "RO20.DAT" => {
            for (offset, destination_x) in [(0x0000, 0), (0x3000, 136), (0x6000, 272)] {
                render_row_interleaved_brgi_region(
                    decoded,
                    offset,
                    128,
                    192,
                    destination_x,
                    0,
                    &mut rgb,
                )?;
            }
        }
        "RO21.DAT" => {
            render_row_interleaved_brgi_region(decoded, 0x0000, 160, 192, 0, 0, &mut rgb)?;
            render_plane_major_brgi_region(
                decoded,
                PlaneMajorRegion::new(0x3C00, 36, 0x1B00, 288, 192, 168, 0),
                &mut rgb,
            )?;
            render_row_interleaved_brgi_region(decoded, 0xA800, 96, 80, 464, 0, &mut rgb)?;
            render_row_interleaved_brgi_region(decoded, 0xB700, 224, 80, 0, 200, &mut rgb)?;
        }
        "RO22.DAT" => {
            render_row_interleaved_brgi_region(decoded, 0x0000, 168, 56, 0, 0, &mut rgb)?;
            render_row_interleaved_brgi_region(decoded, 0x1260, 288, 192, 176, 0, &mut rgb)?;
        }
        "RO23_1.DAT" => {
            render_row_interleaved_brgi_region(decoded, 0x0000, 288, 192, 0, 0, &mut rgb)?;
        }
        "RO24.DAT" => {
            render_plane_major_brgi_region(
                decoded,
                PlaneMajorRegion::new(0x0000, 36, 0x1B00, 288, 192, 0, 0),
                &mut rgb,
            )?;
            for (offset, width, height, destination_x, destination_y) in [
                (0x6C00, 32, 48, 296, 0),
                (0x6F00, 256, 96, 336, 0),
                (0x9F00, 80, 96, 296, 104),
            ] {
                render_row_interleaved_brgi_region(
                    decoded,
                    offset,
                    width,
                    height,
                    destination_x,
                    destination_y,
                    &mut rgb,
                )?;
            }
            for (offset, destination_x) in [(0xAE00, 384), (0xAF20, 416)] {
                render_plane_major_brgi_region(
                    decoded,
                    PlaneMajorRegion::new(offset, 3, 0x0048, 24, 24, destination_x, 104),
                    &mut rgb,
                )?;
            }
            for (index, offset) in (0xB040..0xB140).step_by(0x20).enumerate() {
                render_row_interleaved_brgi_region(
                    decoded,
                    offset,
                    8,
                    8,
                    448 + index * 16,
                    104,
                    &mut rgb,
                )?;
            }
        }
        "RO30.DAT" => {
            for (offset, width, height, destination_x) in [
                (0x0000, 192, 176, 0),
                (0x4200, 208, 176, 200),
                (0x8980, 48, 72, 416),
                (0x9040, 48, 112, 472),
            ] {
                render_row_interleaved_brgi_region(
                    decoded,
                    offset,
                    width,
                    height,
                    destination_x,
                    0,
                    &mut rgb,
                )?;
            }
        }
        "RO31.DAT" => {
            for (offset, destination_x) in [(0x0000, 0), (0x3440, 160)] {
                render_row_interleaved_brgi_region(
                    decoded,
                    offset,
                    152,
                    176,
                    destination_x,
                    0,
                    &mut rgb,
                )?;
            }
        }
        _ => unreachable!("named RO dispatcher passed {name}"),
    }

    Ok(RenderedScreen {
        layout: ScreenLayout::BrgiSceneAtlas,
        stream_sizes: vec![decoded.len()],
        metadata_tail_bytes: Vec::new(),
        companion_stream_sizes: Vec::new(),
        companion_audio: None,
        unrendered_ranges,
        rgb,
    })
}

fn render_s_scene_atlas(
    name: &str,
    packed: &[u8],
    expected_decoded_size: usize,
) -> Result<RenderedScreen> {
    let streams = decode_exact_nonempty_streams(packed)?;
    let [decoded] = streams.as_slice() else {
        bail!(
            "unsupported S stream count {}; expected one stream",
            streams.len()
        );
    };
    if decoded.len() != expected_decoded_size {
        bail!(
            "unsupported {name} decoded size {}; expected {expected_decoded_size}",
            decoded.len()
        );
    }

    let mut rgb = vec![0u8; SCREEN_WIDTH * SCREEN_HEIGHT * 3];
    let mut unrendered_ranges = Vec::new();
    match name.to_ascii_uppercase().as_str() {
        "S1.DAT" => {
            render_plane_major_brgi_region(
                decoded,
                PlaneMajorRegion::new(0x0000, 40, 0x2580, 320, 240, 0, 0),
                &mut rgb,
            )?;
            for (offset, destination_x, destination_y) in [
                (0x9600, 328, 0),
                (0xAE00, 464, 0),
                (0xC600, 328, 104),
                (0xDE00, 464, 104),
            ] {
                render_row_interleaved_brgi(
                    &decoded[offset..offset + 0x1800],
                    128,
                    96,
                    destination_x,
                    destination_y,
                    &mut rgb,
                )?;
            }
        }
        "S2.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 52, 0x2D80, 416, 224, 0, 0),
                PlaneMajorRegion::new(0xB600, 12, 0x06C0, 96, 144, 424, 0),
                PlaneMajorRegion::new(0xD100, 2, 0x0020, 16, 16, 528, 0),
                PlaneMajorRegion::new(0xD180, 2, 0x0020, 16, 16, 552, 0),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
        }
        "S3.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 44, 0x2100, 352, 192, 0, 0),
                PlaneMajorRegion::new(0x8400, 7, 0x00A8, 56, 24, 360, 0),
                PlaneMajorRegion::new(0x86A0, 7, 0x00A8, 56, 24, 424, 0),
                PlaneMajorRegion::new(0x8940, 7, 0x00A8, 56, 24, 488, 0),
                PlaneMajorRegion::new(0x8BE0, 7, 0x0150, 56, 48, 552, 0),
                PlaneMajorRegion::new(0x9120, 7, 0x0150, 56, 48, 360, 32),
                PlaneMajorRegion::new(0x9660, 7, 0x0150, 56, 48, 424, 32),
                PlaneMajorRegion::new(0x9BA0, 12, 0x03C0, 96, 80, 488, 56),
                PlaneMajorRegion::new(0xAAA0, 5, 0x00A0, 40, 32, 360, 88),
                PlaneMajorRegion::new(0xAD20, 7, 0x0118, 56, 40, 408, 88),
                PlaneMajorRegion::new(0xB180, 8, 0x0080, 64, 16, 360, 136),
                PlaneMajorRegion::new(0xB380, 8, 0x0080, 64, 16, 432, 136),
                PlaneMajorRegion::new(0xB580, 1, 0x0010, 8, 16, 504, 136),
                PlaneMajorRegion::new(0xB5C0, 2, 0x0030, 16, 24, 520, 136),
                PlaneMajorRegion::new(0xB680, 2, 0x0030, 16, 24, 544, 136),
                PlaneMajorRegion::new(0xB740, 3, 0x0030, 24, 16, 568, 136),
                PlaneMajorRegion::new(0xB800, 2, 0x0020, 16, 16, 600, 136),
                PlaneMajorRegion::new(0xB880, 2, 0x0020, 16, 16, 620, 136),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
        }
        "S7.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 23, 0x1140, 184, 192, 0, 0),
                PlaneMajorRegion::new(0x4500, 8, 0x0100, 64, 32, 576, 0),
                PlaneMajorRegion::new(0x4900, 7, 0x0150, 56, 48, 576, 40),
                PlaneMajorRegion::new(0x4E40, 23, 0x1140, 184, 192, 192, 0),
                PlaneMajorRegion::new(0x9340, 8, 0x0200, 64, 64, 576, 96),
                PlaneMajorRegion::new(0x9B40, 23, 0x1140, 184, 192, 384, 0),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
        }
        "S8.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 36, 0x1B00, 288, 192, 0, 0),
                PlaneMajorRegion::new(0x6C00, 6, 0x0120, 48, 48, 296, 0),
                PlaneMajorRegion::new(0x7080, 6, 0x0120, 48, 48, 352, 0),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
        }
        "S9.DAT" => {
            for (offset, width, height, destination_x, destination_y) in [
                (0x0000, 112, 64, 296, 0),
                (0x0E00, 176, 96, 416, 0),
                (0x2F00, 112, 64, 296, 72),
                (0x3D00, 176, 96, 416, 104),
                (0x5E00, 112, 64, 296, 144),
                (0x6C00, 176, 96, 0, 168),
            ] {
                let bytes = width / 8 * 4 * height;
                render_row_interleaved_brgi(
                    &decoded[offset..offset + bytes],
                    width,
                    height,
                    destination_x,
                    destination_y,
                    &mut rgb,
                )?;
            }
            render_plane_major_brgi_region(
                decoded,
                PlaneMajorRegion::new(0x8D00, 36, 0x1680, 288, 160, 0, 0),
                &mut rgb,
            )?;
        }
        "S11_1.DAT" => {
            render_plane_major_brgi_region(
                decoded,
                PlaneMajorRegion::new(0x0000, 36, 0x1B00, 288, 192, 0, 0),
                &mut rgb,
            )?;
            render_row_interleaved_brgi(&decoded[0x6C00..0xB200], 224, 160, 296, 0, &mut rgb)?;
            render_row_interleaved_brgi(&decoded[0xB200..0xF800], 224, 160, 296, 168, &mut rgb)?;
        }
        "S11_2.DAT" => {
            render_row_interleaved_brgi(&decoded[0x0000..0x4600], 224, 160, 0, 0, &mut rgb)?;
            render_row_interleaved_brgi(&decoded[0x4600..0x8C00], 224, 160, 232, 0, &mut rgb)?;
            for (offset, height, destination_x, destination_y) in [
                (0x8C00, 144, 464, 0),
                (0x9980, 144, 520, 0),
                (0xA700, 144, 576, 0),
                (0xB480, 144, 464, 152),
                (0xC200, 144, 520, 152),
                (0xCF80, 152, 576, 152),
                (0xDDC0, 152, 0, 168),
            ] {
                let bytes = 6 * 4 * height;
                render_row_interleaved_brgi(
                    &decoded[offset..offset + bytes],
                    48,
                    height,
                    destination_x,
                    destination_y,
                    &mut rgb,
                )?;
            }
            // Sixteen state-table calls advance the source every second call,
            // selecting eight contiguous 0x20-byte 8x8 effects.
            for index in 0..8 {
                let offset = 0xEC00 + index * 0x20;
                render_row_interleaved_brgi(
                    &decoded[offset..offset + 0x20],
                    8,
                    8,
                    56 + index * 12,
                    168,
                    &mut rgb,
                )?;
            }
        }
        "S13.DAT" => {
            for (offset, destination_x) in [(0x0000, 0), (0x1400, 88), (0x2800, 176), (0x3C00, 264)]
            {
                render_row_interleaved_brgi(
                    &decoded[offset..offset + 0x1400],
                    80,
                    128,
                    destination_x,
                    0,
                    &mut rgb,
                )?;
            }
            for (offset, width, destination_x) in
                [(0x5000, 32, 352), (0x5100, 32, 392), (0x5200, 16, 432)]
            {
                let bytes = width / 8 * 4 * 16;
                render_row_interleaved_brgi(
                    &decoded[offset..offset + bytes],
                    width,
                    16,
                    destination_x,
                    0,
                    &mut rgb,
                )?;
            }
        }
        "S15.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 4, 0x0300, 32, 192, 0, 0),
                PlaneMajorRegion::new(0x0C00, 27, 0x0360, 216, 32, 256, 0),
                PlaneMajorRegion::new(0x1980, 5, 0x03C0, 40, 192, 40, 0),
                PlaneMajorRegion::new(0x2880, 27, 0x0510, 216, 48, 256, 40),
                PlaneMajorRegion::new(0x3CC0, 4, 0x01C0, 32, 112, 256, 152),
                PlaneMajorRegion::new(0x43C0, 3, 0x0150, 24, 112, 296, 152),
                PlaneMajorRegion::new(0x4900, 27, 0x0510, 216, 48, 256, 96),
                PlaneMajorRegion::new(0x5D40, 10, 0x0320, 80, 80, 480, 0),
                PlaneMajorRegion::new(0x69C0, 16, 0x0200, 128, 32, 480, 88),
                PlaneMajorRegion::new(0x71C0, 12, 0x0300, 96, 64, 480, 128),
                PlaneMajorRegion::new(0x7DC0, 8, 0x0080, 64, 16, 328, 152),
                PlaneMajorRegion::new(0x7FC0, 20, 0x0960, 160, 120, 88, 0),
                PlaneMajorRegion::new(0xA540, 1, 0x0010, 8, 16, 400, 152),
                PlaneMajorRegion::new(0xA580, 1, 0x0010, 8, 16, 416, 152),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
        }
        "S16.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 10, 0x0500, 80, 128, 0, 0),
                PlaneMajorRegion::new(0x1400, 4, 0x0040, 32, 16, 88, 0),
                PlaneMajorRegion::new(0x1500, 4, 0x0040, 32, 16, 128, 0),
                PlaneMajorRegion::new(0x1600, 11, 0x00B0, 88, 16, 168, 0),
                PlaneMajorRegion::new(0x18C0, 11, 0x01B8, 88, 40, 264, 0),
                PlaneMajorRegion::new(0x1FA0, 13, 0x0680, 104, 128, 360, 0),
                PlaneMajorRegion::new(0x39A0, 6, 0x0120, 48, 48, 472, 0),
                PlaneMajorRegion::new(0x3E20, 2, 0x0120, 16, 144, 528, 0),
                PlaneMajorRegion::new(0x42A0, 10, 0x03C0, 80, 96, 552, 0),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
            // All 29 direct DS loads through the reused segment variable have
            // immediate SI; the ten S16-phase sources stop at 0x51A0. Keep the
            // structured tail explicit without inventing a storage shape.
            unrendered_ranges.push(DecodedRange {
                offset: 0x51A0,
                bytes: 0x0080,
            });
        }
        "S18.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 25, 0x1068, 200, 168, 0, 0),
                PlaneMajorRegion::new(0x41A0, 25, 0x1068, 200, 168, 208, 0),
                PlaneMajorRegion::new(0x8340, 25, 0x1068, 200, 168, 416, 0),
                PlaneMajorRegion::new(0xC4E0, 14, 0x05B0, 112, 104, 0, 176),
                PlaneMajorRegion::new(0xDBA0, 5, 0x0050, 40, 16, 120, 176),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
        }
        "S20.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 16, 0x0100, 128, 16, 448, 0),
                PlaneMajorRegion::new(0x0400, 12, 0x00C0, 96, 16, 448, 24),
                PlaneMajorRegion::new(0x0700, 10, 0x00A0, 80, 16, 552, 24),
                PlaneMajorRegion::new(0x0980, 8, 0x0080, 64, 16, 448, 48),
                PlaneMajorRegion::new(0x0B80, 6, 0x0060, 48, 16, 520, 48),
                PlaneMajorRegion::new(0x0D00, 4, 0x0040, 32, 16, 576, 48),
                PlaneMajorRegion::new(0x0E00, 2, 0x0020, 16, 16, 616, 48),
                PlaneMajorRegion::new(0x0E80, 6, 0x0120, 48, 48, 448, 72),
                PlaneMajorRegion::new(0x1300, 27, 0x10E0, 216, 160, 0, 0),
                PlaneMajorRegion::new(0x5680, 27, 0x10E0, 216, 160, 224, 0),
                PlaneMajorRegion::new(0x9A00, 27, 0x10E0, 216, 160, 0, 168),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
        }
        "S21.DAT" => {
            render_plane_major_brgi_region(
                decoded,
                PlaneMajorRegion::new(0x0000, 20, 0x08C0, 160, 112, 0, 0),
                &mut rgb,
            )?;
        }
        "S1316.DAT" => {
            render_plane_major_brgi_region(
                decoded,
                PlaneMajorRegion::new(0x0000, 52, 0x2700, 416, 192, 0, 0),
                &mut rgb,
            )?;
        }
        _ => unreachable!("named S dispatcher passed {name}"),
    }

    Ok(RenderedScreen {
        layout: ScreenLayout::BrgiSceneAtlas,
        stream_sizes: vec![decoded.len()],
        metadata_tail_bytes: Vec::new(),
        companion_stream_sizes: Vec::new(),
        companion_audio: None,
        unrendered_ranges,
        rgb,
    })
}

fn render_st_scene_atlas(
    name: &str,
    packed: &[u8],
    expected_decoded_size: usize,
) -> Result<RenderedScreen> {
    let streams = decode_exact_nonempty_streams(packed)?;
    let [decoded] = streams.as_slice() else {
        bail!(
            "unsupported ST stream count {}; expected one stream",
            streams.len()
        );
    };
    if decoded.len() != expected_decoded_size {
        bail!(
            "unsupported {name} decoded size {}; expected {expected_decoded_size}",
            decoded.len()
        );
    }

    let mut rgb = vec![0u8; SCREEN_WIDTH * SCREEN_HEIGHT * 3];
    let mut unrendered_ranges = Vec::new();
    match name.to_ascii_uppercase().as_str() {
        "ST1.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 30, 0x0A50, 240, 88, 0, 0),
                PlaneMajorRegion::new(0x2940, 4, 0x0080, 32, 32, 488, 0),
                PlaneMajorRegion::new(0x2B40, 2, 0x0080, 16, 64, 528, 0),
                PlaneMajorRegion::new(0x2D40, 6, 0x0210, 48, 88, 248, 96),
                PlaneMajorRegion::new(0x3580, 6, 0x01B0, 48, 72, 304, 96),
                PlaneMajorRegion::new(0x3C40, 30, 0x0A50, 240, 88, 248, 0),
                PlaneMajorRegion::new(0x6580, 30, 0x0A50, 240, 88, 0, 96),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
        }
        "ST2.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 30, 0x1FE0, 240, 272, 0, 0),
                PlaneMajorRegion::new(0x7F80, 11, 0x0108, 88, 24, 248, 128),
                PlaneMajorRegion::new(0x83A0, 11, 0x0108, 88, 24, 344, 128),
                PlaneMajorRegion::new(0x8D00, 4, 0x0040, 32, 16, 440, 128),
                PlaneMajorRegion::new(0x8E00, 4, 0x0040, 32, 16, 440, 152),
                PlaneMajorRegion::new(0x8F00, 4, 0x0040, 32, 16, 440, 176),
                PlaneMajorRegion::new(0x9380, 21, 0x09D8, 168, 120, 248, 0),
                PlaneMajorRegion::new(0xBAE0, 25, 0x04B0, 200, 48, 424, 0),
                PlaneMajorRegion::new(0xCDA0, 25, 0x04B0, 200, 48, 424, 56),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
            unrendered_ranges.push(DecodedRange {
                offset: 0x87C0,
                bytes: 0x540,
            });
            // The apparent 0x9000 shape came from a consumer after ST10 had
            // replaced this reused segment. ST2 itself never selects it.
            unrendered_ranges.push(DecodedRange {
                offset: 0x9000,
                bytes: 0x380,
            });
        }
        "ST4.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 16, 0x0600, 128, 96, 0, 0),
                PlaneMajorRegion::new(0x1800, 18, 0x05A0, 144, 80, 136, 0),
                PlaneMajorRegion::new(0x2E80, 21, 0x0C78, 168, 152, 0, 104),
                PlaneMajorRegion::new(0x6060, 21, 0x0C78, 168, 152, 176, 104),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
            for (index, offset) in [0x9240, 0x9260, 0x9280, 0x92A0].into_iter().enumerate() {
                render_row_interleaved_brgi(
                    &decoded[offset..offset + 0x20],
                    8,
                    8,
                    352 + index * 12,
                    104,
                    &mut rgb,
                )?;
            }
            for (index, offset) in [0x92C0, 0x92E0, 0x9300].into_iter().enumerate() {
                render_plane_major_brgi_region(
                    decoded,
                    PlaneMajorRegion::new(offset, 1, 8, 8, 8, 400 + index * 12, 104),
                    &mut rgb,
                )?;
            }
        }
        "ST6.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 22, 0x0D10, 176, 152, 0, 0),
                PlaneMajorRegion::new(0x3440, 11, 0x0580, 88, 128, 184, 0),
                PlaneMajorRegion::new(0x4A40, 11, 0x0580, 88, 128, 280, 0),
                PlaneMajorRegion::new(0x6040, 11, 0x0580, 88, 128, 376, 0),
                PlaneMajorRegion::new(0x7640, 4, 0x0080, 32, 32, 472, 0),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
        }
        "ST8.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 20, 0x0780, 160, 96, 0, 0),
                PlaneMajorRegion::new(0x1E00, 20, 0x0780, 160, 96, 168, 0),
                PlaneMajorRegion::new(0x3C00, 20, 0x08C0, 160, 112, 336, 0),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
        }
        "ST8_1.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 27, 0x10E0, 216, 160, 0, 0),
                PlaneMajorRegion::new(0x4380, 21, 0x0D20, 168, 160, 224, 0),
                PlaneMajorRegion::new(0x7800, 21, 0x0D20, 168, 160, 400, 0),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
        }
        "ST10.DAT" => {
            render_row_interleaved_brgi(decoded, 288, 384, 0, 0, &mut rgb)?;
        }
        "ST10_1.DAT" => {
            // The 0x90-byte storage stride admits 192 complete diagnostic rows.
            // The pinned post-reload controller's direct read union ends at
            // 0x6540; the remaining complete rows stay visible for review, while
            // the final partial row remains explicit instead of gaining a shape.
            render_row_interleaved_brgi(&decoded[..0x6C00], 288, 192, 0, 0, &mut rgb)?;
            unrendered_ranges.push(DecodedRange {
                offset: 0x6C00,
                bytes: 0x80,
            });
        }
        "ST11.DAT" => {
            render_plane_major_brgi_region(
                decoded,
                PlaneMajorRegion::new(0, 22, 0x1AD0, 176, 312, 0, 0),
                &mut rgb,
            )?;
        }
        _ => unreachable!("named ST dispatcher passed {name}"),
    }

    Ok(RenderedScreen {
        layout: ScreenLayout::BrgiSceneAtlas,
        stream_sizes: vec![decoded.len()],
        metadata_tail_bytes: Vec::new(),
        companion_stream_sizes: Vec::new(),
        companion_audio: None,
        unrendered_ranges,
        rgb,
    })
}

fn render_re_scene_atlas(
    name: &str,
    packed: &[u8],
    expected_decoded_size: usize,
) -> Result<RenderedScreen> {
    let streams = decode_exact_nonempty_streams(packed)?;
    let [decoded] = streams.as_slice() else {
        bail!(
            "unsupported RE stream count {}; expected one stream",
            streams.len()
        );
    };
    if decoded.len() != expected_decoded_size {
        bail!(
            "unsupported {name} decoded size {}; expected {expected_decoded_size}",
            decoded.len()
        );
    }

    let mut rgb = vec![0u8; SCREEN_WIDTH * SCREEN_HEIGHT * 3];
    let mut unrendered_ranges = Vec::new();
    match name.to_ascii_uppercase().as_str() {
        "RE1.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 68, 0x3300, 544, 192, 0, 0),
                PlaneMajorRegion::new(0xCC00, 3, 0x0048, 24, 24, 0, 200),
                PlaneMajorRegion::new(0xCD20, 3, 0x0048, 24, 24, 32, 200),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
        }
        "RE2.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 36, 0x1B00, 288, 192, 0, 0),
                PlaneMajorRegion::new(0x6C00, 8, 0x00C0, 64, 24, 296, 0),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
        }
        "RE3.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 36, 0x1B00, 288, 192, 0, 0),
                PlaneMajorRegion::new(0x6C00, 8, 0x0080, 64, 16, 296, 0),
                PlaneMajorRegion::new(0x6E00, 2, 0x0010, 16, 8, 368, 0),
                PlaneMajorRegion::new(0x6E40, 6, 0x00F0, 48, 40, 392, 0),
                PlaneMajorRegion::new(0x7200, 36, 0x1B00, 288, 192, 0, 200),
                PlaneMajorRegion::new(0xDE00, 5, 0x00A0, 40, 32, 296, 200),
                PlaneMajorRegion::new(0xE080, 5, 0x00A0, 40, 32, 344, 200),
                PlaneMajorRegion::new(0xE300, 4, 0x0060, 32, 24, 392, 200),
                PlaneMajorRegion::new(0xE480, 2, 0x0020, 16, 16, 432, 200),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
        }
        "RE6.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 36, 0x1B00, 288, 192, 0, 0),
                PlaneMajorRegion::new(0x6C00, 6, 0x00F0, 48, 40, 296, 0),
                PlaneMajorRegion::new(0x6FC0, 5, 0x0078, 40, 24, 352, 0),
                PlaneMajorRegion::new(0x71A0, 1, 0x0018, 8, 24, 400, 0),
                PlaneMajorRegion::new(0x7200, 1, 0x0018, 8, 24, 416, 0),
                PlaneMajorRegion::new(0x7260, 1, 0x0018, 8, 24, 432, 0),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
        }
        "RE7.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 36, 0x1B00, 288, 192, 0, 0),
                PlaneMajorRegion::new(0x6C00, 21, 0x0540, 168, 64, 296, 0),
                PlaneMajorRegion::new(0x8100, 36, 0x1200, 288, 128, 0, 200),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
        }
        "RE9.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 36, 0x1B00, 288, 192, 0, 0),
                PlaneMajorRegion::new(0x6C00, 3, 0x0030, 24, 16, 296, 0),
                PlaneMajorRegion::new(0x6D80, 2, 0x0020, 16, 16, 328, 0),
                PlaneMajorRegion::new(0x6E00, 2, 0x0020, 16, 16, 352, 0),
                PlaneMajorRegion::new(0x6E80, 4, 0x00E0, 32, 56, 376, 0),
                PlaneMajorRegion::new(0x7200, 7, 0x0230, 56, 80, 416, 0),
                PlaneMajorRegion::new(0x7AC0, 3, 0x0030, 24, 16, 480, 0),
                PlaneMajorRegion::new(0x7B80, 3, 0x0030, 24, 16, 512, 0),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
            // ENDING_R's exhaustive direct/state-driven source inventory
            // proves this 192-byte gap is not consumed by the RE9 phase.
            unrendered_ranges.push(DecodedRange {
                offset: 0x6CC0,
                bytes: 0x00C0,
            });
        }
        "RE11.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 36, 0x1B00, 288, 192, 0, 0),
                PlaneMajorRegion::new(0x6C00, 2, 0x0020, 16, 16, 296, 0),
                PlaneMajorRegion::new(0x6C80, 5, 0x00C8, 40, 40, 320, 0),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
        }
        "RE15.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 36, 0x1B00, 288, 192, 0, 0),
                PlaneMajorRegion::new(0x6C00, 8, 0x01C0, 64, 56, 296, 0),
                PlaneMajorRegion::new(0x7300, 8, 0x00C0, 64, 24, 368, 0),
                PlaneMajorRegion::new(0x7600, 8, 0x0300, 64, 96, 440, 0),
                PlaneMajorRegion::new(0x8200, 8, 0x00C0, 64, 24, 296, 104),
                PlaneMajorRegion::new(0x8500, 8, 0x00C0, 64, 24, 368, 104),
                PlaneMajorRegion::new(0x8800, 8, 0x00C0, 64, 24, 440, 104),
                PlaneMajorRegion::new(0x8B00, 8, 0x00C0, 64, 24, 512, 104),
                PlaneMajorRegion::new(0x8E00, 3, 0x0030, 24, 16, 584, 104),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
            // The only dynamic 0x3E6E consumer belongs to the earlier RE1
            // phase; every post-RE15-reload source ends at or before 0x8EC0.
            unrendered_ranges.push(DecodedRange {
                offset: 0x8EC0,
                bytes: 0x0080,
            });
        }
        "RE17.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 40, 0x1E00, 320, 192, 0, 0),
                PlaneMajorRegion::new(0x7800, 14, 0x0A80, 112, 192, 320, 0),
                PlaneMajorRegion::new(0xA200, 17, 0x0CC0, 136, 192, 432, 0),
                PlaneMajorRegion::new(0xD500, 9, 0x06C0, 72, 192, 568, 0),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
        }
        "RE17_1.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 12, 0x0900, 96, 192, 0, 0),
                PlaneMajorRegion::new(0x2400, 7, 0x0540, 56, 192, 96, 0),
                PlaneMajorRegion::new(0x3900, 12, 0x0180, 96, 32, 160, 0),
                PlaneMajorRegion::new(0x3F00, 12, 0x0180, 96, 32, 264, 0),
                PlaneMajorRegion::new(0x4500, 3, 0x0048, 24, 24, 368, 0),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
        }
        "RE19.DAT" => {
            render_plane_major_brgi_region(
                decoded,
                PlaneMajorRegion::new(0x0000, 36, 0x1B00, 288, 192, 0, 0),
                &mut rgb,
            )?;
        }
        "RE21_1.DAT" => {
            for (offset, width, height, destination_x, destination_y) in [
                (0x0000, 64, 48, 96, 168),
                (0x0600, 80, 112, 504, 0),
                (0x1780, 72, 48, 168, 168),
                (0x1E40, 88, 112, 0, 168),
                (0x3180, 104, 160, 392, 0),
                (0x5200, 16, 64, 472, 168),
                (0x5400, 16, 80, 496, 168),
                (0x5680, 16, 128, 520, 168),
                (0x5A80, 16, 144, 544, 168),
                (0x5F00, 152, 160, 232, 0),
                (0x8E80, 16, 144, 568, 168),
                (0x9300, 16, 112, 592, 168),
                (0x9680, 48, 16, 0, 320),
                (0x9800, 64, 16, 56, 320),
                (0x9A00, 48, 16, 128, 320),
                (0x9B80, 32, 64, 336, 168),
                (0x9F80, 80, 16, 184, 320),
                (0xA200, 224, 96, 0, 0),
                (0xCC00, 80, 16, 272, 320),
                (0xCE80, 48, 32, 376, 168),
                (0xD180, 80, 64, 248, 168),
                (0xDB80, 16, 48, 616, 168),
                (0xDD00, 32, 32, 432, 168),
                (0xDF00, 16, 16, 360, 320),
            ] {
                let bytes = width / 8 * 4 * height;
                render_row_interleaved_brgi(
                    &decoded[offset..offset + bytes],
                    width,
                    height,
                    destination_x,
                    destination_y,
                    &mut rgb,
                )?;
            }
        }
        "RE21_2.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 2, 0x0040, 16, 32, 472, 0),
                PlaneMajorRegion::new(0x0100, 2, 0x0020, 16, 16, 496, 0),
                PlaneMajorRegion::new(0x0180, 4, 0x0040, 32, 16, 520, 0),
                PlaneMajorRegion::new(0x0280, 2, 0x0020, 16, 16, 560, 0),
                PlaneMajorRegion::new(0x0300, 2, 0x0020, 16, 16, 584, 0),
                PlaneMajorRegion::new(0x0380, 10, 0x0320, 80, 80, 0, 0),
                PlaneMajorRegion::new(0x1000, 2, 0x0040, 16, 32, 608, 0),
                PlaneMajorRegion::new(0x1100, 4, 0x0040, 32, 16, 472, 40),
                PlaneMajorRegion::new(0x1200, 4, 0x0080, 32, 32, 512, 40),
                PlaneMajorRegion::new(0x1400, 4, 0x0040, 32, 16, 552, 40),
                PlaneMajorRegion::new(0x1500, 6, 0x0060, 48, 16, 472, 80),
                PlaneMajorRegion::new(0x1680, 22, 0x02C0, 176, 32, 88, 0),
                PlaneMajorRegion::new(0x2180, 2, 0x0020, 16, 16, 528, 80),
                PlaneMajorRegion::new(0x2200, 6, 0x01E0, 48, 80, 272, 0),
                PlaneMajorRegion::new(0x2980, 8, 0x0380, 64, 112, 328, 0),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
            for (offset, width, height, destination_x, destination_y) in [
                (0x3780, 32, 32, 552, 80),
                (0x3980, 40, 64, 400, 0),
                (0x3E80, 16, 48, 448, 0),
            ] {
                let bytes = width / 8 * 4 * height;
                render_row_interleaved_brgi(
                    &decoded[offset..offset + bytes],
                    width,
                    height,
                    destination_x,
                    destination_y,
                    &mut rgb,
                )?;
            }
        }
        _ => unreachable!("named RE dispatcher passed {name}"),
    }

    Ok(RenderedScreen {
        layout: ScreenLayout::BrgiSceneAtlas,
        stream_sizes: vec![decoded.len()],
        metadata_tail_bytes: Vec::new(),
        companion_stream_sizes: Vec::new(),
        companion_audio: None,
        unrendered_ranges,
        rgb,
    })
}

fn render_se_scene_atlas(
    name: &str,
    packed: &[u8],
    expected_decoded_size: usize,
) -> Result<RenderedScreen> {
    let streams = decode_exact_nonempty_streams(packed)?;
    let [decoded] = streams.as_slice() else {
        bail!(
            "unsupported SE stream count {}; expected one stream",
            streams.len()
        );
    };
    if decoded.len() != expected_decoded_size {
        bail!(
            "unsupported {name} decoded size {}; expected {expected_decoded_size}",
            decoded.len()
        );
    }

    let mut rgb = vec![0u8; SCREEN_WIDTH * SCREEN_HEIGHT * 3];
    match name.to_ascii_uppercase().as_str() {
        "SE1.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 36, 0x1B00, 288, 192, 0, 0),
                PlaneMajorRegion::new(0x6C00, 36, 0x1B00, 288, 192, 296, 0),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
            for (offset, width, height, destination_x, destination_y) in [
                (0xD800, 72, 48, 0, 200),
                (0xDEC0, 48, 32, 80, 200),
                (0xE1C0, 48, 16, 136, 200),
                (0xE340, 96, 32, 192, 200),
                (0xE940, 32, 16, 296, 200),
                (0xEA40, 64, 64, 336, 200),
                (0xF240, 80, 48, 408, 200),
            ] {
                let bytes = width / 8 * 4 * height;
                render_row_interleaved_brgi(
                    &decoded[offset..offset + bytes],
                    width,
                    height,
                    destination_x,
                    destination_y,
                    &mut rgb,
                )?;
            }
        }
        "SE3.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 36, 0x1B00, 288, 192, 0, 0),
                PlaneMajorRegion::new(0x6C00, 10, 0x00A0, 80, 16, 296, 0),
                PlaneMajorRegion::new(0x6E80, 16, 0x0300, 128, 48, 384, 0),
                PlaneMajorRegion::new(0x7A80, 20, 0x0140, 160, 16, 296, 56),
                PlaneMajorRegion::new(0x7F80, 24, 0x0A80, 192, 112, 296, 80),
                PlaneMajorRegion::new(0xA980, 2, 0x0060, 16, 48, 496, 80),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
        }
        "SE4.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 36, 0x1B00, 288, 192, 0, 0),
                PlaneMajorRegion::new(0x6C00, 22, 0x0F20, 176, 176, 296, 0),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
        }
        "SE5.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 36, 0x1B00, 288, 192, 0, 0),
                PlaneMajorRegion::new(0x6C00, 10, 0x0280, 80, 64, 296, 0),
                PlaneMajorRegion::new(0x7600, 10, 0x0280, 80, 64, 384, 0),
                PlaneMajorRegion::new(0x8000, 13, 0x00D0, 104, 16, 296, 72),
                PlaneMajorRegion::new(0x8340, 17, 0x0110, 136, 16, 408, 72),
                PlaneMajorRegion::new(0x8780, 21, 0x05E8, 168, 72, 296, 96),
                PlaneMajorRegion::new(0x9F20, 14, 0x00E0, 112, 16, 472, 96),
                PlaneMajorRegion::new(0xA2A0, 10, 0x00A0, 80, 16, 472, 120),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
        }
        "SE7.DAT" => {
            render_plane_major_brgi_region(
                decoded,
                PlaneMajorRegion::new(0x0000, 36, 0x3600, 288, 384, 0, 0),
                &mut rgb,
            )?;
        }
        "SE7_1.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 4, 0x01E0, 32, 120, 0, 0),
                PlaneMajorRegion::new(0x0780, 5, 0x00C8, 40, 40, 40, 0),
                PlaneMajorRegion::new(0x0AA0, 4, 0x00A0, 32, 40, 88, 0),
                PlaneMajorRegion::new(0x0D20, 4, 0x00A0, 32, 40, 128, 0),
                PlaneMajorRegion::new(0x0FA0, 4, 0x00A0, 32, 40, 168, 0),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
        }
        "SE10.DAT" => {
            render_plane_major_brgi_region(
                decoded,
                PlaneMajorRegion::new(0x0000, 36, 0x1B00, 288, 192, 0, 0),
                &mut rgb,
            )?;
        }
        "SE10_1.DAT" => {
            for (offset, width, height, destination_x, destination_y) in [
                (0x0000, 112, 16, 0, 0),
                (0x0380, 144, 16, 120, 0),
                (0x0800, 160, 16, 272, 0),
                (0x0D00, 168, 16, 440, 0),
                (0x1240, 160, 32, 0, 24),
                (0x1C40, 192, 16, 168, 24),
                (0x2240, 240, 16, 368, 24),
                (0x29C0, 256, 16, 168, 48),
                (0x31C0, 104, 16, 432, 48),
                (0x3500, 120, 16, 0, 64),
                (0x38C0, 136, 80, 0, 88),
                (0x4E00, 200, 16, 128, 64),
                (0x5440, 248, 32, 336, 64),
                (0x63C0, 88, 96, 144, 104),
                (0x7440, 64, 64, 240, 104),
                (0x8440, 40, 168, 384, 104),
                (0x9160, 24, 144, 432, 104),
                (0x9820, 24, 56, 464, 104),
                (0x9AC0, 8, 56, 496, 104),
                (0x9BA0, 8, 32, 512, 104),
                (0x9C20, 8, 16, 528, 104),
                (0x9C60, 8, 8, 544, 104),
            ] {
                let bytes = width / 8 * 4 * height;
                render_row_interleaved_brgi(
                    &decoded[offset..offset + bytes],
                    width,
                    height,
                    destination_x,
                    destination_y,
                    &mut rgb,
                )?;
            }
            render_plane_major_brgi_region(
                decoded,
                PlaneMajorRegion::new(0x7C40, 8, 0x0200, 64, 64, 312, 104),
                &mut rgb,
            )?;
        }
        "SE12.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 36, 0x1B00, 288, 192, 0, 0),
                PlaneMajorRegion::new(0x6C00, 8, 0x0140, 64, 40, 296, 0),
                PlaneMajorRegion::new(0x7100, 8, 0x0140, 64, 40, 368, 0),
                PlaneMajorRegion::new(0x7600, 1, 0x0018, 8, 24, 440, 0),
                PlaneMajorRegion::new(0x76C0, 1, 0x0018, 8, 24, 456, 0),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
        }
        "SE13.DAT" => {
            for (offset, width, height, destination_x, destination_y) in [
                (0x0000, 48, 16, 0, 0),
                (0x0180, 128, 16, 56, 0),
                (0x0580, 136, 16, 192, 0),
                (0x09C0, 144, 16, 336, 0),
                (0x0E40, 136, 16, 488, 0),
                (0x1280, 128, 16, 0, 24),
                (0x1680, 120, 16, 132, 24),
                (0x1A40, 96, 16, 256, 24),
                (0x1D40, 88, 16, 356, 24),
                (0x2000, 80, 16, 448, 24),
                (0x2280, 96, 16, 532, 24),
                (0x2580, 48, 32, 0, 48),
                (0x2880, 48, 32, 56, 48),
                (0x2B80, 48, 32, 112, 48),
                (0x2E80, 48, 32, 168, 48),
                (0x3180, 64, 72, 224, 48),
            ] {
                let bytes = width / 8 * 4 * height;
                render_row_interleaved_brgi(
                    &decoded[offset..offset + bytes],
                    width,
                    height,
                    destination_x,
                    destination_y,
                    &mut rgb,
                )?;
            }
        }
        "SE17.DAT" => {
            render_plane_major_brgi_region(
                decoded,
                PlaneMajorRegion::new(0x0000, 36, 0x1B00, 288, 192, 0, 0),
                &mut rgb,
            )?;
            render_row_interleaved_brgi(&decoded[0x6C00..], 168, 320, 296, 0, &mut rgb)?;
        }
        "SE18.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 36, 0x1B00, 288, 192, 0, 0),
                PlaneMajorRegion::new(0x6C00, 6, 0x01B0, 48, 72, 296, 0),
                PlaneMajorRegion::new(0x72C0, 6, 0x01B0, 48, 72, 352, 0),
                PlaneMajorRegion::new(0x7980, 6, 0x01B0, 48, 72, 408, 0),
                PlaneMajorRegion::new(0x8040, 18, 0x0900, 144, 128, 464, 0),
                PlaneMajorRegion::new(0xA440, 8, 0x0140, 64, 40, 296, 136),
                PlaneMajorRegion::new(0xA940, 8, 0x0140, 64, 40, 368, 136),
                PlaneMajorRegion::new(0xAE40, 8, 0x0140, 64, 40, 440, 136),
                PlaneMajorRegion::new(0xB340, 8, 0x0140, 64, 40, 512, 136),
                PlaneMajorRegion::new(0xB840, 6, 0x0090, 48, 24, 296, 184),
                PlaneMajorRegion::new(0xBA80, 6, 0x0090, 48, 24, 352, 184),
                PlaneMajorRegion::new(0xBCC0, 6, 0x0090, 48, 24, 408, 184),
                PlaneMajorRegion::new(0xBF00, 6, 0x0090, 48, 24, 464, 184),
                PlaneMajorRegion::new(0xC140, 6, 0x0090, 48, 24, 520, 184),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
        }
        "SE19.DAT" => {
            for region in [
                PlaneMajorRegion::new(0x0000, 36, 0x1B00, 288, 192, 0, 0),
                PlaneMajorRegion::new(0x6C00, 5, 0x0078, 40, 24, 296, 0),
                PlaneMajorRegion::new(0x6DE0, 5, 0x0078, 40, 24, 344, 0),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
        }
        "SE23.DAT" => {
            for (offset, width, height, destination_x, destination_y) in [
                (0x0000, 48, 48, 296, 88),
                (0x0480, 96, 88, 352, 88),
                (0x1500, 32, 64, 456, 88),
                (0x1900, 40, 112, 496, 88),
            ] {
                let bytes = width / 8 * 4 * height;
                render_row_interleaved_brgi(
                    &decoded[offset..offset + bytes],
                    width,
                    height,
                    destination_x,
                    destination_y,
                    &mut rgb,
                )?;
            }
            for region in [
                PlaneMajorRegion::new(0x21C0, 36, 0x0B40, 288, 80, 296, 0),
                PlaneMajorRegion::new(0x4EC0, 36, 0x1B00, 288, 192, 0, 0),
            ] {
                render_plane_major_brgi_region(decoded, region, &mut rgb)?;
            }
        }
        _ => unreachable!("named SE dispatcher passed {name}"),
    }

    Ok(RenderedScreen {
        layout: ScreenLayout::BrgiSceneAtlas,
        stream_sizes: vec![decoded.len()],
        metadata_tail_bytes: Vec::new(),
        companion_stream_sizes: Vec::new(),
        companion_audio: None,
        unrendered_ranges: Vec::new(),
        rgb,
    })
}

fn render_plane_major_brgi_region(
    source: &[u8],
    region: PlaneMajorRegion,
    rgb: &mut [u8],
) -> Result<()> {
    let PlaneMajorRegion {
        source_offset,
        source_row_bytes,
        plane_stride,
        width,
        height,
        destination_x,
        destination_y,
    } = region;
    if width == 0 || height == 0 || !width.is_multiple_of(8) {
        bail!("plane-major dimensions {width}x{height} are not positive and byte aligned");
    }
    let width_bytes = width / 8;
    if width_bytes > source_row_bytes {
        bail!(
            "plane-major width uses {width_bytes} bytes per row, exceeding source pitch {source_row_bytes}"
        );
    }
    if destination_x + width > SCREEN_WIDTH || destination_y + height > SCREEN_HEIGHT {
        bail!("plane-major region exceeds the diagnostic atlas");
    }
    let last_source = source_offset
        .checked_add(
            plane_stride
                .checked_mul(3)
                .context("plane stride overflow")?,
        )
        .and_then(|offset| offset.checked_add(source_row_bytes.checked_mul(height - 1)?))
        .and_then(|offset| offset.checked_add(width_bytes))
        .ok_or_else(|| anyhow::anyhow!("plane-major source range overflow"))?;
    if last_source > source.len() {
        bail!(
            "plane-major region ends at 0x{last_source:X}, past decoded size 0x{:X}",
            source.len()
        );
    }

    for row in 0..height {
        for column in 0..width {
            let byte_in_row = column / 8;
            let mask = 0x80 >> (column % 8);
            let mut color = 0usize;
            for plane in 0..4 {
                let offset =
                    source_offset + plane * plane_stride + row * source_row_bytes + byte_in_row;
                color |= usize::from(source[offset] & mask != 0) << plane;
            }
            let pixel = (destination_y + row) * SCREEN_WIDTH + destination_x + column;
            rgb[pixel * 3..pixel * 3 + 3].copy_from_slice(&DIAGNOSTIC_RGBI_PALETTE[color]);
        }
    }
    Ok(())
}

fn render_ishi_sprite_atlas(packed: &[u8]) -> Result<RenderedScreen> {
    let streams = decode_exact_nonempty_streams(packed)?;
    let [decoded] = streams.as_slice() else {
        bail!(
            "unsupported ISHI stream count {}; expected one stream",
            streams.len()
        );
    };
    if decoded.len() != ISHI_DECODED_BYTES {
        bail!(
            "unsupported ISHI decoded size {}; expected {ISHI_DECODED_BYTES}",
            decoded.len()
        );
    }

    let mut rgb = vec![0u8; SCREEN_WIDTH * SCREEN_HEIGHT * 3];

    // The tile-map consumer addresses the first 0x1100 bytes as 34 conventional
    // plane-major 16x16 cells.
    for tile_index in 0..(ISHI_TILE_END / TILE_BYTES) {
        let tile_start = tile_index * TILE_BYTES;
        render_brgi_tile(
            &decoded[tile_start..tile_start + TILE_BYTES],
            tile_index,
            &mut rgb,
        );
    }

    // The animation registrar proves seven slots from 0x1100. Three overlapping
    // masked-blitter entries prove the remaining slots at 0x1480, 0x1500, and
    // 0x1580, so the diagnostic sheet retains all ten consumer-selected frames.
    for frame in 0..((ISHI_SMALL_END - ISHI_TILE_END) / TILE_BYTES) {
        let start = ISHI_TILE_END + frame * TILE_BYTES;
        render_row_interleaved_brgi(
            &decoded[start..start + TILE_BYTES],
            16,
            16,
            frame * 16,
            24,
            &mut rgb,
        )?;
    }
    for frame in 0..((ISHI_MEDIUM_END - ISHI_SMALL_END) / 0x200) {
        let start = ISHI_SMALL_END + frame * 0x200;
        render_row_interleaved_brgi(
            &decoded[start..start + 0x200],
            32,
            32,
            frame * 40,
            48,
            &mut rgb,
        )?;
    }
    render_row_interleaved_brgi(
        &decoded[ISHI_MEDIUM_END..ISHI_DECODED_BYTES],
        64,
        112,
        176,
        48,
        &mut rgb,
    )?;

    Ok(RenderedScreen {
        layout: ScreenLayout::BrgiMixedSpriteAtlas,
        stream_sizes: vec![decoded.len()],
        metadata_tail_bytes: Vec::new(),
        companion_stream_sizes: Vec::new(),
        companion_audio: None,
        unrendered_ranges: Vec::new(),
        rgb,
    })
}

fn render_row_interleaved_brgi(
    source: &[u8],
    width: usize,
    height: usize,
    destination_x: usize,
    destination_y: usize,
    rgb: &mut [u8],
) -> Result<()> {
    if !width.is_multiple_of(8) {
        bail!("row-interleaved width {width} is not byte aligned");
    }
    let plane_row_bytes = width / 8;
    let expected = plane_row_bytes
        .checked_mul(4)
        .and_then(|row| row.checked_mul(height))
        .ok_or_else(|| anyhow::anyhow!("row-interleaved dimensions overflow"))?;
    if source.len() != expected {
        bail!(
            "row-interleaved source has {} bytes; expected {expected} for {width}x{height}",
            source.len()
        );
    }
    if destination_x + width > SCREEN_WIDTH || destination_y + height > SCREEN_HEIGHT {
        bail!("row-interleaved sprite exceeds the diagnostic atlas");
    }

    let source_row_bytes = plane_row_bytes * 4;
    for row in 0..height {
        for column in 0..width {
            let byte_in_row = column / 8;
            let mask = 0x80 >> (column % 8);
            let mut color = 0usize;
            for plane in 0..4 {
                let offset = row * source_row_bytes + plane * plane_row_bytes + byte_in_row;
                color |= usize::from(source[offset] & mask != 0) << plane;
            }
            let pixel = (destination_y + row) * SCREEN_WIDTH + destination_x + column;
            rgb[pixel * 3..pixel * 3 + 3].copy_from_slice(&DIAGNOSTIC_RGBI_PALETTE[color]);
        }
    }
    Ok(())
}

fn render_character_tile_atlas(
    packed: &[u8],
    expected_tile_base: usize,
    expected_decoded_size: usize,
) -> Result<RenderedScreen> {
    let streams = decode_exact_nonempty_streams(packed)?;
    let [decoded] = streams.as_slice() else {
        bail!(
            "unsupported character tile stream count {}; expected one stream",
            streams.len()
        );
    };
    if decoded.len() != expected_decoded_size {
        bail!(
            "unsupported character tile decoded size {}; expected {expected_decoded_size}",
            decoded.len()
        );
    }
    let tile_base = usize::from(u16::from_le_bytes([decoded[0], decoded[1]]));
    if tile_base != expected_tile_base {
        bail!(
            "unsupported character tile base 0x{tile_base:04X}; expected 0x{expected_tile_base:04X}"
        );
    }
    let tile_region_bytes = decoded.len() - tile_base;
    if !tile_region_bytes.is_multiple_of(TILE_BYTES) {
        bail!(
            "unsupported character tile region size 0x{tile_region_bytes:X}; expected whole 0x80-byte tiles"
        );
    }
    let tile_count = tile_region_bytes / TILE_BYTES;
    let atlas_capacity = (SCREEN_WIDTH / TILE_WIDTH) * (SCREEN_HEIGHT / TILE_HEIGHT);
    if tile_count > atlas_capacity {
        bail!("tile count {tile_count} exceeds atlas capacity {atlas_capacity}");
    }

    let mut rgb = vec![0u8; SCREEN_WIDTH * SCREEN_HEIGHT * 3];
    for tile_index in 0..tile_count {
        let tile_start = tile_base + tile_index * TILE_BYTES;
        render_brgi_tile(
            &decoded[tile_start..tile_start + TILE_BYTES],
            tile_index,
            &mut rgb,
        );
    }
    Ok(RenderedScreen {
        layout: ScreenLayout::BrgiTileAtlas,
        stream_sizes: vec![decoded.len()],
        metadata_tail_bytes: Vec::new(),
        companion_stream_sizes: Vec::new(),
        companion_audio: None,
        unrendered_ranges: Vec::new(),
        rgb,
    })
}

fn render_mg_tile_atlas(
    packed: &[u8],
    expected_tile_base: usize,
    expected_decoded_size: usize,
) -> Result<RenderedScreen> {
    let streams = decode_exact_nonempty_streams(packed)?;
    let [decoded] = streams.as_slice() else {
        bail!(
            "unsupported MG tile stream count {}; expected one stream",
            streams.len()
        );
    };
    if decoded.len() != expected_decoded_size {
        bail!(
            "unsupported MG tile decoded size {}; expected {expected_decoded_size}",
            decoded.len()
        );
    }
    let tile_base = usize::from(u16::from_le_bytes([decoded[0], decoded[1]]));
    if tile_base != expected_tile_base {
        bail!("unsupported MG tile base 0x{tile_base:04X}; expected 0x{expected_tile_base:04X}");
    }
    let tile_region_bytes = decoded.len() - tile_base;
    let full_banks = tile_region_bytes / TILE_BANK_BYTES;
    let final_bank_bytes = tile_region_bytes % TILE_BANK_BYTES;
    if full_banks >= 4 || final_bank_bytes == 0 || !final_bank_bytes.is_multiple_of(TILE_BYTES) {
        bail!(
            "unsupported MG tile banks: {full_banks} full bank(s) plus 0x{final_bank_bytes:X} bytes"
        );
    }
    let tile_count = full_banks * (TILE_BANK_BYTES / TILE_BYTES) + final_bank_bytes / TILE_BYTES;
    let atlas_capacity = (SCREEN_WIDTH / TILE_WIDTH) * (SCREEN_HEIGHT / TILE_HEIGHT);
    if tile_count > atlas_capacity {
        bail!("MG tile count {tile_count} exceeds atlas capacity {atlas_capacity}");
    }

    let mut rgb = vec![0u8; SCREEN_WIDTH * SCREEN_HEIGHT * 3];
    for tile_index in 0..tile_count {
        let bank = tile_index / (TILE_BANK_BYTES / TILE_BYTES);
        let index_in_bank = tile_index % (TILE_BANK_BYTES / TILE_BYTES);
        let tile_start = tile_base + bank * TILE_BANK_BYTES + index_in_bank * TILE_BYTES;
        render_brgi_tile(
            &decoded[tile_start..tile_start + TILE_BYTES],
            tile_index,
            &mut rgb,
        );
    }
    Ok(RenderedScreen {
        layout: ScreenLayout::BrgiTileAtlas,
        stream_sizes: vec![decoded.len()],
        metadata_tail_bytes: Vec::new(),
        companion_stream_sizes: Vec::new(),
        companion_audio: None,
        unrendered_ranges: Vec::new(),
        rgb,
    })
}

fn render_brgi_tile(tile: &[u8], tile_index: usize, rgb: &mut [u8]) {
    let tiles_per_row = SCREEN_WIDTH / TILE_WIDTH;
    let tile_x = (tile_index % tiles_per_row) * TILE_WIDTH;
    let tile_y = (tile_index / tiles_per_row) * TILE_HEIGHT;
    for row in 0..TILE_HEIGHT {
        for column in 0..TILE_WIDTH {
            let byte_in_row = column / 8;
            let mask = 0x80 >> (column % 8);
            let mut color = 0usize;
            for plane in 0..4 {
                let offset = plane * TILE_PLANE_BYTES + row * 2 + byte_in_row;
                color |= usize::from(tile[offset] & mask != 0) << plane;
            }
            let pixel = (tile_y + row) * SCREEN_WIDTH + tile_x + column;
            rgb[pixel * 3..pixel * 3 + 3].copy_from_slice(&DIAGNOSTIC_RGBI_PALETTE[color]);
        }
    }
}

fn render_single_column_major_brgi_resource(packed: &[u8]) -> Result<RenderedScreen> {
    let streams = decode_exact_nonempty_streams(packed)?;
    let [primary] = streams.as_slice() else {
        bail!(
            "unsupported single-stream AH=5 stream count {}; expected one 128000-byte primary",
            streams.len()
        );
    };
    if primary.len() != PLANE_BYTES * 4 {
        bail!(
            "unsupported single-stream AH=5 decoded size {}; expected {}",
            primary.len(),
            PLANE_BYTES * 4
        );
    }
    Ok(RenderedScreen {
        layout: ScreenLayout::ColumnMajorBrgiPlanes,
        stream_sizes: vec![primary.len()],
        metadata_tail_bytes: vec![0; 4],
        companion_stream_sizes: Vec::new(),
        companion_audio: None,
        unrendered_ranges: Vec::new(),
        rgb: render_column_major_brgi(primary),
    })
}

fn render_column_major_brgi_resource(
    name: &str,
    packed: &[u8],
    expected_companion_size: usize,
) -> Result<RenderedScreen> {
    let streams = decode_exact_nonempty_streams(packed)?;
    let [primary, companion] = streams.as_slice() else {
        bail!(
            "unsupported AH=5 stream count {}; expected one 128000-byte primary and one companion",
            streams.len()
        );
    };
    if primary.len() != PLANE_BYTES * 4 || companion.len() != expected_companion_size {
        bail!(
            "unsupported AH=5 decoded stream sizes [{}, {}]; expected [{}, {expected_companion_size}]",
            primary.len(),
            companion.len(),
            PLANE_BYTES * 4,
        );
    }
    let companion_audio = parse_named_bsamp_companion(name, companion)?;
    Ok(RenderedScreen {
        layout: ScreenLayout::ColumnMajorBrgiPlanes,
        stream_sizes: streams.iter().map(Vec::len).collect(),
        metadata_tail_bytes: vec![0; 4],
        companion_stream_sizes: vec![companion.len()],
        companion_audio,
        unrendered_ranges: Vec::new(),
        rgb: render_column_major_brgi(primary),
    })
}

fn render_column_major_brgi(primary: &[u8]) -> Vec<u8> {
    let planes = primary
        .as_chunks::<PLANE_BYTES>()
        .0
        .iter()
        .map(|plane| transpose_column_major_plane(plane))
        .collect::<Vec<_>>();
    render_brgi([&planes[0], &planes[1], &planes[2], &planes[3]])
}

fn transpose_column_major_plane(column_major: &[u8]) -> Vec<u8> {
    let mut row_major = vec![0u8; PLANE_BYTES];
    for column in 0..(SCREEN_WIDTH / 8) {
        for row in 0..SCREEN_HEIGHT {
            row_major[row * (SCREEN_WIDTH / 8) + column] =
                column_major[column * SCREEN_HEIGHT + row];
        }
    }
    row_major
}

fn decode_exact_nonempty_streams(packed: &[u8]) -> Result<Vec<Vec<u8>>> {
    let streams = decode_exact_streams(packed)?;
    if let Some(index) = streams.iter().position(Vec::is_empty) {
        bail!("empty LZ stream at index {index}");
    }
    Ok(streams)
}

fn decode_exact_streams(packed: &[u8]) -> Result<Vec<Vec<u8>>> {
    let mut offset = 0usize;
    let mut streams = Vec::new();
    while offset < packed.len() {
        let report = decode_overlay_lz(&packed[offset..])?;
        offset += report.bytes_consumed;
        streams.push(report.output);
    }
    if offset != packed.len() {
        bail!(
            "resource decode stopped at 0x{offset:X} with {} packed bytes trailing",
            packed.len() - offset
        );
    }
    Ok(streams)
}

fn render_monochrome(plane: &[u8]) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(SCREEN_WIDTH * SCREEN_HEIGHT * 3);
    for pixel in 0..SCREEN_WIDTH * SCREEN_HEIGHT {
        let bit = plane[pixel / 8] & (0x80 >> (pixel % 8));
        let value = if bit == 0 { 0 } else { 255 };
        rgb.extend_from_slice(&[value, value, value]);
    }
    rgb
}

fn render_brgi(planes: [&[u8]; 4]) -> Vec<u8> {
    render_brgi_dimensions(planes, SCREEN_WIDTH, SCREEN_HEIGHT)
}

fn render_brgi_dimensions(planes: [&[u8]; 4], width: usize, height: usize) -> Vec<u8> {
    debug_assert_eq!(width % 8, 0);
    debug_assert!(planes.iter().all(|plane| plane.len() == width * height / 8));
    let mut rgb = Vec::with_capacity(width * height * 3);
    for pixel in 0..width * height {
        let mask = 0x80 >> (pixel % 8);
        let byte = pixel / 8;
        let mut index = 0usize;
        for (bit, plane) in planes.iter().enumerate() {
            index |= usize::from(plane[byte] & mask != 0) << bit;
        }
        rgb.extend_from_slice(&DIAGNOSTIC_RGBI_PALETTE[index]);
    }
    rgb
}

fn blit_demo_linear_rgb24(source: &[u8], destination: &mut [u8]) -> Result<()> {
    if source.len() != DEMO_LINEAR_WIDTH * DEMO_LINEAR_HEIGHT * 3 {
        bail!("source RGB24 size does not match its dimensions");
    }
    if destination.len() != SCREEN_WIDTH * SCREEN_HEIGHT * 3 {
        bail!("destination RGB24 size does not match its dimensions");
    }
    let source_row_bytes = DEMO_LINEAR_WIDTH * 3;
    let destination_row_bytes = SCREEN_WIDTH * 3;
    for row in 0..DEMO_LINEAR_HEIGHT {
        let source_start = row * source_row_bytes;
        let destination_start =
            (DEMO_LINEAR_DEST_Y + row) * destination_row_bytes + DEMO_LINEAR_DEST_X * 3;
        destination[destination_start..destination_start + source_row_bytes]
            .copy_from_slice(&source[source_start..source_start + source_row_bytes]);
    }
    Ok(())
}

/// Diagnostic palette for content inspection. A.R.S programs scene palettes at
/// runtime, so these colors do not claim to reproduce the final screen palette.
const DIAGNOSTIC_RGBI_PALETTE: [[u8; 3]; 16] = [
    [0x00, 0x00, 0x00],
    [0x00, 0x00, 0xAA],
    [0xAA, 0x00, 0x00],
    [0xAA, 0x00, 0xAA],
    [0x00, 0xAA, 0x00],
    [0x00, 0xAA, 0xAA],
    [0xAA, 0x55, 0x00],
    [0xAA, 0xAA, 0xAA],
    [0x55, 0x55, 0x55],
    [0x55, 0x55, 0xFF],
    [0xFF, 0x55, 0x55],
    [0xFF, 0x55, 0xFF],
    [0x55, 0xFF, 0x55],
    [0x55, 0xFF, 0xFF],
    [0xFF, 0xFF, 0x55],
    [0xFF, 0xFF, 0xFF],
];

/// Encode an RGB24 buffer as an uncompressed bottom-up 24-bit BMP. BMP keeps
/// the analysis path dependency-free and is directly viewable on the host.
pub fn encode_bmp24(width: usize, height: usize, rgb: &[u8]) -> Result<Vec<u8>> {
    let expected = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(3))
        .ok_or_else(|| anyhow::anyhow!("BMP dimensions overflow"))?;
    if rgb.len() != expected {
        bail!(
            "RGB buffer has {} bytes, expected {expected} for {width}x{height}",
            rgb.len()
        );
    }
    let row_bytes = width
        .checked_mul(3)
        .ok_or_else(|| anyhow::anyhow!("BMP row size overflow"))?;
    let row_stride = row_bytes
        .checked_add(3)
        .map(|bytes| bytes & !3)
        .ok_or_else(|| anyhow::anyhow!("BMP row stride overflow"))?;
    let pixel_bytes = row_stride
        .checked_mul(height)
        .ok_or_else(|| anyhow::anyhow!("BMP pixel size overflow"))?;
    let file_size = 54usize
        .checked_add(pixel_bytes)
        .ok_or_else(|| anyhow::anyhow!("BMP file size overflow"))?;
    let file_size = u32::try_from(file_size)?;
    let width_i32 = i32::try_from(width)?;
    let height_i32 = i32::try_from(height)?;
    let pixel_bytes_u32 = u32::try_from(pixel_bytes)?;

    let mut out = Vec::with_capacity(file_size as usize);
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&file_size.to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&width_i32.to_le_bytes());
    out.extend_from_slice(&height_i32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&pixel_bytes_u32.to_le_bytes());
    out.extend_from_slice(&0i32.to_le_bytes());
    out.extend_from_slice(&0i32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());

    let padding = row_stride - row_bytes;
    for y in (0..height).rev() {
        let row = &rgb[y * row_bytes..(y + 1) * row_bytes];
        for pixel in row.as_chunks::<3>().0 {
            out.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
        }
        out.resize(out.len() + padding, 0);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::overlay_lz::encode_overlay_lz;

    #[test]
    fn renders_four_concatenated_streams_in_brgi_order() {
        let mut planes = [
            vec![0u8; PLANE_BYTES],
            vec![0u8; PLANE_BYTES],
            vec![0u8; PLANE_BYTES],
            vec![0u8; PLANE_BYTES + 128],
        ];
        planes[0][0] = 0x80;
        planes[1][0] = 0x40;
        planes[2][0] = 0x20;
        planes[3][0] = 0x10;
        let packed = planes
            .iter()
            .flat_map(|plane| encode_overlay_lz(plane))
            .collect::<Vec<_>>();
        let rendered = render_screen_resource(&packed).unwrap();
        assert_eq!(rendered.layout, ScreenLayout::BrgiPlanes);
        assert_eq!(rendered.metadata_tail_bytes, [0, 0, 0, 128]);
        assert_eq!(&rendered.rgb[0..3], &[0x00, 0x00, 0xAA]);
        assert_eq!(&rendered.rgb[3..6], &[0xAA, 0x00, 0x00]);
        assert_eq!(&rendered.rgb[6..9], &[0x00, 0xAA, 0x00]);
        assert_eq!(&rendered.rgb[9..12], &[0x55, 0x55, 0x55]);
    }

    #[test]
    fn renders_demo_direct_copy_as_one_contiguous_four_plane_image() {
        let mut decoded = vec![0u8; DEMO_LINEAR_PLANE_BYTES * 4];
        decoded[0] = 0x80;
        decoded[DEMO_LINEAR_PLANE_BYTES] = 0x40;
        decoded[DEMO_LINEAR_PLANE_BYTES * 2] = 0x20;
        decoded[DEMO_LINEAR_PLANE_BYTES * 3] = 0x10;
        let packed = encode_overlay_lz(&decoded);

        let rendered = render_named_screen_resource("OP20.CNS", &packed).unwrap();
        assert_eq!(rendered.layout, ScreenLayout::LinearBrgiPlanes);
        assert_eq!(rendered.stream_sizes, [32_000]);
        let first = (DEMO_LINEAR_DEST_Y * SCREEN_WIDTH + DEMO_LINEAR_DEST_X) * 3;
        assert_eq!(&rendered.rgb[first..first + 3], &[0x00, 0x00, 0xAA]);
        assert_eq!(&rendered.rgb[first + 3..first + 6], &[0xAA, 0x00, 0x00]);
        assert_eq!(&rendered.rgb[first + 6..first + 9], &[0x00, 0xAA, 0x00]);
        assert_eq!(&rendered.rgb[first + 9..first + 12], &[0x55, 0x55, 0x55]);
        assert!(rendered.rgb[..first].iter().all(|value| *value == 0));
    }

    #[test]
    fn demo_direct_copy_preserves_a_128_byte_metadata_tail() {
        let decoded = vec![0u8; DEMO_LINEAR_PLANE_BYTES * 4 + 128];
        let packed = encode_overlay_lz(&decoded);

        let rendered = render_named_screen_resource("OP18.CNS", &packed).unwrap();
        assert_eq!(rendered.stream_sizes, [32_128]);
        assert_eq!(rendered.metadata_tail_bytes, [128]);
    }

    #[test]
    fn rejects_a_shape_without_a_proven_screen_layout() {
        let packed = encode_overlay_lz(&vec![0u8; 12_345]);
        let error = render_screen_resource(&packed).unwrap_err().to_string();
        assert!(error.contains("unsupported decoded stream sizes"));
    }

    #[test]
    fn renders_tc_column_major_primary_as_brgi_planes() {
        let mut primary = vec![0u8; PLANE_BYTES * 4];
        primary[0] = 0x80;
        primary[PLANE_BYTES] = 0x40;
        primary[PLANE_BYTES * 2] = 0x20;
        primary[PLANE_BYTES * 3] = 0x10;
        // Column 1, row 0 becomes row-major byte 1 (pixel 8).
        primary[SCREEN_HEIGHT] = 0x80;
        let mut companion = vec![0x55; 4_740];
        companion[0..2].copy_from_slice(&0x127Eu16.to_le_bytes());
        let mut packed = encode_overlay_lz(&primary);
        packed.extend_from_slice(&encode_overlay_lz(&companion));

        let rendered = render_named_screen_resource("tc.cns", &packed).unwrap();
        assert_eq!(rendered.layout, ScreenLayout::ColumnMajorBrgiPlanes);
        assert_eq!(rendered.stream_sizes, [128_000, 4_740]);
        assert_eq!(rendered.metadata_tail_bytes, [0, 0, 0, 0]);
        assert_eq!(rendered.companion_stream_sizes, [4_740]);
        let audio = rendered.companion_audio.unwrap();
        assert_eq!(audio.tracks[0].payload_bytes, 0x127E);
        assert_eq!(audio.trailing_bytes, 4);
        assert_eq!(&rendered.rgb[0..3], &[0x00, 0x00, 0xAA]);
        assert_eq!(&rendered.rgb[3..6], &[0xAA, 0x00, 0x00]);
        assert_eq!(&rendered.rgb[6..9], &[0x00, 0xAA, 0x00]);
        assert_eq!(&rendered.rgb[9..12], &[0x55, 0x55, 0x55]);
        assert_eq!(&rendered.rgb[8 * 3..9 * 3], &[0x00, 0x00, 0xAA]);
    }

    #[test]
    fn named_ah5_renderer_rejects_a_companion_size_drift() {
        let primary = vec![0u8; PLANE_BYTES * 4];
        let mut packed = encode_overlay_lz(&primary);
        packed.extend_from_slice(&encode_overlay_lz(&vec![0u8; 4_739]));
        let error = render_named_screen_resource("TC.CNS", &packed)
            .unwrap_err()
            .to_string();
        assert!(error.contains("expected [128000, 4740]"));
    }

    #[test]
    fn renders_named_single_stream_ah5_resource() {
        let mut primary = vec![0u8; PLANE_BYTES * 4];
        primary[0] = 0x80;
        primary[PLANE_BYTES] = 0x40;
        primary[PLANE_BYTES * 2] = 0x20;
        primary[PLANE_BYTES * 3] = 0x10;
        primary[SCREEN_HEIGHT] = 0x80;
        let packed = encode_overlay_lz(&primary);

        let rendered = render_named_screen_resource("waku_a.cs", &packed).unwrap();
        let scenario_title = render_named_screen_resource("ARS_RX.CS", &packed).unwrap();
        assert_eq!(scenario_title, rendered);
        assert_eq!(rendered.layout, ScreenLayout::ColumnMajorBrgiPlanes);
        assert_eq!(rendered.stream_sizes, [128_000]);
        assert_eq!(rendered.metadata_tail_bytes, [0, 0, 0, 0]);
        assert!(rendered.companion_stream_sizes.is_empty());
        assert!(rendered.companion_audio.is_none());
        assert_eq!(&rendered.rgb[0..3], &[0x00, 0x00, 0xAA]);
        assert_eq!(&rendered.rgb[3..6], &[0xAA, 0x00, 0x00]);
        assert_eq!(&rendered.rgb[6..9], &[0x00, 0xAA, 0x00]);
        assert_eq!(&rendered.rgb[9..12], &[0x55, 0x55, 0x55]);
        assert_eq!(&rendered.rgb[8 * 3..9 * 3], &[0x00, 0x00, 0xAA]);
    }

    #[test]
    fn unknown_single_128000_stream_stays_rejected() {
        let packed = encode_overlay_lz(&vec![0u8; PLANE_BYTES * 4]);
        let error = render_named_screen_resource("UNKNOWN.CNS", &packed)
            .unwrap_err()
            .to_string();
        assert!(error.contains("unsupported decoded stream sizes [128000]"));
    }

    #[test]
    fn named_single_ah5_renderer_rejects_a_size_drift() {
        let packed = encode_overlay_lz(&vec![0u8; PLANE_BYTES * 4 - 1]);
        let error = render_named_screen_resource("WAKU_S.CS", &packed)
            .unwrap_err()
            .to_string();
        assert!(error.contains("expected 128000"));
    }

    #[test]
    fn renders_an_mg_tile_bank_as_a_brgi_atlas() {
        let tile_base = 0x10;
        let mut decoded = vec![0u8; tile_base + TILE_BYTES];
        decoded[0..2].copy_from_slice(&(tile_base as u16).to_le_bytes());
        decoded[tile_base] = 0x80;
        decoded[tile_base + TILE_PLANE_BYTES] = 0x40;
        decoded[tile_base + TILE_PLANE_BYTES * 2] = 0x20;
        decoded[tile_base + TILE_PLANE_BYTES * 3] = 0x10;
        let packed = encode_overlay_lz(&decoded);

        let rendered = render_mg_tile_atlas(&packed, tile_base, decoded.len()).unwrap();
        assert_eq!(rendered.layout, ScreenLayout::BrgiTileAtlas);
        assert_eq!(rendered.stream_sizes, [tile_base + TILE_BYTES]);
        assert_eq!(&rendered.rgb[0..3], &[0x00, 0x00, 0xAA]);
        assert_eq!(&rendered.rgb[3..6], &[0xAA, 0x00, 0x00]);
        assert_eq!(&rendered.rgb[6..9], &[0x00, 0xAA, 0x00]);
        assert_eq!(&rendered.rgb[9..12], &[0x55, 0x55, 0x55]);
    }

    #[test]
    fn mg_tile_atlas_rejects_a_partial_tile() {
        let tile_base = 0x10;
        let mut decoded = vec![0u8; tile_base + TILE_BYTES - 1];
        decoded[0..2].copy_from_slice(&(tile_base as u16).to_le_bytes());
        let packed = encode_overlay_lz(&decoded);
        let error = render_mg_tile_atlas(&packed, tile_base, decoded.len())
            .unwrap_err()
            .to_string();
        assert!(error.contains("unsupported MG tile banks"));
    }

    #[test]
    fn renders_a_character_tile_in_brgi_plane_order() {
        let tile_base = 0x10;
        let mut decoded = vec![0u8; tile_base + TILE_BYTES];
        decoded[0..2].copy_from_slice(&(tile_base as u16).to_le_bytes());
        decoded[tile_base] = 0x80;
        decoded[tile_base + TILE_PLANE_BYTES] = 0x40;
        decoded[tile_base + TILE_PLANE_BYTES * 2] = 0x20;
        decoded[tile_base + TILE_PLANE_BYTES * 3] = 0x10;
        let packed = encode_overlay_lz(&decoded);

        let rendered = render_character_tile_atlas(&packed, tile_base, decoded.len()).unwrap();
        assert_eq!(rendered.layout, ScreenLayout::BrgiTileAtlas);
        assert_eq!(rendered.stream_sizes, [tile_base + TILE_BYTES]);
        assert_eq!(&rendered.rgb[0..3], &[0x00, 0x00, 0xAA]);
        assert_eq!(&rendered.rgb[3..6], &[0xAA, 0x00, 0x00]);
        assert_eq!(&rendered.rgb[6..9], &[0x00, 0xAA, 0x00]);
        assert_eq!(&rendered.rgb[9..12], &[0x55, 0x55, 0x55]);
    }

    #[test]
    fn character_tile_atlas_rejects_a_partial_tile() {
        let tile_base = 0x10;
        let mut decoded = vec![0u8; tile_base + TILE_BYTES - 1];
        decoded[0..2].copy_from_slice(&(tile_base as u16).to_le_bytes());
        let packed = encode_overlay_lz(&decoded);
        let error = render_character_tile_atlas(&packed, tile_base, decoded.len())
            .unwrap_err()
            .to_string();
        assert!(error.contains("expected whole 0x80-byte tiles"));
    }

    #[test]
    fn renders_each_ishi_storage_region_with_its_consumer_order() {
        let mut decoded = vec![0u8; ISHI_DECODED_BYTES];
        // Plane-major tile B pixel.
        decoded[0] = 0x80;
        // First 16x16 row-interleaved frame B pixel.
        decoded[ISHI_TILE_END] = 0x80;
        // First 32x32 row-interleaved frame R pixel.
        decoded[ISHI_SMALL_END + 4] = 0x80;
        // 64x112 row-interleaved image G pixel (eight bytes per plane row).
        decoded[ISHI_MEDIUM_END + 16] = 0x80;
        let packed = encode_overlay_lz(&decoded);

        let rendered = render_ishi_sprite_atlas(&packed).unwrap();
        assert_eq!(rendered.layout, ScreenLayout::BrgiMixedSpriteAtlas);
        assert_eq!(rendered.stream_sizes, [ISHI_DECODED_BYTES]);
        assert_eq!(&rendered.rgb[0..3], &[0x00, 0x00, 0xAA]);
        let small = (24 * SCREEN_WIDTH) * 3;
        assert_eq!(&rendered.rgb[small..small + 3], &[0x00, 0x00, 0xAA]);
        let medium = (48 * SCREEN_WIDTH) * 3;
        assert_eq!(&rendered.rgb[medium..medium + 3], &[0xAA, 0x00, 0x00]);
        let large = (48 * SCREEN_WIDTH + 176) * 3;
        assert_eq!(&rendered.rgb[large..large + 3], &[0x00, 0xAA, 0x00]);
    }

    #[test]
    fn ishi_renderer_rejects_a_decoded_size_drift() {
        let packed = encode_overlay_lz(&vec![0u8; ISHI_DECODED_BYTES - 1]);
        let error = render_ishi_sprite_atlas(&packed).unwrap_err().to_string();
        assert!(error.contains("unsupported ISHI decoded size"));
    }

    #[test]
    fn renders_plane_major_scene_regions_in_brgi_order() {
        let source = [0x80, 0x40, 0x20, 0x10];
        let mut rgb = vec![0u8; SCREEN_WIDTH * SCREEN_HEIGHT * 3];
        render_plane_major_brgi_region(
            &source,
            PlaneMajorRegion::new(0, 1, 1, 8, 1, 0, 0),
            &mut rgb,
        )
        .unwrap();

        assert_eq!(&rgb[0..3], &[0x00, 0x00, 0xAA]);
        assert_eq!(&rgb[3..6], &[0xAA, 0x00, 0x00]);
        assert_eq!(&rgb[6..9], &[0x00, 0xAA, 0x00]);
        assert_eq!(&rgb[9..12], &[0x55, 0x55, 0x55]);
    }

    #[test]
    fn rtu1_renderer_keeps_the_directly_unconsumed_tail_explicit() {
        let packed = encode_overlay_lz(&vec![0u8; 28_672]);
        let rendered = render_named_screen_resource("rtu1.dat", &packed).unwrap();
        assert_eq!(rendered.layout, ScreenLayout::BrgiSceneAtlas);
        assert_eq!(
            rendered.unrendered_ranges,
            [DecodedRange {
                offset: 0x6C00,
                bytes: 0x400,
            }]
        );

        let drifted = encode_overlay_lz(&vec![0u8; 28_671]);
        let error = render_named_screen_resource("RTU1.DAT", &drifted)
            .unwrap_err()
            .to_string();
        assert!(error.contains("expected 28672"));
    }

    #[test]
    fn st_renderer_keeps_st2_unconsumed_ranges_explicit_and_rejects_size_drift() {
        let st2 = render_named_screen_resource("ST2.DAT", &encode_overlay_lz(&vec![0u8; 57_440]))
            .unwrap();
        assert_eq!(
            st2.unrendered_ranges,
            [
                DecodedRange {
                    offset: 0x87C0,
                    bytes: 0x540,
                },
                DecodedRange {
                    offset: 0x9000,
                    bytes: 0x380,
                },
            ]
        );

        let packed = encode_overlay_lz(&vec![0u8; 36_543]);
        let error = render_named_screen_resource("ST1.DAT", &packed)
            .unwrap_err()
            .to_string();
        assert!(error.contains("expected 36544"));
    }

    #[test]
    fn s_renderer_keeps_non_shape_tail_explicit_and_rejects_size_drift() {
        let s11_2 =
            render_named_screen_resource("s11_2.dat", &encode_overlay_lz(&vec![0u8; 60_672]))
                .unwrap();
        assert!(s11_2.unrendered_ranges.is_empty());

        let s16 = render_named_screen_resource("S16.DAT", &encode_overlay_lz(&vec![0u8; 21_024]))
            .unwrap();
        assert_eq!(
            s16.unrendered_ranges,
            [DecodedRange {
                offset: 0x51A0,
                bytes: 0x0080,
            }]
        );

        let drifted = encode_overlay_lz(&vec![0u8; 62_975]);
        let error = render_named_screen_resource("S1.DAT", &drifted)
            .unwrap_err()
            .to_string();
        assert!(error.contains("expected 62976"));
    }

    #[test]
    fn ro_renderer_keeps_directly_unconsumed_ranges_explicit_and_rejects_size_drift() {
        let ro1 = render_named_screen_resource("ro1.dat", &encode_overlay_lz(&vec![0u8; 49_152]))
            .unwrap();
        assert_eq!(
            ro1.unrendered_ranges,
            [
                DecodedRange {
                    offset: 0x2B80,
                    bytes: 0x2100,
                },
                DecodedRange {
                    offset: 0xAE80,
                    bytes: 0x1000,
                },
            ]
        );

        let ro6 = render_named_screen_resource("RO6.DAT", &encode_overlay_lz(&vec![0u8; 44_736]))
            .unwrap();
        assert!(ro6.unrendered_ranges.is_empty());

        let drifted = encode_overlay_lz(&vec![0u8; 49_151]);
        let error = render_named_screen_resource("RO1.DAT", &drifted)
            .unwrap_err()
            .to_string();
        assert!(error.contains("expected 49152"));
    }

    #[test]
    fn re_renderer_keeps_directly_unconsumed_ranges_explicit_and_rejects_size_drift() {
        let re9 = render_named_screen_resource("re9.dat", &encode_overlay_lz(&vec![0u8; 31_808]))
            .unwrap();
        assert_eq!(
            re9.unrendered_ranges,
            [DecodedRange {
                offset: 0x6CC0,
                bytes: 0x00C0,
            }]
        );

        let re15 = render_named_screen_resource("RE15.DAT", &encode_overlay_lz(&vec![0u8; 36_672]))
            .unwrap();
        assert_eq!(
            re15.unrendered_ranges,
            [DecodedRange {
                offset: 0x8EC0,
                bytes: 0x0080,
            }]
        );

        let drifted = encode_overlay_lz(&vec![0u8; 57_215]);
        let error = render_named_screen_resource("RE21_1.DAT", &drifted)
            .unwrap_err()
            .to_string();
        assert!(error.contains("expected 57216"));
    }

    #[test]
    fn se_renderer_rejects_a_named_size_drift() {
        let packed = encode_overlay_lz(&vec![0u8; 40_063]);
        let error = render_named_screen_resource("SE10_1.DAT", &packed)
            .unwrap_err()
            .to_string();
        assert!(error.contains("expected 40064"));
    }

    #[test]
    fn bmp_is_bottom_up_bgr_with_padded_rows() {
        let rgb = [255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255];
        let bmp = encode_bmp24(2, 2, &rgb).unwrap();
        assert_eq!(&bmp[0..2], b"BM");
        assert_eq!(u32::from_le_bytes(bmp[10..14].try_into().unwrap()), 54);
        // Bottom row first: blue then white, encoded BGR and padded to 8 bytes.
        assert_eq!(&bmp[54..62], &[255, 0, 0, 255, 255, 255, 0, 0]);
        // Top row: red then green.
        assert_eq!(&bmp[62..70], &[0, 0, 255, 0, 255, 0, 0, 0]);
    }
}
