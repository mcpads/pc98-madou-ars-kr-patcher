use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

#[derive(Parser)]
#[command(name = "pc98_madou_ars")]
#[command(about = "PC-98 Madou Monogatari A.R.S Korean patch research tool")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show HDM geometry and FAT12 layout metadata.
    Info {
        /// Source HDM.
        disk: PathBuf,
    },
    /// List files in an HDM FAT12 root directory.
    Files {
        /// Source HDM.
        disk: PathBuf,
    },
    /// Extract one FAT12 root file from an HDM for local analysis.
    ExtractFile {
        /// Source HDM.
        #[arg(long)]
        disk: PathBuf,
        /// Case-insensitive FAT12 8.3 root name.
        #[arg(long)]
        name: String,
        /// Local extracted output; original-derived files must remain ignored.
        #[arg(long)]
        output: PathBuf,
    },
    /// Render a statically proven 640x400 screen resource as a diagnostic BMP.
    RenderScreenResource {
        /// Source HDM.
        #[arg(long)]
        disk: PathBuf,
        /// Case-insensitive FAT12 8.3 root name.
        #[arg(long)]
        name: String,
        /// Diagnostic BMP output; original-derived images must remain ignored.
        #[arg(long)]
        output: PathBuf,
    },
    /// Render every statically supported screen resource on one disk and write
    /// a keyed local HTML contact sheet plus a JSON manifest.
    RenderScreenCatalog {
        /// Source HDM.
        #[arg(long)]
        disk: PathBuf,
        /// Comma-separated root-file extensions to inspect.
        #[arg(long, value_delimiter = ',', default_value = "CNS,CS")]
        extensions: Vec<String>,
        /// Ignored local output directory for BMPs, index.html, and manifest.json.
        #[arg(long)]
        output_dir: PathBuf,
    },
    /// Recover the Demo opening's statically sourced OP rectangles from the
    /// decoded ARS_DEMO.OVL compositor. This is source-use evidence, not a
    /// runtime-visibility claim.
    AuditDemoCompositor {
        /// Exact original Demo HDM.
        #[arg(long)]
        demo_disk: PathBuf,
        /// Ignored local JSON output.
        #[arg(long)]
        output: PathBuf,
    },
    /// Rebuild the shared Demo-disk OP20 title from the reviewed PC-98-source
    /// Korean master and insert it with exact source, palette, LZ, and HDM
    /// readback validation.
    BuildDemoTitle {
        /// Exact original shared Demo HDM.
        #[arg(long)]
        demo_disk: PathBuf,
        /// Project asset root containing graphics_text/title_logo.json.
        #[arg(long, default_value = "assets")]
        assets_dir: PathBuf,
        /// Output patched Demo HDM.
        #[arg(long)]
        output: PathBuf,
        /// Optional ignored 320x200 RGB preview BMP.
        #[arg(long)]
        preview: Option<PathBuf>,
    },
    /// Rebuild localized graphics on one exact character Data disk. Every
    /// product receives the shared scenario title; Arle also receives the
    /// opening `앗`, and Rulue receives the RTU1 interlude choices.
    BuildScenarioTitle {
        /// Exact original Arle, Rulue, or Schezo Data HDM.
        #[arg(long)]
        data_disk: PathBuf,
        /// Project asset root containing graphics_text/title_logo.json.
        #[arg(long, default_value = "assets")]
        assets_dir: PathBuf,
        /// Versioned 16x16 font profile for Rulue's RTU1 bitmap choices.
        #[arg(long, default_value = "assets/fonts/font_profile.json")]
        font_profile: PathBuf,
        /// Permit draft RTU1 choice wording for local emulator QA.
        #[arg(long)]
        allow_needs_review: bool,
        /// Output patched Data HDM.
        #[arg(long)]
        output: PathBuf,
        /// Optional distributable BPS for the exact original Data HDM.
        #[arg(long)]
        bps_output: Option<PathBuf>,
        /// Optional ignored 640x400 RGB preview BMP.
        #[arg(long)]
        preview: Option<PathBuf>,
        /// Optional ignored 288x192 Arle opening `앗` preview BMP.
        #[arg(long)]
        opening_preview: Option<PathBuf>,
    },
    /// Rebuild all reviewed shared Demo-disk graphics text in one pass: the
    /// OP20 Korean title and only MU.CNS's Japanese scenario prompt. Existing
    /// English menu labels and the MU.CNS audio bank remain source-identical.
    BuildDemoGraphics {
        /// Exact original shared Demo HDM.
        #[arg(long)]
        demo_disk: PathBuf,
        /// Project asset root containing the graphics-text manifests.
        #[arg(long, default_value = "assets")]
        assets_dir: PathBuf,
        /// Project-owned font profile for the MU.CNS Hangul prompt.
        #[arg(long, default_value = "assets/fonts/maplestory_bold_menu_profile.json")]
        font_profile: PathBuf,
        /// Output patched Demo HDM.
        #[arg(long)]
        output: PathBuf,
        /// Optional distributable BPS for the exact original Demo HDM. The
        /// freshly created patch is self-applied before either output is written.
        #[arg(long)]
        bps_output: Option<PathBuf>,
        /// Optional ignored 320x200 RGB title preview BMP.
        #[arg(long)]
        title_preview: Option<PathBuf>,
        /// Optional ignored 640x400 RGB menu preview BMP.
        #[arg(long)]
        menu_preview: Option<PathBuf>,
    },
    /// Audit likely resource files across HDMs: exact A.R.S LZ decode, decoded
    /// byte profile/plane-block shape, and conservative Shift-JIS candidates.
    AuditResources {
        /// Source HDM(s).
        #[arg(required = true)]
        disks: Vec<PathBuf>,
        /// Comma-separated root-file extensions to include.
        #[arg(long, value_delimiter = ',', default_value = "DAT,OVL,CNS,CS,Z04")]
        extensions: Vec<String>,
        /// Minimum consecutive glyphs in a candidate Shift-JIS region.
        #[arg(long, default_value_t = 8)]
        min_run: usize,
        /// Minimum double-byte Japanese characters in a candidate region.
        #[arg(long, default_value_t = 5)]
        min_japanese: usize,
        /// Suppress the per-resource table; still print the final summary.
        #[arg(long)]
        quiet: bool,
        /// JSON inventory output.
        #[arg(long)]
        output: PathBuf,
    },
    /// Scan a binary or one file inside an HDM for Shift-JIS string regions.
    SjisSweep {
        /// Binary input or source HDM when --in-disk is present.
        input: PathBuf,
        /// FAT12 root file to scan from the HDM.
        #[arg(long)]
        in_disk: Option<String>,
        /// Minimum consecutive glyphs in a candidate region.
        #[arg(long, default_value_t = 8)]
        min_run: usize,
        /// Minimum double-byte Japanese characters in a candidate region.
        #[arg(long, default_value_t = 5)]
        min_japanese: usize,
        /// Maximum regions to print.
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Build the current Arle overlay marker PoC disk.
    BuildMarker {
        /// Source Arle Game HDM.
        #[arg(long)]
        disk: PathBuf,
        /// Output patched HDM.
        #[arg(long)]
        output: PathBuf,
    },
    /// Build a one-glyph Hangul gaiji probe disk for the boot prompt.
    BuildHangulProbe {
        /// Source Arle Game HDM.
        #[arg(long)]
        disk: PathBuf,
        /// Output patched HDM.
        #[arg(long)]
        output: PathBuf,
    },
    /// Build a Schezo opening-cutscene Hangul PoC. Registers one BIOS gaiji,
    /// injects it into SHEZO_OP.OVL, and stages the boot media.
    BuildCutsceneHangulPoc {
        /// Source She-zo Game HDM.
        #[arg(long)]
        game_disk: PathBuf,
        /// Source Demo HDM; provides TC.CNS for the boot workaround.
        #[arg(long)]
        demo_disk: PathBuf,
        /// Source She-zo Data HDM; provides MADO-S.DAT.
        #[arg(long)]
        data_disk: PathBuf,
        /// Output patched HDM.
        #[arg(long)]
        output: PathBuf,
    },
    /// Build a temporary one-floppy Arle boot smoke disk.
    BuildArleBootSmoke {
        /// Source Arle Game HDM.
        #[arg(long)]
        game_disk: PathBuf,
        /// Source Demo HDM; provides TC.CNS.
        #[arg(long)]
        demo_disk: PathBuf,
        /// Source Arle Data HDM; provides MADO-A2.DAT.
        #[arg(long)]
        data_disk: PathBuf,
        /// Also apply the one-glyph Hangul boot-prompt probe.
        #[arg(long)]
        hangul_probe: bool,
        /// Output patched HDM.
        #[arg(long)]
        output: PathBuf,
    },
    /// Build a renderer-path Hangul PoC disk: register one gaiji glyph and
    /// overwrite an uncompressed enemy name so the graphics text renderer draws
    /// Hangul. Also stages the two-floppy boot-smoke media workaround.
    BuildRendererHangulPoc {
        /// Source Arle Game HDM (boot disk; holds MAIN.COM and ENEMY*.DAT).
        #[arg(long)]
        game_disk: PathBuf,
        /// Source Demo HDM; provides TC.CNS for the boot workaround.
        #[arg(long)]
        demo_disk: PathBuf,
        /// Source Arle Data HDM; provides MADO-A2.DAT for the boot workaround.
        #[arg(long)]
        data_disk: PathBuf,
        /// Uncompressed per-enemy data file(s) to patch (repeatable).
        #[arg(long = "enemy-file", default_values = ["ENEMY001.DAT", "ENEMY002.DAT"])]
        enemy_files: Vec<String>,
        /// Output patched HDM.
        #[arg(long)]
        output: PathBuf,
    },
    /// Build a multi-glyph Hangul PoC disk: derive and register every glyph
    /// required by the Korean enemy names. Also stages the two-floppy
    /// boot-smoke media workaround.
    BuildMultiHangulPoc {
        /// Source Arle Game HDM (boot disk; holds MAIN.COM and ENEMY*.DAT).
        #[arg(long)]
        game_disk: PathBuf,
        /// Source Demo HDM; provides TC.CNS for the boot workaround.
        #[arg(long)]
        demo_disk: PathBuf,
        /// Source Arle Data HDM; provides MADO-A2.DAT for the boot workaround.
        #[arg(long)]
        data_disk: PathBuf,
        /// Versioned font source and raster parameters.
        #[arg(long, default_value = "assets/fonts/font_profile.json")]
        font_profile: PathBuf,
        /// Enemy name replacement(s) as FILE=KOREAN, e.g. ENEMY001.DAT=뿌요.
        #[arg(long = "enemy", default_values = ["ENEMY001.DAT=뿌요", "ENEMY002.DAT=나스그레이브"])]
        enemies: Vec<String>,
        /// Output patched HDM.
        #[arg(long)]
        output: PathBuf,
    },
    /// Build a renderer-hook proof disk: patch the GAME_A.OVL packed trampolines
    /// and add a MAIN.COM boot stub that copies the hook + a small box sheet to
    /// 0x8800. Self-contained (no file I/O) for verifying the on-disk hook.
    BuildHookPocDisk {
        /// Source Arle Game HDM.
        #[arg(long)]
        game_disk: PathBuf,
        /// Source Demo HDM; provides TC.CNS.
        #[arg(long)]
        demo_disk: PathBuf,
        /// Source Arle Data HDM; provides MADO-A2.DAT.
        #[arg(long)]
        data_disk: PathBuf,
        /// Output patched HDM.
        #[arg(long)]
        output: PathBuf,
    },
    /// Build the production renderer-hook disk: patch the GAME_A.OVL trampolines,
    /// add a MAIN.COM boot stub that loads KFONT.BIN from disk into 0x8800:0 via
    /// INT 21h and copies the hook, and ship KFONT.BIN as a root file. Unlike
    /// build-hook-poc-disk, the full glyph sheet is loaded from disk.
    BuildHookLoaderDisk {
        /// Source Arle Game HDM.
        #[arg(long)]
        game_disk: PathBuf,
        /// Source Demo HDM; provides TC.CNS.
        #[arg(long)]
        demo_disk: PathBuf,
        /// Source Arle Data HDM; provides MADO-A2.DAT.
        #[arg(long)]
        data_disk: PathBuf,
        /// Output patched HDM.
        #[arg(long)]
        output: PathBuf,
    },
    /// Build a per-character renderer-hook disk: detect the overlay on this Game
    /// disk (GAME_A/R/S for Arle/Rulue/Schezo), patch its trampolines, and ship
    /// the matching hook blob in a combined KFONT.BIN loaded into 0x8800:0 at boot.
    BuildHookCharDisk {
        /// Source character Game HDM (Arle, Rulue, or Schezo).
        #[arg(long)]
        game_disk: PathBuf,
        /// Source Demo HDM; provides TC.CNS.
        #[arg(long)]
        demo_disk: PathBuf,
        /// Source Arle Data HDM; provides MADO-A2.DAT.
        #[arg(long)]
        data_disk: PathBuf,
        /// Output patched HDM.
        #[arg(long)]
        output: PathBuf,
    },
    /// Build a disk with batch Korean translations applied to GAME_A.OVL: decode,
    /// install the renderer-hook trampolines, apply every translation from a JSON
    /// table (in place when it fits the byte budget, else relocated above the
    /// render scratch with all pointer sites rewritten), re-encode, and stage the
    /// KFONT sheet + boot media.
    BuildTranslatedDisk {
        /// Source Arle Game HDM.
        #[arg(long)]
        game_disk: PathBuf,
        /// Source Demo HDM; provides TC.CNS.
        #[arg(long)]
        demo_disk: PathBuf,
        /// Source Arle Data HDM; provides MADO-A2.DAT.
        #[arg(long)]
        data_disk: PathBuf,
        /// JSON `{ "0xHHHH": "Korean", ... }` keyed by decoded string offset.
        #[arg(long)]
        translations: PathBuf,
        /// Output patched HDM.
        #[arg(long)]
        output: PathBuf,
    },
    /// Build the renderer-hook disk via the decoded-level re-encode path: decode
    /// GAME_A.OVL, install both trampolines at their decoded offsets, re-encode
    /// with the LZ encoder, and write it back -- instead of patching packed-stream
    /// literals in place. Proves the game's decompressor accepts re-encoder output
    /// (criterion C) while still rendering Hangul through the hook.
    BuildReencodeHookDisk {
        /// Source Arle Game HDM.
        #[arg(long)]
        game_disk: PathBuf,
        /// Source Demo HDM; provides TC.CNS.
        #[arg(long)]
        demo_disk: PathBuf,
        /// Source Arle Data HDM; provides MADO-A2.DAT.
        #[arg(long)]
        data_disk: PathBuf,
        /// Output patched HDM.
        #[arg(long)]
        output: PathBuf,
    },
    /// Build the message-relocation PoC disk: relocate one real dialogue message
    /// to Korean (longer, appended to the decoded overlay with its `mov si`
    /// pointer rewritten), install the renderer-hook trampolines, re-encode, and
    /// ship the glyph sheet. Proves variable-length Korean reinsertion into the
    /// LZ-packed overlay end to end.
    BuildMessageRelocPoc {
        /// Source Arle Game HDM.
        #[arg(long)]
        game_disk: PathBuf,
        /// Source Demo HDM; provides TC.CNS.
        #[arg(long)]
        demo_disk: PathBuf,
        /// Source Arle Data HDM; provides MADO-A2.DAT.
        #[arg(long)]
        data_disk: PathBuf,
        /// Force relocation, padding the decoded overlay so the relocated string
        /// lands at this decoded offset (e.g. 0xC000) -- a probe for whether the
        /// segment space above the overlay is usable for longer Korean.
        #[arg(long)]
        reloc_to: Option<String>,
        /// Output patched HDM.
        #[arg(long)]
        output: PathBuf,
    },
    /// Build a per-cell sweep disk: load the checked crosshair sweep
    /// fixture and relocate the battle "appeared"
    /// message to one JIS row's 94 cell codes, so the real renderer draws every
    /// cell of that row. On screen the crosshair marches col 0->15 with the
    /// horizontal bar stepping down every 16 cells -- a gap is a dead cell, a
    /// jump is a mis-mapped cell.
    BuildSheetSweepDisk {
        /// Source Arle Game HDM.
        #[arg(long)]
        game_disk: PathBuf,
        /// Source Demo HDM; provides TC.CNS.
        #[arg(long)]
        demo_disk: PathBuf,
        /// Source Arle Data HDM; provides MADO-A2.DAT.
        #[arg(long)]
        data_disk: PathBuf,
        /// Sweep row index 0..9 -> contiguous JIS rows 0x75..0x7E.
        #[arg(long)]
        row: usize,
        /// Sweep sheet binary.
        #[arg(long, default_value = "assets/gaiji/kfont_sweep.bin")]
        sheet: PathBuf,
        /// Sweep sheet JSON (per-cell sjis codes).
        #[arg(long, default_value = "assets/gaiji/kfont_sweep.json")]
        sheet_json: PathBuf,
        /// Hex of the renderer message to relocate onto. Default: the battle
        /// "appeared" suffix (RNG-gated). Pass a deterministic covered message
        /// instead, e.g. the wall-bump line `817582a282c182bd815b82a2`
        /// (「いったーい, always shown by bumping a wall) for a reliable capture.
        #[arg(long)]
        target_hex: Option<String>,
        /// Output patched HDM.
        #[arg(long)]
        output: PathBuf,
    },
    /// Build a per-character reinsertion PoC disk: detect the overlay (GAME_A/R/S),
    /// patch one message to Korean (in place if it fits the byte budget, else
    /// relocated), install the renderer-hook trampolines found by byte-pattern
    /// (not GAME_A-hardcoded offsets), re-encode, and ship the matching per-overlay
    /// hook + the real glyph sheet + the character's boot media. Generalizes the
    /// GAME_A-only build-translated-disk so R/S reinsertion can be proven E2E.
    BuildCharRelocPoc {
        /// Source character Game HDM (Arle, Rulue, or Schezo).
        #[arg(long)]
        game_disk: PathBuf,
        /// Source Demo HDM; provides TC.CNS.
        #[arg(long)]
        demo_disk: PathBuf,
        /// Source character Data HDM; provides MADO-?2.DAT.
        #[arg(long)]
        data_disk: PathBuf,
        /// Hex of the target message to overwrite (a covered renderer message).
        #[arg(long)]
        message_hex: String,
        /// Korean replacement (encoded through the sheet).
        #[arg(long)]
        korean: String,
        /// Glyph-sheet binary shipped as KFONT.BIN (default: the real kfont).
        #[arg(long, default_value = "assets/gaiji/kfont.bin")]
        sheet: PathBuf,
        /// Glyph-sheet JSON used to encode the Korean.
        #[arg(long, default_value = "assets/gaiji/kfont.json")]
        sheet_json: PathBuf,
        /// Output patched HDM.
        #[arg(long)]
        output: PathBuf,
    },
    /// Build the translated Game disk for one character: detect the overlay
    /// (GAME_A/R/S), apply all translation batches, install the renderer hook and
    /// glyph sheet, and preserve the source media layout and boot script.
    BuildCharDisk {
        /// Source character Game HDM (Arle, Rulue, or Schezo).
        #[arg(long)]
        game_disk: PathBuf,
        /// Directory of translation batch JSONs; those whose `overlay` matches the
        /// detected overlay are merged (shipping: assets/translations/complete).
        #[arg(long)]
        translations_dir: PathBuf,
        /// Per-overlay glyph-sheet binary shipped as KFONT.BIN.
        #[arg(long)]
        sheet: PathBuf,
        /// Per-overlay glyph-sheet JSON used to encode the Korean.
        #[arg(long)]
        sheet_json: PathBuf,
        /// Permit draft statuses for emulator PoCs. Shipping builds require
        /// every included translation entry to be `complete`.
        #[arg(long)]
        allow_needs_review: bool,
        /// Output patched HDM.
        #[arg(long)]
        output: PathBuf,
    },
    /// Apply the optional two-drive boot transform to a composed character
    /// Game disk. Copies TC.CNS from the exact Demo source and the character's
    /// opening music from the exact matching Data source, then routes R/S
    /// AUTOEXEC.BAT through drive A:. The matching Data disk remains in drive 2
    /// for every other resource. This local-media operation emits no
    /// distributable BPS or protocol package.
    ApplyTwoDiskBoot {
        /// Exact source or already composed Korean character Game HDM.
        #[arg(long)]
        game_disk: PathBuf,
        /// Exact original shared Demo HDM that owns TC.CNS.
        #[arg(long)]
        demo_disk: PathBuf,
        /// Exact original matching character Data HDM that owns MADO-A2/R/S.DAT.
        #[arg(long)]
        data_disk: PathBuf,
        /// Output Game HDM to pair with the matching character Data HDM.
        #[arg(long)]
        output: PathBuf,
    },
    /// Patch the translated executable `*.DAT` resources (NPC/ENEMY/SHOP and
    /// separately loaded EVENT modules; LZ-compressed with the overlay codec).
    /// Reads every translation batch in the dir that has a `dat` field, resolves
    /// canonical translation bindings from the parent translation root, then
    /// patches each resource in place, re-encodes it, and verifies FAT12 readback.
    PatchDiskDats {
        /// Source HDM (usually a disk already built by build-char-disk).
        #[arg(long)]
        disk: PathBuf,
        /// Directory of DAT translation batch JSONs (entries with a `dat` field).
        /// Bound catalogs resolve their canonical source file from this directory's
        /// parent translation root.
        #[arg(long)]
        translations_dir: PathBuf,
        /// Per-overlay glyph-sheet JSON to encode the Korean (same sheet the disk
        /// ships as KFONT.BIN; must include the DAT syllables).
        #[arg(long)]
        sheet_json: PathBuf,
        /// Permit draft statuses for emulator PoCs. Shipping builds require
        /// every included translation entry to be `complete`.
        #[arg(long)]
        allow_needs_review: bool,
        /// Output patched HDM.
        #[arg(long)]
        output: PathBuf,
    },
    /// Replace an existing FAT12 file's contents in place (within its cluster
    /// capacity). Used to rewrite a boot script -- e.g. point a character disk's
    /// AUTOEXEC.BAT at its own GAME_?.OVL so R/S boot under the MAME 2-floppy
    /// workaround instead of demanding the Arle game disk.
    ReplaceFile {
        /// Source HDM.
        #[arg(long)]
        disk: PathBuf,
        /// 8.3 name of the existing file to overwrite (e.g. AUTOEXEC.BAT).
        #[arg(long)]
        name: String,
        /// Local file whose bytes become the new contents.
        #[arg(long)]
        content: PathBuf,
        /// Output patched HDM.
        #[arg(long)]
        output: PathBuf,
    },
    /// Add a local binary file (e.g. the Hangul glyph sheet) into an HDM's FAT12
    /// root directory. Used to stage the renderer-hook sheet onto a disk.
    AddFile {
        /// Source HDM.
        #[arg(long)]
        disk: PathBuf,
        /// Local file to add.
        #[arg(long)]
        file: PathBuf,
        /// 8.3 name to store it as (e.g. KFONT.BIN).
        #[arg(long)]
        name: String,
        /// Output patched HDM.
        #[arg(long)]
        output: PathBuf,
    },
    /// Register a box marker across several gaiji JIS rows and show them in the
    /// GAOO boot prompt, to measure how many gaiji rows the BIOS accepts (text
    /// plane shares the gaiji RAM with the graphics renderer fetch).
    BuildGaijiRowsProbe {
        /// Source Arle Game HDM.
        #[arg(long)]
        disk: PathBuf,
        /// Output patched HDM.
        #[arg(long)]
        output: PathBuf,
    },
    /// Decode a packed A.R.S overlay/resource stream.
    DecodeOverlay {
        /// Packed input file, for example an extracted GAME_A.OVL.
        input: PathBuf,
        /// Optional decoded output path.
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// List decoded overlay strings passed to the graphics text renderer.
    ListOverlayMessages {
        /// Packed input file(s), for example extracted GAME_A.OVL/GAME_R.OVL.
        #[arg(required = true)]
        inputs: Vec<PathBuf>,
        /// Runtime logical renderer offset inside the decoded overlay segment, or "auto".
        #[arg(long, default_value = "auto")]
        renderer_offset: String,
        /// Runtime logical offset that decoded byte index 0 maps to.
        #[arg(long, default_value = "0x100")]
        load_offset: String,
        /// Maximum rows to print.
        #[arg(long)]
        limit: Option<usize>,
        /// Optional JSON corpus output path.
        #[arg(long)]
        json_output: Option<PathBuf>,
    },
    /// Catalog unique renderer messages (deduped, with byte budget and every
    /// pointer) plus an SJIS cross-check for messages the pointer scan missed.
    CatalogMessages {
        /// Packed overlay input(s), for example extracted GAME_A.OVL.
        #[arg(required = true)]
        inputs: Vec<PathBuf>,
        /// Runtime logical offset that decoded byte index 0 maps to.
        #[arg(long, default_value = "0x100")]
        load_offset: String,
        /// Minimum double-byte chars for an SJIS run to count in the cross-check.
        #[arg(long, default_value_t = 2)]
        min_double: usize,
        /// Optional JSON catalog output path.
        #[arg(long)]
        json_output: Option<PathBuf>,
    },
    /// Detect the pointer tables that index messages the `mov si` scan misses,
    /// and report how much of the SJIS cross-check they cover.
    FindPointerTables {
        /// Packed overlay input(s), for example extracted GAME_A.OVL.
        #[arg(required = true)]
        inputs: Vec<PathBuf>,
        /// Runtime logical offset that decoded byte index 0 maps to.
        #[arg(long, default_value = "0x100")]
        load_offset: String,
        /// Minimum consecutive entries for a run to count as a table.
        #[arg(long, default_value_t = 4)]
        min_entries: usize,
        /// Largest record stride to probe (entries are read every 2..=max bytes).
        #[arg(long, default_value_t = 16)]
        max_stride: usize,
        /// Optional JSON table-map output path.
        #[arg(long)]
        json_output: Option<PathBuf>,
    },
    /// Build the unified per-message map: every string with its full set of
    /// pointer rewrite sites (mov si immediates and table entries), the input
    /// the batch reinserter consumes.
    BuildMessageMap {
        /// Packed overlay input(s), for example extracted GAME_A.OVL.
        #[arg(required = true)]
        inputs: Vec<PathBuf>,
        /// Runtime logical offset that decoded byte index 0 maps to.
        #[arg(long, default_value = "0x100")]
        load_offset: String,
        /// Minimum consecutive entries for a pointer table.
        #[arg(long, default_value_t = 4)]
        min_entries: usize,
        /// Largest record stride to probe for pointer tables.
        #[arg(long, default_value_t = 16)]
        max_stride: usize,
        /// Optional JSON message-map output path.
        #[arg(long)]
        json_output: Option<PathBuf>,
    },
    /// Emit the unified message map as a translation-workflow raw baseline:
    /// protected fields (text, raw_hex, offsets, rewrite sites) plus empty
    /// editable fields (ko, status, notes) for assets/translations/raw/.
    EmitTranslationRaw {
        /// Packed overlay input, for example extracted GAME_A.OVL.
        input: PathBuf,
        /// Runtime logical offset that decoded byte index 0 maps to.
        #[arg(long, default_value = "0x100")]
        load_offset: String,
        /// Minimum consecutive entries for a pointer table.
        #[arg(long, default_value_t = 4)]
        min_entries: usize,
        /// Largest record stride to probe for pointer tables.
        #[arg(long, default_value_t = 16)]
        max_stride: usize,
        /// Output raw script path (e.g. assets/translations/raw/game_a.json).
        #[arg(long)]
        output: PathBuf,
    },
    /// Emit the complete opening/interlude/ending cutscene corpus from all
    /// three character Game disks as translation-workflow raw JSON files.
    EmitCutsceneRaw {
        /// Source Arle Game HDM.
        #[arg(long)]
        arle_game: PathBuf,
        /// Source Rulue Game HDM.
        #[arg(long)]
        rulue_game: PathBuf,
        /// Source Schezo Game HDM.
        #[arg(long)]
        schezo_game: PathBuf,
        /// Output directory for the nine resource catalogs and manifest.
        #[arg(long)]
        output_dir: PathBuf,
    },
    /// Count the unique Hangul syllables used by each cutscene translation
    /// catalog and fail if a runtime set exceeds the 188-glyph BIOS gaiji cap.
    CheckCutsceneDemand {
        /// Directory containing cutscene translation JSON files.
        #[arg(long)]
        translations_dir: PathBuf,
        /// Optional JSON summary output.
        #[arg(long)]
        json_output: Option<PathBuf>,
    },
    /// Validate staged cutscene translations against the immutable raw catalogs.
    ValidateCutsceneTranslation {
        /// Directory containing immutable raw cutscene catalogs.
        #[arg(long)]
        raw_dir: PathBuf,
        /// Directory containing staged translated cutscene catalogs.
        #[arg(long)]
        translations_dir: PathBuf,
    },
    /// Refresh protected cutscene-catalog metadata after re-extraction while
    /// preserving only ko/status/notes from the staged translation catalogs.
    RefreshCutsceneTranslationMetadata {
        /// Directory containing newly emitted immutable raw catalogs.
        #[arg(long)]
        raw_dir: PathBuf,
        /// Directory containing staged translated catalogs to refresh in place.
        #[arg(long)]
        translations_dir: PathBuf,
    },
    /// Build all translated cutscene phases for one character Game disk.
    /// Rulue/Schezo executable overlays carry phase-local tables; Arle loads a
    /// shared three-phase table and selects it at DEMO.OVL video reset. An Arle
    /// input carrying the exact scenario KFONT loader is composed in place into
    /// one non-overlapping all-track font image.
    BuildCutsceneDisk {
        #[arg(long)]
        game_disk: PathBuf,
        #[arg(long)]
        raw_dir: PathBuf,
        #[arg(long)]
        translations_dir: PathBuf,
        /// Directory of per-resource gaiji tables (e.g. shezo_op.json).
        #[arg(long)]
        gaiji_dir: PathBuf,
        /// Font used to compile Arle's ending bitmap caption.
        #[arg(long, default_value = "assets/fonts/font_profile.json")]
        font_profile: PathBuf,
        /// Permit draft statuses for emulator PoCs. Shipping builds require
        /// every non-structural entry to be `complete`.
        #[arg(long)]
        allow_needs_review: bool,
        #[arg(long)]
        output: PathBuf,
    },
    /// Validate a translation script against the overlay: protected-field
    /// integrity, encodability, residual source text, empty/`done` mismatch, and
    /// the byte/display budget -- the gate that keeps a bad edit out of a build.
    ValidateTranslation {
        /// Packed overlay input, for example extracted GAME_A.OVL.
        input: PathBuf,
        /// Translation script (raw/complete schema) to validate.
        #[arg(long)]
        script: PathBuf,
        /// Runtime logical offset that decoded byte index 0 maps to.
        #[arg(long, default_value = "0x100")]
        load_offset: String,
        /// Soft display-width warning threshold (full-width chars per line).
        #[arg(long, default_value_t = 18)]
        max_line: usize,
        /// Glyph-sheet JSON to check encodability against. Defaults to the built-in
        /// sheet; production builds pass the per-character 940-cell KFONT map.
        #[arg(long)]
        sheet_json: Option<PathBuf>,
    },
    /// Check the selected translation tree against project-approved series
    /// terminology. This validates mutable project data explicitly rather than
    /// freezing its current contents in the Rust test suite.
    ValidateApprovedTerminology {
        /// Translation-stage root containing overlay JSON files plus dat/ and
        /// cutscene/ subdirectories.
        #[arg(long, default_value = "assets/translations/needs_review")]
        translations_dir: PathBuf,
        /// Project-owned source-term to Korean-term rules.
        #[arg(
            long,
            default_value = "assets/translation_guide/approved_terminology.json"
        )]
        rules: PathBuf,
    },
    /// Reject translations that exceed the normal in-game message pane, an
    /// extracted cutscene call-site window, or the corresponding source rows.
    ValidateDialogueLayout {
        /// Translation-stage root containing overlay JSON files plus dat/ and
        /// cutscene/ subdirectories.
        #[arg(long, default_value = "assets/translations/needs_review")]
        translations_dir: PathBuf,
    },
    /// List enemy names anchored by the standard attack message in ENEMY*.DAT.
    ListEnemyNames {
        /// Enemy data file(s), for example extracted ENEMY001.DAT.
        #[arg(required = true)]
        inputs: Vec<PathBuf>,
        /// Optional JSON corpus output path.
        #[arg(long)]
        json_output: Option<PathBuf>,
    },
    /// Count valid SJIS pairs for prefix/glyph-budget probes.
    GlyphStats {
        /// External binary file(s), for example decoded overlays.
        inputs: Vec<PathBuf>,
        /// HDM FAT12 disk image(s); every root file is scanned.
        #[arg(long)]
        disk: Vec<PathBuf>,
        /// Number of most frequent pairs to print.
        #[arg(long, default_value_t = 20)]
        top: usize,
        /// Optional JSON output path.
        #[arg(long)]
        json_output: Option<PathBuf>,
    },
    /// Create a distributable BPS patch from a recognized primary A/R/S Game
    /// or Data HDM, or the shared Demo HDM, to a built HDM. Embeds source/target SHA-256
    /// metadata, writes all three BPS CRC32 fields, and self-applies before
    /// writing the patch.
    BuildBps {
        /// Exact original character Game or shared Demo HDM.
        #[arg(long)]
        source: PathBuf,
        /// Patched HDM produced by the build pipeline.
        #[arg(long)]
        target: PathBuf,
        /// Output BPS file. Draft patches belong under out/.
        #[arg(long)]
        output: PathBuf,
    },
    /// Apply a project BPS patch after validating its embedded source identity,
    /// SHA-256 metadata, all BPS CRC32 fields, and output HDM geometry.
    ApplyBps {
        /// Exact original character Game or shared Demo HDM.
        #[arg(long)]
        source: PathBuf,
        /// Project BPS patch.
        #[arg(long)]
        patch: PathBuf,
        /// Output patched HDM.
        #[arg(long)]
        output: PathBuf,
    },
    /// Materialize the same Rust-derived A/R/S font assets used by the full
    /// build. This is for inspection; build-full-patch derives them in scratch.
    GenerateFontAssets {
        /// Translation-stage root containing overlay JSON files plus dat/ and
        /// cutscene/ subdirectories.
        #[arg(long, default_value = "assets/translations/complete")]
        translations_dir: PathBuf,
        /// Versioned font source and raster parameters.
        #[arg(long, default_value = "assets/fonts/font_profile.json")]
        font_profile: PathBuf,
        /// Permit draft statuses for local QA. Omit for release inputs.
        #[arg(long)]
        allow_needs_review: bool,
        /// Output root; each character receives an independent generated set.
        #[arg(long)]
        output_dir: PathBuf,
    },
    /// Run the complete character pipeline from exact original Game/Demo/Data
    /// media: scenario overlay, gameplay DAT, cutscenes, then verified BPS.
    /// Intermediate HDMs stay in a temporary directory and are removed.
    BuildFullPatch {
        /// Exact original character Game HDM.
        #[arg(long)]
        game_disk: PathBuf,
        /// Exact original shared Demo HDM.
        #[arg(long)]
        demo_disk: PathBuf,
        /// Exact original matching character Data HDM.
        #[arg(long)]
        data_disk: PathBuf,
        /// Translation-stage root containing overlay JSON files plus dat/ and
        /// cutscene/ subdirectories.
        #[arg(long, default_value = "assets/translations/complete")]
        translations_dir: PathBuf,
        /// Immutable raw cutscene catalogs.
        #[arg(long, default_value = "assets/translations/raw/cutscene")]
        cutscene_raw_dir: PathBuf,
        /// Versioned font source and raster parameters. Renderer and cutscene
        /// glyph assets are derived from this profile during the build.
        #[arg(long, default_value = "assets/fonts/font_profile.json")]
        font_profile: PathBuf,
        /// Permit draft statuses for local QA. Omit for a release build.
        #[arg(long)]
        allow_needs_review: bool,
        /// Output composed HDM for emulator verification.
        #[arg(long)]
        image_output: PathBuf,
        /// Output distributable BPS patch.
        #[arg(long)]
        bps_output: PathBuf,
    },
    /// Build the complete A.R.S protocol patch set in one fail-closed run: seven
    /// file-level FAT12 patch packages nested in one hash-matched multi-disk ZIP.
    BuildReleaseSet {
        /// Exact original shared Demo HDM.
        #[arg(long)]
        demo_disk: PathBuf,
        /// Exact original Arle Game HDM.
        #[arg(long)]
        arle_game_disk: PathBuf,
        /// Exact original Arle Data HDM.
        #[arg(long)]
        arle_data_disk: PathBuf,
        /// Exact original Rulue Game HDM.
        #[arg(long)]
        rulue_game_disk: PathBuf,
        /// Exact original Rulue Data HDM.
        #[arg(long)]
        rulue_data_disk: PathBuf,
        /// Exact original Schezo Game HDM.
        #[arg(long)]
        schezo_game_disk: PathBuf,
        /// Exact original Schezo Data HDM.
        #[arg(long)]
        schezo_data_disk: PathBuf,
        /// Translation-stage root containing overlay JSON files plus dat/ and
        /// cutscene/ subdirectories.
        #[arg(long, default_value = "assets/translations/complete")]
        translations_dir: PathBuf,
        /// Immutable raw cutscene catalogs.
        #[arg(long, default_value = "assets/translations/raw/cutscene")]
        cutscene_raw_dir: PathBuf,
        /// Versioned renderer/cutscene font profile.
        #[arg(long, default_value = "assets/fonts/font_profile.json")]
        font_profile: PathBuf,
        /// Asset root containing the reviewed Demo graphics manifests.
        #[arg(long, default_value = "assets")]
        assets_dir: PathBuf,
        /// Versioned MapleStory Bold profile for the Demo menu prompt.
        #[arg(long, default_value = "assets/fonts/maplestory_bold_menu_profile.json")]
        demo_font_profile: PathBuf,
        /// Permit draft statuses for local QA. Omit for a release build.
        #[arg(long)]
        allow_needs_review: bool,
        /// New output directory. It must not already exist; it receives one
        /// protocol patch-set ZIP after all seven nested packages validate.
        #[arg(long)]
        output_dir: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Info { disk } => disk_info(disk),
        Command::Files { disk } => disk_files(disk),
        Command::ExtractFile { disk, name, output } => extract_file(disk, &name, output),
        Command::RenderScreenResource { disk, name, output } => {
            render_screen_resource(disk, &name, output)
        }
        Command::RenderScreenCatalog {
            disk,
            extensions,
            output_dir,
        } => render_screen_catalog(disk, &extensions, output_dir),
        Command::AuditDemoCompositor { demo_disk, output } => {
            audit_demo_compositor(demo_disk, output)
        }
        Command::BuildDemoTitle {
            demo_disk,
            assets_dir,
            output,
            preview,
        } => build_demo_title(demo_disk, assets_dir, output, preview),
        Command::BuildScenarioTitle {
            data_disk,
            assets_dir,
            font_profile,
            allow_needs_review,
            output,
            bps_output,
            preview,
            opening_preview,
        } => build_character_data_graphics(CharacterDataGraphicsBuild {
            data_disk,
            assets_dir,
            font_profile,
            allow_needs_review,
            output,
            bps_output,
            title_preview: preview,
            arle_opening_preview: opening_preview,
        }),
        Command::BuildDemoGraphics {
            demo_disk,
            assets_dir,
            font_profile,
            output,
            bps_output,
            title_preview,
            menu_preview,
        } => build_demo_graphics(
            demo_disk,
            assets_dir,
            font_profile,
            output,
            bps_output,
            title_preview,
            menu_preview,
        ),
        Command::AuditResources {
            disks,
            extensions,
            min_run,
            min_japanese,
            quiet,
            output,
        } => audit_resources(disks, &extensions, min_run, min_japanese, quiet, output),
        Command::SjisSweep {
            input,
            in_disk,
            min_run,
            min_japanese,
            limit,
        } => sjis_sweep(input, in_disk.as_deref(), min_run, min_japanese, limit),
        Command::BuildMarker { disk, output } => build_marker(disk, output),
        Command::BuildHangulProbe { disk, output } => build_hangul_probe(disk, output),
        Command::BuildCutsceneHangulPoc {
            game_disk,
            demo_disk,
            data_disk,
            output,
        } => build_cutscene_hangul_poc(game_disk, demo_disk, data_disk, output),
        Command::BuildArleBootSmoke {
            game_disk,
            demo_disk,
            data_disk,
            hangul_probe,
            output,
        } => build_arle_boot_smoke(game_disk, demo_disk, data_disk, hangul_probe, output),
        Command::BuildRendererHangulPoc {
            game_disk,
            demo_disk,
            data_disk,
            enemy_files,
            output,
        } => build_renderer_hangul_poc(game_disk, demo_disk, data_disk, &enemy_files, output),
        Command::BuildMultiHangulPoc {
            game_disk,
            demo_disk,
            data_disk,
            font_profile,
            enemies,
            output,
        } => build_multi_hangul_poc(
            game_disk,
            demo_disk,
            data_disk,
            &font_profile,
            &enemies,
            output,
        ),
        Command::BuildHookPocDisk {
            game_disk,
            demo_disk,
            data_disk,
            output,
        } => build_hook_poc_disk(game_disk, demo_disk, data_disk, output),
        Command::BuildHookLoaderDisk {
            game_disk,
            demo_disk,
            data_disk,
            output,
        } => build_hook_loader_disk(game_disk, demo_disk, data_disk, output),
        Command::BuildHookCharDisk {
            game_disk,
            demo_disk,
            data_disk,
            output,
        } => build_hook_char_disk(game_disk, demo_disk, data_disk, output),
        Command::BuildReencodeHookDisk {
            game_disk,
            demo_disk,
            data_disk,
            output,
        } => build_reencode_hook_disk(game_disk, demo_disk, data_disk, output),
        Command::BuildTranslatedDisk {
            game_disk,
            demo_disk,
            data_disk,
            translations,
            output,
        } => build_translated_disk(game_disk, demo_disk, data_disk, translations, output),
        Command::BuildMessageRelocPoc {
            game_disk,
            demo_disk,
            data_disk,
            reloc_to,
            output,
        } => build_message_reloc_poc(game_disk, demo_disk, data_disk, reloc_to, output),
        Command::BuildSheetSweepDisk {
            game_disk,
            demo_disk,
            data_disk,
            row,
            sheet,
            sheet_json,
            target_hex,
            output,
        } => build_sheet_sweep_disk(
            game_disk, demo_disk, data_disk, row, sheet, sheet_json, target_hex, output,
        ),
        Command::BuildCharRelocPoc {
            game_disk,
            demo_disk,
            data_disk,
            message_hex,
            korean,
            sheet,
            sheet_json,
            output,
        } => build_char_reloc_poc(
            game_disk,
            demo_disk,
            data_disk,
            &message_hex,
            &korean,
            sheet,
            sheet_json,
            output,
        ),
        Command::BuildCharDisk {
            game_disk,
            translations_dir,
            sheet,
            sheet_json,
            allow_needs_review,
            output,
        } => build_char_disk(
            game_disk,
            translations_dir,
            sheet,
            sheet_json,
            allow_needs_review,
            output,
        ),
        Command::ApplyTwoDiskBoot {
            game_disk,
            demo_disk,
            data_disk,
            output,
        } => apply_two_disk_boot(game_disk, demo_disk, data_disk, output),
        Command::PatchDiskDats {
            disk,
            translations_dir,
            sheet_json,
            allow_needs_review,
            output,
        } => patch_disk_dats(
            disk,
            translations_dir,
            sheet_json,
            allow_needs_review,
            output,
        ),
        Command::ReplaceFile {
            disk,
            name,
            content,
            output,
        } => replace_file_cmd(disk, &name, content, output),
        Command::AddFile {
            disk,
            file,
            name,
            output,
        } => add_file_cmd(disk, file, &name, output),
        Command::BuildGaijiRowsProbe { disk, output } => build_gaiji_rows_probe(disk, output),
        Command::DecodeOverlay { input, output } => decode_overlay(input, output),
        Command::ListOverlayMessages {
            inputs,
            renderer_offset,
            load_offset,
            limit,
            json_output,
        } => list_overlay_messages(inputs, &renderer_offset, &load_offset, limit, json_output),
        Command::CatalogMessages {
            inputs,
            load_offset,
            min_double,
            json_output,
        } => catalog_messages_cmd(inputs, &load_offset, min_double, json_output),
        Command::FindPointerTables {
            inputs,
            load_offset,
            min_entries,
            max_stride,
            json_output,
        } => find_pointer_tables_cmd(inputs, &load_offset, min_entries, max_stride, json_output),
        Command::BuildMessageMap {
            inputs,
            load_offset,
            min_entries,
            max_stride,
            json_output,
        } => build_message_map_cmd(inputs, &load_offset, min_entries, max_stride, json_output),
        Command::EmitTranslationRaw {
            input,
            load_offset,
            min_entries,
            max_stride,
            output,
        } => emit_translation_raw(input, &load_offset, min_entries, max_stride, output),
        Command::EmitCutsceneRaw {
            arle_game,
            rulue_game,
            schezo_game,
            output_dir,
        } => emit_cutscene_raw(arle_game, rulue_game, schezo_game, output_dir),
        Command::CheckCutsceneDemand {
            translations_dir,
            json_output,
        } => check_cutscene_demand(translations_dir, json_output),
        Command::ValidateCutsceneTranslation {
            raw_dir,
            translations_dir,
        } => validate_cutscene_translation(raw_dir, translations_dir),
        Command::RefreshCutsceneTranslationMetadata {
            raw_dir,
            translations_dir,
        } => refresh_cutscene_translation_metadata(raw_dir, translations_dir),
        Command::BuildCutsceneDisk {
            game_disk,
            raw_dir,
            translations_dir,
            gaiji_dir,
            font_profile,
            allow_needs_review,
            output,
        } => build_cutscene_disk(
            game_disk,
            raw_dir,
            translations_dir,
            gaiji_dir,
            font_profile,
            allow_needs_review,
            output,
        ),
        Command::ValidateTranslation {
            input,
            script,
            load_offset,
            max_line,
            sheet_json,
        } => validate_translation(input, script, &load_offset, max_line, sheet_json),
        Command::ValidateApprovedTerminology {
            translations_dir,
            rules,
        } => validate_approved_terminology(translations_dir, rules),
        Command::ValidateDialogueLayout { translations_dir } => {
            validate_dialogue_layout(translations_dir)
        }
        Command::ListEnemyNames {
            inputs,
            json_output,
        } => list_enemy_names(inputs, json_output),
        Command::GlyphStats {
            inputs,
            disk,
            top,
            json_output,
        } => glyph_stats(inputs, disk, top, json_output),
        Command::BuildBps {
            source,
            target,
            output,
        } => build_bps(source, target, output),
        Command::ApplyBps {
            source,
            patch,
            output,
        } => apply_bps(source, patch, output),
        Command::GenerateFontAssets {
            translations_dir,
            font_profile,
            allow_needs_review,
            output_dir,
        } => generate_font_assets(
            translations_dir,
            font_profile,
            allow_needs_review,
            output_dir,
        ),
        Command::BuildFullPatch {
            game_disk,
            demo_disk,
            data_disk,
            translations_dir,
            cutscene_raw_dir,
            font_profile,
            allow_needs_review,
            image_output,
            bps_output,
        } => build_full_patch(
            game_disk,
            demo_disk,
            data_disk,
            translations_dir,
            cutscene_raw_dir,
            font_profile,
            allow_needs_review,
            image_output,
            Some(bps_output),
        ),
        Command::BuildReleaseSet {
            demo_disk,
            arle_game_disk,
            arle_data_disk,
            rulue_game_disk,
            rulue_data_disk,
            schezo_game_disk,
            schezo_data_disk,
            translations_dir,
            cutscene_raw_dir,
            font_profile,
            assets_dir,
            demo_font_profile,
            allow_needs_review,
            output_dir,
        } => build_release_set(
            demo_disk,
            arle_game_disk,
            arle_data_disk,
            rulue_game_disk,
            rulue_data_disk,
            schezo_game_disk,
            schezo_data_disk,
            translations_dir,
            cutscene_raw_dir,
            font_profile,
            assets_dir,
            demo_font_profile,
            allow_needs_review,
            output_dir,
        ),
    }
}

fn build_bps(source_path: PathBuf, target_path: PathBuf, output_path: PathBuf) -> Result<()> {
    ensure_distinct_output(
        &output_path,
        &[("source HDM", &source_path), ("target HDM", &target_path)],
    )?;
    let source = std::fs::read(&source_path)
        .with_context(|| format!("read BPS source {}", source_path.display()))?;
    let target = std::fs::read(&target_path)
        .with_context(|| format!("read BPS target {}", target_path.display()))?;
    let created = pc98_madou_ars::release_patch::create_release_patch(&source, &target)
        .with_context(|| {
            format!(
                "build BPS {} -> {}",
                source_path.display(),
                target_path.display()
            )
        })?;
    std::fs::write(&output_path, &created.patch)
        .with_context(|| format!("write BPS {}", output_path.display()))?;

    println!(
        "source: {} ({}) SHA-256 {}",
        created.source.label, created.source.id, created.source.sha256
    );
    println!(
        "target: {} bytes SHA-256 {}",
        target.len(),
        created.target_sha256
    );
    println!(
        "BPS: {} bytes SHA-256 {}; CRC32 source={:08x} target={:08x} patch={:08x}",
        created.patch.len(),
        pc98_madou_ars::release_patch::sha256_hex(&created.patch),
        created.info.source_crc32,
        created.info.target_crc32,
        created.info.patch_crc32
    );
    println!(
        "self-apply verified byte-identical; wrote {}",
        output_path.display()
    );
    Ok(())
}

fn apply_bps(source_path: PathBuf, patch_path: PathBuf, output_path: PathBuf) -> Result<()> {
    ensure_distinct_output(
        &output_path,
        &[("source HDM", &source_path), ("BPS patch", &patch_path)],
    )?;
    let source = std::fs::read(&source_path)
        .with_context(|| format!("read BPS source {}", source_path.display()))?;
    let patch = std::fs::read(&patch_path)
        .with_context(|| format!("read BPS patch {}", patch_path.display()))?;
    let applied = pc98_madou_ars::release_patch::apply_release_patch(&source, &patch)
        .with_context(|| {
            format!(
                "apply BPS {} to {}",
                patch_path.display(),
                source_path.display()
            )
        })?;
    std::fs::write(&output_path, &applied.target)
        .with_context(|| format!("write patched HDM {}", output_path.display()))?;

    println!(
        "source: {} ({}) SHA-256 {}",
        applied.source.label, applied.source.id, applied.source.sha256
    );
    println!(
        "BPS CRC32 verified: source={:08x} target={:08x} patch={:08x}",
        applied.info.source_crc32, applied.info.target_crc32, applied.info.patch_crc32
    );
    println!(
        "target: {} bytes SHA-256 {}; wrote {}",
        applied.target.len(),
        applied.target_sha256,
        output_path.display()
    );
    Ok(())
}

fn validate_approved_terminology(translations_dir: PathBuf, rules: PathBuf) -> Result<()> {
    let report = pc98_madou_ars::approved_terminology::validate_approved_terminology(
        &translations_dir,
        &rules,
    )?;
    println!(
        "approved terminology: {} rules, {} matching entries in {}",
        report.rules_checked,
        report.matching_entries,
        translations_dir.display()
    );
    Ok(())
}

fn validate_dialogue_layout(translations_dir: PathBuf) -> Result<()> {
    let report = pc98_madou_ars::dialogue_layout::validate_translation_tree(&translations_dir)?;
    println!(
        "dialogue layout: {} entries checked ({} normal 28-cell pane, {} exact cutscene windows, {} cutscene source-row-only); widest authored line {} at {}",
        report.entries,
        report.normal_pane_entries,
        report.exact_cutscene_entries,
        report.row_only_cutscene_entries,
        pc98_madou_ars::dialogue_layout::format_half_cells(report.widest_half_cells),
        report.widest_id,
    );
    Ok(())
}

fn generate_font_assets(
    translations_dir: PathBuf,
    font_profile: PathBuf,
    allow_needs_review: bool,
    output_dir: PathBuf,
) -> Result<()> {
    for character in ["arle", "rulue", "schezo"] {
        let character_dir = output_dir.join(character);
        let fonts = pc98_madou_ars::font_build::generate_full_build_fonts(
            &font_profile,
            &translations_dir,
            character,
            allow_needs_review,
            &character_dir,
        )
        .with_context(|| format!("generate {character} font assets"))?;
        println!(
            "{character}: {} renderer glyphs; cutscene [{}] -> {}",
            fonts.renderer_glyphs,
            fonts
                .cutscene_glyphs
                .iter()
                .map(|(phase, count)| format!("{phase}:{count}"))
                .collect::<Vec<_>>()
                .join(", "),
            character_dir.display()
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn build_full_patch(
    game_disk_path: PathBuf,
    demo_disk_path: PathBuf,
    data_disk_path: PathBuf,
    translations_dir: PathBuf,
    cutscene_raw_dir: PathBuf,
    font_profile: PathBuf,
    allow_needs_review: bool,
    image_output: PathBuf,
    bps_output: Option<PathBuf>,
) -> Result<()> {
    let game = std::fs::read(&game_disk_path)
        .with_context(|| format!("read Game HDM {}", game_disk_path.display()))?;
    let demo = std::fs::read(&demo_disk_path)
        .with_context(|| format!("read Demo HDM {}", demo_disk_path.display()))?;
    let data = std::fs::read(&data_disk_path)
        .with_context(|| format!("read Data HDM {}", data_disk_path.display()))?;
    let media = pc98_madou_ars::media_identity::validate_build_inputs(&game, &demo, &data)?;

    ensure_output_parent(&image_output)?;
    ensure_distinct_output(
        &image_output,
        &[
            ("Game HDM", &game_disk_path),
            ("Demo HDM", &demo_disk_path),
            ("Data HDM", &data_disk_path),
        ],
    )?;
    let resolved_image = resolve_output_path(&image_output)?;
    if let Some(bps_output) = &bps_output {
        ensure_output_parent(bps_output)?;
        ensure_distinct_output(
            bps_output,
            &[
                ("Game HDM", &game_disk_path),
                ("Demo HDM", &demo_disk_path),
                ("Data HDM", &data_disk_path),
            ],
        )?;
        let resolved_bps = resolve_output_path(bps_output)?;
        if resolved_image == resolved_bps {
            bail!("--image-output and --bps-output resolve to the same path");
        }
    }

    let dat_translations = translations_dir.join("dat");
    let cutscene_translations = translations_dir.join("cutscene");

    println!(
        "full build media: {} ({}) + exact Demo/Data",
        media.game.label, media.id,
    );
    let scratch = ScratchDir::near(&image_output)?;
    let fonts = pc98_madou_ars::font_build::generate_full_build_fonts(
        &font_profile,
        &translations_dir,
        &media.id,
        allow_needs_review,
        &scratch.path().join("font"),
    )
    .context("full build font derivation")?;
    println!(
        "derived {} renderer glyphs and cutscene phases [{}] from {}",
        fonts.renderer_glyphs,
        fonts
            .cutscene_glyphs
            .iter()
            .map(|(phase, count)| format!("{phase}:{count}"))
            .collect::<Vec<_>>()
            .join(", "),
        font_profile.display(),
    );
    // Build from the exact byte snapshots that passed identity validation. This
    // avoids reopening mutable caller paths between validation and the stages.
    let game_input = scratch.path().join("00-game.hdm");
    std::fs::write(&game_input, &game).context("stage validated Game HDM")?;
    let overlay_image = scratch.path().join("01-overlay.hdm");
    let gameplay_image = scratch.path().join("02-gameplay.hdm");
    let full_image = scratch.path().join("03-full.hdm");

    build_char_disk(
        game_input,
        translations_dir,
        fonts.renderer_bin,
        fonts.renderer_json.clone(),
        allow_needs_review,
        overlay_image.clone(),
    )
    .context("full build stage 1/3: scenario overlay")?;
    patch_disk_dats(
        overlay_image,
        dat_translations,
        fonts.renderer_json,
        allow_needs_review,
        gameplay_image.clone(),
    )
    .context("full build stage 2/3: gameplay DAT")?;
    build_cutscene_disk(
        gameplay_image,
        cutscene_raw_dir,
        cutscene_translations,
        fonts.cutscene_dir,
        font_profile.to_path_buf(),
        allow_needs_review,
        full_image.clone(),
    )
    .context("full build stage 3/3: cutscenes")?;

    let target = std::fs::read(&full_image)
        .with_context(|| format!("read composed image {}", full_image.display()))?;
    std::fs::write(&image_output, &target)
        .with_context(|| format!("write composed HDM {}", image_output.display()))?;

    println!(
        "full target: {} bytes SHA-256 {} -> {}",
        target.len(),
        pc98_madou_ars::media_identity::sha256_hex(&target),
        image_output.display()
    );
    if let Some(bps_output) = bps_output {
        let created = pc98_madou_ars::release_patch::create_release_patch(&game, &target)
            .context("create and self-apply full-build BPS")?;
        std::fs::write(&bps_output, &created.patch)
            .with_context(|| format!("write BPS {}", bps_output.display()))?;
        println!(
            "BPS: {} bytes SHA-256 {} (source={:08x} target={:08x} patch={:08x}) -> {}",
            created.patch.len(),
            pc98_madou_ars::release_patch::sha256_hex(&created.patch),
            created.info.source_crc32,
            created.info.target_crc32,
            created.info.patch_crc32,
            bps_output.display()
        );
    }
    Ok(())
}

fn apply_two_disk_boot(
    game_disk_path: PathBuf,
    demo_disk_path: PathBuf,
    data_disk_path: PathBuf,
    output_path: PathBuf,
) -> Result<()> {
    ensure_output_parent(&output_path)?;
    ensure_distinct_output(
        &output_path,
        &[
            ("Game HDM", &game_disk_path),
            ("Demo HDM", &demo_disk_path),
            ("Data HDM", &data_disk_path),
        ],
    )?;
    let game = std::fs::read(&game_disk_path)
        .with_context(|| format!("read Game HDM {}", game_disk_path.display()))?;
    let demo = std::fs::read(&demo_disk_path)
        .with_context(|| format!("read exact Demo HDM {}", demo_disk_path.display()))?;
    let data = std::fs::read(&data_disk_path)
        .with_context(|| format!("read exact Data HDM {}", data_disk_path.display()))?;
    let built = pc98_madou_ars::two_disk_boot::build_game_disk(&game, &demo, &data)?;
    write_output_disk(&output_path, &built.image)?;
    println!(
        "two-disk boot: {} Game, TC.CNS {}, {} {}, AUTOEXEC.BAT {} -> {} (SHA-256 {})",
        built.character_id,
        if built.tc_cns_added {
            "added"
        } else {
            "reused"
        },
        built.opening_music_file,
        if built.opening_music_added {
            "added"
        } else {
            "reused"
        },
        if built.autoexec_rewritten {
            "rewritten"
        } else {
            "already selected"
        },
        output_path.display(),
        pc98_madou_ars::media_identity::sha256_hex(&built.image),
    );
    Ok(())
}

struct ReleaseSetCharacterInput {
    id: &'static str,
    game: Vec<u8>,
    data: Vec<u8>,
}

#[allow(clippy::too_many_arguments)]
fn build_release_set(
    demo_disk_path: PathBuf,
    arle_game_disk_path: PathBuf,
    arle_data_disk_path: PathBuf,
    rulue_game_disk_path: PathBuf,
    rulue_data_disk_path: PathBuf,
    schezo_game_disk_path: PathBuf,
    schezo_data_disk_path: PathBuf,
    translations_dir: PathBuf,
    cutscene_raw_dir: PathBuf,
    font_profile_path: PathBuf,
    assets_dir: PathBuf,
    demo_font_profile_path: PathBuf,
    allow_needs_review: bool,
    output_dir: PathBuf,
) -> Result<()> {
    if output_dir.exists() {
        bail!(
            "release-set output directory must not already exist: {}",
            output_dir.display()
        );
    }
    ensure_output_parent(&output_dir)?;

    let manifest = pc98_madou_ars::media_identity::load_build_media()?;
    let demo = std::fs::read(&demo_disk_path)
        .with_context(|| format!("read exact Demo HDM {}", demo_disk_path.display()))?;
    pc98_madou_ars::media_identity::validate_disk(&demo, &manifest.demo)
        .context("release-set Demo identity")?;

    let requested = [
        ("arle", arle_game_disk_path, arle_data_disk_path),
        ("rulue", rulue_game_disk_path, rulue_data_disk_path),
        ("schezo", schezo_game_disk_path, schezo_data_disk_path),
    ];
    let mut characters = Vec::with_capacity(requested.len());
    for (id, game_path, data_path) in requested {
        let media = manifest
            .characters
            .iter()
            .find(|character| character.id == id)
            .with_context(|| format!("build-media manifest has no {id} entry"))?
            .clone();
        let game = std::fs::read(&game_path)
            .with_context(|| format!("read exact {id} Game HDM {}", game_path.display()))?;
        let data = std::fs::read(&data_path)
            .with_context(|| format!("read exact {id} Data HDM {}", data_path.display()))?;
        pc98_madou_ars::media_identity::validate_disk(&game, &media.game)
            .with_context(|| format!("release-set {id} Game identity"))?;
        pc98_madou_ars::media_identity::validate_disk(&data, &media.data)
            .with_context(|| format!("release-set {id} Data identity"))?;
        characters.push(ReleaseSetCharacterInput { id, game, data });
    }

    // Snapshot every exact validated input before any child stage reopens it.
    // Final publication remains absent until all seven protocol packages and
    // their one enclosing patch set have passed self-application and inspection.
    let scratch = ScratchDir::near(&output_dir)?;
    let input_dir = scratch.path().join("inputs");
    let product_dir = scratch.path().join("products");
    std::fs::create_dir_all(&input_dir).context("create release-set input snapshots")?;
    std::fs::create_dir_all(&product_dir).context("create release-set products")?;
    let demo_snapshot = input_dir.join("demo.hdm");
    std::fs::write(&demo_snapshot, &demo).context("snapshot exact Demo HDM")?;

    build_demo_graphics(
        demo_snapshot.clone(),
        assets_dir.clone(),
        demo_font_profile_path,
        product_dir.join("demo.hdm"),
        None,
        None,
        None,
    )
    .context("release-set shared Demo graphics product")?;
    for character in &characters {
        let game_snapshot = input_dir.join(format!("{}-game.hdm", character.id));
        let data_snapshot = input_dir.join(format!("{}-data.hdm", character.id));
        std::fs::write(&game_snapshot, &character.game)
            .with_context(|| format!("snapshot exact {} Game HDM", character.id))?;
        std::fs::write(&data_snapshot, &character.data)
            .with_context(|| format!("snapshot exact {} Data HDM", character.id))?;
        build_full_patch(
            game_snapshot,
            demo_snapshot.clone(),
            data_snapshot.clone(),
            translations_dir.clone(),
            cutscene_raw_dir.clone(),
            font_profile_path.clone(),
            allow_needs_review,
            product_dir.join(format!("{}-game.hdm", character.id)),
            None,
        )
        .with_context(|| format!("release-set {} character product", character.id))?;
        let game_product_path = product_dir.join(format!("{}-game.hdm", character.id));
        let game_product = std::fs::read(&game_product_path)
            .with_context(|| format!("read composed {} Game HDM", character.id))?;
        let profile = pc98_madou_ars::character_build::profile_for_id(character.id)
            .with_context(|| format!("no character build profile for {}", character.id))?;
        let extent = pc98_madou_ars::character_build::validate_normal_selector_overlay_extent(
            &character.game,
            &game_product,
            profile,
        )
        .with_context(|| {
            format!(
                "release-set {} normal-selector GAME ownership",
                character.id
            )
        })?;
        println!(
            "normal-selector {} extent: source-exact 0x{:X} decoded bytes",
            profile.game_overlay, extent.source_decoded_bytes
        );
        build_character_data_graphics(CharacterDataGraphicsBuild {
            data_disk: data_snapshot,
            assets_dir: assets_dir.clone(),
            font_profile: font_profile_path.clone(),
            allow_needs_review,
            output: product_dir.join(format!("{}-data.hdm", character.id)),
            bps_output: None,
            title_preview: None,
            arle_opening_preview: None,
        })
        .with_context(|| format!("release-set {} Data graphics product", character.id))?;
    }

    let demo_content =
        std::fs::read(product_dir.join("demo.hdm")).context("read composed Demo HDM")?;
    let mut packages = vec![pc98_madou_ars::protocol_patch::create_protocol_package(
        pc98_madou_ars::protocol_patch::ProtocolPackageSpec {
            key: "demo",
            label: "데모 디스크",
            title: "마도물어 A.R.S 한글패치 - 데모 디스크",
            output_filename: "Madou Monogatari A.R.S KR (Demo disk).hdm",
            source: &demo,
            content: &demo_content,
        },
    )?];
    for character in &characters {
        let (display_name, game_label, data_label, game_output, data_output) = match character.id {
            "arle" => (
                "아르르",
                "아르르 게임 디스크",
                "아르르 데이터 디스크",
                "Madou Monogatari A.R.S KR (Arle Game disk).hdm",
                "Madou Monogatari A.R.S KR (Arle Data disk).hdm",
            ),
            "rulue" => (
                "루루",
                "루루 게임 디스크",
                "루루 데이터 디스크",
                "Madou Monogatari A.R.S KR (Rulue Game disk).hdm",
                "Madou Monogatari A.R.S KR (Rulue Data disk).hdm",
            ),
            "schezo" => (
                "셰죠",
                "셰죠 게임 디스크",
                "셰죠 데이터 디스크",
                "Madou Monogatari A.R.S KR (Schezo Game disk).hdm",
                "Madou Monogatari A.R.S KR (Schezo Data disk).hdm",
            ),
            id => bail!("unsupported release-set character {id}"),
        };
        let game_content = std::fs::read(product_dir.join(format!("{}-game.hdm", character.id)))
            .with_context(|| format!("read composed {} Game HDM", character.id))?;
        let data_content = std::fs::read(product_dir.join(format!("{}-data.hdm", character.id)))
            .with_context(|| format!("read composed {} Data HDM", character.id))?;
        let game_title = format!("마도물어 A.R.S 한글패치 - {display_name} 게임 디스크");
        let data_title = format!("마도물어 A.R.S 한글패치 - {display_name} 데이터 디스크");
        packages.push(pc98_madou_ars::protocol_patch::create_protocol_package(
            pc98_madou_ars::protocol_patch::ProtocolPackageSpec {
                key: &format!("{}-game", character.id),
                label: game_label,
                title: &game_title,
                output_filename: game_output,
                source: &character.game,
                content: &game_content,
            },
        )?);
        packages.push(pc98_madou_ars::protocol_patch::create_protocol_package(
            pc98_madou_ars::protocol_patch::ProtocolPackageSpec {
                key: &format!("{}-data", character.id),
                label: data_label,
                title: &data_title,
                output_filename: data_output,
                source: &character.data,
                content: &data_content,
            },
        )?);
    }
    for package in &packages {
        println!(
            "protocol member {}: {} retained, {} patched, {} bytes SHA-256 {}, target SHA-256 {}",
            package.key,
            package.retained_files,
            package.patched_files,
            package.bytes.len(),
            pc98_madou_ars::media_identity::sha256_hex(&package.bytes),
            package.target_sha256,
        );
    }
    let patch_set = pc98_madou_ars::protocol_patch::create_protocol_patch_set(packages)?;
    let artifact_name = format!("madou-ars-kr-patch-{}.zip", env!("CARGO_PKG_VERSION"));

    let publish_dir = scratch.path().join("publish");
    std::fs::create_dir(&publish_dir).context("create staged release-set output")?;
    let artifact_path = publish_dir.join(&artifact_name);
    std::fs::write(&artifact_path, &patch_set)
        .with_context(|| format!("stage protocol patch set {}", artifact_path.display()))?;
    std::fs::rename(&publish_dir, &output_dir).with_context(|| {
        format!(
            "atomically publish release-set output {}",
            output_dir.display()
        )
    })?;

    println!(
        "complete A.R.S protocol patch set: validated 7 exact input HDMs; published {} bytes SHA-256 {} -> {}",
        patch_set.len(),
        pc98_madou_ars::media_identity::sha256_hex(&patch_set),
        output_dir.join(artifact_name).display()
    );
    Ok(())
}

struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    fn near(output: &Path) -> Result<Self> {
        let parent = output
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let parent = std::fs::canonicalize(parent)
            .with_context(|| format!("resolve scratch parent for {}", output.display()))?;
        for attempt in 0..100u32 {
            let path = parent.join(format!(
                ".pc98-madou-ars-build-{}-{attempt}",
                std::process::id()
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("create scratch directory {}", path.display()));
                }
            }
        }
        bail!("could not allocate a unique full-build scratch directory")
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.path) {
            eprintln!(
                "warning: failed to remove build scratch {}: {error}",
                self.path.display()
            );
        }
    }
}

fn ensure_output_parent(output: &Path) -> Result<()> {
    if let Some(parent) = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create output directory {}", parent.display()))?;
    }
    Ok(())
}

fn ensure_distinct_output(output: &Path, inputs: &[(&str, &Path)]) -> Result<()> {
    let resolved_output = resolve_output_path(output)?;
    for (label, input) in inputs {
        let resolved_input = std::fs::canonicalize(input)
            .with_context(|| format!("resolve {label} path {}", input.display()))?;
        if resolved_output == resolved_input {
            bail!(
                "output {} resolves to the same file as {label} {}",
                output.display(),
                input.display()
            );
        }
    }
    Ok(())
}

fn resolve_output_path(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return std::fs::canonicalize(path)
            .with_context(|| format!("resolve output path {}", path.display()));
    }
    let file_name = path
        .file_name()
        .with_context(|| format!("output path has no file name: {}", path.display()))?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    let resolved_parent = std::fs::canonicalize(parent.unwrap_or_else(|| Path::new(".")))
        .with_context(|| format!("resolve output parent for {}", path.display()))?;
    Ok(resolved_parent.join(file_name))
}

#[cfg(test)]
mod release_output_path_tests {
    use super::*;

    #[test]
    fn rejects_an_input_reached_through_parent_components() {
        let root =
            std::env::temp_dir().join(format!("pc98-madou-ars-output-path-{}", std::process::id()));
        let nested = root.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        let source = root.join("source.hdm");
        std::fs::write(&source, b"source").unwrap();
        let disguised = nested.join("..").join("source.hdm");

        let error = ensure_distinct_output(&disguised, &[("source HDM", source.as_path())])
            .unwrap_err()
            .to_string();
        assert!(error.contains("same file as source HDM"));
        std::fs::remove_dir_all(root).unwrap();
    }
}

fn disk_info(disk_path: PathBuf) -> Result<()> {
    let disk =
        std::fs::read(&disk_path).with_context(|| format!("read {}", disk_path.display()))?;
    let hdm = pc98_madou_ars::hdm::HdmFile::parse(&disk)
        .with_context(|| format!("parse {}", disk_path.display()))?;
    let volume = pc98_madou_ars::fat12::Fat12Volume::open(hdm.as_bytes())
        .with_context(|| format!("open FAT12 {}", disk_path.display()))?;
    let geometry = hdm.geometry;
    let bpb = volume.bpb;

    println!("File: {}", disk_path.display());
    println!(
        "HDM:  {} cylinders, {} heads, {} sectors/track, {} bytes/sector",
        geometry.cylinders, geometry.heads, geometry.sectors_per_track, geometry.sector_size
    );
    println!(
        "FAT12: {} sectors, {} sector(s)/cluster, {} FAT(s), {} root entries",
        bpb.total_sectors, bpb.sectors_per_cluster, bpb.num_fats, bpb.root_dir_entries
    );
    println!("Files: {}", volume.list_files().len());
    Ok(())
}

fn disk_files(disk_path: PathBuf) -> Result<()> {
    let disk =
        std::fs::read(&disk_path).with_context(|| format!("read {}", disk_path.display()))?;
    let hdm = pc98_madou_ars::hdm::HdmFile::parse(&disk)
        .with_context(|| format!("parse {}", disk_path.display()))?;
    let volume = pc98_madou_ars::fat12::Fat12Volume::open(hdm.as_bytes())
        .with_context(|| format!("open FAT12 {}", disk_path.display()))?;
    let files = volume.list_files();

    println!("File: {}", disk_path.display());
    println!("Total: {} files", files.len());
    println!(
        "{:<14} {:>10}  {:>5}  attr  date        time",
        "name", "size", "clust"
    );
    for entry in files {
        let (year, month, day) = decode_fat_date(entry.date);
        let (hour, minute, _) = decode_fat_time(entry.time);
        println!(
            "{:<14} {:>10}  {:>5}  {:<4}  {:04}-{:02}-{:02}  {:02}:{:02}",
            entry.name,
            entry.size,
            entry.first_cluster,
            format_fat_attrs(entry.attr),
            year,
            month,
            day,
            hour,
            minute
        );
    }
    Ok(())
}

fn extract_file(disk_path: PathBuf, name: &str, output_path: PathBuf) -> Result<()> {
    ensure_output_parent(&output_path)?;
    ensure_distinct_output(&output_path, &[("source HDM", disk_path.as_path())])?;
    let disk = std::fs::read(&disk_path)
        .with_context(|| format!("read extraction disk {}", disk_path.display()))?;
    let bytes = pc98_madou_ars::read_fat12_file_from_hdm(&disk, name)?;
    std::fs::write(&output_path, &bytes)
        .with_context(|| format!("write extracted file {}", output_path.display()))?;
    println!(
        "extracted {name} ({} bytes, SHA-256 {}) from {} -> {}",
        bytes.len(),
        pc98_madou_ars::media_identity::sha256_hex(&bytes),
        disk_path.display(),
        output_path.display()
    );
    Ok(())
}

fn render_screen_resource(disk_path: PathBuf, name: &str, output_path: PathBuf) -> Result<()> {
    ensure_output_parent(&output_path)?;
    ensure_distinct_output(&output_path, &[("source HDM", disk_path.as_path())])?;
    let disk = std::fs::read(&disk_path)
        .with_context(|| format!("read screen-resource disk {}", disk_path.display()))?;
    let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, name)?;
    let rendered = pc98_madou_ars::graphics_resource::render_named_screen_resource(name, &packed)?;
    let bmp = pc98_madou_ars::graphics_resource::encode_bmp24(
        pc98_madou_ars::graphics_resource::SCREEN_WIDTH,
        pc98_madou_ars::graphics_resource::SCREEN_HEIGHT,
        &rendered.rgb,
    )?;
    std::fs::write(&output_path, bmp)
        .with_context(|| format!("write diagnostic BMP {}", output_path.display()))?;
    println!(
        "rendered {name}: {} streams {:?}, metadata tails {:?}, companion streams {:?}, companion audio {:?}, unrendered ranges {:?}, layout {}; wrote {}",
        rendered.stream_sizes.len(),
        rendered.stream_sizes,
        rendered.metadata_tail_bytes,
        rendered.companion_stream_sizes,
        rendered.companion_audio,
        rendered.unrendered_ranges,
        rendered.layout.label(),
        output_path.display()
    );
    Ok(())
}

fn render_screen_catalog(
    disk_path: PathBuf,
    extensions: &[String],
    output_dir: PathBuf,
) -> Result<()> {
    use std::fmt::Write as _;

    let extensions = extensions
        .iter()
        .map(|extension| extension.trim_start_matches('.').to_ascii_uppercase())
        .collect::<std::collections::BTreeSet<_>>();
    if extensions.is_empty() || extensions.iter().any(String::is_empty) {
        bail!("--extensions must contain at least one non-empty extension");
    }
    std::fs::create_dir_all(&output_dir)
        .with_context(|| format!("create screen catalog directory {}", output_dir.display()))?;
    let index_path = output_dir.join("index.html");
    let manifest_path = output_dir.join("manifest.json");
    ensure_distinct_output(&index_path, &[("source HDM", disk_path.as_path())])?;
    ensure_distinct_output(&manifest_path, &[("source HDM", disk_path.as_path())])?;

    let disk = std::fs::read(&disk_path)
        .with_context(|| format!("read screen-catalog disk {}", disk_path.display()))?;
    let hdm = pc98_madou_ars::hdm::HdmFile::parse(&disk)
        .with_context(|| format!("parse screen-catalog disk {}", disk_path.display()))?;
    let volume = pc98_madou_ars::fat12::Fat12Volume::open(hdm.as_bytes())
        .with_context(|| format!("open FAT12 screen-catalog disk {}", disk_path.display()))?;

    let mut rendered_records = Vec::new();
    let mut skipped_records = Vec::new();
    for entry in volume.list_files() {
        let extension = entry
            .name
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_uppercase());
        if !extension
            .as_ref()
            .is_some_and(|extension| extensions.contains(extension))
        {
            continue;
        }
        let packed = volume
            .read_entry(&entry)
            .with_context(|| format!("read {} from {}", entry.name, disk_path.display()))?;
        match pc98_madou_ars::graphics_resource::render_named_screen_resource(&entry.name, &packed)
        {
            Ok(rendered) => {
                let image_name = format!("{}.bmp", entry.name);
                let bmp = pc98_madou_ars::graphics_resource::encode_bmp24(
                    pc98_madou_ars::graphics_resource::SCREEN_WIDTH,
                    pc98_madou_ars::graphics_resource::SCREEN_HEIGHT,
                    &rendered.rgb,
                )?;
                std::fs::write(output_dir.join(&image_name), bmp).with_context(|| {
                    format!(
                        "write screen-catalog image {}",
                        output_dir.join(&image_name).display()
                    )
                })?;
                let companion_audio = rendered.companion_audio.as_ref().map(|bank| {
                    json!({
                        "format": "BSAMP length-prefixed sample bank",
                        "tracks": bank.tracks.iter().map(|track| json!({
                            "offset": track.offset,
                            "payload_offset": track.payload_offset,
                            "payload_bytes": track.payload_bytes,
                        })).collect::<Vec<_>>(),
                        "trailing_bytes": bank.trailing_bytes,
                    })
                });
                rendered_records.push(json!({
                    "name": entry.name,
                    "image": image_name,
                    "layout": rendered.layout.label(),
                    "stream_sizes": rendered.stream_sizes,
                    "metadata_tail_bytes": rendered.metadata_tail_bytes,
                    "companion_stream_sizes": rendered.companion_stream_sizes,
                    "companion_audio": companion_audio,
                    "unrendered_ranges": rendered.unrendered_ranges.iter().map(|range| json!({
                        "offset": range.offset,
                        "bytes": range.bytes,
                    })).collect::<Vec<_>>(),
                    "rgb_sha256": pc98_madou_ars::media_identity::sha256_hex(&rendered.rgb),
                }));
            }
            Err(error) => skipped_records.push(json!({
                "name": entry.name,
                "reason": error.to_string(),
            })),
        }
    }
    rendered_records.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    skipped_records.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));

    let manifest = json!({
        "schema": "pc98_madou_ars.screen_catalog.v1",
        "source": {
            "path": disk_path.display().to_string(),
            "size": disk.len(),
            "sha256": pc98_madou_ars::media_identity::sha256_hex(&disk),
        },
        "extensions": extensions,
        "rendered": rendered_records,
        "skipped": skipped_records,
    });
    std::fs::write(
        &manifest_path,
        serde_json::to_vec_pretty(&manifest).context("serialize screen catalog manifest")?,
    )
    .with_context(|| format!("write screen catalog manifest {}", manifest_path.display()))?;

    let rendered = manifest["rendered"]
        .as_array()
        .context("screen catalog rendered records are not an array")?;
    let skipped = manifest["skipped"]
        .as_array()
        .context("screen catalog skipped records are not an array")?;
    let mut html = String::from(
        "<!doctype html><meta charset=\"utf-8\"><title>A.R.S screen catalog</title>\n\
         <style>body{font:14px system-ui;background:#181818;color:#eee;margin:20px}\
         h1,h2{font-weight:600}.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(320px,1fr));gap:18px}\
         figure{margin:0;background:#282828;padding:10px}img{width:100%;height:auto;image-rendering:pixelated}\
         figcaption{margin-top:8px}code{color:#9ee}table{border-collapse:collapse}td,th{padding:5px 8px;border:1px solid #555;text-align:left}</style>\n",
    );
    writeln!(
        html,
        "<h1>A.R.S screen catalog</h1><p>Source: <code>{}</code></p><div class=\"grid\">",
        html_escape(&disk_path.display().to_string())
    )?;
    for record in rendered {
        let name = record["name"].as_str().unwrap_or("unknown");
        let image = record["image"].as_str().unwrap_or("");
        let layout = record["layout"].as_str().unwrap_or("unknown");
        writeln!(
            html,
            "<figure><img src=\"{}\" alt=\"{}\"><figcaption><code>{}</code><br>{}</figcaption></figure>",
            html_escape(image),
            html_escape(name),
            html_escape(name),
            html_escape(layout),
        )?;
    }
    html.push_str("</div><h2>Skipped by the proven-layout gate</h2><table><tr><th>Resource</th><th>Reason</th></tr>\n");
    for record in skipped {
        writeln!(
            html,
            "<tr><td><code>{}</code></td><td>{}</td></tr>",
            html_escape(record["name"].as_str().unwrap_or("unknown")),
            html_escape(record["reason"].as_str().unwrap_or("unknown")),
        )?;
    }
    html.push_str("</table>\n");
    std::fs::write(&index_path, html)
        .with_context(|| format!("write screen catalog index {}", index_path.display()))?;
    println!(
        "screen catalog rendered {} resource(s), skipped {}; wrote {} and {}",
        rendered.len(),
        skipped.len(),
        index_path.display(),
        manifest_path.display()
    );
    Ok(())
}

fn build_demo_title(
    demo_disk_path: PathBuf,
    assets_dir: PathBuf,
    output_path: PathBuf,
    preview_path: Option<PathBuf>,
) -> Result<()> {
    ensure_output_parent(&output_path)?;
    ensure_distinct_output(
        &output_path,
        &[("source Demo HDM", demo_disk_path.as_path())],
    )?;
    if let Some(preview_path) = &preview_path {
        ensure_output_parent(preview_path)?;
        ensure_distinct_output(
            preview_path,
            &[("source Demo HDM", demo_disk_path.as_path())],
        )?;
        if resolve_output_path(preview_path)? == resolve_output_path(&output_path)? {
            bail!("title preview and patched Demo HDM outputs must be distinct");
        }
    }

    let mut disk = std::fs::read(&demo_disk_path)
        .with_context(|| format!("read exact Demo HDM {}", demo_disk_path.display()))?;
    let source_packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "OP20.CNS")?;
    let build = pc98_madou_ars::demo_title::build_demo_title(&source_packed, &assets_dir)?;
    let replace =
        pc98_madou_ars::fat12_add::replace_file_grow(&mut disk, "OP20.CNS", &build.packed)?;
    let readback = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "OP20.CNS")?;
    if readback != build.packed {
        bail!("patched Demo HDM OP20.CNS packed readback differs from compiler output");
    }
    let decoded = pc98_madou_ars::overlay_lz::decode_overlay_lz(&readback)?;
    if decoded.bytes_consumed != readback.len() || decoded.output != build.decoded {
        bail!("patched Demo HDM OP20.CNS decoded readback differs from compiler output");
    }
    write_output_disk(&output_path, &disk)?;

    if let Some(preview_path) = preview_path {
        let bmp = pc98_madou_ars::graphics_resource::encode_bmp24(
            pc98_madou_ars::graphics_resource::DEMO_LINEAR_WIDTH,
            pc98_madou_ars::graphics_resource::DEMO_LINEAR_HEIGHT,
            &build.preview_rgb,
        )?;
        std::fs::write(&preview_path, bmp)
            .with_context(|| format!("write title preview {}", preview_path.display()))?;
    }

    println!(
        "rebuilt PC-98-source title OP20.CNS: exact text 魔導傳記, matched medallions 마/도/전/기, suffix A.R.S.; {} indexed pixels changed, {} original pixels protected, {} source-background pixels preserved, {} source-logo pixels replaced with background; master SHA-256 {}; {} -> {} packed bytes, {} clusters; wrote {}",
        build.changed_pixels,
        build.protected_pixels,
        build.source_background_pixels_preserved,
        build.source_logo_pixels_replaced_with_background,
        build.master_sha256,
        source_packed.len(),
        build.packed.len(),
        replace.clusters,
        output_path.display()
    );
    Ok(())
}

struct CharacterDataGraphicsBuild {
    data_disk: PathBuf,
    assets_dir: PathBuf,
    font_profile: PathBuf,
    allow_needs_review: bool,
    output: PathBuf,
    bps_output: Option<PathBuf>,
    title_preview: Option<PathBuf>,
    arle_opening_preview: Option<PathBuf>,
}

fn build_character_data_graphics(request: CharacterDataGraphicsBuild) -> Result<()> {
    let CharacterDataGraphicsBuild {
        data_disk: data_disk_path,
        assets_dir,
        font_profile: font_profile_path,
        allow_needs_review,
        output: output_path,
        bps_output: bps_output_path,
        title_preview: preview_path,
        arle_opening_preview: opening_preview_path,
    } = request;
    ensure_output_parent(&output_path)?;
    ensure_distinct_output(
        &output_path,
        &[("source Data HDM", data_disk_path.as_path())],
    )?;
    let resolved_output = resolve_output_path(&output_path)?;
    let resolved_bps = if let Some(path) = &bps_output_path {
        ensure_output_parent(path)?;
        ensure_distinct_output(path, &[("source Data HDM", data_disk_path.as_path())])?;
        let resolved = resolve_output_path(path)?;
        if resolved == resolved_output {
            bail!("scenario-title Data HDM and BPS outputs must be distinct");
        }
        Some(resolved)
    } else {
        None
    };
    if let Some(path) = &preview_path {
        ensure_output_parent(path)?;
        ensure_distinct_output(path, &[("source Data HDM", data_disk_path.as_path())])?;
        let resolved = resolve_output_path(path)?;
        if resolved == resolved_output || resolved_bps.as_ref() == Some(&resolved) {
            bail!("scenario-title preview must be distinct from HDM and BPS outputs");
        }
    }
    if let Some(path) = &opening_preview_path {
        ensure_output_parent(path)?;
        ensure_distinct_output(path, &[("source Data HDM", data_disk_path.as_path())])?;
        let resolved = resolve_output_path(path)?;
        if resolved == resolved_output || resolved_bps.as_ref() == Some(&resolved) {
            bail!("Arle opening preview must be distinct from HDM and BPS outputs");
        }
        if preview_path
            .as_ref()
            .map(|path| resolve_output_path(path))
            .transpose()?
            .as_ref()
            == Some(&resolved)
        {
            bail!("scenario-title and Arle opening previews must be distinct");
        }
    }

    let source_disk = std::fs::read(&data_disk_path)
        .with_context(|| format!("read exact Data HDM {}", data_disk_path.display()))?;
    let source_identity = pc98_madou_ars::media_identity::identify_patch_source(&source_disk)
        .context("identify exact scenario-title Data source")?;
    if !source_identity.id.ends_with("_data") {
        bail!(
            "scenario-title source must be a character Data HDM, got {}",
            source_identity.id
        );
    }

    let mut disk = source_disk.clone();
    let source_packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "ARS_RX.CS")?;
    let build = pc98_madou_ars::scenario_title::build_scenario_title(&source_packed, &assets_dir)?;
    let replace =
        pc98_madou_ars::fat12_add::replace_file_grow(&mut disk, "ARS_RX.CS", &build.packed)?;
    let readback = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "ARS_RX.CS")?;
    if readback != build.packed {
        bail!("patched Data HDM ARS_RX.CS packed readback differs from compiler output");
    }
    let decoded = pc98_madou_ars::overlay_lz::decode_overlay_lz(&readback)?;
    if decoded.bytes_consumed != readback.len() || decoded.output != build.decoded {
        bail!("patched Data HDM ARS_RX.CS decoded readback differs from compiler output");
    }

    let mut opening_preview_rgb = None;
    if source_identity.id == "arle_data" {
        let source_opening = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "OPA_PT1.CNS")?;
        let opening = pc98_madou_ars::arle_opening_graphics::build_arle_opening_exclamation(
            &source_opening,
            &assets_dir,
        )?;
        let opening_replace = pc98_madou_ars::fat12_add::replace_file_grow(
            &mut disk,
            "OPA_PT1.CNS",
            &opening.packed,
        )?;
        let opening_readback = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "OPA_PT1.CNS")?;
        if opening_readback != opening.packed {
            bail!("patched Arle Data HDM OPA_PT1.CNS packed readback differs from compiler output");
        }
        let opening_decoded = pc98_madou_ars::overlay_lz::decode_overlay_lz(&opening_readback)?;
        if opening_decoded.bytes_consumed != opening_readback.len()
            || opening_decoded.output != opening.decoded
        {
            bail!(
                "patched Arle Data HDM OPA_PT1.CNS decoded readback differs from compiler output"
            );
        }
        println!(
            "rebuilt Arle opening OPA_PT1.CNS: 앗; {} indexed pixels changed, {} unique tiles in {:#x}..={:#x}; master SHA-256 {}; {} -> {} packed bytes, {} clusters",
            opening.changed_pixels,
            opening.unique_tiles,
            opening.allocated_tile_start,
            opening.allocated_tile_end,
            opening.master_sha256,
            source_opening.len(),
            opening.packed.len(),
            opening_replace.clusters,
        );
        opening_preview_rgb = Some(opening.preview_rgb);
    } else if opening_preview_path.is_some() {
        bail!("--opening-preview is only valid for the exact Arle Data HDM");
    }

    if source_identity.id == "rulue_data" {
        let source_interlude = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "RTU1.DAT")?;
        let font_profile = pc98_madou_ars::font_build::FontProfile::load(&font_profile_path)
            .context("load Rulue interlude graphics font profile")?;
        let source_credits = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "RMS.DAT")?;
        let credits = pc98_madou_ars::rulue_credit_graphics::build(
            &source_credits,
            &assets_dir,
            &font_profile,
            allow_needs_review,
        )?;
        pc98_madou_ars::fat12_add::replace_file_grow(&mut disk, "RMS.DAT", &credits.packed)?;
        let readback = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "RMS.DAT")?;
        if readback != credits.packed
            || pc98_madou_ars::overlay_lz::decode_overlay_lz(&readback)?.output != credits.decoded
        {
            bail!("RMS.DAT final disk readback differs from compiler output");
        }
        println!(
            "rebuilt Rulue RMS.DAT credits: {} role/company rows",
            credits.surfaces
        );
        let interlude = pc98_madou_ars::rulue_interlude_graphics::build_rulue_interlude_choices(
            &source_interlude,
            &assets_dir,
            &font_profile,
            allow_needs_review,
        )?;
        let interlude_replace =
            pc98_madou_ars::fat12_add::replace_file_grow(&mut disk, "RTU1.DAT", &interlude.packed)?;
        let interlude_readback = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "RTU1.DAT")?;
        if interlude_readback != interlude.packed {
            bail!("patched Rulue Data HDM RTU1.DAT packed readback differs from compiler output");
        }
        let interlude_decoded = pc98_madou_ars::overlay_lz::decode_overlay_lz(&interlude_readback)?;
        if interlude_decoded.bytes_consumed != interlude_readback.len()
            || interlude_decoded.output != interlude.decoded
        {
            bail!("patched Rulue Data HDM RTU1.DAT decoded readback differs from compiler output");
        }
        println!(
            "rebuilt Rulue RTU1.DAT choices: 잔다 / 안 잔다; {} indexed pixels changed, {} pixels protected; {} -> {} packed bytes, {} clusters",
            interlude.changed_pixels,
            interlude.protected_pixels,
            source_interlude.len(),
            interlude.packed.len(),
            interlude_replace.clusters,
        );
    }

    let release_patch = if bps_output_path.is_some() {
        Some(
            pc98_madou_ars::release_patch::create_release_patch(&source_disk, &disk)
                .context("build and self-apply scenario-title Data BPS")?,
        )
    } else {
        None
    };

    write_output_disk(&output_path, &disk)?;
    if let (Some(path), Some(created)) = (&bps_output_path, &release_patch) {
        std::fs::write(path, &created.patch)
            .with_context(|| format!("write scenario-title Data BPS {}", path.display()))?;
    }
    if let Some(path) = preview_path {
        let bmp = pc98_madou_ars::graphics_resource::encode_bmp24(
            pc98_madou_ars::scenario_title::SCENARIO_TITLE_WIDTH,
            pc98_madou_ars::scenario_title::SCENARIO_TITLE_HEIGHT,
            &build.preview_rgb,
        )?;
        std::fs::write(&path, bmp)
            .with_context(|| format!("write scenario-title preview {}", path.display()))?;
    }
    if let (Some(path), Some(rgb)) = (opening_preview_path, opening_preview_rgb) {
        let bmp = pc98_madou_ars::graphics_resource::encode_bmp24(288, 192, &rgb)?;
        std::fs::write(&path, bmp)
            .with_context(|| format!("write Arle opening preview {}", path.display()))?;
    }

    println!(
        "rebuilt shared scenario title ARS_RX.CS on {}: exact text 魔導傳記, matched medallions 마/도/전/기, suffix A.R.S.; {} indexed pixels changed, {} original pixels protected, {} source-background pixels preserved, {} source-logo pixels replaced with background; master SHA-256 {}; {} -> {} packed bytes, {} clusters; wrote {}",
        source_identity.id,
        build.changed_pixels,
        build.protected_pixels,
        build.source_background_pixels_preserved,
        build.source_logo_pixels_replaced_with_background,
        build.master_sha256,
        source_packed.len(),
        build.packed.len(),
        replace.clusters,
        output_path.display()
    );
    if let (Some(path), Some(created)) = (bps_output_path, release_patch) {
        println!(
            "scenario-title Data BPS self-apply verified: {} bytes, SHA-256 {}, source CRC32 {:08x}, target CRC32 {:08x}, patch CRC32 {:08x}; wrote {}",
            created.patch.len(),
            pc98_madou_ars::release_patch::sha256_hex(&created.patch),
            created.info.source_crc32,
            created.info.target_crc32,
            created.info.patch_crc32,
            path.display()
        );
    }
    Ok(())
}

fn build_demo_graphics(
    demo_disk_path: PathBuf,
    assets_dir: PathBuf,
    font_profile_path: PathBuf,
    output_path: PathBuf,
    bps_output_path: Option<PathBuf>,
    title_preview_path: Option<PathBuf>,
    menu_preview_path: Option<PathBuf>,
) -> Result<()> {
    ensure_output_parent(&output_path)?;
    ensure_distinct_output(
        &output_path,
        &[("source Demo HDM", demo_disk_path.as_path())],
    )?;
    let resolved_output = resolve_output_path(&output_path)?;
    let resolved_bps = if let Some(path) = &bps_output_path {
        ensure_output_parent(path)?;
        ensure_distinct_output(path, &[("source Demo HDM", demo_disk_path.as_path())])?;
        let resolved = resolve_output_path(path)?;
        if resolved == resolved_output {
            bail!("Demo HDM and BPS outputs must be distinct");
        }
        Some(resolved)
    } else {
        None
    };
    let mut resolved_previews = Vec::new();
    for (label, path) in [
        ("title preview", title_preview_path.as_ref()),
        ("menu preview", menu_preview_path.as_ref()),
    ] {
        if let Some(path) = path {
            ensure_output_parent(path)?;
            ensure_distinct_output(path, &[("source Demo HDM", demo_disk_path.as_path())])?;
            let resolved = resolve_output_path(path)?;
            if resolved == resolved_output {
                bail!("{label} and patched Demo HDM outputs must be distinct");
            }
            if resolved_bps.as_ref() == Some(&resolved) {
                bail!("{label} and BPS outputs must be distinct");
            }
            if resolved_previews.contains(&resolved) {
                bail!("title and menu preview outputs must be distinct");
            }
            resolved_previews.push(resolved);
        }
    }

    let source_disk = std::fs::read(&demo_disk_path)
        .with_context(|| format!("read exact Demo HDM {}", demo_disk_path.display()))?;
    let mut disk = source_disk.clone();

    let source_title = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "OP20.CNS")?;
    let title = pc98_madou_ars::demo_title::build_demo_title(&source_title, &assets_dir)?;
    let title_replace =
        pc98_madou_ars::fat12_add::replace_file_grow(&mut disk, "OP20.CNS", &title.packed)?;
    let title_readback = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "OP20.CNS")?;
    if title_readback != title.packed {
        bail!("patched Demo HDM OP20.CNS packed readback differs from compiler output");
    }
    let title_decoded = pc98_madou_ars::overlay_lz::decode_overlay_lz(&title_readback)?;
    if title_decoded.bytes_consumed != title_readback.len() || title_decoded.output != title.decoded
    {
        bail!("patched Demo HDM OP20.CNS decoded readback differs from compiler output");
    }

    let font_profile = pc98_madou_ars::font_build::FontProfile::load(&font_profile_path)?;
    let source_menu = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "MU.CNS")?;
    let menu =
        pc98_madou_ars::demo_menu::build_demo_menu_text(&source_menu, &assets_dir, &font_profile)?;
    let menu_replace =
        pc98_madou_ars::fat12_add::replace_file_grow(&mut disk, "MU.CNS", &menu.packed)?;
    let menu_readback = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "MU.CNS")?;
    if menu_readback != menu.packed {
        bail!("patched Demo HDM MU.CNS packed readback differs from compiler output");
    }
    let menu_primary = pc98_madou_ars::overlay_lz::decode_overlay_lz(&menu_readback)?;
    if menu_primary.output != menu.primary_decoded {
        bail!("patched Demo HDM MU.CNS primary readback differs from compiler output");
    }
    let menu_companion = menu_readback
        .get(menu_primary.bytes_consumed..)
        .context("patched Demo HDM MU.CNS lost its companion stream")?;
    if menu_companion != menu.companion_packed {
        bail!("patched Demo HDM MU.CNS changed its packed companion audio stream");
    }

    let source_selector = pc98_madou_ars::read_fat12_file_from_hdm(
        &disk,
        pc98_madou_ars::demo_selector::SELECTOR_OVERLAY,
    )?;
    let source_selector_decoded = pc98_madou_ars::overlay_lz::decode_overlay_lz(&source_selector)?;
    if source_selector_decoded.bytes_consumed != source_selector.len() {
        bail!(
            "{} decoder consumed {} of {} packed bytes",
            pc98_madou_ars::demo_selector::SELECTOR_OVERLAY,
            source_selector_decoded.bytes_consumed,
            source_selector.len()
        );
    }
    let mut selector_decoded = source_selector_decoded.output;
    let selector =
        pc98_madou_ars::demo_selector::install_selector_renderer_load(&mut selector_decoded)?;
    let selector_packed = pc98_madou_ars::overlay_lz::encode_overlay_lz(&selector_decoded);
    let selector_redecoded = pc98_madou_ars::overlay_lz::decode_overlay_lz(&selector_packed)?;
    if selector_redecoded.bytes_consumed != selector_packed.len()
        || selector_redecoded.output != selector_decoded
    {
        bail!("shared normal-selector renderer loader failed LZ round-trip");
    }
    let selector_replace = pc98_madou_ars::fat12_add::replace_file_grow(
        &mut disk,
        pc98_madou_ars::demo_selector::SELECTOR_OVERLAY,
        &selector_packed,
    )?;
    let selector_readback = pc98_madou_ars::read_fat12_file_from_hdm(
        &disk,
        pc98_madou_ars::demo_selector::SELECTOR_OVERLAY,
    )?;
    if selector_readback != selector_packed {
        bail!("patched Demo HDM normal-selector packed readback differs from compiler output");
    }
    let selector_decoded_readback =
        pc98_madou_ars::overlay_lz::decode_overlay_lz(&selector_readback)?;
    if selector_decoded_readback.bytes_consumed != selector_readback.len()
        || selector_decoded_readback.output != selector_decoded
    {
        bail!("patched Demo HDM normal-selector decoded readback differs from compiler output");
    }

    let release_patch = if bps_output_path.is_some() {
        Some(
            pc98_madou_ars::release_patch::create_release_patch(&source_disk, &disk)
                .context("build and self-apply shared Demo BPS")?,
        )
    } else {
        None
    };

    write_output_disk(&output_path, &disk)?;
    if let (Some(path), Some(created)) = (&bps_output_path, &release_patch) {
        std::fs::write(path, &created.patch)
            .with_context(|| format!("write Demo BPS {}", path.display()))?;
    }

    if let Some(path) = title_preview_path {
        let bmp = pc98_madou_ars::graphics_resource::encode_bmp24(
            pc98_madou_ars::graphics_resource::DEMO_LINEAR_WIDTH,
            pc98_madou_ars::graphics_resource::DEMO_LINEAR_HEIGHT,
            &title.preview_rgb,
        )?;
        std::fs::write(&path, bmp)
            .with_context(|| format!("write title preview {}", path.display()))?;
    }
    if let Some(path) = menu_preview_path {
        let rendered = pc98_madou_ars::graphics_resource::render_named_screen_resource(
            "MU.CNS",
            &menu.packed,
        )?;
        let bmp = pc98_madou_ars::graphics_resource::encode_bmp24(
            pc98_madou_ars::graphics_resource::SCREEN_WIDTH,
            pc98_madou_ars::graphics_resource::SCREEN_HEIGHT,
            &rendered.rgb,
        )?;
        std::fs::write(&path, bmp)
            .with_context(|| format!("write menu preview {}", path.display()))?;
    }

    println!(
        "rebuilt shared Demo graphics and normal-selector renderer supply: OP20.CNS title {} -> {} packed bytes ({} changed, {} protected pixels, {} source-background pixels preserved, {} source-logo pixels replaced with background, matched 마/도/전/기 medallions owned by the reviewed master, {} clusters); MU.CNS Japanese prompt only {} -> {} packed bytes ({} changed, {} protected pixels, {} surface, {}-byte packed audio companion exact, {} clusters); {} decoded 0x{:X} -> 0x{:X}, one A/R/S KFONT load at 0x{:04X} ({} clusters); English ARLE/RURUE/SHE-ZO preserved; wrote {}",
        source_title.len(),
        title.packed.len(),
        title.changed_pixels,
        title.protected_pixels,
        title.source_background_pixels_preserved,
        title.source_logo_pixels_replaced_with_background,
        title_replace.clusters,
        source_menu.len(),
        menu.packed.len(),
        menu.changed_pixels,
        menu.protected_pixels,
        menu.surfaces,
        menu.companion_packed.len(),
        menu_replace.clusters,
        pc98_madou_ars::demo_selector::SELECTOR_OVERLAY,
        selector.source_decoded_bytes,
        selector.product_decoded_bytes,
        selector.call_decoded_offset,
        selector_replace.clusters,
        output_path.display(),
    );
    if let (Some(path), Some(created)) = (bps_output_path, release_patch) {
        println!(
            "Demo BPS self-apply verified: {} bytes, SHA-256 {}, source CRC32 {:08x}, target CRC32 {:08x}, patch CRC32 {:08x}; wrote {}",
            created.patch.len(),
            pc98_madou_ars::media_identity::sha256_hex(&created.patch),
            created.info.source_crc32,
            created.info.target_crc32,
            created.info.patch_crc32,
            path.display(),
        );
    }
    Ok(())
}

fn html_escape(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn audit_demo_compositor(demo_disk: PathBuf, output_path: PathBuf) -> Result<()> {
    ensure_output_parent(&output_path)?;
    ensure_distinct_output(&output_path, &[("source Demo HDM", demo_disk.as_path())])?;
    let disk = std::fs::read(&demo_disk)
        .with_context(|| format!("read Demo compositor disk {}", demo_disk.display()))?;
    let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "ARS_DEMO.OVL")?;
    let decoded = pc98_madou_ars::overlay_lz::decode_overlay_lz(&packed)
        .context("decode ARS_DEMO.OVL for compositor audit")?;
    if decoded.bytes_consumed != packed.len() {
        bail!(
            "ARS_DEMO.OVL decoder consumed {} of {} packed bytes",
            decoded.bytes_consumed,
            packed.len()
        );
    }
    let audit = pc98_madou_ars::demo_compositor::audit_demo_compositor(&decoded.output)?;

    let mut per_resource = pc98_madou_ars::demo_compositor::OP_RESOURCES
        .into_iter()
        .map(|resource| (resource, (0usize, 0usize, 0usize)))
        .collect::<std::collections::BTreeMap<_, _>>();
    for blit in &audit.blits {
        if let Some(resource) = blit.resource {
            let counts = per_resource.entry(resource).or_default();
            counts.0 += 1;
            counts.1 += usize::from(blit.source_resolved());
            counts.2 += usize::from(blit.fully_resolved());
        }
    }
    let records = audit
        .blits
        .iter()
        .map(|blit| {
            json!({
                "phase": blit.phase,
                "call_offset": blit.call_offset,
                "call_offset_hex": format!("0x{:04X}", blit.call_offset),
                "routine": blit.routine,
                "routine_offset": blit.routine_offset,
                "routine_offset_hex": format!("0x{:04X}", blit.routine_offset),
                "resource": blit.resource,
                "source_mode": blit.source_mode.label(),
                "arguments": {
                    "si": blit.args.si,
                    "di": blit.args.di,
                    "dx": blit.args.dx,
                    "cx": blit.args.cx,
                    "ds_segment_var": blit.args.ds_segment_var,
                    "source_row_bytes": blit.args.source_row_bytes,
                },
                "source_rect": blit.source_rect.map(rect_json),
                "source_span": blit.source_span.map(|span| json!({
                    "offset": span.offset,
                    "length": span.length,
                    "row_bytes": span.row_bytes,
                    "rows": span.rows,
                    "planes": span.planes,
                })),
                "source_envelope": blit.source_envelope.map(rect_json),
                "destination_rect": blit.destination_rect.map(rect_json),
                "source_resolution": blit.source_resolution_label(),
                "source_unresolved_reason": blit.source_unresolved_reason(),
                "destination_resolution": if blit.destination_resolved() { "static_immediates" } else { "unresolved" },
                "destination_unresolved_reason": blit.destination_unresolved_reason(),
                "runtime_visibility": "unverified",
            })
        })
        .collect::<Vec<_>>();
    let resource_summary = per_resource
        .into_iter()
        .map(|(resource, (calls, source_resolved, fully_resolved))| {
            json!({
                "resource": resource,
                "calls": calls,
                "source_resolved": source_resolved,
                "source_unresolved": calls - source_resolved,
                "fully_resolved": fully_resolved,
            })
        })
        .collect::<Vec<_>>();
    let root = json!({
        "schema": "pc98_madou_ars.demo_compositor_audit.v2",
        "evidence_scope": {
            "source_use": "static",
            "runtime_visibility": "unverified",
        },
        "source": {
            "disk_path": demo_disk.display().to_string(),
            "disk_size": disk.len(),
            "disk_sha256": pc98_madou_ars::media_identity::sha256_hex(&disk),
            "resource": "ARS_DEMO.OVL",
            "packed_size": packed.len(),
            "packed_sha256": pc98_madou_ars::media_identity::sha256_hex(&packed),
            "decoded_size": decoded.output.len(),
            "decoded_sha256": pc98_madou_ars::media_identity::sha256_hex(&decoded.output),
            "logical_load_offset": pc98_madou_ars::demo_compositor::OVERLAY_LOAD_OFFSET,
        },
        "summary": {
            "calls": audit.blits.len(),
            "source_resolved": audit.source_resolved_count(),
            "source_unresolved": audit.source_unresolved_count(),
            "fully_resolved": audit.fully_resolved_count(),
            "resources": resource_summary,
        },
        "blits": records,
    });
    std::fs::write(
        &output_path,
        serde_json::to_vec_pretty(&root).context("serialize Demo compositor audit")?,
    )
    .with_context(|| format!("write Demo compositor audit {}", output_path.display()))?;
    println!(
        "Demo compositor: {} call(s), {} source-resolved, {} source-unresolved, {} fully resolved; wrote {}",
        audit.blits.len(),
        audit.source_resolved_count(),
        audit.source_unresolved_count(),
        audit.fully_resolved_count(),
        output_path.display()
    );
    Ok(())
}

fn rect_json(rect: pc98_madou_ars::demo_compositor::Rect) -> serde_json::Value {
    json!({
        "x": rect.x,
        "y": rect.y,
        "width": rect.width,
        "height": rect.height,
    })
}

fn audit_resources(
    disk_paths: Vec<PathBuf>,
    extensions: &[String],
    min_run: usize,
    min_japanese: usize,
    quiet: bool,
    output_path: PathBuf,
) -> Result<()> {
    let extensions = extensions
        .iter()
        .map(|extension| extension.trim_start_matches('.').to_ascii_uppercase())
        .collect::<std::collections::BTreeSet<_>>();
    if extensions.is_empty() || extensions.iter().any(String::is_empty) {
        bail!("--extensions must contain at least one non-empty extension");
    }
    ensure_output_parent(&output_path)?;
    ensure_distinct_output(
        &output_path,
        &disk_paths
            .iter()
            .map(|disk| ("source HDM", disk.as_path()))
            .collect::<Vec<_>>(),
    )?;

    if !quiet {
        println!(
            "{:<18} {:<14} {:>8}  {:>8}  {:>10}  {:>4}  {:>4}",
            "disk", "resource", "packed", "decoded", "shape", "rawS", "decS"
        );
    }
    let mut disk_records = Vec::with_capacity(disk_paths.len());
    let mut total_resources = 0usize;
    let mut exact_lz_resources = 0usize;
    for disk_path in &disk_paths {
        let disk = std::fs::read(disk_path)
            .with_context(|| format!("read resource-audit disk {}", disk_path.display()))?;
        let hdm = pc98_madou_ars::hdm::HdmFile::parse(&disk)
            .with_context(|| format!("parse resource-audit disk {}", disk_path.display()))?;
        let volume = pc98_madou_ars::fat12::Fat12Volume::open(hdm.as_bytes())
            .with_context(|| format!("open FAT12 resource-audit disk {}", disk_path.display()))?;
        let disk_stem = disk_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("disk");
        let disk_label = disk_stem
            .rsplit_once('(')
            .map(|(_, tail)| tail.trim_end_matches(')'))
            .unwrap_or(disk_stem);
        let mut resources = Vec::new();
        for entry in volume.list_files() {
            let extension = entry
                .name
                .rsplit_once('.')
                .map(|(_, extension)| extension.to_ascii_uppercase());
            if !extension
                .as_ref()
                .is_some_and(|extension| extensions.contains(extension))
            {
                continue;
            }
            let bytes = volume
                .read_entry(&entry)
                .with_context(|| format!("read {} from {}", entry.name, disk_path.display()))?;
            let audit =
                pc98_madou_ars::resource_audit::audit_resource(&bytes, min_run, min_japanese);
            let (decoded_size, shape, decoded_sjis, lz_json) = match &audit.lz {
                Some(lz) => {
                    if lz.exact {
                        exact_lz_resources += 1;
                    }
                    (
                        Some(lz.decoded.size),
                        lz.decoded
                            .screen_planes_32000
                            .map(|planes| format!("{planes}x32000"))
                            .or_else(|| {
                                lz.decoded
                                    .blocks_0x4000
                                    .map(|blocks| format!("{blocks}x16384"))
                            }),
                        Some(lz.decoded.sjis_regions),
                        json!({
                            "exact": lz.exact,
                            "bytes_consumed": lz.bytes_consumed,
                            "trailing_bytes": lz.trailing_bytes,
                            "commands": lz.commands,
                            "streams": lz.streams.iter().map(|stream| json!({
                                "input_offset": stream.input_offset,
                                "bytes_consumed": stream.bytes_consumed,
                                "commands": stream.commands,
                                "decoded_size": stream.decoded_size,
                                "decoded_sha256": stream.decoded_sha256,
                            })).collect::<Vec<_>>(),
                            "decoded": byte_profile_json(&lz.decoded),
                        }),
                    )
                }
                None => (None, None, None, serde_json::Value::Null),
            };
            if !quiet {
                println!(
                    "{:<18} {:<14} {:>8}  {:>8}  {:>10}  {:>4}  {:>4}",
                    disk_label.chars().take(18).collect::<String>(),
                    entry.name,
                    audit.packed.size,
                    decoded_size
                        .map(|size| size.to_string())
                        .unwrap_or_else(|| "-".to_owned()),
                    shape.unwrap_or_else(|| "-".to_owned()),
                    audit.packed.sjis_regions,
                    decoded_sjis
                        .map(|count| count.to_string())
                        .unwrap_or_else(|| "-".to_owned()),
                );
            }
            resources.push(json!({
                "name": entry.name,
                "packed": byte_profile_json(&audit.packed),
                "lz": lz_json,
            }));
            total_resources += 1;
        }
        disk_records.push(json!({
            "path": disk_path.display().to_string(),
            "size": disk.len(),
            "sha256": pc98_madou_ars::media_identity::sha256_hex(&disk),
            "resources": resources,
        }));
    }

    let root = json!({
        "schema": "pc98_madou_ars.resource_audit.v1",
        "extensions": extensions,
        "sjis_threshold": {
            "min_run": min_run,
            "min_japanese": min_japanese,
        },
        "summary": {
            "disks": disk_records.len(),
            "resources": total_resources,
            "exact_lz_resources": exact_lz_resources,
        },
        "disks": disk_records,
    });
    std::fs::write(
        &output_path,
        serde_json::to_vec_pretty(&root).context("serialize resource audit")?,
    )
    .with_context(|| format!("write resource audit {}", output_path.display()))?;
    println!(
        "audited {total_resources} resources across {} disks ({exact_lz_resources} exact LZ); wrote {}",
        disk_paths.len(),
        output_path.display()
    );
    Ok(())
}

fn byte_profile_json(profile: &pc98_madou_ars::resource_audit::ByteProfile) -> serde_json::Value {
    json!({
        "size": profile.size,
        "sha256": profile.sha256,
        "zero_bytes": profile.zero_bytes,
        "ff_bytes": profile.ff_bytes,
        "byte_55": profile.byte_55,
        "byte_aa": profile.byte_aa,
        "distinct_bytes": profile.distinct_bytes,
        "sjis_regions": profile.sjis_regions,
        "blocks_0x4000": profile.blocks_0x4000,
        "screen_planes_32000": profile.screen_planes_32000,
    })
}

fn sjis_sweep(
    input_path: PathBuf,
    in_disk: Option<&str>,
    min_run: usize,
    min_japanese: usize,
    limit: Option<usize>,
) -> Result<()> {
    let bytes = if let Some(file_name) = in_disk {
        let disk =
            std::fs::read(&input_path).with_context(|| format!("read {}", input_path.display()))?;
        pc98_madou_ars::read_fat12_file_from_hdm(&disk, file_name)?
    } else {
        std::fs::read(&input_path).with_context(|| format!("read {}", input_path.display()))?
    };
    let regions = pc98_madou_ars::sjis_sweep::scan_string_regions(&bytes, min_run, min_japanese);

    println!("File: {}", input_path.display());
    if let Some(file_name) = in_disk {
        println!("Inner: {file_name}");
    }
    println!("Size: {} bytes", bytes.len());
    println!(
        "Regions: {} (min_run={min_run}, min_japanese={min_japanese})",
        regions.len()
    );
    let shown = limit.unwrap_or(regions.len());
    for region in regions.iter().take(shown) {
        let text = pc98_madou_ars::sjis_sweep::decode_region(&bytes[region.start..region.end]);
        println!(
            "  0x{:04X}-0x{:04X} ({:>4}B, {:>3} glyphs, {:>3} Japanese): {}",
            region.start,
            region.end,
            region.end - region.start,
            region.glyph_count,
            region.japanese_count,
            sanitize_console_text(&text)
        );
    }
    if regions.len() > shown {
        println!("  ... ({} more)", regions.len() - shown);
    }
    Ok(())
}

fn format_fat_attrs(attr: u8) -> String {
    [
        if attr & 0x01 != 0 { 'R' } else { '-' },
        if attr & 0x02 != 0 { 'H' } else { '-' },
        if attr & 0x04 != 0 { 'S' } else { '-' },
        if attr & 0x10 != 0 { 'D' } else { '-' },
    ]
    .into_iter()
    .collect()
}

fn decode_fat_date(date: u16) -> (u16, u8, u8) {
    (
        1980 + (date >> 9),
        ((date >> 5) & 0x0F) as u8,
        (date & 0x1F) as u8,
    )
}

fn decode_fat_time(time: u16) -> (u8, u8, u8) {
    (
        (time >> 11) as u8,
        ((time >> 5) & 0x3F) as u8,
        ((time & 0x1F) * 2) as u8,
    )
}

fn sanitize_console_text(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        match ch {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            ch if ch.is_control() => out.push_str(&format!("\\u{{{:04X}}}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out
}

fn build_marker(disk_path: PathBuf, output_path: PathBuf) -> Result<()> {
    let mut disk =
        std::fs::read(&disk_path).with_context(|| format!("read {}", disk_path.display()))?;
    let overlay = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "GAME_A.OVL")?;
    let mut patched_overlay = overlay.clone();

    let from = pc98_madou_ars::sjis_marker::encode_sjis("魔導物語Ａ.Ｒ.Ｓ アルル編の")?;
    let to = pc98_madou_ars::sjis_marker::encode_sjis("魔導物語Ａ.Ｒ.Ｓ テスト編の")?;
    let offset =
        pc98_madou_ars::sjis_marker::replace_first_exact(&mut patched_overlay, &from, &to)?;

    let report = pc98_madou_ars::fat12_replace::replace_file_in_place(
        &mut disk,
        "GAME_A.OVL",
        &patched_overlay,
    )?;
    let gaoo_report = patch_gaoo_boot_prompt(&mut disk)?;

    if let Some(parent) = output_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("mkdir -p {}", parent.display()))?;
    }
    std::fs::write(&output_path, &disk)
        .with_context(|| format!("write {}", output_path.display()))?;

    println!(
        "patched GAME_A.OVL at 0x{offset:04X}; wrote {} ({} bytes, capacity {} bytes)",
        output_path.display(),
        report.bytes,
        report.capacity
    );
    println!(
        "patched GAOO.OVL visible boot prompt ({} bytes, capacity {} bytes)",
        gaoo_report.bytes, gaoo_report.capacity
    );
    Ok(())
}

fn build_hangul_probe(disk_path: PathBuf, output_path: PathBuf) -> Result<()> {
    let mut disk =
        std::fs::read(&disk_path).with_context(|| format!("read {}", disk_path.display()))?;
    let report = pc98_madou_ars::hangul_probe::patch_disk_for_hangul_probe(&mut disk)?;
    write_output_disk(&output_path, &disk)?;

    println!(
        "patched MAIN.COM gaiji registration stub; wrote {} ({} bytes, capacity {} bytes)",
        output_path.display(),
        report.main_report.bytes,
        report.main_report.capacity
    );
    println!(
        "patched GAOO.OVL hangul probe offsets: {:04X?} ({} bytes, capacity {} bytes)",
        report.gaoo_offsets, report.gaoo_report.bytes, report.gaoo_report.capacity
    );
    Ok(())
}

fn build_cutscene_hangul_poc(
    game_disk_path: PathBuf,
    demo_disk_path: PathBuf,
    data_disk_path: PathBuf,
    output_path: PathBuf,
) -> Result<()> {
    use pc98_madou_ars::overlay_lz::{decode_overlay_lz, encode_overlay_lz};

    let mut disk = std::fs::read(&game_disk_path)
        .with_context(|| format!("read {}", game_disk_path.display()))?;
    let demo_disk = std::fs::read(&demo_disk_path)
        .with_context(|| format!("read {}", demo_disk_path.display()))?;
    let data_disk = std::fs::read(&data_disk_path)
        .with_context(|| format!("read {}", data_disk_path.display()))?;

    // Patch the decoded script and make this phase register its own gaiji on the
    // first text-consumer call, after video setup. This proves the phase-local
    // set can replace a previous set instead of depending on one boot-global
    // union.
    let overlay_name = pc98_madou_ars::cutscene_text::SHEZO_OPENING_OVERLAY;
    let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, overlay_name)?;
    let mut decoded = decode_overlay_lz(&packed)?.output;
    let gaiji_report = pc98_madou_ars::cutscene_text::install_overlay_gaiji_registration(
        &mut decoded,
        &[pc98_madou_ars::hangul_probe::GaijiGlyph {
            jis: 0x7621,
            bitmap: pc98_madou_ars::hangul_probe::HANGUL_GA_GLYPH_16X16_1BPP,
        }],
    )?;
    let glyph_offset = pc98_madou_ars::cutscene_text::patch_shezo_opening_one_gaiji(&mut decoded)?;
    let reencoded = encode_overlay_lz(&decoded);
    if decode_overlay_lz(&reencoded)?.output != decoded {
        bail!("re-encoded {overlay_name} did not round-trip; refusing to ship");
    }
    let overlay_report =
        pc98_madou_ars::fat12_add::replace_file_grow(&mut disk, overlay_name, &reencoded)?;
    let overlay_back = pc98_madou_ars::read_fat12_file_from_hdm(&disk, overlay_name)?;
    if decode_overlay_lz(&overlay_back)?.output != decoded {
        bail!("readback of {overlay_name} did not match the cutscene probe");
    }

    // The stock R/S AUTOEXEC points through the wrong character boot path under
    // the two-floppy workaround. Boot the She-zo overlays from A: and stage the
    // two files the verified smoke route needs.
    let autoexec = b"FPLAY.COM\r\nBPLAY.COM\r\nBSAMP.COM\r\nREADJS.COM\r\nMAIN.COM /M D:GAOO.OVL A:GAME_S.OVL A:SHEZO_OP.OVL\r\n";
    pc98_madou_ars::fat12_replace::replace_file_in_place(&mut disk, "AUTOEXEC.BAT", autoexec)?;
    add_file_from_disk(&mut disk, &demo_disk, "TC.CNS")?;
    add_file_from_disk(&mut disk, &data_disk, "MADO-S.DAT")?;

    write_output_disk(&output_path, &disk)?;
    println!(
        "installed one-glyph phase registration in {overlay_name} decoded 0x{:04X}..0x{:04X} (stack 0x{:04X}); patched text at decoded 0x{glyph_offset:04X} ({} -> {} packed bytes, {} clusters); wrote {}",
        gaiji_report.stub_decoded_offset,
        gaiji_report.static_end_logical - 0x100,
        gaiji_report.stack_top,
        packed.len(),
        reencoded.len(),
        overlay_report.clusters,
        output_path.display(),
    );
    Ok(())
}

fn build_arle_boot_smoke(
    game_disk_path: PathBuf,
    demo_disk_path: PathBuf,
    data_disk_path: PathBuf,
    hangul_probe: bool,
    output_path: PathBuf,
) -> Result<()> {
    let mut disk = std::fs::read(&game_disk_path)
        .with_context(|| format!("read {}", game_disk_path.display()))?;
    let demo_disk = std::fs::read(&demo_disk_path)
        .with_context(|| format!("read {}", demo_disk_path.display()))?;
    let data_disk = std::fs::read(&data_disk_path)
        .with_context(|| format!("read {}", data_disk_path.display()))?;

    if hangul_probe {
        let report = pc98_madou_ars::hangul_probe::patch_disk_for_hangul_probe(&mut disk)?;
        println!(
            "patched Hangul probe: MAIN.COM {} bytes, GAOO.OVL offsets {:04X?}",
            report.main_report.bytes, report.gaoo_offsets
        );
    }

    let tc_report = add_file_from_disk(&mut disk, &demo_disk, "TC.CNS")?;
    let mado_a2_report = add_file_from_disk(&mut disk, &data_disk, "MADO-A2.DAT")?;
    write_output_disk(&output_path, &disk)?;

    println!(
        "added {} ({} bytes, {} clusters) and {} ({} bytes, {} clusters); wrote {}",
        tc_report.file_name,
        tc_report.bytes,
        tc_report.clusters,
        mado_a2_report.file_name,
        mado_a2_report.bytes,
        mado_a2_report.clusters,
        output_path.display()
    );
    Ok(())
}

fn build_renderer_hangul_poc(
    game_disk_path: PathBuf,
    demo_disk_path: PathBuf,
    data_disk_path: PathBuf,
    enemy_files: &[String],
    output_path: PathBuf,
) -> Result<()> {
    let mut disk = std::fs::read(&game_disk_path)
        .with_context(|| format!("read {}", game_disk_path.display()))?;
    let demo_disk = std::fs::read(&demo_disk_path)
        .with_context(|| format!("read {}", demo_disk_path.display()))?;
    let data_disk = std::fs::read(&data_disk_path)
        .with_context(|| format!("read {}", data_disk_path.display()))?;

    // 1. Register the one Hangul gaiji glyph at boot (no GAOO text-plane patch,
    //    so the only gaiji reference is the graphics-rendered enemy name).
    let main_com = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "MAIN.COM")?;
    let patched_main = pc98_madou_ars::hangul_probe::patch_main_com_register_gaiji(&main_com)?;
    let main_report =
        pc98_madou_ars::fat12_replace::replace_file_in_place(&mut disk, "MAIN.COM", &patched_main)?;
    println!(
        "registered gaiji via MAIN.COM stub ({} bytes, capacity {} bytes)",
        main_report.bytes, main_report.capacity
    );

    // 2. Overwrite the uncompressed enemy name(s) with the gaiji code, preserving
    //    each field byte length so the runtime buffer assembly is undisturbed.
    for enemy_file in enemy_files {
        let mut enemy = pc98_madou_ars::read_fat12_file_from_hdm(&disk, enemy_file)?;
        let patch = pc98_madou_ars::enemy_text::overwrite_first_enemy_name_with_gaiji(
            &mut enemy,
            pc98_madou_ars::hangul_probe::GAIJI_JIS_7621_SJIS,
        )?;
        let enemy_report =
            pc98_madou_ars::fat12_replace::replace_file_in_place(&mut disk, enemy_file, &enemy)?;
        println!(
            "patched {enemy_file} name '{}' at 0x{:04X}..0x{:04X} with {} gaiji glyph(s) ({} bytes, capacity {} bytes)",
            patch.original_name,
            patch.name_offset,
            patch.name_end_offset,
            patch.glyph_count,
            enemy_report.bytes,
            enemy_report.capacity
        );
    }

    // 3. Stage the two-floppy boot-smoke media workaround.
    let tc_report = add_file_from_disk(&mut disk, &demo_disk, "TC.CNS")?;
    let mado_a2_report = add_file_from_disk(&mut disk, &data_disk, "MADO-A2.DAT")?;
    write_output_disk(&output_path, &disk)?;

    println!(
        "added {} ({} bytes, {} clusters) and {} ({} bytes, {} clusters); wrote {}",
        tc_report.file_name,
        tc_report.bytes,
        tc_report.clusters,
        mado_a2_report.file_name,
        mado_a2_report.bytes,
        mado_a2_report.clusters,
        output_path.display()
    );
    Ok(())
}

/// Pre-rendered glyph sheets used by the hook proof-of-concept commands are
/// generated from fonts that this repository does not redistribute, so they
/// are read from `assets/gaiji/` when a command first needs them.
fn read_gaiji_input(path: &str) -> Vec<u8> {
    std::fs::read(path)
        .unwrap_or_else(|error| panic!("required input {path} is unavailable: {error}"))
}

/// Two-box proof sheet consumed by the typed renderer hook.
static PROOF_SHEET: LazyLock<Vec<u8>> =
    LazyLock::new(|| read_gaiji_input("assets/gaiji/proof_boxsheet.bin"));

fn build_hook_poc_disk(
    game_disk_path: PathBuf,
    demo_disk_path: PathBuf,
    data_disk_path: PathBuf,
    output_path: PathBuf,
) -> Result<()> {
    use pc98_madou_ars::hook_geometry::{RendererOverlay, renderer_hook};
    use pc98_madou_ars::sjis_marker::replace_first_exact;

    let hook_code = renderer_hook(RendererOverlay::Arle)?;
    let mut disk = std::fs::read(&game_disk_path)
        .with_context(|| format!("read {}", game_disk_path.display()))?;
    let demo_disk = std::fs::read(&demo_disk_path)
        .with_context(|| format!("read {}", demo_disk_path.display()))?;
    let data_disk = std::fs::read(&data_disk_path)
        .with_context(|| format!("read {}", data_disk_path.display()))?;

    // 1. Patch the two packed trampolines in GAME_A.OVL (length-preserving).
    let mut game_a = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "GAME_A.OVL")?;
    let entry_off = replace_first_exact(
        &mut game_a,
        &pc98_madou_ars::hook_geometry::ENTRY_TRAMPOLINE_SOURCE,
        &pc98_madou_ars::hook_geometry::entry_trampoline(), // jmp 0x8800:0x8000 (entry_hook)
    )
    .context("patch GAME_A.OVL entry trampoline")?;
    let draw_off = replace_first_exact(
        &mut game_a,
        &pc98_madou_ars::hook_geometry::DRAW_SETUP_TRAMPOLINE_SOURCE,
        &pc98_madou_ars::hook_geometry::draw_trampoline(), // jmp 0x8800:0x8020 (draw_hook)
    )
    .context("patch GAME_A.OVL draw trampoline")?;
    let overlay_report =
        pc98_madou_ars::fat12_replace::replace_file_in_place(&mut disk, "GAME_A.OVL", &game_a)?;
    println!(
        "patched GAME_A.OVL trampolines at packed 0x{entry_off:04X} (entry) / 0x{draw_off:04X} (draw) ({} bytes)",
        overlay_report.bytes
    );

    // 2. MAIN.COM stub copies the hook to 0x8800:0x8000 and the box sheet to 0x8800:0.
    let main_com = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "MAIN.COM")?;
    let patched_main = pc98_madou_ars::hangul_probe::patch_main_com_install_hook(
        &main_com,
        &hook_code,
        &PROOF_SHEET,
    )?;
    let main_report =
        pc98_madou_ars::fat12_replace::replace_file_in_place(&mut disk, "MAIN.COM", &patched_main)?;
    println!(
        "installed hook boot stub ({}-byte hook, {}-byte sheet); MAIN.COM {} bytes, capacity {}",
        hook_code.len(),
        PROOF_SHEET.len(),
        main_report.bytes,
        main_report.capacity
    );

    // 3. Patch ENEMY001 name -> gaiji codes 0x7621/0x7622 (sheet slots 0/1 = box)
    //    so the first Puyo battle's "appeared" message renders from the hook
    //    automatically -- no battle-menu navigation needed to see it.
    let mut enemy = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "ENEMY001.DAT")?;
    let enemy_patch = pc98_madou_ars::enemy_text::overwrite_first_enemy_name(
        &mut enemy,
        &[0xEB, 0x9F, 0xEB, 0xA0], // 뿌요 -> 0x7621/0x7622
    )?;
    pc98_madou_ars::fat12_replace::replace_file_in_place(&mut disk, "ENEMY001.DAT", &enemy)?;
    println!(
        "patched ENEMY001.DAT name '{}' -> sheet codes EB9F EBA0",
        enemy_patch.original_name
    );

    // 4. Stage the two-floppy boot-smoke media workaround.
    let tc_report = add_file_from_disk(&mut disk, &demo_disk, "TC.CNS")?;
    let mado_a2_report = add_file_from_disk(&mut disk, &data_disk, "MADO-A2.DAT")?;
    write_output_disk(&output_path, &disk)?;
    println!(
        "added {} and {}; wrote {}",
        tc_report.file_name,
        mado_a2_report.file_name,
        output_path.display()
    );
    Ok(())
}

/// Legacy full-sheet PoC fixture, loaded from disk at boot.
static KFONT_SHEET: LazyLock<Vec<u8>> = LazyLock::new(|| read_gaiji_input("assets/gaiji/kfont.bin"));

fn build_hook_loader_disk(
    game_disk_path: PathBuf,
    demo_disk_path: PathBuf,
    data_disk_path: PathBuf,
    output_path: PathBuf,
) -> Result<()> {
    use pc98_madou_ars::hook_geometry::{RendererOverlay, renderer_hook};
    use pc98_madou_ars::sjis_marker::replace_first_exact;

    let hook_code = renderer_hook(RendererOverlay::Arle)?;
    let mut disk = std::fs::read(&game_disk_path)
        .with_context(|| format!("read {}", game_disk_path.display()))?;
    let demo_disk = std::fs::read(&demo_disk_path)
        .with_context(|| format!("read {}", demo_disk_path.display()))?;
    let data_disk = std::fs::read(&data_disk_path)
        .with_context(|| format!("read {}", data_disk_path.display()))?;

    // 1. Patch the two packed trampolines in GAME_A.OVL (length-preserving).
    let mut game_a = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "GAME_A.OVL")?;
    replace_first_exact(
        &mut game_a,
        &pc98_madou_ars::hook_geometry::ENTRY_TRAMPOLINE_SOURCE,
        &pc98_madou_ars::hook_geometry::entry_trampoline(),
    )
    .context("patch GAME_A.OVL entry trampoline")?;
    replace_first_exact(
        &mut game_a,
        &pc98_madou_ars::hook_geometry::DRAW_SETUP_TRAMPOLINE_SOURCE,
        &pc98_madou_ars::hook_geometry::draw_trampoline(),
    )
    .context("patch GAME_A.OVL draw trampoline")?;
    pc98_madou_ars::fat12_replace::replace_file_in_place(&mut disk, "GAME_A.OVL", &game_a)?;

    // 2. MAIN.COM stub: load KFONT.BIN -> 0x8800:0 via INT 21h, copy hook -> 0x8800:0x8000.
    let main_com = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "MAIN.COM")?;
    let sheet_size = u16::try_from(KFONT_SHEET.len()).context("KFONT.BIN too large")?;
    let patched_main = pc98_madou_ars::hangul_probe::patch_main_com_load_sheet_hook(
        &main_com,
        &hook_code,
        sheet_size,
        "KFONT.BIN",
    )?;
    let main_report =
        pc98_madou_ars::fat12_replace::replace_file_in_place(&mut disk, "MAIN.COM", &patched_main)?;
    println!(
        "installed sheet-loader boot stub ({}-byte hook, loads {}-byte KFONT.BIN); MAIN.COM {} bytes, capacity {}",
        hook_code.len(),
        sheet_size,
        main_report.bytes,
        main_report.capacity
    );

    // 3. Add KFONT.BIN as a root file (the boot stub reads it by name).
    let kfont_meta = pc98_madou_ars::fat12_add::RootFileMeta {
        attr: 0x20,
        date: 0,
        time: 0,
    };
    pc98_madou_ars::fat12_add::add_root_file(&mut disk, "KFONT.BIN", &KFONT_SHEET, kfont_meta)
        .context("add KFONT.BIN to disk")?;
    if pc98_madou_ars::read_fat12_file_from_hdm(&disk, "KFONT.BIN")? != *KFONT_SHEET {
        bail!("KFONT.BIN readback did not match");
    }

    // 4. Patch ENEMY001 name -> sheet codes 0x7621/0x7622 (뿌요 in the sheet).
    let mut enemy = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "ENEMY001.DAT")?;
    pc98_madou_ars::enemy_text::overwrite_first_enemy_name(&mut enemy, &[0xEB, 0x9F, 0xEB, 0xA0])?;
    pc98_madou_ars::fat12_replace::replace_file_in_place(&mut disk, "ENEMY001.DAT", &enemy)?;

    // 5. Stage the two-floppy boot-smoke media workaround.
    add_file_from_disk(&mut disk, &demo_disk, "TC.CNS")?;
    add_file_from_disk(&mut disk, &data_disk, "MADO-A2.DAT")?;
    write_output_disk(&output_path, &disk)?;
    println!(
        "added KFONT.BIN + boot media; wrote {}",
        output_path.display()
    );
    Ok(())
}

fn build_translated_disk(
    game_disk_path: PathBuf,
    demo_disk_path: PathBuf,
    data_disk_path: PathBuf,
    translations_path: PathBuf,
    output_path: PathBuf,
) -> Result<()> {
    use pc98_madou_ars::hook_geometry::{RendererOverlay, renderer_hook};
    use pc98_madou_ars::overlay_batch::{
        PROVEN_GAME_RELOC_BASE, PROVEN_GAME_RELOC_END, apply_translations,
    };
    use pc98_madou_ars::overlay_lz::{decode_overlay_lz, encode_overlay_lz};
    use pc98_madou_ars::overlay_messages::{first_inconsistent_site, unified_catalog};
    use pc98_madou_ars::overlay_reloc::SheetCodes;
    use std::collections::HashMap;

    const LOAD_OFFSET: usize = 0x100;
    let hook_code = renderer_hook(RendererOverlay::Arle)?;
    let mut disk = std::fs::read(&game_disk_path)
        .with_context(|| format!("read {}", game_disk_path.display()))?;
    let demo_disk = std::fs::read(&demo_disk_path)
        .with_context(|| format!("read {}", demo_disk_path.display()))?;
    let data_disk = std::fs::read(&data_disk_path)
        .with_context(|| format!("read {}", data_disk_path.display()))?;

    // Translation input: either a workflow script (`{entries:[{string_decoded_offset,
    // ko, ..}]}`, the raw/complete schema) or a flat `{ "0xHHHH": "Korean" }` map.
    let translations_text = std::fs::read_to_string(&translations_path)
        .with_context(|| format!("read {}", translations_path.display()))?;
    let translations_json: serde_json::Value =
        serde_json::from_str(&translations_text).context("parse translations JSON")?;
    let mut translations = HashMap::new();
    if let Some(entries) = translations_json.get("entries").and_then(|v| v.as_array()) {
        for entry in entries {
            let ko = entry.get("ko").and_then(|v| v.as_str()).unwrap_or("");
            if ko.is_empty() {
                continue;
            }
            let offset = entry
                .get("string_decoded_offset")
                .and_then(|v| v.as_str())
                .with_context(|| "script entry missing string_decoded_offset")
                .and_then(parse_usize)?;
            translations.insert(offset, ko.to_string());
        }
    } else {
        for (key, value) in translations_json
            .as_object()
            .context("translations JSON must be a script or an offset -> string map")?
        {
            let offset = parse_usize(key)?;
            let korean = value
                .as_str()
                .with_context(|| format!("translation for {key} must be a string"))?
                .to_string();
            translations.insert(offset, korean);
        }
    }

    // 1. Decode GAME_A.OVL and build the message map from the pristine image.
    let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "GAME_A.OVL")?;
    let mut decoded = decode_overlay_lz(&packed)?.output;
    let messages = unified_catalog(&decoded, LOAD_OFFSET, 4, &[2, 4, 6, 8, 10, 12, 14, 16])?;

    // 2. Install both renderer-hook trampolines (overwrites code, not pointers).
    install_decoded_trampoline(
        &mut decoded,
        ENTRY_TRAMPOLINE_DECODED,
        &pc98_madou_ars::hook_geometry::ENTRY_TRAMPOLINE_SOURCE,
        &pc98_madou_ars::hook_geometry::entry_trampoline(),
        "entry",
    )?;
    install_decoded_trampoline(
        &mut decoded,
        DRAW_SETUP_TRAMPOLINE_DECODED,
        &pc98_madou_ars::hook_geometry::DRAW_SETUP_TRAMPOLINE_ANCHOR,
        &pc98_madou_ars::hook_geometry::draw_trampoline(),
        "draw",
    )?;

    // A translation must key a real cataloged message, and every rewrite site
    // must still hold its logical offset after the trampolines (guard against a
    // trampoline landing on a pointer).
    let by_offset: HashMap<usize, &_> = messages
        .iter()
        .map(|m| (m.string_decoded_offset, m))
        .collect();
    for offset in translations.keys() {
        let message = by_offset
            .get(offset)
            .with_context(|| format!("no cataloged message at 0x{offset:04X}"))?;
        if let Some(bad) = first_inconsistent_site(&decoded, message) {
            bail!(
                "rewrite site 0x{:04X} for 0x{offset:04X} no longer holds its offset",
                bad.site
            );
        }
    }

    // 3. Apply the batch translations (in-place or relocate + rewrite pointers).
    let sheet = SheetCodes::from_json_str(&KFONT_SHEET_JSON)?;
    let report = apply_translations(
        &mut decoded,
        LOAD_OFFSET,
        &messages,
        &translations,
        &sheet,
        PROVEN_GAME_RELOC_BASE,
        PROVEN_GAME_RELOC_END,
    )?;
    println!(
        "applied translations: {} in place, {} relocated ({} band bytes), {} of {} cataloged untranslated",
        report.in_place,
        report.relocated,
        report.reloc_bytes_used,
        report.untranslated,
        messages.len(),
    );

    // 4. Re-encode and confirm the LZ round-trips before shipping.
    let reencoded = encode_overlay_lz(&decoded);
    if decode_overlay_lz(&reencoded)?.output != decoded {
        bail!("re-encoded GAME_A.OVL did not round-trip; refusing to ship");
    }
    let overlay_report =
        pc98_madou_ars::fat12_replace::replace_file_in_place(&mut disk, "GAME_A.OVL", &reencoded)?;
    println!(
        "re-encoded GAME_A.OVL: {} -> {} packed bytes (capacity {})",
        packed.len(),
        reencoded.len(),
        overlay_report.capacity,
    );

    // 5. MAIN.COM stub loads KFONT.BIN -> 0x8800:0 and copies the hook.
    let main_com = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "MAIN.COM")?;
    let sheet_size = u16::try_from(KFONT_SHEET.len()).context("KFONT.BIN too large")?;
    let patched_main = pc98_madou_ars::hangul_probe::patch_main_com_load_sheet_hook(
        &main_com,
        &hook_code,
        sheet_size,
        "KFONT.BIN",
    )?;
    pc98_madou_ars::fat12_replace::replace_file_in_place(&mut disk, "MAIN.COM", &patched_main)?;

    // 6. Ship KFONT.BIN, patch the enemy name to sheet codes, stage boot media.
    let kfont_meta = pc98_madou_ars::fat12_add::RootFileMeta {
        attr: 0x20,
        date: 0,
        time: 0,
    };
    pc98_madou_ars::fat12_add::add_root_file(&mut disk, "KFONT.BIN", &KFONT_SHEET, kfont_meta)
        .context("add KFONT.BIN to disk")?;
    let mut enemy = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "ENEMY001.DAT")?;
    pc98_madou_ars::enemy_text::overwrite_first_enemy_name(&mut enemy, &[0xEB, 0x9F, 0xEB, 0xA0])?;
    pc98_madou_ars::fat12_replace::replace_file_in_place(&mut disk, "ENEMY001.DAT", &enemy)?;
    add_file_from_disk(&mut disk, &demo_disk, "TC.CNS")?;
    add_file_from_disk(&mut disk, &data_disk, "MADO-A2.DAT")?;
    write_output_disk(&output_path, &disk)?;
    println!("wrote {}", output_path.display());
    Ok(())
}

/// Per-character renderer-hook disk. Each character ships on its own Game disk
/// (Arle=GAME_A, Rulue=GAME_R, Schezo=GAME_S); detect which overlay the disk has,
/// patch its trampolines, and ship the matching hook blob (its rejoin offsets are
/// delta-shifted because the renderer decodes to a different base per overlay).
fn build_hook_char_disk(
    game_disk_path: PathBuf,
    demo_disk_path: PathBuf,
    data_disk_path: PathBuf,
    output_path: PathBuf,
) -> Result<()> {
    use pc98_madou_ars::hook_geometry::{RendererOverlay, renderer_hook};
    use pc98_madou_ars::sjis_marker::replace_first_exact;

    let mut disk = std::fs::read(&game_disk_path)
        .with_context(|| format!("read {}", game_disk_path.display()))?;
    let demo_disk = std::fs::read(&demo_disk_path)
        .with_context(|| format!("read {}", demo_disk_path.display()))?;
    let data_disk = std::fs::read(&data_disk_path)
        .with_context(|| format!("read {}", data_disk_path.display()))?;

    // Detect the overlay on this disk and pick its matching hook blob.
    let hooks = [
        renderer_hook(RendererOverlay::Arle)?,
        renderer_hook(RendererOverlay::Rulue)?,
        renderer_hook(RendererOverlay::Schezo)?,
    ];
    let (ovl, hook): (&str, &[u8]) = ["GAME_A.OVL", "GAME_R.OVL", "GAME_S.OVL"]
        .iter()
        .zip(hooks.iter().map(Vec::as_slice))
        .find_map(|(name, h)| {
            pc98_madou_ars::read_fat12_file_from_hdm(&disk, name)
                .ok()
                .map(|_| (*name, h))
        })
        .context("no GAME_A/R/S.OVL on this Game disk")?;
    println!("character overlay: {ovl}");

    // Combined image: sheet at 0, the one matching hook blob at 0x8000.
    const HOOK_OFF: usize = pc98_madou_ars::hook_geometry::HOOK_ENTRY_OFF as usize;
    let mut combined = vec![0u8; HOOK_OFF + hook.len()];
    combined[..KFONT_SHEET.len()].copy_from_slice(&KFONT_SHEET);
    combined[HOOK_OFF..HOOK_OFF + hook.len()].copy_from_slice(hook);
    let combined_size = u16::try_from(combined.len()).context("combined KFONT.BIN too large")?;

    // Patch the overlay's two trampolines to the hook at 0x8800:0x8000 (draw +0x20).
    let mut data = pc98_madou_ars::read_fat12_file_from_hdm(&disk, ovl)?;
    replace_first_exact(
        &mut data,
        &pc98_madou_ars::hook_geometry::ENTRY_TRAMPOLINE_SOURCE,
        &pc98_madou_ars::hook_geometry::entry_trampoline(),
    )
    .with_context(|| format!("patch {ovl} entry trampoline"))?;
    replace_first_exact(
        &mut data,
        &pc98_madou_ars::hook_geometry::DRAW_SETUP_TRAMPOLINE_SOURCE,
        &pc98_madou_ars::hook_geometry::draw_trampoline(),
    )
    .with_context(|| format!("patch {ovl} draw trampoline"))?;
    pc98_madou_ars::fat12_replace::replace_file_in_place(&mut disk, ovl, &data)?;

    // MAIN.COM stub: load the combined image into 0x8800:0 via INT 21h.
    let main_com = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "MAIN.COM")?;
    let patched_main = pc98_madou_ars::hangul_probe::patch_main_com_load_combined(
        &main_com,
        combined_size,
        "KFONT.BIN",
    )?;
    let main_report =
        pc98_madou_ars::fat12_replace::replace_file_in_place(&mut disk, "MAIN.COM", &patched_main)?;
    println!(
        "patched {ovl} trampolines + combined loader stub (loads {combined_size}-byte KFONT.BIN); MAIN.COM {} bytes, capacity {}",
        main_report.bytes, main_report.capacity
    );

    // Ship KFONT.BIN + stage boot media (TC.CNS always; MADO-?2.DAT if present).
    let kfont_meta = pc98_madou_ars::fat12_add::RootFileMeta {
        attr: 0x20,
        date: 0,
        time: 0,
    };
    pc98_madou_ars::fat12_add::add_root_file(&mut disk, "KFONT.BIN", &combined, kfont_meta)
        .context("add KFONT.BIN to disk")?;
    if pc98_madou_ars::read_fat12_file_from_hdm(&disk, "KFONT.BIN")? != combined {
        bail!("KFONT.BIN readback did not match");
    }
    add_file_from_disk(&mut disk, &demo_disk, "TC.CNS")?;
    for mado in ["MADO-A2.DAT", "MADO-R2.DAT", "MADO-S2.DAT"] {
        if pc98_madou_ars::read_fat12_file_from_hdm(&data_disk, mado).is_ok() {
            add_file_from_disk(&mut disk, &data_disk, mado)?;
            println!("staged {mado}");
        }
    }
    write_output_disk(&output_path, &disk)?;
    println!(
        "added KFONT.BIN + boot media; wrote {}",
        output_path.display()
    );
    Ok(())
}

/// Decoded-overlay offsets of the two GAME_A renderer-hook trampoline sites
/// (decoded = phys - 0x1E040): entry phys 0x23330, draw setup phys 0x233A2.
const ENTRY_TRAMPOLINE_DECODED: usize = 0x52F0;
const DRAW_SETUP_TRAMPOLINE_DECODED: usize = 0x5362;

/// Verify the expected source anchor at `off`, then overwrite its prefix with
/// the trampoline. Refuses to patch if the site has shifted.
fn install_decoded_trampoline(
    decoded: &mut [u8],
    off: usize,
    source_anchor: &[u8],
    tramp: &[u8],
    label: &str,
) -> Result<()> {
    if tramp.len() > source_anchor.len() {
        bail!(
            "{label} trampoline is longer than its source anchor: {} > {}",
            tramp.len(),
            source_anchor.len()
        );
    }
    let slot = decoded
        .get(off..off + source_anchor.len())
        .with_context(|| format!("{label} trampoline site 0x{off:X} outside decoded overlay"))?;
    if slot != source_anchor {
        bail!(
            "{label} trampoline site 0x{off:X}: expected {source_anchor:02X?}, found {slot:02X?}"
        );
    }
    decoded[off..off + tramp.len()].copy_from_slice(tramp);
    Ok(())
}

fn build_reencode_hook_disk(
    game_disk_path: PathBuf,
    demo_disk_path: PathBuf,
    data_disk_path: PathBuf,
    output_path: PathBuf,
) -> Result<()> {
    use pc98_madou_ars::hook_geometry::{RendererOverlay, renderer_hook};
    use pc98_madou_ars::overlay_lz::{decode_overlay_lz, encode_overlay_lz};

    let hook_code = renderer_hook(RendererOverlay::Arle)?;
    let mut disk = std::fs::read(&game_disk_path)
        .with_context(|| format!("read {}", game_disk_path.display()))?;
    let demo_disk = std::fs::read(&demo_disk_path)
        .with_context(|| format!("read {}", demo_disk_path.display()))?;
    let data_disk = std::fs::read(&data_disk_path)
        .with_context(|| format!("read {}", data_disk_path.display()))?;

    // 1. GAME_A.OVL: decode -> install both trampolines at their decoded offsets
    //    -> re-encode with the LZ encoder. This is the general decoded-level path
    //    (vs. patching packed-stream literals in place).
    let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "GAME_A.OVL")?;
    let mut decoded = decode_overlay_lz(&packed)?.output;
    install_decoded_trampoline(
        &mut decoded,
        ENTRY_TRAMPOLINE_DECODED,
        &pc98_madou_ars::hook_geometry::ENTRY_TRAMPOLINE_SOURCE,
        &pc98_madou_ars::hook_geometry::entry_trampoline(), // jmp 0x8800:0x8000 (entry_hook)
        "entry",
    )?;
    install_decoded_trampoline(
        &mut decoded,
        DRAW_SETUP_TRAMPOLINE_DECODED,
        &pc98_madou_ars::hook_geometry::DRAW_SETUP_TRAMPOLINE_ANCHOR,
        &pc98_madou_ars::hook_geometry::draw_trampoline(), // jmp 0x8800:0x8020 (draw_hook)
        "draw",
    )?;
    let reencoded = encode_overlay_lz(&decoded);
    if decode_overlay_lz(&reencoded)?.output != decoded {
        bail!("re-encoded GAME_A.OVL did not round-trip; refusing to ship");
    }
    let overlay_report =
        pc98_madou_ars::fat12_replace::replace_file_in_place(&mut disk, "GAME_A.OVL", &reencoded)?;
    println!(
        "re-encoded GAME_A.OVL: {} -> {} packed bytes (capacity {}); trampolines at decoded 0x{:04X}/0x{:04X}",
        packed.len(),
        reencoded.len(),
        overlay_report.capacity,
        ENTRY_TRAMPOLINE_DECODED,
        DRAW_SETUP_TRAMPOLINE_DECODED,
    );

    // 2. MAIN.COM stub: load KFONT.BIN -> 0x8800:0 via INT 21h, copy hook -> 0x8800:0x8000.
    let main_com = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "MAIN.COM")?;
    let sheet_size = u16::try_from(KFONT_SHEET.len()).context("KFONT.BIN too large")?;
    let patched_main = pc98_madou_ars::hangul_probe::patch_main_com_load_sheet_hook(
        &main_com,
        &hook_code,
        sheet_size,
        "KFONT.BIN",
    )?;
    pc98_madou_ars::fat12_replace::replace_file_in_place(&mut disk, "MAIN.COM", &patched_main)?;

    // 3. Ship KFONT.BIN as a root file (the boot stub reads it by name).
    let kfont_meta = pc98_madou_ars::fat12_add::RootFileMeta {
        attr: 0x20,
        date: 0,
        time: 0,
    };
    pc98_madou_ars::fat12_add::add_root_file(&mut disk, "KFONT.BIN", &KFONT_SHEET, kfont_meta)
        .context("add KFONT.BIN to disk")?;
    if pc98_madou_ars::read_fat12_file_from_hdm(&disk, "KFONT.BIN")? != *KFONT_SHEET {
        bail!("KFONT.BIN readback did not match");
    }

    // 4. Patch ENEMY001 name -> sheet codes 0x7621/0x7622 (뿌요 in the sheet).
    let mut enemy = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "ENEMY001.DAT")?;
    pc98_madou_ars::enemy_text::overwrite_first_enemy_name(&mut enemy, &[0xEB, 0x9F, 0xEB, 0xA0])?;
    pc98_madou_ars::fat12_replace::replace_file_in_place(&mut disk, "ENEMY001.DAT", &enemy)?;

    // 5. Stage the two-floppy boot-smoke media workaround.
    add_file_from_disk(&mut disk, &demo_disk, "TC.CNS")?;
    add_file_from_disk(&mut disk, &data_disk, "MADO-A2.DAT")?;
    write_output_disk(&output_path, &disk)?;
    println!(
        "added KFONT.BIN + boot media; wrote {}",
        output_path.display()
    );
    Ok(())
}

/// Glyph-sheet code map (kept in sync with KFONT_SHEET at build time).
static KFONT_SHEET_JSON: LazyLock<String> = LazyLock::new(|| {
    String::from_utf8(read_gaiji_input("assets/gaiji/kfont.json"))
        .expect("assets/gaiji/kfont.json is not UTF-8")
});

/// The relocation PoC target: the static "appeared!" battle suffix the renderer
/// draws after the (hook-rendered) enemy name. Logical offset 0xB106, call site
/// 0x2AB2. Bytes: が　現れた！\n\n (no NUL; relocate_message appends one).
const RELOC_TARGET_JP: [u8; 14] = [
    0x82, 0xAA, 0x81, 0x40, 0x8C, 0xBB, 0x82, 0xEA, 0x82, 0xBD, 0x81, 0x49, 0x0A, 0x0A,
];
/// Korean replacement (+ the two trailing newline control bytes appended in
/// code). "가나타났다！" is 12 bytes; with \n\n it is 14 bytes -- exactly the
/// original length, so the build patches it in place (no relocation, no growth).
const RELOC_NEW_KO: &str = "가나타났다！";

fn build_message_reloc_poc(
    game_disk_path: PathBuf,
    demo_disk_path: PathBuf,
    data_disk_path: PathBuf,
    reloc_to: Option<String>,
    output_path: PathBuf,
) -> Result<()> {
    use pc98_madou_ars::hook_geometry::{RendererOverlay, renderer_hook};
    use pc98_madou_ars::overlay_lz::{decode_overlay_lz, encode_overlay_lz};
    use pc98_madou_ars::overlay_reloc::{SheetCodes, patch_message_in_place, relocate_message};

    const LOAD_OFFSET: usize = 0x100;
    let hook_code = renderer_hook(RendererOverlay::Arle)?;
    let reloc_to = reloc_to.as_deref().map(parse_usize).transpose()?;

    let mut disk = std::fs::read(&game_disk_path)
        .with_context(|| format!("read {}", game_disk_path.display()))?;
    let demo_disk = std::fs::read(&demo_disk_path)
        .with_context(|| format!("read {}", demo_disk_path.display()))?;
    let data_disk = std::fs::read(&data_disk_path)
        .with_context(|| format!("read {}", data_disk_path.display()))?;

    // 1. Decode GAME_A.OVL.
    let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "GAME_A.OVL")?;
    let mut decoded = decode_overlay_lz(&packed)?.output;

    // 2. Replace the target message with Korean. If it fits the original byte
    //    budget, patch in place (the overlay's decoded RAM image has no slack
    //    past its tail, so appended bytes land in adjacent data); otherwise
    //    relocate by appending and rewriting the pointer.
    let sheet = SheetCodes::from_json_str(&KFONT_SHEET_JSON)?;
    if let Some(target) = reloc_to {
        // Growth probe: force relocation to a chosen high offset above the
        // overlay, using the longer Korean (with the full-width space).
        let mut grow = sheet.encode_line("가　나타났다！")?;
        grow.extend_from_slice(&[0x0A, 0x0A]);
        if target < decoded.len() {
            bail!(
                "--reloc-to 0x{target:X} is below the overlay end 0x{:X}",
                decoded.len()
            );
        }
        decoded.resize(target, 0); // pad the gap with NUL
        let reloc = relocate_message(&mut decoded, LOAD_OFFSET, &RELOC_TARGET_JP, &grow)?;
        println!(
            "relocated message at decoded 0x{:04X} -> 0x{:04X} (logical 0x{:04X}); \
             {} -> {} bytes; rewrote {} pointer(s) at {:04X?}",
            reloc.target_decoded_offset,
            reloc.new_decoded_offset,
            reloc.new_logical_offset,
            RELOC_TARGET_JP.len(),
            grow.len(),
            reloc.rewritten_calls.len(),
            reloc.rewritten_calls,
        );
    } else {
        let mut new_string = sheet.encode_line(RELOC_NEW_KO)?;
        new_string.extend_from_slice(&[0x0A, 0x0A]); // preserve the trailing newlines
        let at = patch_message_in_place(&mut decoded, &RELOC_TARGET_JP, &new_string)?;
        println!(
            "patched message in place at decoded 0x{at:04X}; {} -> {} bytes (no relocation)",
            RELOC_TARGET_JP.len(),
            new_string.len(),
        );
    }

    // 3. Install both renderer-hook trampolines at their decoded offsets.
    install_decoded_trampoline(
        &mut decoded,
        ENTRY_TRAMPOLINE_DECODED,
        &pc98_madou_ars::hook_geometry::ENTRY_TRAMPOLINE_SOURCE,
        &pc98_madou_ars::hook_geometry::entry_trampoline(),
        "entry",
    )?;
    install_decoded_trampoline(
        &mut decoded,
        DRAW_SETUP_TRAMPOLINE_DECODED,
        &pc98_madou_ars::hook_geometry::DRAW_SETUP_TRAMPOLINE_ANCHOR,
        &pc98_madou_ars::hook_geometry::draw_trampoline(),
        "draw",
    )?;

    // 4. Re-encode and write back.
    let reencoded = encode_overlay_lz(&decoded);
    if decode_overlay_lz(&reencoded)?.output != decoded {
        bail!("re-encoded GAME_A.OVL did not round-trip; refusing to ship");
    }
    let overlay_report =
        pc98_madou_ars::fat12_replace::replace_file_in_place(&mut disk, "GAME_A.OVL", &reencoded)?;
    println!(
        "re-encoded GAME_A.OVL: {} -> {} packed bytes (capacity {})",
        packed.len(),
        reencoded.len(),
        overlay_report.capacity,
    );

    // 5. MAIN.COM sheet loader + KFONT.BIN + ENEMY001 + boot media (as the
    //    re-encode hook disk).
    let main_com = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "MAIN.COM")?;
    let sheet_size = u16::try_from(KFONT_SHEET.len()).context("KFONT.BIN too large")?;
    let patched_main = pc98_madou_ars::hangul_probe::patch_main_com_load_sheet_hook(
        &main_com,
        &hook_code,
        sheet_size,
        "KFONT.BIN",
    )?;
    pc98_madou_ars::fat12_replace::replace_file_in_place(&mut disk, "MAIN.COM", &patched_main)?;

    let kfont_meta = pc98_madou_ars::fat12_add::RootFileMeta {
        attr: 0x20,
        date: 0,
        time: 0,
    };
    pc98_madou_ars::fat12_add::add_root_file(&mut disk, "KFONT.BIN", &KFONT_SHEET, kfont_meta)
        .context("add KFONT.BIN to disk")?;
    if pc98_madou_ars::read_fat12_file_from_hdm(&disk, "KFONT.BIN")? != *KFONT_SHEET {
        bail!("KFONT.BIN readback did not match");
    }

    let mut enemy = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "ENEMY001.DAT")?;
    pc98_madou_ars::enemy_text::overwrite_first_enemy_name(&mut enemy, &[0xEB, 0x9F, 0xEB, 0xA0])?;
    pc98_madou_ars::fat12_replace::replace_file_in_place(&mut disk, "ENEMY001.DAT", &enemy)?;

    add_file_from_disk(&mut disk, &demo_disk, "TC.CNS")?;
    add_file_from_disk(&mut disk, &data_disk, "MADO-A2.DAT")?;
    write_output_disk(&output_path, &disk)?;
    println!(
        "added KFONT.BIN + boot media; wrote {}",
        output_path.display()
    );
    Ok(())
}

/// The ten gaiji JIS rows the hook intercepts, indexed by `--row`.
const SWEEP_ROWS: [u16; 10] = [0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, 0x7B, 0x7C, 0x7D, 0x7E];

/// Decode a plain hex string (even length, no separators) into bytes.
fn decode_hex(s: &str) -> Result<Vec<u8>> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) {
        bail!("hex string has odd length: {}", s.len());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16).with_context(|| format!("bad hex byte at {i}"))
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn build_sheet_sweep_disk(
    game_disk_path: PathBuf,
    demo_disk_path: PathBuf,
    data_disk_path: PathBuf,
    row: usize,
    sheet_path: PathBuf,
    sheet_json_path: PathBuf,
    target_hex: Option<String>,
    output_path: PathBuf,
) -> Result<()> {
    use pc98_madou_ars::hook_geometry::{RendererOverlay, renderer_hook};
    use pc98_madou_ars::overlay_lz::{decode_overlay_lz, encode_overlay_lz};
    use pc98_madou_ars::overlay_reloc::relocate_message;

    const LOAD_OFFSET: usize = 0x100;
    // Vetted free band above the render scratch, the same target the
    // reloc PoC and the batch build relocate longer Korean into.
    const RELOC_BASE: usize = 0xBFC0;
    let jis_row = *SWEEP_ROWS
        .get(row)
        .with_context(|| format!("row must be 0..9, got {row}"))?;

    // The renderer message to relocate onto: a custom deterministic target if
    // given, else the battle "appeared" suffix.
    let target: Vec<u8> = match &target_hex {
        Some(h) => decode_hex(h).context("parse --target-hex")?,
        None => RELOC_TARGET_JP.to_vec(),
    };

    let hook_code = renderer_hook(RendererOverlay::Arle)?;
    let mut disk = std::fs::read(&game_disk_path)
        .with_context(|| format!("read {}", game_disk_path.display()))?;
    let demo_disk = std::fs::read(&demo_disk_path)
        .with_context(|| format!("read {}", demo_disk_path.display()))?;
    let data_disk = std::fs::read(&data_disk_path)
        .with_context(|| format!("read {}", data_disk_path.display()))?;
    let sheet =
        std::fs::read(&sheet_path).with_context(|| format!("read {}", sheet_path.display()))?;
    let sheet_json: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&sheet_json_path)
            .with_context(|| format!("read {}", sheet_json_path.display()))?,
    )
    .context("parse sweep sheet JSON")?;

    // Collect this row's 94 cell codes, ordered by cell.
    let mut cells: Vec<(u64, [u8; 2])> = sheet_json["glyphs"]
        .as_array()
        .context("sweep sheet JSON missing `glyphs`")?
        .iter()
        .filter(|g| {
            g["jis"]
                .as_str()
                .and_then(|s| u16::from_str_radix(s.trim_start_matches("0x"), 16).ok())
                .map(|j| (j >> 8) == jis_row)
                .unwrap_or(false)
        })
        .map(|g| {
            let cell = g["cell"].as_u64().context("glyph missing `cell`")?;
            let sjis = g["sjis"].as_str().context("glyph missing `sjis`")?;
            let code = u16::from_str_radix(sjis, 16)
                .with_context(|| format!("bad sjis {sjis:?}"))?
                .to_be_bytes();
            Ok((cell, code))
        })
        .collect::<Result<Vec<_>>>()?;
    cells.sort_by_key(|(c, _)| *c);
    if cells.len() != 94 {
        bail!(
            "row 0x{jis_row:02X} has {} cells in the sheet, expected 94",
            cells.len()
        );
    }

    // Display string: just the 94 cell codes with a newline every 30 cells
    // (~4 lines), then the trailing newlines -- the box shows a clean crosshair
    // grid to read for gaps (dead cells) and jumps (mis-maps).
    let mut new_string: Vec<u8> = Vec::new();
    for (i, (_, code)) in cells.iter().enumerate() {
        new_string.extend_from_slice(code);
        if (i + 1) % 30 == 0 {
            new_string.push(0x0A);
        }
    }
    new_string.extend_from_slice(&[0x0A, 0x0A]);

    // Decode GAME_A.OVL and relocate the appeared message into the free band.
    let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "GAME_A.OVL")?;
    let mut decoded = decode_overlay_lz(&packed)?.output;
    if RELOC_BASE < decoded.len() {
        bail!(
            "reloc base 0x{RELOC_BASE:X} is below the overlay end 0x{:X}",
            decoded.len()
        );
    }
    decoded.resize(RELOC_BASE, 0); // pad the gap with NUL
    let reloc = relocate_message(&mut decoded, LOAD_OFFSET, &target, &new_string)?;
    println!(
        "row 0x{jis_row:02X}: relocated target message -> decoded 0x{:04X} (logical 0x{:04X}); \
         {} code bytes; rewrote {} pointer(s)",
        reloc.new_decoded_offset,
        reloc.new_logical_offset,
        new_string.len(),
        reloc.rewritten_calls.len(),
    );

    // Renderer-hook trampolines.
    install_decoded_trampoline(
        &mut decoded,
        ENTRY_TRAMPOLINE_DECODED,
        &pc98_madou_ars::hook_geometry::ENTRY_TRAMPOLINE_SOURCE,
        &pc98_madou_ars::hook_geometry::entry_trampoline(),
        "entry",
    )?;
    install_decoded_trampoline(
        &mut decoded,
        DRAW_SETUP_TRAMPOLINE_DECODED,
        &pc98_madou_ars::hook_geometry::DRAW_SETUP_TRAMPOLINE_ANCHOR,
        &pc98_madou_ars::hook_geometry::draw_trampoline(),
        "draw",
    )?;

    let reencoded = encode_overlay_lz(&decoded);
    if decode_overlay_lz(&reencoded)?.output != decoded {
        bail!("re-encoded GAME_A.OVL did not round-trip; refusing to ship");
    }
    pc98_madou_ars::fat12_replace::replace_file_in_place(&mut disk, "GAME_A.OVL", &reencoded)?;

    // Combined image: sweep sheet at 0, hook at HOOK_ENTRY_OFF; loaded to 0x8800:0.
    const HOOK_OFF: usize = pc98_madou_ars::hook_geometry::HOOK_ENTRY_OFF as usize;
    const SCRATCH_OFF: usize = pc98_madou_ars::hook_geometry::SCRATCH_OFF as usize;
    if sheet.len() > SCRATCH_OFF {
        bail!(
            "sweep sheet {} bytes reaches the hook scratch word at 0x{SCRATCH_OFF:X}",
            sheet.len()
        );
    }
    let mut combined = vec![0u8; HOOK_OFF + hook_code.len()];
    combined[..sheet.len()].copy_from_slice(&sheet);
    combined[HOOK_OFF..].copy_from_slice(&hook_code);
    let combined_size = u16::try_from(combined.len()).context("combined KFONT.BIN too large")?;

    let main_com = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "MAIN.COM")?;
    let patched_main = pc98_madou_ars::hangul_probe::patch_main_com_load_combined(
        &main_com,
        combined_size,
        "KFONT.BIN",
    )?;
    pc98_madou_ars::fat12_replace::replace_file_in_place(&mut disk, "MAIN.COM", &patched_main)?;

    let kfont_meta = pc98_madou_ars::fat12_add::RootFileMeta {
        attr: 0x20,
        date: 0,
        time: 0,
    };
    pc98_madou_ars::fat12_add::add_root_file(&mut disk, "KFONT.BIN", &combined, kfont_meta)
        .context("add KFONT.BIN to disk")?;
    if pc98_madou_ars::read_fat12_file_from_hdm(&disk, "KFONT.BIN")? != combined {
        bail!("KFONT.BIN readback did not match");
    }

    add_file_from_disk(&mut disk, &demo_disk, "TC.CNS")?;
    add_file_from_disk(&mut disk, &data_disk, "MADO-A2.DAT")?;
    write_output_disk(&output_path, &disk)?;
    println!(
        "wrote {} (sweep row 0x{jis_row:02X}, {}-byte KFONT.BIN)",
        output_path.display(),
        combined_size
    );
    Ok(())
}

/// Find the single source anchor in `decoded` and overwrite its prefix with the
/// trampoline. Unlike install_decoded_trampoline (fixed
/// GAME_A offset), this locates the site by byte-pattern so it works on any
/// overlay -- the renderer code is identical across GAME_A/R/S, only its decoded
/// offset differs. Refuses to patch if the pattern is not unique.
fn install_trampoline_by_pattern(
    decoded: &mut [u8],
    source_anchor: &[u8],
    tramp: &[u8],
    label: &str,
) -> Result<usize> {
    let hits: Vec<usize> = decoded
        .windows(source_anchor.len())
        .enumerate()
        .filter(|(_, w)| *w == source_anchor)
        .map(|(i, _)| i)
        .collect();
    match hits.as_slice() {
        [off] => {
            if tramp.len() > source_anchor.len() {
                bail!(
                    "{label} trampoline is longer than its source anchor: {} > {}",
                    tramp.len(),
                    source_anchor.len()
                );
            }
            decoded[*off..*off + tramp.len()].copy_from_slice(tramp);
            Ok(*off)
        }
        [] => bail!("{label} trampoline anchor {source_anchor:02X?} not found"),
        many => bail!(
            "{label} trampoline pattern is not unique ({} sites)",
            many.len()
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn build_char_reloc_poc(
    game_disk_path: PathBuf,
    demo_disk_path: PathBuf,
    data_disk_path: PathBuf,
    message_hex: &str,
    korean: &str,
    sheet_path: PathBuf,
    sheet_json_path: PathBuf,
    output_path: PathBuf,
) -> Result<()> {
    use pc98_madou_ars::hook_geometry::{RendererOverlay, renderer_hook};
    use pc98_madou_ars::overlay_lz::{decode_overlay_lz, encode_overlay_lz};
    use pc98_madou_ars::overlay_reloc::{SheetCodes, patch_message_in_place, relocate_message};

    const LOAD_OFFSET: usize = 0x100;

    let mut disk = std::fs::read(&game_disk_path)
        .with_context(|| format!("read {}", game_disk_path.display()))?;
    let demo_disk = std::fs::read(&demo_disk_path)
        .with_context(|| format!("read {}", demo_disk_path.display()))?;
    let data_disk = std::fs::read(&data_disk_path)
        .with_context(|| format!("read {}", data_disk_path.display()))?;
    let sheet_bin =
        std::fs::read(&sheet_path).with_context(|| format!("read {}", sheet_path.display()))?;
    let sheet_json_text = std::fs::read_to_string(&sheet_json_path)
        .with_context(|| format!("read {}", sheet_json_path.display()))?;

    // Detect the overlay on this Game disk and pick its matching hook + boot data.
    let (ovl, hook, mado): (&str, Vec<u8>, &str) = [
        (
            "GAME_A.OVL",
            renderer_hook(RendererOverlay::Arle)?,
            "MADO-A2.DAT",
        ),
        (
            "GAME_R.OVL",
            renderer_hook(RendererOverlay::Rulue)?,
            "MADO-R2.DAT",
        ),
        (
            "GAME_S.OVL",
            renderer_hook(RendererOverlay::Schezo)?,
            "MADO-S2.DAT",
        ),
    ]
    .into_iter()
    .find(|(name, _, _)| pc98_madou_ars::read_fat12_file_from_hdm(&disk, name).is_ok())
    .context("no GAME_A/R/S.OVL on this Game disk")?;
    println!("character overlay: {ovl}");

    // Decode, then patch the target message to Korean (in place if it fits).
    let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, ovl)?;
    let mut decoded = decode_overlay_lz(&packed)?.output;
    let target = decode_hex(message_hex).context("parse --message-hex")?;
    let sheet = SheetCodes::from_json_str(&sheet_json_text)?;
    let ko_bytes = sheet.encode_line(korean)?;
    if ko_bytes.len() <= target.len() {
        let at = patch_message_in_place(&mut decoded, &target, &ko_bytes)?;
        println!(
            "patched message in place at decoded 0x{at:04X}: {} -> {} bytes",
            target.len(),
            ko_bytes.len()
        );
    } else {
        let reloc = relocate_message(&mut decoded, LOAD_OFFSET, &target, &ko_bytes)?;
        println!(
            "relocated message -> logical 0x{:04X}: {} -> {} bytes; rewrote {} pointer(s)",
            reloc.new_logical_offset,
            target.len(),
            ko_bytes.len(),
            reloc.rewritten_calls.len()
        );
    }

    // Renderer-hook trampolines, located by pattern (overlay-agnostic).
    let e = install_trampoline_by_pattern(
        &mut decoded,
        &pc98_madou_ars::hook_geometry::ENTRY_TRAMPOLINE_SOURCE,
        &pc98_madou_ars::hook_geometry::entry_trampoline(),
        "entry",
    )?;
    let d = install_trampoline_by_pattern(
        &mut decoded,
        &pc98_madou_ars::hook_geometry::DRAW_SETUP_TRAMPOLINE_ANCHOR,
        &pc98_madou_ars::hook_geometry::draw_trampoline(),
        "draw",
    )?;
    println!("trampolines at decoded 0x{e:04X} (entry) / 0x{d:04X} (draw)");

    let reencoded = encode_overlay_lz(&decoded);
    if decode_overlay_lz(&reencoded)?.output != decoded {
        bail!("re-encoded {ovl} did not round-trip; refusing to ship");
    }
    pc98_madou_ars::fat12_replace::replace_file_in_place(&mut disk, ovl, &reencoded)?;

    // Combined sheet + matching hook -> KFONT.BIN loaded to 0x8800:0.
    const HOOK_OFF: usize = pc98_madou_ars::hook_geometry::HOOK_ENTRY_OFF as usize;
    const SCRATCH_OFF: usize = pc98_madou_ars::hook_geometry::SCRATCH_OFF as usize;
    if sheet_bin.len() > SCRATCH_OFF {
        bail!(
            "glyph sheet {} bytes reaches the hook scratch word at 0x{SCRATCH_OFF:X}",
            sheet_bin.len()
        );
    }
    let mut combined = vec![0u8; HOOK_OFF + hook.len()];
    combined[..sheet_bin.len()].copy_from_slice(&sheet_bin);
    combined[HOOK_OFF..].copy_from_slice(&hook);
    let combined_size = u16::try_from(combined.len()).context("combined KFONT.BIN too large")?;

    let main_com = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "MAIN.COM")?;
    let patched_main = pc98_madou_ars::hangul_probe::patch_main_com_load_combined(
        &main_com,
        combined_size,
        "KFONT.BIN",
    )?;
    pc98_madou_ars::fat12_replace::replace_file_in_place(&mut disk, "MAIN.COM", &patched_main)?;

    let kfont_meta = pc98_madou_ars::fat12_add::RootFileMeta {
        attr: 0x20,
        date: 0,
        time: 0,
    };
    pc98_madou_ars::fat12_add::add_root_file(&mut disk, "KFONT.BIN", &combined, kfont_meta)
        .context("add KFONT.BIN to disk")?;
    if pc98_madou_ars::read_fat12_file_from_hdm(&disk, "KFONT.BIN")? != combined {
        bail!("KFONT.BIN readback did not match");
    }

    add_file_from_disk(&mut disk, &demo_disk, "TC.CNS")?;
    if pc98_madou_ars::read_fat12_file_from_hdm(&data_disk, mado).is_ok() {
        add_file_from_disk(&mut disk, &data_disk, mado)?;
        println!("staged {mado}");
    } else {
        println!("note: {mado} not on the data disk; boot may need the matching Data disk");
    }
    write_output_disk(&output_path, &disk)?;
    println!(
        "wrote {} ({ovl}, {}-byte KFONT.BIN)",
        output_path.display(),
        combined_size
    );
    Ok(())
}

/// Patch a NUL-terminated string slot in `decoded` in place with `enc`. Derives
/// the REAL slot length from the image (up to and including the terminating NUL)
/// and validates any caller-declared `budget` against it, so a stale offset or a
/// wrong/oversized budget fails loudly instead of clobbering the neighbouring
/// string or leaking a trailing byte. Always writes a NUL after the Korean.
/// Returns Ok(true) patched, Ok(false) over-budget (left as-is), Err on a
/// range/consistency violation.
fn patch_in_place_slot(
    decoded: &mut [u8],
    off: usize,
    enc: &[u8],
    budget: usize,
    label: &str,
) -> Result<bool> {
    if off >= decoded.len() {
        bail!(
            "{label}: offset 0x{off:04X} past decoded image (len 0x{:04X})",
            decoded.len()
        );
    }
    // Always derive the slot from the LIVE image (up to the terminating NUL) and
    // pad to it -- this is what makes an over/under-sized declared budget unable to
    // clobber the neighbour or leak a trailing byte. The declared budget is only a
    // cross-check; a mismatch is a data smell worth surfacing, not a hard stop
    // (e.g. a message whose neighbour was already shortened moves the NUL).
    let slot = decoded[off..]
        .iter()
        .position(|&b| b == 0)
        .map(|n| n + 1)
        .with_context(|| {
            format!("{label}: offset 0x{off:04X} has no NUL terminator; not a string slot")
        })?;
    if budget != 0 && budget != slot {
        eprintln!("  {label}: declared byte_budget {budget} != live slot {slot} (using live slot)");
    }
    if enc.len() + 1 > slot {
        return Ok(false);
    }
    decoded[off..off + enc.len()].copy_from_slice(enc);
    for pad in decoded[off + enc.len()..off + slot].iter_mut() {
        *pad = 0; // clears the old tail and guarantees the terminator
    }
    Ok(true)
}

fn build_translation<'a>(
    entry: &'a serde_json::Value,
    allow_needs_review: bool,
    label: &str,
) -> Result<&'a str> {
    let ko = entry
        .get("ko")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if ko.is_empty() {
        bail!("{label}: translation is empty");
    }
    let status = entry
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    match status {
        "complete" => Ok(ko),
        "needs_review" | "translated" if allow_needs_review => Ok(ko),
        "needs_review" | "translated" => bail!(
            "{label}: status={status}; shipping builds require status=complete (use --allow-needs-review only for a draft PoC)"
        ),
        _ => bail!("{label}: unsupported translation status {status:?}"),
    }
}

#[allow(clippy::too_many_arguments)]
fn build_char_disk(
    game_disk_path: PathBuf,
    translations_dir: PathBuf,
    sheet_path: PathBuf,
    sheet_json_path: PathBuf,
    allow_needs_review: bool,
    output_path: PathBuf,
) -> Result<()> {
    use pc98_madou_ars::hook_geometry::renderer_hook;
    use pc98_madou_ars::overlay_batch::apply_translations_in_source_slots;
    use pc98_madou_ars::overlay_lz::{decode_overlay_lz, encode_overlay_lz};
    use pc98_madou_ars::overlay_messages::{first_inconsistent_site, unified_catalog};
    use pc98_madou_ars::overlay_reloc::SheetCodes;
    use std::collections::HashMap;

    const LOAD_OFFSET: usize = 0x100;

    let mut disk = std::fs::read(&game_disk_path)
        .with_context(|| format!("read {}", game_disk_path.display()))?;
    let sheet_bin =
        std::fs::read(&sheet_path).with_context(|| format!("read {}", sheet_path.display()))?;
    let sheet_json = std::fs::read_to_string(&sheet_json_path)
        .with_context(|| format!("read {}", sheet_json_path.display()))?;
    let sheet = SheetCodes::from_json_str(&sheet_json)
        .with_context(|| format!("parse {}", sheet_json_path.display()))?;
    let josa_data =
        pc98_madou_ars::josa::build_runtime_data_from_json(&sheet_json).with_context(|| {
            format!(
                "build runtime particle metadata from {}",
                sheet_json_path.display()
            )
        })?;
    let cfg = pc98_madou_ars::character_build::CHARACTER_BUILD_PROFILES
        .iter()
        .find(|c| pc98_madou_ars::read_fat12_file_from_hdm(&disk, c.game_overlay).is_ok())
        .context("no GAME_A/R/S.OVL on this Game disk")?;
    let hook = renderer_hook(cfg.renderer_overlay)?;
    println!("character overlay: {}", cfg.game_overlay);

    // Merge every translation batch whose `overlay` matches this one. Sort the
    // directory so the SSoT tie-break (keep-shorter) is reproducible across
    // machines rather than dependent on filesystem read_dir order.
    let mut translations: HashMap<
        usize,
        (String, pc98_madou_ars::character_build::StagedSourceSlot),
    > = HashMap::new();
    let mut batch_paths: Vec<PathBuf> = std::fs::read_dir(&translations_dir)
        .with_context(|| format!("read dir {}", translations_dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
        .collect();
    batch_paths.sort();
    for path in &batch_paths {
        let text = std::fs::read_to_string(path)?;
        let json: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if json.get("overlay").and_then(|v| v.as_str()) != Some(cfg.game_overlay) {
            continue;
        }
        for e in json
            .get("entries")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
        {
            let id = e.get("id").and_then(|value| value.as_str()).unwrap_or("?");
            let label = format!("{}:{id}", path.display());
            let ko = build_translation(e, allow_needs_review, &label)?;
            let source_slot =
                pc98_madou_ars::character_build::StagedSourceSlot::from_json(e, &label)?;
            let off = source_slot.decoded_offset;
            if translations
                .insert(off, (ko.to_string(), source_slot))
                .is_some()
            {
                bail!(
                    "{} has more than one translation for decoded offset 0x{off:04X}",
                    cfg.game_overlay
                );
            }
        }
    }
    println!(
        "merged {} translations for {} from {}",
        translations.len(),
        cfg.game_overlay,
        translations_dir.display()
    );
    if translations.is_empty() {
        bail!(
            "no translations matched overlay {} in {}",
            cfg.game_overlay,
            translations_dir.display()
        );
    }
    if translations.len() != cfg.expected_translations {
        bail!(
            "{} staged corpus has {} entries; expected {} ({} renderer-relocatable + {} fixed-slot + {} preserved source)",
            cfg.game_overlay,
            translations.len(),
            cfg.expected_translations,
            cfg.expected_cataloged,
            cfg.expected_uncovered,
            cfg.expected_preserved,
        );
    }

    // Decode, catalog messages from the pristine image, install trampolines.
    let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, cfg.game_overlay)?;
    let mut decoded = decode_overlay_lz(&packed)?.output;
    let source_decoded_len = decoded.len();
    for (_, source_slot) in translations.values() {
        source_slot.validate_against(&decoded, cfg.game_overlay)?;
    }
    let detected_preserved =
        pc98_madou_ars::character_build::source_identity_slots(&decoded, LOAD_OFFSET);
    let declared_preserved: std::collections::HashSet<usize> = translations
        .values()
        .filter_map(|(_, slot)| {
            (slot.application
                == pc98_madou_ars::character_build::SourceSlotApplication::PreserveSource)
                .then_some(slot.decoded_offset)
        })
        .collect();
    if declared_preserved != detected_preserved {
        let mut missing_declarations = detected_preserved
            .difference(&declared_preserved)
            .map(|offset| format!("0x{offset:04X}"))
            .collect::<Vec<_>>();
        let mut unsupported_declarations = declared_preserved
            .difference(&detected_preserved)
            .map(|offset| format!("0x{offset:04X}"))
            .collect::<Vec<_>>();
        missing_declarations.sort();
        unsupported_declarations.sort();
        bail!(
            "{} preserve-source declarations do not match the statically proven disk-identity consumers: missing [{}], unsupported [{}]",
            cfg.game_overlay,
            missing_declarations.join(", "),
            unsupported_declarations.join(", "),
        );
    }
    if declared_preserved.len() != cfg.expected_preserved {
        bail!(
            "{} has {} preserved source slot(s); expected {}",
            cfg.game_overlay,
            declared_preserved.len(),
            cfg.expected_preserved,
        );
    }
    let messages = unified_catalog(&decoded, LOAD_OFFSET, 4, &[2, 4, 6, 8, 10, 12, 14, 16])?;
    if messages.len() != cfg.expected_cataloged {
        bail!(
            "{} extractor produced {} cataloged messages; expected {}",
            cfg.game_overlay,
            messages.len(),
            cfg.expected_cataloged
        );
    }
    install_trampoline_by_pattern(
        &mut decoded,
        &pc98_madou_ars::hook_geometry::ENTRY_TRAMPOLINE_SOURCE,
        &pc98_madou_ars::hook_geometry::entry_trampoline(),
        "entry",
    )?;
    install_trampoline_by_pattern(
        &mut decoded,
        &pc98_madou_ars::hook_geometry::DRAW_SETUP_TRAMPOLINE_ANCHOR,
        &pc98_madou_ars::hook_geometry::draw_trampoline(),
        "draw",
    )?;

    // Split staged source slots by proven consumer. Renderer references can be
    // relocated, fixed-slot visible text stays in place, and disk identities are
    // validated but never translated.
    let by_offset: HashMap<usize, &_> = messages
        .iter()
        .map(|m| (m.string_decoded_offset, m))
        .collect();
    let mut cataloged: HashMap<usize, String> = HashMap::new();
    let mut uncovered: Vec<(usize, String, usize)> = Vec::new();
    for (off, (ko, source_slot)) in translations {
        if source_slot.application
            == pc98_madou_ars::character_build::SourceSlotApplication::PreserveSource
        {
            if by_offset.contains_key(&off) {
                bail!(
                    "{}:{} is marked preserve_source but also has a renderer rewrite owner",
                    cfg.game_overlay,
                    source_slot.id,
                );
            }
            continue;
        }
        let budget = source_slot.byte_budget;
        if let Some(message) = by_offset.get(&off) {
            if message.byte_budget != budget {
                bail!(
                    "{}:{} source budget {} does not match catalog budget {}",
                    cfg.game_overlay,
                    source_slot.id,
                    budget,
                    message.byte_budget
                );
            }
            if let Some(bad) = first_inconsistent_site(&decoded, message) {
                bail!(
                    "rewrite site 0x{:04X} for 0x{off:04X} no longer holds its offset",
                    bad.site
                );
            }
            cataloged.insert(off, ko);
        } else {
            uncovered.push((off, ko, budget));
        }
    }

    if cataloged.len() != messages.len() {
        let missing = messages
            .iter()
            .filter(|message| !cataloged.contains_key(&message.string_decoded_offset))
            .map(|message| format!("0x{:04X}", message.string_decoded_offset))
            .collect::<Vec<_>>();
        bail!(
            "{} has {} untranslated cataloged message(s): {}",
            cfg.game_overlay,
            missing.len(),
            missing.join(", ")
        );
    }
    if uncovered.len() != cfg.expected_uncovered {
        bail!(
            "{} has {} in-place extraction-gap translations; expected {}",
            cfg.game_overlay,
            uncovered.len(),
            cfg.expected_uncovered
        );
    }

    for (offset, _, budget) in &uncovered {
        let uncovered_end = offset
            .checked_add(*budget)
            .context("uncovered translation slot overflow")?;
        if let Some(message) = messages.iter().find(|message| {
            let message_end = message.string_decoded_offset + message.byte_budget;
            *offset < message_end && message.string_decoded_offset < uncovered_end
        }) {
            bail!(
                "uncovered slot 0x{offset:04X}..0x{uncovered_end:04X} overlaps cataloged source slot 0x{:04X}",
                message.string_decoded_offset
            );
        }
    }

    let mut stub_sizes = vec![
        pc98_madou_ars::josa::OVERLAY_STUB_LEN,
        pc98_madou_ars::received_damage_sfx::S0_DAMAGE_OVERLAY_STUB_BYTES,
    ];
    if cfg.id == "rulue" {
        stub_sizes.push(pc98_madou_ars::failure_message::STUB_BYTES);
    }
    if cfg.id != "rulue" {
        stub_sizes.push(pc98_madou_ars::illusion::STUB_BYTES);
    }
    stub_sizes.push(pc98_madou_ars::dancer_thunder::STUB_BYTES);
    let report = apply_translations_in_source_slots(
        &mut decoded,
        LOAD_OFFSET,
        &messages,
        &cataloged,
        &sheet,
        &stub_sizes,
    )?;
    println!(
        "applied {} cataloged inside source slots: {} in place, {} relocated, {}/{} bytes including stubs, {} cataloged untranslated",
        cataloged.len(),
        report.in_place,
        report.relocated,
        report.slot_bytes_used,
        report.slot_capacity,
        report.untranslated,
    );

    // Uncovered: patch each in place (overwrite the slot, NUL-pad), requiring fit.
    let mut unc_ok = 0usize;
    for (off, ko, budget) in &uncovered {
        let enc = sheet
            .encode_line(ko)
            .map_err(|e| e.context(format!("uncovered 0x{off:04X}")))?;
        if patch_in_place_slot(
            &mut decoded,
            *off,
            &enc,
            *budget,
            &format!("uncovered 0x{off:04X}"),
        )? {
            unc_ok += 1;
        } else {
            bail!(
                "uncovered 0x{off:04X} is {} bytes and does not fit its {budget}-byte slot",
                enc.len() + 1
            );
        }
    }
    if !uncovered.is_empty() {
        println!("uncovered (extraction-gap) in place: {unc_ok} patched");
    }
    println!(
        "preserved {} statically proven disk-identity source slot(s)",
        declared_preserved.len()
    );

    if cfg.id == "schezo" {
        pc98_madou_ars::lever_text::install(
            &mut decoded,
            &report.placements,
            &sheet,
            &pc98_madou_ars::lever_text::state_catalog(&translations_dir)?,
            allow_needs_review,
        )?;
    }

    pc98_madou_ars::save_encounter::install(&mut decoded, cfg.renderer_overlay)?;
    pc98_madou_ars::shop_sale::install(&mut decoded, cfg.renderer_overlay)?;
    if cfg.id == "rulue" {
        pc98_madou_ars::failure_message::install(&mut decoded, report.reservations[2].offset)?;
    }

    if cfg.id != "rulue" {
        pc98_madou_ars::illusion::install(
            &mut decoded,
            cfg.renderer_overlay,
            report.reservations[2].offset,
        )?;
    }
    pc98_madou_ars::dancer_thunder::install(
        &mut decoded,
        cfg.renderer_overlay,
        report
            .reservations
            .last()
            .context("missing thunder reservation")?
            .offset,
    )?;

    let topic_reservation = report.reservations[0];
    let damage_reservation = report.reservations[1];
    let josa = pc98_madou_ars::josa::install_topic_particle_selector_at(
        &mut decoded,
        LOAD_OFFSET,
        topic_reservation.offset,
    )?;
    println!(
        "dynamic topic particles: {} sites -> stub decoded 0x{:04X} (logical 0x{:04X}), direct draw 0x{:04X}",
        josa.sites.len(),
        josa.stub_decoded_offset,
        josa.stub_logical_offset,
        josa.direct_draw_target,
    );

    let damage = pc98_madou_ars::received_damage_sfx::install_s0_damage_route_at(
        &mut decoded,
        cfg.renderer_overlay,
        LOAD_OFFSET,
        damage_reservation.offset,
    )?;
    println!(
        "isolated S0 playback: combo/damage calls decoded 0x{:04X}/0x{:04X} -> stub decoded 0x{:04X} (logical 0x{:04X})",
        damage.call_decoded_offset,
        damage.damage_call_decoded_offset,
        damage.stub_decoded_offset,
        damage.stub_logical_offset,
    );
    if decoded.len() != source_decoded_len {
        bail!(
            "{} decoded extent changed from source 0x{source_decoded_len:X} to 0x{:X}",
            cfg.game_overlay,
            decoded.len()
        );
    }

    let reencoded = encode_overlay_lz(&decoded);
    if decode_overlay_lz(&reencoded)?.output != decoded {
        bail!(
            "re-encoded {} did not round-trip; refusing to ship",
            cfg.game_overlay
        );
    }
    let ovl_report =
        pc98_madou_ars::fat12_add::replace_file_grow(&mut disk, cfg.game_overlay, &reencoded)?;
    // Defence-in-depth: read the overlay back through FAT12 and re-decode it, so a
    // FAT-layer regression can never ship a corrupt overlay silently.
    let ovl_back = pc98_madou_ars::read_fat12_file_from_hdm(&disk, cfg.game_overlay)
        .with_context(|| format!("readback {}", cfg.game_overlay))?;
    if decode_overlay_lz(&ovl_back)?.output != decoded {
        bail!(
            "readback of {} did not decode to the patched content; FAT write is wrong",
            cfg.game_overlay
        );
    }
    println!(
        "re-encoded {}: {} -> {} packed bytes ({} clusters, first {:#X})",
        cfg.game_overlay,
        packed.len(),
        reencoded.len(),
        ovl_report.clusters,
        ovl_report.first_cluster,
    );

    // Combined per-overlay sheet + matching hook -> KFONT.BIN loaded to 0x8800:0.
    const HOOK_OFF: usize = pc98_madou_ars::hook_geometry::HOOK_ENTRY_OFF as usize;
    const SCRATCH_OFF: usize = pc98_madou_ars::hook_geometry::SCRATCH_OFF as usize;
    if sheet_bin.len() > SCRATCH_OFF {
        bail!(
            "sheet {} bytes reaches the hook scratch word at 0x{SCRATCH_OFF:X}",
            sheet_bin.len()
        );
    }
    let josa_start = pc98_madou_ars::josa::RUNTIME_DATA_OFF;
    let josa_end = josa_start
        .checked_add(josa_data.len())
        .context("runtime particle metadata range overflow")?;
    if sheet_bin.len() > josa_start {
        bail!(
            "sheet {} bytes overlaps runtime particle metadata at 0x{josa_start:X}",
            sheet_bin.len()
        );
    }
    if josa_end > SCRATCH_OFF {
        bail!(
            "runtime particle metadata ends at 0x{josa_end:X}, reaching hook scratch 0x{SCRATCH_OFF:X}"
        );
    }
    let mut combined = vec![0u8; HOOK_OFF + hook.len()];
    combined[..sheet_bin.len()].copy_from_slice(&sheet_bin);
    combined[josa_start..josa_end].copy_from_slice(&josa_data);
    combined[HOOK_OFF..].copy_from_slice(&hook);
    let damage_scratch =
        usize::from(pc98_madou_ars::received_damage_sfx::S0_DAMAGE_TRACK_SCRATCH_OFF);
    if combined.len() > damage_scratch {
        bail!(
            "combined KFONT.BIN ends at 0x{:04X}, overlapping the received-damage S0 scratch at 0x{damage_scratch:04X}",
            combined.len()
        );
    }
    let combined_size = u16::try_from(combined.len()).context("combined KFONT.BIN too large")?;
    let main_com = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "MAIN.COM")?;
    let patched_main = pc98_madou_ars::hangul_probe::patch_main_com_load_combined(
        &main_com,
        combined_size,
        pc98_madou_ars::character_build::RENDERER_IMAGE_FILE,
    )?;
    pc98_madou_ars::fat12_replace::replace_file_in_place(&mut disk, "MAIN.COM", &patched_main)?;
    let kfont_meta = pc98_madou_ars::fat12_add::RootFileMeta {
        attr: 0x20,
        date: 0,
        time: 0,
    };
    pc98_madou_ars::fat12_add::add_root_file(&mut disk, "KFONT.BIN", &combined, kfont_meta)
        .context("add KFONT.BIN")?;
    if pc98_madou_ars::read_fat12_file_from_hdm(&disk, "KFONT.BIN")? != combined {
        bail!("KFONT.BIN readback did not match sheet + particle metadata + hook");
    }
    write_output_disk(&output_path, &disk)?;
    println!(
        "wrote {} ({}, {}-byte KFONT.BIN)",
        output_path.display(),
        cfg.game_overlay,
        combined_size
    );
    Ok(())
}

fn patch_disk_dats(
    disk_path: PathBuf,
    translations_dir: PathBuf,
    sheet_json_path: PathBuf,
    allow_needs_review: bool,
    output_path: PathBuf,
) -> Result<()> {
    use pc98_madou_ars::overlay_lz::{decode_overlay_lz, encode_overlay_lz};
    use pc98_madou_ars::overlay_reloc::SheetCodes;

    let mut disk =
        std::fs::read(&disk_path).with_context(|| format!("read {}", disk_path.display()))?;
    let sheet = SheetCodes::load(&sheet_json_path)?;

    let mut dat_batches: Vec<(String, serde_json::Value)> = Vec::new();
    let mut dat_paths: Vec<PathBuf> = std::fs::read_dir(&translations_dir)
        .with_context(|| format!("read dir {}", translations_dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("json"))
        .collect();
    dat_paths.sort();
    // Detect a second batch translating the same (dat, offset) -> SSoT violation.
    let mut seen_dat_offsets: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    for path in &dat_paths {
        let mut json: serde_json::Value =
            match serde_json::from_str(&std::fs::read_to_string(path)?) {
                Ok(v) => v,
                Err(_) => continue,
            };
        if pc98_madou_ars::translation_binding::has_bound_translations(&json) {
            let translation_root = translations_dir.parent().with_context(|| {
                format!(
                    "bound DAT catalog {} requires its parent translation root",
                    path.display()
                )
            })?;
            json = pc98_madou_ars::translation_binding::resolve_bound_translations(
                &json,
                translation_root,
            )
            .with_context(|| format!("resolve bound DAT catalog {}", path.display()))?;
        }
        if let Some(dat) = json.get("dat").and_then(|v| v.as_str()) {
            for e in json
                .get("entries")
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
            {
                if e.get("ko")
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| !s.is_empty())
                    && let Some(off) = e.get("string_decoded_offset").and_then(|v| v.as_str())
                    && !seen_dat_offsets.insert((dat.to_string(), off.to_string()))
                {
                    bail!("SSoT conflict: {dat} {off} translated in more than one DAT batch");
                }
            }
            dat_batches.push((dat.to_string(), json));
        }
    }

    // This disk's character, so a batch tagged for THIS character whose DAT is
    // missing is a typo/missing-file error (loud), not a legit other-disk skip.
    let (disk_char, expected_dats, expected_messages) = [
        ("GAME_A.OVL", "A", 29usize, 298usize),
        ("GAME_R.OVL", "R", 30usize, 217usize),
        ("GAME_S.OVL", "S", 27usize, 259usize),
    ]
    .iter()
    .find(|(ovl, _, _, _)| pc98_madou_ars::read_fat12_file_from_hdm(&disk, ovl).is_ok())
    .map(|(_, character, dats, messages)| (*character, *dats, *messages))
    .context("no GAME_A/R/S.OVL on the DAT target disk")?;

    let (mut n_dats, mut n_msgs, mut n_over, mut n_skipped) = (0usize, 0usize, 0usize, 0usize);
    for (dat, json) in &dat_batches {
        let batch_char = json.get("char").and_then(|v| v.as_str());
        let packed = match pc98_madou_ars::read_fat12_file_from_hdm(&disk, dat) {
            Ok(p) => p,
            Err(_) => {
                // A batch for this disk's character MUST have its DAT present.
                if batch_char == Some(disk_char) {
                    bail!(
                        "DAT batch for '{dat}' (char {batch_char:?}) is not on this disk; typo or missing file"
                    );
                }
                n_skipped += 1;
                continue; // belongs to another character's disk
            }
        };
        let mut decoded = decode_overlay_lz(&packed)?.output;
        pc98_madou_ars::dat_catalog::validate_dat_translation_catalog(&decoded, json)
            .with_context(|| format!("audit complete DAT slot coverage for {dat}"))?;
        let (mut patched, mut over) = (0usize, 0usize);
        let entries = json
            .get("entries")
            .and_then(|v| v.as_array())
            .with_context(|| format!("{dat}: missing entries array"))?;
        let mut entries_by_id = std::collections::HashMap::new();
        for entry in entries {
            let id = entry
                .get("id")
                .and_then(|value| value.as_str())
                .with_context(|| format!("{dat}: DAT entry is missing id"))?;
            if entries_by_id.insert(id, entry).is_some() {
                bail!("{dat}: duplicate DAT entry id {id}");
            }
        }

        struct PendingDatRepack {
            id: String,
            source_offset: usize,
            source_budget: usize,
            encoded: Vec<u8>,
            rewrite_sites: Vec<usize>,
        }

        let mut grouped_ids = std::collections::HashSet::new();
        if let Some(groups) = json.get("repack_groups") {
            let groups = groups
                .as_array()
                .with_context(|| format!("{dat}: repack_groups must be an array"))?;
            for (group_index, group) in groups.iter().enumerate() {
                let entry_ids = group
                    .get("entry_ids")
                    .and_then(|value| value.as_array())
                    .with_context(|| {
                        format!("{dat}: repack group {group_index} is missing entry_ids")
                    })?;
                if entry_ids.is_empty() {
                    bail!("{dat}: repack group {group_index} has no entries");
                }

                let mut pending = Vec::with_capacity(entry_ids.len());
                for id_value in entry_ids {
                    let id = id_value.as_str().with_context(|| {
                        format!("{dat}: repack group {group_index} has a non-string entry id")
                    })?;
                    if !grouped_ids.insert(id.to_owned()) {
                        bail!("{dat}: DAT entry {id} belongs to more than one repack group");
                    }
                    let entry = entries_by_id.get(id).with_context(|| {
                        format!("{dat}: repack group {group_index} references unknown entry {id}")
                    })?;
                    let label = format!("{dat}:{id}");
                    let ko = build_translation(entry, allow_needs_review, &label)?;
                    let source_offset = entry
                        .get("string_decoded_offset")
                        .and_then(|value| value.as_str())
                        .with_context(|| format!("{label}: missing string_decoded_offset"))
                        .and_then(parse_usize)?;
                    let source_budget = entry
                        .get("byte_budget")
                        .and_then(|value| value.as_u64())
                        .with_context(|| format!("{label}: missing byte_budget"))?
                        as usize;
                    let encoded = sheet
                        .encode_line(ko)
                        .map_err(|error| error.context(format!("{dat} 0x{source_offset:04X}")))?;
                    let rewrite_sites = entry
                        .get("rewrite_sites")
                        .and_then(|value| value.as_array())
                        .with_context(|| format!("{label}: rewrite_sites must be an array"))?
                        .iter()
                        .map(|value| {
                            value
                                .as_str()
                                .with_context(|| {
                                    format!("{label}: rewrite site must be a hex string")
                                })
                                .and_then(parse_usize)
                        })
                        .collect::<Result<Vec<_>>>()?;
                    pending.push(PendingDatRepack {
                        id: id.to_owned(),
                        source_offset,
                        source_budget,
                        encoded,
                        rewrite_sites,
                    });
                }

                let repack_entries = pending
                    .iter()
                    .map(|entry| pc98_madou_ars::dat_repack::RepackEntry {
                        id: &entry.id,
                        source_offset: entry.source_offset,
                        source_budget: entry.source_budget,
                        encoded: &entry.encoded,
                        rewrite_sites: &entry.rewrite_sites,
                    })
                    .collect::<Vec<_>>();
                let report = pc98_madou_ars::dat_repack::repack_contiguous_group(
                    &mut decoded,
                    &repack_entries,
                )
                .with_context(|| format!("{dat}: repack group {group_index}"))?;
                println!(
                    "  {dat}: repacked {} messages into {}/{} bytes",
                    report.destinations.len(),
                    report.used,
                    report.capacity,
                );
                patched += report.destinations.len();
            }
        }

        for e in entries {
            let id = e.get("id").and_then(|value| value.as_str()).unwrap_or("?");
            if grouped_ids.contains(id) {
                continue;
            }
            let label = format!("{dat}:{id}");
            let ko = build_translation(e, allow_needs_review, &label)?;
            let off = e
                .get("string_decoded_offset")
                .and_then(|v| v.as_str())
                .context("dat entry missing string_decoded_offset")
                .and_then(parse_usize)?;
            let enc = sheet
                .encode_line(ko)
                .map_err(|err| err.context(format!("{dat} 0x{off:04X}")))?;
            let budget = e.get("byte_budget").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            if patch_in_place_slot(
                &mut decoded,
                off,
                &enc,
                budget,
                &format!("{dat} 0x{off:04X}"),
            )? {
                patched += 1;
            } else {
                over += 1;
                eprintln!("  {dat} 0x{off:04X} over budget ({} bytes)", enc.len());
            }
        }
        if over > 0 {
            bail!("{dat}: {over} translation(s) exceed their fixed slots");
        }
        let reencoded = encode_overlay_lz(&decoded);
        if decode_overlay_lz(&reencoded)?.output != decoded {
            bail!("re-encoded {dat} did not round-trip; refusing to ship");
        }
        pc98_madou_ars::fat12_add::replace_file_grow(&mut disk, dat, &reencoded)?;
        // Defence-in-depth: read the file back through FAT12 and re-decode it, so a
        // FAT-layer regression (bad chain, packing, truncation) can never ship silently.
        let back = pc98_madou_ars::read_fat12_file_from_hdm(&disk, dat)
            .with_context(|| format!("readback {dat}"))?;
        if decode_overlay_lz(&back)?.output != decoded {
            bail!("readback of {dat} did not decode to the patched content; FAT write is wrong");
        }
        println!(
            "{dat}: {patched} patched, {over} over-budget ({} -> {} bytes)",
            packed.len(),
            reencoded.len()
        );
        n_dats += 1;
        n_msgs += patched;
        n_over += over;
    }
    if n_skipped > 0 {
        println!("(skipped {n_skipped} DAT batches for other characters' disks)");
    }
    if n_dats != expected_dats || n_msgs != expected_messages {
        bail!(
            "{disk_char} DAT corpus applied {n_dats} files / {n_msgs} messages; expected {expected_dats} / {expected_messages}"
        );
    }

    write_output_disk(&output_path, &disk)?;
    println!(
        "patched {n_dats} DAT files, {n_msgs} messages ({n_over} over-budget); wrote {}",
        output_path.display()
    );
    Ok(())
}

fn replace_file_cmd(
    disk_path: PathBuf,
    name: &str,
    content_path: PathBuf,
    output_path: PathBuf,
) -> Result<()> {
    let mut disk =
        std::fs::read(&disk_path).with_context(|| format!("read {}", disk_path.display()))?;
    let data =
        std::fs::read(&content_path).with_context(|| format!("read {}", content_path.display()))?;
    let report = pc98_madou_ars::fat12_replace::replace_file_in_place(&mut disk, name, &data)
        .with_context(|| format!("replace {name}"))?;
    if pc98_madou_ars::read_fat12_file_from_hdm(&disk, name)? != data {
        bail!("readback of {name} did not match the new content");
    }
    write_output_disk(&output_path, &disk)?;
    println!(
        "replaced {name} ({} bytes, capacity {} bytes); readback OK; wrote {}",
        report.bytes,
        report.capacity,
        output_path.display()
    );
    Ok(())
}

fn add_file_cmd(
    disk_path: PathBuf,
    file_path: PathBuf,
    name: &str,
    output_path: PathBuf,
) -> Result<()> {
    let mut disk =
        std::fs::read(&disk_path).with_context(|| format!("read {}", disk_path.display()))?;
    let data =
        std::fs::read(&file_path).with_context(|| format!("read {}", file_path.display()))?;
    // Plain archive file; fixed timestamp keeps the build reproducible.
    let meta = pc98_madou_ars::fat12_add::RootFileMeta {
        attr: 0x20,
        date: 0,
        time: 0,
    };
    let report = pc98_madou_ars::fat12_add::add_root_file(&mut disk, name, &data, meta)
        .with_context(|| format!("add {name} to {}", disk_path.display()))?;

    // Verify the file reads back byte-identically through FAT12.
    let readback = pc98_madou_ars::read_fat12_file_from_hdm(&disk, name)?;
    if readback != data {
        bail!("readback of {name} did not match the source file");
    }

    write_output_disk(&output_path, &disk)?;
    println!(
        "added {} ({} bytes, {} clusters, first cluster {:#X}); readback OK; wrote {}",
        report.file_name,
        report.bytes,
        report.clusters,
        report.first_cluster,
        output_path.display()
    );
    Ok(())
}

fn build_gaiji_rows_probe(disk_path: PathBuf, output_path: PathBuf) -> Result<()> {
    use pc98_madou_ars::hangul_probe::GaijiGlyph;

    // Hollow-box marker: top/bottom full rows, side columns elsewhere.
    let mut box_glyph = [0u8; 32];
    box_glyph[0] = 0xFF;
    box_glyph[1] = 0xFF;
    for row in 1..15 {
        box_glyph[row * 2] = 0x80;
        box_glyph[row * 2 + 1] = 0x01;
    }
    box_glyph[30] = 0xFF;
    box_glyph[31] = 0xFF;

    // First cell of three consecutive gaiji rows: 0x76, 0x77, 0x78.
    // SJIS: 0x7621->EB9F, 0x7721->EC40, 0x7821->EC9F.
    let codes = [
        (0x7621u16, [0xEBu8, 0x9F]),
        (0x7721, [0xEC, 0x40]),
        (0x7821, [0xEC, 0x9F]),
    ];

    let mut disk =
        std::fs::read(&disk_path).with_context(|| format!("read {}", disk_path.display()))?;

    let glyphs: Vec<GaijiGlyph> = codes
        .iter()
        .map(|&(jis, _)| GaijiGlyph {
            jis,
            bitmap: box_glyph,
        })
        .collect();
    let main_com = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "MAIN.COM")?;
    let patched_main =
        pc98_madou_ars::hangul_probe::patch_main_com_register_gaiji_multi(&main_com, &glyphs)?;
    pc98_madou_ars::fat12_replace::replace_file_in_place(&mut disk, "MAIN.COM", &patched_main)?;
    println!("registered box marker at rows 0x76/0x77/0x78 (codes EB9F, EC40, EC9F)");

    // Patch the GAOO boot prompt: the three box codes followed by filler, keeping
    // the original 14-byte length of 好きなドライブ.
    let mut gaoo = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "GAOO.OVL")?;
    let from = pc98_madou_ars::sjis_marker::encode_sjis("好きなドライブ")?;
    let mut to = pc98_madou_ars::sjis_marker::encode_sjis("ストテスストテ")?;
    for (slot, (_, sjis)) in codes.iter().enumerate() {
        to[slot * 2..slot * 2 + 2].copy_from_slice(sjis);
    }
    if to.len() != from.len() {
        bail!("boot prompt replacement length mismatch");
    }
    let offsets = pc98_madou_ars::sjis_marker::replace_all_exact(&mut gaoo, &from, &to)?;
    pc98_madou_ars::fat12_replace::replace_file_in_place(&mut disk, "GAOO.OVL", &gaoo)?;
    println!("patched GAOO boot prompt at offsets {offsets:04X?} (box box box ストテ)");

    write_output_disk(&output_path, &disk)?;
    println!("wrote {}", output_path.display());
    println!("expected: count the boxes in the boot prompt -> 1=row 0x76 only, 2=+0x77, 3=+0x78");
    Ok(())
}

fn build_multi_hangul_poc(
    game_disk_path: PathBuf,
    demo_disk_path: PathBuf,
    data_disk_path: PathBuf,
    font_profile_path: &std::path::Path,
    enemies: &[String],
    output_path: PathBuf,
) -> Result<()> {
    let mut disk = std::fs::read(&game_disk_path)
        .with_context(|| format!("read {}", game_disk_path.display()))?;
    let demo_disk = std::fs::read(&demo_disk_path)
        .with_context(|| format!("read {}", demo_disk_path.display()))?;
    let data_disk = std::fs::read(&data_disk_path)
        .with_context(|| format!("read {}", data_disk_path.display()))?;

    let mut demand = std::collections::BTreeSet::new();
    for mapping in enemies {
        let (_, korean) = mapping
            .split_once('=')
            .with_context(|| format!("--enemy expects FILE=KOREAN, got {mapping:?}"))?;
        demand.extend(korean.chars().filter(|ch| ('가'..='힣').contains(ch)));
    }
    let profile = pc98_madou_ars::font_build::FontProfile::load(font_profile_path)?;
    let generated = pc98_madou_ars::font_build::build_gaiji_font(&profile, &demand)?;
    let table = pc98_madou_ars::gaiji_table::GaijiTable::from_json_str(&serde_json::to_string(
        &generated.metadata,
    )?)?;
    println!(
        "derived {} gaiji glyph(s) from {} (font {})",
        table.entries.len(),
        font_profile_path.display(),
        table.font
    );

    // 1. Register every gaiji glyph at boot.
    let main_com = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "MAIN.COM")?;
    let patched_main = pc98_madou_ars::hangul_probe::patch_main_com_register_gaiji_multi(
        &main_com,
        &table.gaiji_glyphs(),
    )?;
    let main_report =
        pc98_madou_ars::fat12_replace::replace_file_in_place(&mut disk, "MAIN.COM", &patched_main)?;
    println!(
        "registered {} gaiji glyph(s) via MAIN.COM stub ({} bytes, capacity {} bytes)",
        table.entries.len(),
        main_report.bytes,
        main_report.capacity
    );

    // 2. Replace each enemy name with real Korean encoded as gaiji codes.
    for mapping in enemies {
        let (enemy_file, korean) = mapping
            .split_once('=')
            .with_context(|| format!("--enemy expects FILE=KOREAN, got {mapping:?}"))?;
        let replacement = table.encode(korean)?;
        let mut enemy = pc98_madou_ars::read_fat12_file_from_hdm(&disk, enemy_file)?;
        let patch =
            pc98_madou_ars::enemy_text::overwrite_first_enemy_name(&mut enemy, &replacement)
                .with_context(|| format!("overwrite {enemy_file} name with {korean:?}"))?;
        let enemy_report =
            pc98_madou_ars::fat12_replace::replace_file_in_place(&mut disk, enemy_file, &enemy)?;
        println!(
            "patched {enemy_file} name '{}' -> '{korean}' at 0x{:04X}..0x{:04X} ({} bytes, capacity {} bytes)",
            patch.original_name,
            patch.name_offset,
            patch.name_end_offset,
            enemy_report.bytes,
            enemy_report.capacity
        );
    }

    // 3. Stage the two-floppy boot-smoke media workaround.
    let tc_report = add_file_from_disk(&mut disk, &demo_disk, "TC.CNS")?;
    let mado_a2_report = add_file_from_disk(&mut disk, &data_disk, "MADO-A2.DAT")?;
    write_output_disk(&output_path, &disk)?;

    println!(
        "added {} ({} bytes, {} clusters) and {} ({} bytes, {} clusters); wrote {}",
        tc_report.file_name,
        tc_report.bytes,
        tc_report.clusters,
        mado_a2_report.file_name,
        mado_a2_report.bytes,
        mado_a2_report.clusters,
        output_path.display()
    );
    Ok(())
}

fn add_file_from_disk(
    target_disk: &mut [u8],
    source_disk: &[u8],
    file_name: &str,
) -> Result<pc98_madou_ars::fat12_add::AddReport> {
    let data = pc98_madou_ars::read_fat12_file_from_hdm(source_disk, file_name)?;
    let meta = pc98_madou_ars::fat12_add::root_file_meta(source_disk, file_name)?;
    pc98_madou_ars::fat12_add::add_root_file(target_disk, file_name, &data, meta)
        .with_context(|| format!("add {file_name} to target disk"))
}

fn patch_gaoo_boot_prompt(disk: &mut [u8]) -> Result<pc98_madou_ars::fat12_replace::ReplaceReport> {
    let mut overlay = pc98_madou_ars::read_fat12_file_from_hdm(disk, "GAOO.OVL")?;
    let from = pc98_madou_ars::sjis_marker::encode_sjis("好きなドライブ")?;
    let to = pc98_madou_ars::sjis_marker::encode_sjis("テストテストテ")?;
    let offsets = pc98_madou_ars::sjis_marker::replace_all_exact(&mut overlay, &from, &to)?;
    println!("patched GAOO.OVL offsets: {offsets:04X?}");
    pc98_madou_ars::fat12_replace::replace_file_in_place(disk, "GAOO.OVL", &overlay)
}

fn write_output_disk(output_path: &PathBuf, disk: &[u8]) -> Result<()> {
    if let Some(parent) = output_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("mkdir -p {}", parent.display()))?;
    }
    std::fs::write(output_path, disk).with_context(|| format!("write {}", output_path.display()))
}

fn decode_overlay(input_path: PathBuf, output_path: Option<PathBuf>) -> Result<()> {
    let input =
        std::fs::read(&input_path).with_context(|| format!("read {}", input_path.display()))?;
    let report = pc98_madou_ars::overlay_lz::decode_overlay_lz(&input)
        .with_context(|| format!("decode {}", input_path.display()))?;

    if let Some(output_path) = output_path {
        if let Some(parent) = output_path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("mkdir -p {}", parent.display()))?;
        }
        std::fs::write(&output_path, &report.output)
            .with_context(|| format!("write {}", output_path.display()))?;
        println!(
            "decoded {} -> {} ({} bytes consumed, {} bytes output, {} commands)",
            input_path.display(),
            output_path.display(),
            report.bytes_consumed,
            report.output.len(),
            report.commands
        );
    } else {
        println!(
            "decoded {} ({} bytes consumed, {} bytes output, {} commands)",
            input_path.display(),
            report.bytes_consumed,
            report.output.len(),
            report.commands
        );
    }

    Ok(())
}

fn list_overlay_messages(
    input_paths: Vec<PathBuf>,
    renderer_offset: &str,
    load_offset: &str,
    limit: Option<usize>,
    json_output: Option<PathBuf>,
) -> Result<()> {
    let renderer_offset = parse_renderer_offset(renderer_offset)?;
    let load_offset = parse_usize(load_offset)?;
    let mut sources = Vec::new();
    let mut total_refs = 0usize;

    println!("source\trenderer\tcall_dec\tcall_log\tmode\tstr_dec\tstr_log\ttext");
    for input_path in &input_paths {
        let input =
            std::fs::read(input_path).with_context(|| format!("read {}", input_path.display()))?;
        let decoded_report = pc98_madou_ars::overlay_lz::decode_overlay_lz(&input)
            .with_context(|| format!("decode {}", input_path.display()))?;
        let scan = match renderer_offset {
            RendererOffset::Auto => {
                pc98_madou_ars::overlay_text::find_dominant_renderer_message_refs(
                    &decoded_report.output,
                    load_offset,
                )
                .unwrap_or_else(|| empty_renderer_scan(0, 0))
            }
            RendererOffset::Fixed(renderer_logical_offset) => {
                pc98_madou_ars::overlay_text::find_renderer_message_scan(
                    &decoded_report.output,
                    renderer_logical_offset,
                    load_offset,
                )
                .unwrap_or_else(|| empty_renderer_scan(0, renderer_logical_offset))
            }
        };
        let refs = &scan.refs;
        let shown = limit.unwrap_or(refs.len()).min(refs.len());
        total_refs += refs.len();

        println!(
            "# {}: {} refs, renderer 0x{:04X}, {} packed bytes, {} decoded bytes",
            input_path.display(),
            refs.len(),
            scan.renderer_logical_offset,
            input.len(),
            decoded_report.output.len()
        );
        for entry in refs.iter().take(shown) {
            println!(
                "{}\t0x{:04X}\t0x{:04X}\t0x{:04X}\t0x{:02X}\t0x{:04X}\t0x{:04X}\t{}",
                input_path.display(),
                entry.renderer_logical_offset,
                entry.call_decoded_offset,
                entry.call_logical_offset,
                entry.mode,
                entry.string_decoded_offset,
                entry.string_logical_offset,
                entry.text.replace('\n', "\\n")
            );
        }

        sources.push(json!({
            "path": input_path.display().to_string(),
            "packed_size": input.len(),
            "decoded_size": decoded_report.output.len(),
            "bytes_consumed": decoded_report.bytes_consumed,
            "commands": decoded_report.commands,
            "renderer_decoded_offset": format!("0x{:04X}", scan.renderer_decoded_offset),
            "renderer_logical_offset": format!("0x{:04X}", scan.renderer_logical_offset),
            "messages": refs.iter().map(|entry| json!({
                "id": format!(
                    "{}_{:04X}_{:04X}",
                    input_path
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .unwrap_or("overlay")
                        .to_ascii_uppercase(),
                    entry.call_logical_offset,
                    entry.string_logical_offset,
                ),
                "source": input_path.display().to_string(),
                "renderer_decoded_offset": format!("0x{:04X}", entry.renderer_decoded_offset),
                "renderer_logical_offset": format!("0x{:04X}", entry.renderer_logical_offset),
                "call_decoded_offset": format!("0x{:04X}", entry.call_decoded_offset),
                "call_logical_offset": format!("0x{:04X}", entry.call_logical_offset),
                "string_decoded_offset": format!("0x{:04X}", entry.string_decoded_offset),
                "string_logical_offset": format!("0x{:04X}", entry.string_logical_offset),
                "mode": format!("0x{:02X}", entry.mode),
                "raw_hex": hex_encode(&entry.raw),
                "text": entry.text,
            })).collect::<Vec<_>>(),
        }));
    }

    println!(
        "found {total_refs} renderer message refs across {} source(s) (renderer {}, load offset 0x{:04X})",
        input_paths.len(),
        renderer_offset.label(),
        load_offset
    );

    if let Some(json_output) = json_output {
        write_json_output(&json_output, renderer_offset.label(), load_offset, sources)?;
        println!("wrote {}", json_output.display());
    }

    Ok(())
}

fn catalog_messages_cmd(
    input_paths: Vec<PathBuf>,
    load_offset: &str,
    min_double: usize,
    json_output: Option<PathBuf>,
) -> Result<()> {
    let load_offset = parse_usize(load_offset)?;
    let mut sources = Vec::new();

    for input_path in &input_paths {
        let input =
            std::fs::read(input_path).with_context(|| format!("read {}", input_path.display()))?;
        let decoded = pc98_madou_ars::overlay_lz::decode_overlay_lz(&input)
            .with_context(|| format!("decode {}", input_path.display()))?
            .output;

        let catalog = pc98_madou_ars::overlay_catalog::catalog_messages(&decoded, load_offset);
        if let Some((first, second)) =
            pc98_madou_ars::overlay_catalog::first_overlapping_pair(&catalog)
        {
            bail!(
                "{}: cataloged strings overlap at decoded 0x{first:04X}/0x{second:04X}; catalog is unsound",
                input_path.display()
            );
        }
        let runs = pc98_madou_ars::overlay_catalog::scan_sjis_runs(&decoded, min_double);
        let uncovered = pc98_madou_ars::overlay_catalog::uncovered_sjis_runs(&catalog, &runs);
        // Real dialogue carries kana; kana-free runs among interleaved code are
        // kanji-by-chance noise, so the cross-check reports only kana-bearing
        // uncovered runs -- the genuine missed-message candidates.
        let texty: Vec<_> = uncovered.iter().filter(|run| run.kana_count > 0).collect();

        let total_string_bytes: usize = catalog.iter().map(|m| m.byte_budget).sum();
        let shared = catalog.iter().filter(|m| m.call_sites.len() > 1).count();
        let stem = input_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("overlay")
            .to_ascii_uppercase();

        println!(
            "# {}: {} unique messages ({} shared-pointer), {} string bytes / {} decoded; {} kana-bearing uncovered SJIS runs (of {} total)",
            input_path.display(),
            catalog.len(),
            shared,
            total_string_bytes,
            decoded.len(),
            texty.len(),
            uncovered.len(),
        );
        for run in &texty {
            println!(
                "  uncovered SJIS @0x{:04X} ({} bytes, {} kana): {}",
                run.decoded_offset,
                run.raw.len(),
                run.kana_count,
                run.text.replace('\n', "\\n"),
            );
        }

        sources.push(json!({
            "source": input_path.display().to_string(),
            "id_prefix": stem,
            "decoded_size": decoded.len(),
            "load_offset": format!("0x{:04X}", load_offset),
            "unique_messages": catalog.len(),
            "shared_pointer_messages": shared,
            "total_string_bytes": total_string_bytes,
            "messages": catalog.iter().map(|message| json!({
                "id": format!("{}_{:04X}", stem, message.string_logical_offset),
                "string_decoded_offset": format!("0x{:04X}", message.string_decoded_offset),
                "string_logical_offset": format!("0x{:04X}", message.string_logical_offset),
                "byte_budget": message.byte_budget,
                "call_sites": message.call_sites.iter()
                    .map(|site| format!("0x{site:04X}")).collect::<Vec<_>>(),
                "modes": message.modes.iter()
                    .map(|mode| format!("0x{mode:02X}")).collect::<Vec<_>>(),
                "raw_hex": hex_encode(&message.raw),
                "text": message.text,
                "translation": serde_json::Value::Null,
            })).collect::<Vec<_>>(),
            "uncovered_sjis": texty.iter().map(|run| json!({
                "decoded_offset": format!("0x{:04X}", run.decoded_offset),
                "byte_len": run.raw.len(),
                "kana_count": run.kana_count,
                "raw_hex": hex_encode(&run.raw),
                "text": run.text,
            })).collect::<Vec<_>>(),
        }));
    }

    if let Some(json_output) = json_output {
        if let Some(parent) = json_output.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
        let document = json!({ "sources": sources });
        std::fs::write(&json_output, serde_json::to_string_pretty(&document)?)
            .with_context(|| format!("write {}", json_output.display()))?;
        println!("wrote {}", json_output.display());
    }

    Ok(())
}

fn find_pointer_tables_cmd(
    input_paths: Vec<PathBuf>,
    load_offset: &str,
    min_entries: usize,
    max_stride: usize,
    json_output: Option<PathBuf>,
) -> Result<()> {
    use pc98_madou_ars::overlay_catalog::{
        catalog_messages, decode_string_at, scan_sjis_runs, uncovered_sjis_runs,
    };
    use pc98_madou_ars::overlay_ptrtable::{find_pointer_tables, string_start_set};

    let load_offset = parse_usize(load_offset)?;
    let strides: Vec<usize> = (2..=max_stride.max(2)).step_by(2).collect();
    let mut sources = Vec::new();

    for input_path in &input_paths {
        let input =
            std::fs::read(input_path).with_context(|| format!("read {}", input_path.display()))?;
        let decoded = pc98_madou_ars::overlay_lz::decode_overlay_lz(&input)
            .with_context(|| format!("decode {}", input_path.display()))?
            .output;

        let catalog = catalog_messages(&decoded, load_offset);
        let runs = scan_sjis_runs(&decoded, 2);
        let uncovered = uncovered_sjis_runs(&catalog, &runs);
        let texty: Vec<_> = uncovered.iter().filter(|run| run.kana_count > 0).collect();
        let starts = string_start_set(&decoded, &catalog, &runs);
        let tables = find_pointer_tables(&decoded, load_offset, &starts, min_entries, &strides);

        let table_targets: std::collections::HashSet<usize> = tables
            .iter()
            .flat_map(|table| table.entries.iter().map(|entry| entry.target_decoded))
            .collect();
        let covered = texty
            .iter()
            .filter(|run| table_targets.contains(&run.decoded_offset))
            .count();
        let total_entries: usize = tables.iter().map(|table| table.entries.len()).sum();
        let stem = input_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("overlay")
            .to_ascii_uppercase();

        println!(
            "# {}: {} pointer tables, {} entries; {} of {} kana-bearing uncovered messages now table-covered",
            input_path.display(),
            tables.len(),
            total_entries,
            covered,
            texty.len(),
        );
        for table in &tables {
            let sample = decode_string_at(&decoded, table.entries[0].target_decoded)
                .unwrap_or_default()
                .replace('\n', "\\n");
            println!(
                "  table @0x{:04X} stride {} x {} -> targets 0x{:04X}.. e.g. {:?}",
                table.start,
                table.stride,
                table.entries.len(),
                table.entries[0].target_decoded,
                sample,
            );
        }

        sources.push(json!({
            "source": input_path.display().to_string(),
            "id_prefix": stem,
            "load_offset": format!("0x{:04X}", load_offset),
            "table_count": tables.len(),
            "entry_count": total_entries,
            "uncovered_kana_messages": texty.len(),
            "uncovered_table_covered": covered,
            "tables": tables.iter().map(|table| json!({
                "start": format!("0x{:04X}", table.start),
                "stride": table.stride,
                "entries": table.entries.iter().map(|entry| json!({
                    "site": format!("0x{:04X}", entry.site),
                    "target_decoded": format!("0x{:04X}", entry.target_decoded),
                    "target_logical": format!("0x{:04X}", entry.target_logical),
                    "text": decode_string_at(&decoded, entry.target_decoded),
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        }));
    }

    if let Some(json_output) = json_output {
        if let Some(parent) = json_output.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
        let document = json!({ "sources": sources });
        std::fs::write(&json_output, serde_json::to_string_pretty(&document)?)
            .with_context(|| format!("write {}", json_output.display()))?;
        println!("wrote {}", json_output.display());
    }

    Ok(())
}

fn build_message_map_cmd(
    input_paths: Vec<PathBuf>,
    load_offset: &str,
    min_entries: usize,
    max_stride: usize,
    json_output: Option<PathBuf>,
) -> Result<()> {
    use pc98_madou_ars::overlay_messages::{PointerKind, first_inconsistent_site, unified_catalog};

    let load_offset = parse_usize(load_offset)?;
    let strides: Vec<usize> = (2..=max_stride.max(2)).step_by(2).collect();
    let mut sources = Vec::new();

    for input_path in &input_paths {
        let input =
            std::fs::read(input_path).with_context(|| format!("read {}", input_path.display()))?;
        let decoded = pc98_madou_ars::overlay_lz::decode_overlay_lz(&input)
            .with_context(|| format!("decode {}", input_path.display()))?
            .output;

        let messages = unified_catalog(&decoded, load_offset, min_entries, &strides)?;

        // Soundness: every rewrite site must hold the string's logical offset.
        for message in &messages {
            if let Some(bad) = first_inconsistent_site(&decoded, message) {
                bail!(
                    "{}: rewrite site 0x{:04X} for string 0x{:04X} does not hold its logical offset",
                    input_path.display(),
                    bad.site,
                    message.string_decoded_offset,
                );
            }
        }

        let mov_si = messages
            .iter()
            .filter(|m| m.rewrite_sites.iter().all(|s| s.kind == PointerKind::MovSi))
            .count();
        let table_only = messages
            .iter()
            .filter(|m| m.rewrite_sites.iter().all(|s| s.kind == PointerKind::Table))
            .count();
        let total_sites: usize = messages.iter().map(|m| m.rewrite_sites.len()).sum();
        let total_bytes: usize = messages.iter().map(|m| m.byte_budget).sum();

        // Remaining gap: real NUL-delimited messages (>= 2 kana) the map does not
        // reach. Counting NUL-delimited starts, not raw runs, avoids inflating the
        // gap with sub-run fragments of messages that are already covered.
        let covered: std::collections::HashSet<usize> =
            messages.iter().map(|m| m.string_decoded_offset).collect();
        let still_uncovered =
            pc98_madou_ars::overlay_catalog::nul_delimited_kana_starts(&decoded, 2)
                .into_iter()
                .filter(|offset| !covered.contains(offset))
                .count();

        let stem = input_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("overlay")
            .to_ascii_uppercase();

        println!(
            "# {}: {} messages ({} mov-si-only, {} table-only), {} rewrite sites, {} string bytes; {} kana strings still uncovered",
            input_path.display(),
            messages.len(),
            mov_si,
            table_only,
            total_sites,
            total_bytes,
            still_uncovered,
        );

        sources.push(json!({
            "source": input_path.display().to_string(),
            "id_prefix": stem,
            "decoded_size": decoded.len(),
            "load_offset": format!("0x{:04X}", load_offset),
            "message_count": messages.len(),
            "mov_si_only": mov_si,
            "table_only": table_only,
            "rewrite_site_count": total_sites,
            "total_string_bytes": total_bytes,
            "still_uncovered_kana": still_uncovered,
            "messages": messages.iter().map(|message| json!({
                "id": format!("{}_{:04X}", stem, message.string_logical_offset),
                "string_decoded_offset": format!("0x{:04X}", message.string_decoded_offset),
                "string_logical_offset": format!("0x{:04X}", message.string_logical_offset),
                "byte_budget": message.byte_budget,
                "raw_hex": hex_encode(&message.raw),
                "text": message.text,
                "translation": serde_json::Value::Null,
                "rewrite_sites": message.rewrite_sites.iter().map(|site| json!({
                    "site": format!("0x{:04X}", site.site),
                    "kind": site.kind.as_str(),
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        }));
    }

    if let Some(json_output) = json_output {
        if let Some(parent) = json_output.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
        let document = json!({ "sources": sources });
        std::fs::write(&json_output, serde_json::to_string_pretty(&document)?)
            .with_context(|| format!("write {}", json_output.display()))?;
        println!("wrote {}", json_output.display());
    }

    Ok(())
}

fn emit_translation_raw(
    input_path: PathBuf,
    load_offset: &str,
    min_entries: usize,
    max_stride: usize,
    output_path: PathBuf,
) -> Result<()> {
    use pc98_madou_ars::overlay_messages::{first_inconsistent_site, unified_catalog};

    let load_offset = parse_usize(load_offset)?;
    let strides: Vec<usize> = (2..=max_stride.max(2)).step_by(2).collect();

    let input =
        std::fs::read(&input_path).with_context(|| format!("read {}", input_path.display()))?;
    let decoded = pc98_madou_ars::overlay_lz::decode_overlay_lz(&input)
        .with_context(|| format!("decode {}", input_path.display()))?
        .output;
    let messages = unified_catalog(&decoded, load_offset, min_entries, &strides)?;

    for message in &messages {
        if let Some(bad) = first_inconsistent_site(&decoded, message) {
            bail!(
                "rewrite site 0x{:04X} for 0x{:04X} does not hold its logical offset",
                bad.site,
                message.string_decoded_offset,
            );
        }
    }

    let stem = input_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("overlay")
        .to_ascii_uppercase();

    // Protected fields (text, raw_hex, offsets, rewrite_sites) carry the baseline
    // the reinserter and the validator trust; ko/status/notes are the only fields
    // a translator edits (references/strategy/translation-workflow.md 1).
    let entries: Vec<_> = messages
        .iter()
        .map(|message| {
            json!({
                "id": format!("{}_{:04X}", stem, message.string_logical_offset),
                "string_decoded_offset": format!("0x{:04X}", message.string_decoded_offset),
                "string_logical_offset": format!("0x{:04X}", message.string_logical_offset),
                "byte_budget": message.byte_budget,
                "raw_hex": hex_encode(&message.raw),
                "text": message.text,
                "rewrite_sites": message.rewrite_sites.iter().map(|site| json!({
                    "site": format!("0x{:04X}", site.site),
                    "kind": site.kind.as_str(),
                })).collect::<Vec<_>>(),
                "ko": "",
                "status": "untranslated",
                "notes": "",
            })
        })
        .collect();

    let document = json!({
        "overlay": input_path.file_name().and_then(|n| n.to_str()).unwrap_or(""),
        "load_offset": format!("0x{:04X}", load_offset),
        "entry_count": entries.len(),
        "protected_fields": ["string_decoded_offset", "string_logical_offset", "byte_budget", "raw_hex", "text", "rewrite_sites"],
        "editable_fields": ["ko", "status", "notes"],
        "entries": entries,
    });

    if let Some(parent) = output_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    std::fs::write(&output_path, serde_json::to_string_pretty(&document)?)
        .with_context(|| format!("write {}", output_path.display()))?;
    println!(
        "wrote {} ({} entries, all rewrite sites verified)",
        output_path.display(),
        messages.len(),
    );
    Ok(())
}

fn emit_cutscene_raw(
    arle_game_path: PathBuf,
    rulue_game_path: PathBuf,
    schezo_game_path: PathBuf,
    output_dir: PathBuf,
) -> Result<()> {
    use pc98_madou_ars::cutscene_catalog::{CUTSCENE_RESOURCES, catalog_resource};
    use std::collections::BTreeMap;

    let arle_game = std::fs::read(&arle_game_path)
        .with_context(|| format!("read {}", arle_game_path.display()))?;
    let rulue_game = std::fs::read(&rulue_game_path)
        .with_context(|| format!("read {}", rulue_game_path.display()))?;
    let schezo_game = std::fs::read(&schezo_game_path)
        .with_context(|| format!("read {}", schezo_game_path.display()))?;
    std::fs::create_dir_all(&output_dir)
        .with_context(|| format!("create {}", output_dir.display()))?;

    let mut manifest_resources = Vec::new();
    let mut total_entries = 0usize;
    let mut total_targets = 0usize;
    let mut total_layout_windows = 0usize;
    for spec in CUTSCENE_RESOURCES {
        let disk = match spec.character {
            "arle" => &arle_game,
            "rulue" => &rulue_game,
            "schezo" => &schezo_game,
            other => bail!("unknown cutscene character {other:?}"),
        };
        let packed = pc98_madou_ars::read_fat12_file_from_hdm(disk, spec.file)
            .with_context(|| format!("read {} from {} Game disk", spec.file, spec.character))?;
        let catalog = catalog_resource(&packed, *spec)?;
        let stem = spec
            .file
            .split_once('.')
            .map(|(stem, _)| stem)
            .unwrap_or(spec.file);
        let id_prefix = stem.to_ascii_uppercase();
        let mut region_counts = BTreeMap::<&str, usize>::new();
        let mut rewrite_site_count = 0usize;
        let mut layout_window_count = 0usize;
        let mut raw_token_entries = 0usize;
        let entries: Vec<_> = catalog
            .entries
            .iter()
            .map(|entry| {
                *region_counts.entry(entry.region).or_default() += 1;
                rewrite_site_count += entry.rewrite_sites.len();
                layout_window_count += entry.layout_windows.len();
                raw_token_entries += usize::from(entry.had_decode_errors);
                // IDs are stable translation keys. OVL dialogue historically
                // uses its logical address, while bounded NUL regions use the
                // decoded slot address even when a newly proven pointer map now
                // gives those entries a logical relocation target.
                let id_offset = if entry.region == "dialogue" {
                    entry
                        .string_logical_offset
                        .unwrap_or(entry.string_decoded_offset)
                } else {
                    entry.string_decoded_offset
                };
                let flags = [
                    entry.structural.then_some("structural"),
                    entry.had_decode_errors.then_some("raw_tokens"),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>();
                json!({
                    "id": format!("{}_{:04X}", id_prefix, id_offset),
                    "region": entry.region,
                    "slot_decoded_offset": format!("0x{:04X}", entry.slot_decoded_offset),
                    "string_decoded_offset": format!("0x{:04X}", entry.string_decoded_offset),
                    "string_logical_offset": entry.string_logical_offset.map(|offset| format!("0x{offset:04X}")),
                    "byte_budget": entry.byte_budget,
                    "prefix_hex": hex_encode(&entry.prefix),
                    "raw_hex": hex_encode(&entry.raw),
                    "terminator_hex": format!("{:02x}", entry.terminator),
                    "text": entry.text,
                    "rewrite_sites": entry.rewrite_sites.iter().map(|site| json!({
                        "site": format!("0x{:04X}", site.site),
                        "kind": site.kind.as_str(),
                    })).collect::<Vec<_>>(),
                    "layout_windows": entry.layout_windows.iter().map(|window| json!({
                        "rewrite_site": format!("0x{:04X}", window.rewrite_site),
                        "origin_column": window.origin_column,
                        "origin_row": window.origin_row,
                        "width_half_cells": window.width_half_cells,
                        "rows": window.rows,
                        "initializer_decoded_offset": format!("0x{:04X}", window.initializer_decoded_offset),
                    })).collect::<Vec<_>>(),
                    "flags": flags,
                    "ko": "",
                    "status": if entry.structural { "not_applicable" } else { "untranslated" },
                    "notes": "",
                })
            })
            .collect();
        let translation_targets = catalog
            .entries
            .iter()
            .filter(|entry| !entry.structural)
            .count();
        total_entries += entries.len();
        total_targets += translation_targets;
        total_layout_windows += layout_window_count;

        let document = json!({
            "schema": "pc98_madou_ars.cutscene_raw.v1",
            "resource": spec.file,
            "character": spec.character,
            "scene": spec.scene,
            "packed_size": catalog.packed_size,
            "decoded_size": catalog.decoded_size,
            "load_offset": catalog.load_offset.map(|offset| format!("0x{offset:04X}")),
            "entry_count": entries.len(),
            "translation_target_count": translation_targets,
            "protected_fields": [
                "id", "region", "slot_decoded_offset", "string_decoded_offset",
                "string_logical_offset", "byte_budget", "prefix_hex", "raw_hex",
                "terminator_hex", "text", "rewrite_sites", "layout_windows", "flags"
            ],
            "editable_fields": ["ko", "status", "notes"],
            "entries": entries,
        });
        let output_path = output_dir.join(format!("{}.json", stem.to_ascii_lowercase()));
        std::fs::write(&output_path, serde_json::to_vec_pretty(&document)?)
            .with_context(|| format!("write {}", output_path.display()))?;
        println!(
            "{}: {} entries ({} translation targets, {} rewrite sites, {} layout windows, {} raw-token entries), load={}",
            spec.file,
            catalog.entries.len(),
            translation_targets,
            rewrite_site_count,
            layout_window_count,
            raw_token_entries,
            catalog
                .load_offset
                .map(|offset| format!("0x{offset:04X}"))
                .unwrap_or_else(|| "n/a".to_string()),
        );
        manifest_resources.push(json!({
            "resource": spec.file,
            "character": spec.character,
            "scene": spec.scene,
            "catalog": output_path.file_name().and_then(|name| name.to_str()),
            "entry_count": catalog.entries.len(),
            "translation_target_count": translation_targets,
            "region_counts": region_counts,
            "rewrite_site_count": rewrite_site_count,
            "layout_window_count": layout_window_count,
            "raw_token_entries": raw_token_entries,
            "load_offset": catalog.load_offset.map(|offset| format!("0x{offset:04X}")),
        }));
    }

    let manifest = json!({
        "schema": "pc98_madou_ars.cutscene_manifest.v1",
        "resource_count": manifest_resources.len(),
        "entry_count": total_entries,
        "translation_target_count": total_targets,
        "layout_window_count": total_layout_windows,
        "bios_gaiji_capacity": pc98_madou_ars::cutscene_catalog::BIOS_GAIJI_CAPACITY,
        "demand_state": "pending_translation",
        "resources": manifest_resources,
    });
    let manifest_path = output_dir.join("manifest.json");
    std::fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest)?)
        .with_context(|| format!("write {}", manifest_path.display()))?;
    println!(
        "wrote {} resources / {} entries / {} translation targets to {}",
        CUTSCENE_RESOURCES.len(),
        total_entries,
        total_targets,
        output_dir.display(),
    );
    Ok(())
}

fn cutscene_catalog_filename(resource: &str) -> Result<String> {
    let stem = resource
        .rsplit_once('.')
        .map(|(stem, _)| stem)
        .context("cutscene resource lacks an extension")?;
    Ok(format!("{}.json", stem.to_ascii_lowercase()))
}

fn cutscene_translation_map(
    catalog: &serde_json::Value,
    allow_needs_review: bool,
) -> Result<std::collections::BTreeMap<usize, String>> {
    let resource = catalog
        .get("resource")
        .and_then(serde_json::Value::as_str)
        .context("cutscene catalog missing resource")?;
    let entries = catalog
        .get("entries")
        .and_then(serde_json::Value::as_array)
        .context("cutscene catalog missing entries")?;
    let mut translations = std::collections::BTreeMap::new();
    for entry in entries {
        let structural = entry
            .get("flags")
            .and_then(serde_json::Value::as_array)
            .is_some_and(|flags| flags.iter().any(|flag| flag.as_str() == Some("structural")));
        if structural {
            continue;
        }
        let id = entry
            .get("id")
            .and_then(serde_json::Value::as_str)
            .context("cutscene entry missing id")?;
        let status = entry
            .get("status")
            .and_then(serde_json::Value::as_str)
            .with_context(|| format!("{id}: missing status"))?;
        if !allow_needs_review && status != "complete" {
            bail!(
                "{resource}: {id} has status {status:?}; shipping build requires `complete` (use --allow-needs-review only for a draft emulator PoC)"
            );
        }
        let offset = entry
            .get("string_decoded_offset")
            .and_then(serde_json::Value::as_str)
            .with_context(|| format!("{id}: missing string_decoded_offset"))
            .and_then(parse_usize)?;
        let ko = entry
            .get("ko")
            .and_then(serde_json::Value::as_str)
            .with_context(|| format!("{id}: missing ko"))?;
        if translations.insert(offset, ko.to_owned()).is_some() {
            bail!("{resource}: duplicate translated offset 0x{offset:04X}");
        }
    }
    Ok(translations)
}

#[allow(clippy::too_many_arguments)]
fn build_cutscene_disk(
    game_disk_path: PathBuf,
    raw_dir: PathBuf,
    translations_dir: PathBuf,
    gaiji_dir: PathBuf,
    font_profile_path: PathBuf,
    allow_needs_review: bool,
    output_path: PathBuf,
) -> Result<()> {
    use pc98_madou_ars::cutscene_catalog::{
        CUTSCENE_RESOURCES, ResourceLayout, catalog_resource, unique_hangul_syllables,
    };
    use pc98_madou_ars::cutscene_reinsert::{CutsceneReinsertReport, apply_cutscene_translations};
    use pc98_madou_ars::cutscene_text::{
        ARLE_CD_PHASE_KEY, ARLE_ED_PHASE_KEY, ARLE_OP_PHASE_KEY, DAT_GAIJI_SEGMENT,
        DemoGaijiStubReport, OverlayGaijiStubReport, build_dat_gaiji_blob,
        install_demo_dat_gaiji_registration_from_segment, install_overlay_gaiji_registration,
        plan_integrated_dat_gaiji_layout,
    };
    use pc98_madou_ars::cutscene_translation::validate_catalog_pair;
    use pc98_madou_ars::gaiji_table::GaijiTable;
    use pc98_madou_ars::hook_geometry::{RendererOverlay, renderer_hook};
    use pc98_madou_ars::overlay_lz::{decode_overlay_lz, encode_overlay_lz};
    use std::collections::BTreeSet;

    let mut disk = std::fs::read(&game_disk_path)
        .with_context(|| format!("read {}", game_disk_path.display()))?;
    let character = if pc98_madou_ars::read_fat12_file_from_hdm(&disk, "OPENINGR.OVL").is_ok() {
        "rulue"
    } else if pc98_madou_ars::read_fat12_file_from_hdm(&disk, "SHEZO_OP.OVL").is_ok() {
        "schezo"
    } else if pc98_madou_ars::read_fat12_file_from_hdm(&disk, "OP_A.DAT").is_ok() {
        "arle"
    } else {
        bail!("game disk has no recognized Arle/Rulue/Schezo cutscene resource set");
    };

    const RENDERER_FONT_FILE: &str = "KFONT.BIN";
    const CUTSCENE_FONT_FILE: &str = "KCSFONT.BIN";
    let arle_hook = renderer_hook(RendererOverlay::Arle)?;
    let renderer_font_present =
        pc98_madou_ars::fat12_add::root_file_exists(&disk, RENDERER_FONT_FILE)?;
    let arle_renderer_font = if character == "arle" && renderer_font_present {
        let bytes = pc98_madou_ars::read_fat12_file_from_hdm(&disk, RENDERER_FONT_FILE)?;
        let hook_offset = pc98_madou_ars::hook_geometry::HOOK_ENTRY_OFF as usize;
        let expected_len = hook_offset
            .checked_add(arle_hook.len())
            .context("Arle renderer image size overflow")?;
        if bytes.len() != expected_len || bytes[hook_offset..] != arle_hook[..] {
            bail!(
                "existing {RENDERER_FONT_FILE} is not the expected Arle sheet+hook image ({} bytes, expected {expected_len})",
                bytes.len()
            );
        }
        Some(bytes)
    } else {
        None
    };

    enum PhaseSupply {
        Overlay(OverlayGaijiStubReport),
        Dat { selector: u16 },
    }

    struct BuiltPhase {
        file: &'static str,
        old_packed_size: usize,
        packed: Vec<u8>,
        decoded: Vec<u8>,
        reinsert: CutsceneReinsertReport,
        supply: PhaseSupply,
        glyphs: usize,
    }

    struct BuiltDemo {
        old_packed_size: usize,
        packed: Vec<u8>,
        decoded: Vec<u8>,
        hook: DemoGaijiStubReport,
    }

    struct BuiltDatFont {
        file: &'static str,
        bytes: Vec<u8>,
        patched_main: Vec<u8>,
        replace_existing: bool,
        blob_offset: usize,
        table_segment: u16,
        phases: usize,
    }

    let mut built = Vec::new();
    let mut dat_phases = Vec::new();
    for spec in CUTSCENE_RESOURCES
        .iter()
        .filter(|spec| spec.character == character)
    {
        let name = cutscene_catalog_filename(spec.file)?;
        let raw_path = raw_dir.join(&name);
        let translated_path = translations_dir.join(&name);
        let gaiji_path = gaiji_dir.join(&name);
        let raw: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&raw_path).with_context(|| format!("read {}", raw_path.display()))?,
        )
        .with_context(|| format!("parse {}", raw_path.display()))?;
        let translated: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&translated_path)
                .with_context(|| format!("read {}", translated_path.display()))?,
        )
        .with_context(|| format!("parse {}", translated_path.display()))?;
        validate_catalog_pair(&raw, &translated)
            .with_context(|| format!("validate {}", spec.file))?;
        let translations = cutscene_translation_map(&translated, allow_needs_review)?;
        let gaiji = GaijiTable::load(&gaiji_path)?;
        let demand = unique_hangul_syllables(translations.values().map(String::as_str));
        let table_chars = gaiji
            .entries
            .iter()
            .map(|entry| entry.ch)
            .collect::<BTreeSet<_>>();
        if table_chars != demand {
            let missing = demand.difference(&table_chars).collect::<String>();
            let extra = table_chars.difference(&demand).collect::<String>();
            bail!(
                "{} gaiji table does not exactly match phase demand (missing {missing:?}, extra {extra:?})",
                spec.file
            );
        }

        let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, spec.file)?;
        let catalog = catalog_resource(&packed, *spec)?;
        let mut decoded = decode_overlay_lz(&packed)?.output;
        let reinsert = apply_cutscene_translations(&mut decoded, &catalog, &translations, &gaiji)?;
        if spec.file == "ED_A.DAT" {
            let profile = pc98_madou_ars::font_build::FontProfile::load(&font_profile_path)?;
            let end_card: serde_json::Value = serde_json::from_slice(&std::fs::read(
                translations_dir
                    .parent()
                    .context("cutscene translation root missing")?
                    .join("graphics/arle_end_card.json"),
            )?)?;
            pc98_madou_ars::arle_ending_graphics::install(
                &mut decoded,
                &end_card,
                &profile,
                allow_needs_review,
            )?;
        }
        if spec.file == "ENDING_R.OVL" {
            pc98_madou_ars::cutscene_text::install_rulue_ending_sample_wait(&mut decoded)?;
        }
        let supply = match spec.layout {
            ResourceLayout::OverlayDialogue { .. } => PhaseSupply::Overlay(
                install_overlay_gaiji_registration(&mut decoded, &gaiji.gaiji_glyphs())?,
            ),
            ResourceLayout::NulRegions(_) if character == "arle" => {
                if reinsert.relocated != 0 || !reinsert.pointer_grid_repacks.is_empty() {
                    bail!("{} DAT reinsertion unexpectedly moved a string", spec.file);
                }
                let selector = match spec.file {
                    "OP_A.DAT" => ARLE_OP_PHASE_KEY,
                    "CD_A.DAT" => ARLE_CD_PHASE_KEY,
                    "ED_A.DAT" => ARLE_ED_PHASE_KEY,
                    _ => bail!("{} has no Arle DAT phase selector", spec.file),
                };
                dat_phases.push((selector, gaiji.gaiji_glyphs()));
                PhaseSupply::Dat { selector }
            }
            ResourceLayout::NulRegions(_) => {
                bail!("{} is not an executable cutscene overlay", spec.file)
            }
        };
        let reencoded = encode_overlay_lz(&decoded);
        if decode_overlay_lz(&reencoded)?.output != decoded {
            bail!("re-encoded {} did not round-trip", spec.file);
        }
        built.push(BuiltPhase {
            file: spec.file,
            old_packed_size: packed.len(),
            packed: reencoded,
            decoded,
            reinsert,
            supply,
            glyphs: gaiji.entries.len(),
        });
    }
    if built.len() != 3 {
        bail!(
            "{character} build planned {} phases, expected 3",
            built.len()
        );
    }

    let built_dat_font = if character == "arle" {
        let blob = build_dat_gaiji_blob(&dat_phases)?;
        let main_com = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "MAIN.COM")?;
        let phases = blob.phases.len();
        if let Some(renderer) = &arle_renderer_font {
            let layout = plan_integrated_dat_gaiji_layout(renderer.len(), blob.bytes.len())?;
            let patched_main = pc98_madou_ars::hangul_probe::retarget_main_com_load_combined(
                &main_com,
                u16::try_from(renderer.len()).context("KFONT.BIN too large")?,
                layout.load_size,
                RENDERER_FONT_FILE,
            )?;
            let mut combined = renderer.clone();
            combined.resize(layout.blob_offset, 0);
            combined.extend_from_slice(&blob.bytes);
            if combined.len() != layout.load_size as usize {
                bail!("integrated KFONT layout size does not match its loader plan");
            }
            Some(BuiltDatFont {
                file: RENDERER_FONT_FILE,
                bytes: combined,
                patched_main,
                replace_existing: true,
                blob_offset: layout.blob_offset,
                table_segment: layout.blob_segment,
                phases,
            })
        } else {
            let blob_size = u16::try_from(blob.bytes.len()).context("KCSFONT.BIN too large")?;
            let patched_main = pc98_madou_ars::hangul_probe::patch_main_com_load_combined(
                &main_com,
                blob_size,
                CUTSCENE_FONT_FILE,
            )?;
            Some(BuiltDatFont {
                file: CUTSCENE_FONT_FILE,
                bytes: blob.bytes,
                patched_main,
                replace_existing: false,
                blob_offset: 0,
                table_segment: DAT_GAIJI_SEGMENT,
                phases,
            })
        }
    } else {
        None
    };

    let built_demo = if character == "arle" {
        let table_segment = built_dat_font
            .as_ref()
            .map(|font| font.table_segment)
            .unwrap_or(DAT_GAIJI_SEGMENT);
        let packed = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "DEMO.OVL")?;
        let mut decoded = decode_overlay_lz(&packed)?.output;
        let hook = install_demo_dat_gaiji_registration_from_segment(&mut decoded, table_segment)?;
        let reencoded = encode_overlay_lz(&decoded);
        if decode_overlay_lz(&reencoded)?.output != decoded {
            bail!("re-encoded DEMO.OVL did not round-trip");
        }
        Some(BuiltDemo {
            old_packed_size: packed.len(),
            packed: reencoded,
            decoded,
            hook,
        })
    } else {
        None
    };

    for phase in &built {
        let fat =
            pc98_madou_ars::fat12_add::replace_file_grow(&mut disk, phase.file, &phase.packed)?;
        let back = pc98_madou_ars::read_fat12_file_from_hdm(&disk, phase.file)?;
        if decode_overlay_lz(&back)?.output != phase.decoded {
            bail!(
                "FAT readback of {} differs from the planned image",
                phase.file
            );
        }
        match &phase.supply {
            PhaseSupply::Overlay(hook) => println!(
                "{}: {} in place, {} relocated ({} bytes), {} grid repack(s), {} gaiji; consumer 0x{:04X}; packed {} -> {} ({} clusters)",
                phase.file,
                phase.reinsert.in_place,
                phase.reinsert.relocated,
                phase.reinsert.relocation_bytes,
                phase.reinsert.pointer_grid_repacks.len(),
                phase.glyphs,
                hook.consumer_decoded_offset,
                phase.old_packed_size,
                phase.packed.len(),
                fat.clusters,
            ),
            PhaseSupply::Dat { selector } => println!(
                "{}: {} in place, {} gaiji selected by phase key 0x{:04X}; packed {} -> {} ({} clusters)",
                phase.file,
                phase.reinsert.in_place,
                phase.glyphs,
                selector,
                phase.old_packed_size,
                phase.packed.len(),
                fat.clusters,
            ),
        }
    }

    if let Some(demo) = built_demo {
        let fat =
            pc98_madou_ars::fat12_add::replace_file_grow(&mut disk, "DEMO.OVL", &demo.packed)?;
        let back = pc98_madou_ars::read_fat12_file_from_hdm(&disk, "DEMO.OVL")?;
        if decode_overlay_lz(&back)?.output != demo.decoded {
            bail!("FAT readback of DEMO.OVL differs from the planned image");
        }
        println!(
            "DEMO.OVL: video reset 0x{:04X}, DAT consumer 0x{:04X}, table segment 0x{:04X}, stub 0x{:04X}..0x{:04X}, bitmap scratch 0x{:04X}, selector scratch 0x{:04X}, stack reserve 0x{:04X}; packed {} -> {} ({} clusters)",
            demo.hook.video_reset_decoded_offset,
            demo.hook.consumer_decoded_offset,
            demo.hook.table_segment,
            demo.hook.stub_decoded_offset,
            demo.hook.static_end_logical - 0x100,
            demo.hook.bitmap_scratch_logical,
            demo.hook.selector_scratch_logical,
            demo.hook.stack_reserve,
            demo.old_packed_size,
            demo.packed.len(),
            fat.clusters,
        );
    }

    if let Some(font_build) = built_dat_font {
        let main = pc98_madou_ars::fat12_replace::replace_file_in_place(
            &mut disk,
            "MAIN.COM",
            &font_build.patched_main,
        )?;
        let font = if font_build.replace_existing {
            pc98_madou_ars::fat12_add::replace_file_grow(
                &mut disk,
                font_build.file,
                &font_build.bytes,
            )?
        } else {
            let meta = pc98_madou_ars::fat12_add::RootFileMeta {
                attr: 0x20,
                date: 0,
                time: 0,
            };
            pc98_madou_ars::fat12_add::add_root_file(
                &mut disk,
                font_build.file,
                &font_build.bytes,
                meta,
            )?
        };
        if pc98_madou_ars::read_fat12_file_from_hdm(&disk, font_build.file)? != font_build.bytes {
            bail!(
                "{} readback did not match the planned image",
                font_build.file
            );
        }
        println!(
            "MAIN.COM: load {}-byte {} at 0x8800 ({} bytes, capacity {}); cutscene blob +0x{:04X} via segment 0x{:04X}, {} phase tables, {} clusters",
            font_build.bytes.len(),
            font_build.file,
            main.bytes,
            main.capacity,
            font_build.blob_offset,
            font_build.table_segment,
            font_build.phases,
            font.clusters,
        );
    }

    write_output_disk(&output_path, &disk)?;
    println!(
        "wrote {} {character} {} disk{}",
        output_path.display(),
        if renderer_font_present {
            "all-track"
        } else {
            "cutscene"
        },
        if allow_needs_review {
            " (draft statuses explicitly allowed)"
        } else {
            ""
        },
    );
    Ok(())
}

fn check_cutscene_demand(translations_dir: PathBuf, json_output: Option<PathBuf>) -> Result<()> {
    use pc98_madou_ars::cutscene_catalog::{BIOS_GAIJI_CAPACITY, unique_hangul_syllables};
    use std::collections::BTreeSet;

    let mut paths = std::fs::read_dir(&translations_dir)
        .with_context(|| format!("read directory {}", translations_dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    paths.sort();

    let mut summaries = Vec::new();
    let mut over_capacity = Vec::new();
    let mut seen_resources = BTreeSet::new();
    for path in paths {
        let value: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&path).with_context(|| format!("read {}", path.display()))?,
        )
        .with_context(|| format!("parse {}", path.display()))?;
        let Some(entries) = value.get("entries").and_then(|entries| entries.as_array()) else {
            continue;
        };
        let target_entries = entries
            .iter()
            .filter(|entry| {
                !entry
                    .get("flags")
                    .and_then(|flags| flags.as_array())
                    .is_some_and(|flags| {
                        flags.iter().any(|flag| flag.as_str() == Some("structural"))
                    })
            })
            .collect::<Vec<_>>();
        let targets = target_entries.len();
        let korean = target_entries
            .iter()
            .filter_map(|entry| entry.get("ko").and_then(|ko| ko.as_str()))
            .filter(|ko| !ko.trim().is_empty())
            .collect::<Vec<_>>();
        let translated = korean.len();
        let syllables = unique_hangul_syllables(korean.iter().copied());
        let unique = syllables.len();
        let resource = value
            .get("resource")
            .and_then(|resource| resource.as_str())
            .unwrap_or_else(|| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("unknown")
            });
        if !seen_resources.insert(resource.to_string()) {
            bail!(
                "duplicate cutscene resource {resource:?} in {}",
                translations_dir.display()
            );
        }
        let capacity =
            BIOS_GAIJI_CAPACITY - pc98_madou_ars::cutscene_catalog::reserved_gaiji_slots(resource);
        let status = if unique > capacity {
            "overflow"
        } else if translated < targets {
            "pending_translation"
        } else {
            "fits"
        };
        if unique > capacity {
            over_capacity.push(resource.to_string());
        }
        println!(
            "{resource}: translated {translated}/{targets}, unique Hangul {unique}/{capacity} ({status})"
        );
        summaries.push(json!({
            "resource": resource,
            "translation_targets": targets,
            "translated_entries": translated,
            "unique_hangul": unique,
            "capacity": capacity,
            "spare": capacity as isize - unique as isize,
            "status": status,
            "syllables": syllables.into_iter().collect::<String>(),
        }));
    }
    if summaries.is_empty() {
        bail!(
            "no cutscene catalogs with an `entries` array in {}",
            translations_dir.display()
        );
    }
    if let Some(output_path) = json_output {
        if let Some(parent) = output_path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
        let document = json!({
            "schema": "pc98_madou_ars.cutscene_demand.v1",
            "bios_gaiji_capacity": BIOS_GAIJI_CAPACITY,
            "resources": summaries,
        });
        std::fs::write(&output_path, serde_json::to_vec_pretty(&document)?)
            .with_context(|| format!("write {}", output_path.display()))?;
        println!("wrote {}", output_path.display());
    }
    if !over_capacity.is_empty() {
        bail!(
            "cutscene gaiji capacity exceeded by: {}",
            over_capacity.join(", ")
        );
    }
    Ok(())
}

fn validate_cutscene_translation(raw_dir: PathBuf, translations_dir: PathBuf) -> Result<()> {
    use pc98_madou_ars::cutscene_translation::validate_catalog_pair;
    use std::collections::BTreeMap;

    fn load_catalogs(dir: &std::path::Path) -> Result<BTreeMap<String, serde_json::Value>> {
        let mut catalogs = BTreeMap::new();
        for entry in
            std::fs::read_dir(dir).with_context(|| format!("read directory {}", dir.display()))?
        {
            let path = entry?.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                continue;
            }
            let value: serde_json::Value = serde_json::from_slice(
                &std::fs::read(&path).with_context(|| format!("read {}", path.display()))?,
            )
            .with_context(|| format!("parse {}", path.display()))?;
            let Some(resource) = value
                .get("resource")
                .and_then(|resource| resource.as_str())
                .map(str::to_owned)
            else {
                continue;
            };
            if catalogs.insert(resource.clone(), value).is_some() {
                bail!(
                    "duplicate cutscene resource {resource:?} in {}",
                    dir.display()
                );
            }
        }
        Ok(catalogs)
    }

    let raw = load_catalogs(&raw_dir)?;
    let translations = load_catalogs(&translations_dir)?;
    if raw.keys().collect::<Vec<_>>() != translations.keys().collect::<Vec<_>>() {
        bail!(
            "cutscene resource set differs between {} and {}",
            raw_dir.display(),
            translations_dir.display()
        );
    }

    let mut resource_count = 0usize;
    let mut entry_count = 0usize;
    let mut target_count = 0usize;
    let mut structural_count = 0usize;
    let mut over_budget_count = 0usize;
    let mut max_growth = 0usize;
    for (resource, raw_catalog) in &raw {
        let translated_catalog = translations
            .get(resource)
            .with_context(|| format!("missing translated catalog for {resource}"))?;
        let report = validate_catalog_pair(raw_catalog, translated_catalog)
            .with_context(|| format!("validate {resource}"))?;
        println!(
            "{resource}: {} entries / {} translated targets / {} structural / {} over budget (max +{} bytes)",
            report.entries,
            report.targets,
            report.structural,
            report.over_budget.len(),
            report.max_growth
        );
        for overflow in &report.over_budget {
            println!(
                "  {}: {} bytes / {} budget (+{})",
                overflow.id,
                overflow.required,
                overflow.budget,
                overflow.required - overflow.budget
            );
        }
        if let Some(repack) = &report.pointer_grid_repack {
            println!(
                "  {} repack: {} bytes / {} capacity ({} spare)",
                repack.region,
                repack.required,
                repack.capacity,
                repack.capacity - repack.required,
            );
        }
        resource_count += 1;
        entry_count += report.entries;
        target_count += report.targets;
        structural_count += report.structural;
        over_budget_count += report.over_budget.len();
        max_growth = max_growth.max(report.max_growth);
    }
    println!(
        "validated {resource_count} resources / {entry_count} entries / {target_count} translated targets / {structural_count} structural / {over_budget_count} over budget (max +{max_growth} bytes)"
    );
    Ok(())
}

fn refresh_cutscene_translation_metadata(
    raw_dir: PathBuf,
    translations_dir: PathBuf,
) -> Result<()> {
    use pc98_madou_ars::cutscene_translation::{refresh_catalog_metadata, validate_catalog_pair};

    let mut raw_paths = std::fs::read_dir(&raw_dir)
        .with_context(|| format!("read directory {}", raw_dir.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    raw_paths.sort();

    // Complete every merge and validation before the first write, so one bad
    // catalog cannot leave the staged directory partially refreshed.
    let mut planned = Vec::<(PathBuf, Vec<u8>)>::new();
    for raw_path in raw_paths {
        if raw_path
            .extension()
            .and_then(|extension| extension.to_str())
            != Some("json")
        {
            continue;
        }
        let raw: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&raw_path).with_context(|| format!("read {}", raw_path.display()))?,
        )
        .with_context(|| format!("parse {}", raw_path.display()))?;
        if raw
            .get("resource")
            .and_then(|value| value.as_str())
            .is_none()
        {
            continue;
        }
        let file_name = raw_path
            .file_name()
            .context("raw catalog path lacks a file name")?;
        let translation_path = translations_dir.join(file_name);
        let translation: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&translation_path)
                .with_context(|| format!("read {}", translation_path.display()))?,
        )
        .with_context(|| format!("parse {}", translation_path.display()))?;
        let merged = refresh_catalog_metadata(&raw, &translation)
            .with_context(|| format!("refresh {}", translation_path.display()))?;
        validate_catalog_pair(&raw, &merged)
            .with_context(|| format!("validate refreshed {}", translation_path.display()))?;
        let mut output = serde_json::to_vec_pretty(&merged)?;
        output.push(b'\n');
        planned.push((translation_path, output));
    }
    if planned.is_empty() {
        bail!(
            "no cutscene resource catalogs found in {}",
            raw_dir.display()
        );
    }
    for (translation_path, output) in &planned {
        std::fs::write(translation_path, output)
            .with_context(|| format!("write {}", translation_path.display()))?;
        println!("refreshed {}", translation_path.display());
    }
    println!("refreshed {} cutscene translation catalogs", planned.len());
    Ok(())
}

fn validate_translation(
    input_path: PathBuf,
    script_path: PathBuf,
    load_offset: &str,
    max_line: usize,
    sheet_json_path: Option<PathBuf>,
) -> Result<()> {
    use pc98_madou_ars::overlay_messages::unified_catalog;
    use pc98_madou_ars::overlay_reloc::SheetCodes;
    use std::collections::HashMap;

    let load_offset = parse_usize(load_offset)?;
    let input =
        std::fs::read(&input_path).with_context(|| format!("read {}", input_path.display()))?;
    let decoded = pc98_madou_ars::overlay_lz::decode_overlay_lz(&input)
        .with_context(|| format!("decode {}", input_path.display()))?
        .output;
    let baseline: HashMap<usize, _> =
        unified_catalog(&decoded, load_offset, 4, &[2, 4, 6, 8, 10, 12, 14, 16])?
            .into_iter()
            .map(|m| (m.string_decoded_offset, m))
            .collect();

    let script_text = std::fs::read_to_string(&script_path)
        .with_context(|| format!("read {}", script_path.display()))?;
    let script: serde_json::Value =
        serde_json::from_str(&script_text).context("parse translation script")?;
    let entries = script
        .get("entries")
        .and_then(|v| v.as_array())
        .context("script missing `entries` array")?;
    let sheet = match &sheet_json_path {
        Some(p) => SheetCodes::load(p)?,
        None => SheetCodes::from_json_str(&KFONT_SHEET_JSON)?,
    };

    let is_source_cjk = |c: char| {
        matches!(c as u32,
            0x3040..=0x309F | 0x30A0..=0x30FF | 0x4E00..=0x9FFF | 0x3400..=0x4DBF)
    };

    let mut critical: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut translated = 0usize;

    for entry in entries {
        let id = entry.get("id").and_then(|v| v.as_str()).unwrap_or("?");
        let off = entry
            .get("string_decoded_offset")
            .and_then(|v| v.as_str())
            .and_then(|s| parse_usize(s).ok())
            .with_context(|| format!("{id}: bad string_decoded_offset"))?;
        let ko = entry.get("ko").and_then(|v| v.as_str()).unwrap_or("");
        let status = entry.get("status").and_then(|v| v.as_str()).unwrap_or("");

        // Protected-field integrity: text/raw_hex/byte_budget must match the
        // overlay-derived baseline (the translator only edits ko/status/notes).
        match baseline.get(&off) {
            None => critical.push(format!("{id}: offset 0x{off:04X} not in overlay baseline")),
            Some(base) => {
                let script_text = entry.get("text").and_then(|v| v.as_str()).unwrap_or("");
                if script_text != base.text {
                    critical.push(format!("{id}: protected `text` altered"));
                }
                let script_raw = entry.get("raw_hex").and_then(|v| v.as_str()).unwrap_or("");
                if script_raw != hex_encode(&base.raw) {
                    critical.push(format!("{id}: protected `raw_hex` altered"));
                }
                let script_budget = entry
                    .get("byte_budget")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                if script_budget as usize != base.byte_budget {
                    critical.push(format!("{id}: protected `byte_budget` altered"));
                }
            }
        }

        if ko.is_empty() {
            if status == "done" {
                critical.push(format!("{id}: status=done but ko is empty"));
            }
            continue;
        }
        translated += 1;

        // Encodability: an unmappable glyph is a build error, never a silent skip.
        match sheet.encode_line(ko) {
            Err(_) => critical.push(format!(
                "{id}: ko has a glyph not in the sheet or Shift-JIS"
            )),
            Ok(encoded) => {
                // Preserve complete control instructions, including parameters.
                // Newlines may move, but an embedded 0x00 parameter must never be
                // mistaken for a message terminator or an independent code.
                if let Some(base) = baseline.get(&off) {
                    match (
                        pc98_madou_ars::renderer_control::control_sequences(&base.raw, false),
                        pc98_madou_ars::renderer_control::control_sequences(&encoded, false),
                    ) {
                        (Ok(source), Ok(translated)) => {
                            let mut delta = std::collections::BTreeMap::<Vec<u8>, i32>::new();
                            for sequence in source {
                                *delta.entry(sequence).or_default() += 1;
                            }
                            for sequence in translated {
                                *delta.entry(sequence).or_default() -= 1;
                            }
                            let mismatch = delta
                                .into_iter()
                                .filter(|(_, count)| *count != 0)
                                .map(|(sequence, count)| {
                                    let code = hex_encode(&sequence);
                                    if count > 0 {
                                        format!("0x{code} dropped x{count}")
                                    } else {
                                        format!("0x{code} added x{}", -count)
                                    }
                                })
                                .collect::<Vec<_>>();
                            if !mismatch.is_empty() {
                                critical.push(format!(
                                    "{id}: control instruction not preserved ({})",
                                    mismatch.join(", ")
                                ));
                            }
                        }
                        (Err(err), _) => {
                            critical.push(format!("{id}: source control stream is invalid ({err})"))
                        }
                        (_, Err(err)) => critical.push(format!(
                            "{id}: translated control stream is invalid ({err})"
                        )),
                    }
                }
                if let Some(base) = baseline.get(&off)
                    && encoded.len() >= base.byte_budget
                {
                    // Not fatal -- relocation handles growth -- but surface it.
                    warnings.push(format!(
                        "{id}: ko {} bytes over budget {} -> will relocate",
                        encoded.len() + 1,
                        base.byte_budget
                    ));
                }
            }
        }

        // Residual source text: leftover Japanese in a translated line.
        if ko.chars().any(is_source_cjk) {
            warnings.push(format!("{id}: ko still contains Japanese characters"));
        }

        // Display width: each visual line's full-width cell count vs the box.
        for line in ko.split('\n') {
            let cells = line.chars().filter(|&c| c != '\r' && !c.is_ascii()).count();
            if cells > max_line {
                warnings.push(format!("{id}: line {cells} cells > {max_line}"));
            }
        }
    }

    println!(
        "validated {} entries ({} translated): {} critical, {} warnings",
        entries.len(),
        translated,
        critical.len(),
        warnings.len(),
    );
    for warning in warnings.iter().take(40) {
        println!("  warn: {warning}");
    }
    for issue in &critical {
        println!("  CRITICAL: {issue}");
    }
    if !critical.is_empty() {
        bail!(
            "{} critical translation issue(s); not build-ready",
            critical.len()
        );
    }
    Ok(())
}

fn list_enemy_names(input_paths: Vec<PathBuf>, json_output: Option<PathBuf>) -> Result<()> {
    let mut sources = Vec::new();
    let mut total_names = 0usize;

    println!("source\tname_off\tattack_off\tname");
    for input_path in &input_paths {
        let input =
            std::fs::read(input_path).with_context(|| format!("read {}", input_path.display()))?;
        let names = pc98_madou_ars::enemy_text::find_enemy_attack_names(&input);
        total_names += names.len();

        println!("# {}: {} names", input_path.display(), names.len());
        for entry in &names {
            println!(
                "{}\t0x{:04X}\t0x{:04X}\t{}",
                input_path.display(),
                entry.name_offset,
                entry.attack_text_offset,
                entry.name
            );
        }

        sources.push(json!({
            "path": input_path.display().to_string(),
            "size": input.len(),
            "names": names.iter().map(|entry| json!({
                "id": format!(
                    "{}_{:04X}",
                    input_path
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .unwrap_or("enemy")
                        .to_ascii_uppercase(),
                    entry.name_offset,
                ),
                "source": input_path.display().to_string(),
                "name_offset": format!("0x{:04X}", entry.name_offset),
                "name_end_offset": format!("0x{:04X}", entry.name_end_offset),
                "attack_text_offset": format!("0x{:04X}", entry.attack_text_offset),
                "raw_hex": hex_encode(&entry.raw),
                "trailing_hex": hex_encode(&entry.trailing),
                "name": entry.name,
            })).collect::<Vec<_>>(),
        }));
    }

    println!(
        "found {total_names} enemy attack name(s) across {} source(s)",
        input_paths.len()
    );

    if let Some(json_output) = json_output {
        write_enemy_names_json(&json_output, sources)?;
        println!("wrote {}", json_output.display());
    }

    Ok(())
}

fn glyph_stats(
    input_paths: Vec<PathBuf>,
    disk_paths: Vec<PathBuf>,
    top: usize,
    json_output: Option<PathBuf>,
) -> Result<()> {
    if input_paths.is_empty() && disk_paths.is_empty() {
        bail!("glyph-stats requires at least one input file or --disk image");
    }

    let mut stats = pc98_madou_ars::glyph_stats::GlyphStats::new();

    for input_path in input_paths {
        let input =
            std::fs::read(&input_path).with_context(|| format!("read {}", input_path.display()))?;
        stats.add_source(input_path.display().to_string(), &input);
    }

    for disk_path in disk_paths {
        let disk =
            std::fs::read(&disk_path).with_context(|| format!("read {}", disk_path.display()))?;
        let hdm = pc98_madou_ars::hdm::HdmFile::parse(&disk)
            .with_context(|| format!("parse {}", disk_path.display()))?;
        let volume = pc98_madou_ars::fat12::Fat12Volume::open(hdm.as_bytes())
            .with_context(|| format!("open FAT12 {}", disk_path.display()))?;
        for entry in volume.list_files() {
            let data = volume
                .read_entry(&entry)
                .with_context(|| format!("read {} from {}", entry.name, disk_path.display()))?;
            stats.add_source(format!("{}::{}", disk_path.display(), entry.name), &data);
        }
    }

    print_glyph_stats(&stats, top);

    if let Some(json_output) = json_output {
        write_glyph_stats_json(&json_output, &stats)?;
        println!("wrote {}", json_output.display());
    }

    Ok(())
}

#[derive(Clone, Copy)]
enum RendererOffset {
    Auto,
    Fixed(usize),
}

impl RendererOffset {
    fn label(self) -> String {
        match self {
            RendererOffset::Auto => "auto".to_string(),
            RendererOffset::Fixed(offset) => format!("0x{offset:04X}"),
        }
    }
}

fn parse_renderer_offset(raw: &str) -> Result<RendererOffset> {
    if raw.eq_ignore_ascii_case("auto") {
        Ok(RendererOffset::Auto)
    } else {
        parse_usize(raw).map(RendererOffset::Fixed)
    }
}

fn empty_renderer_scan(
    renderer_decoded_offset: usize,
    renderer_logical_offset: usize,
) -> pc98_madou_ars::overlay_text::RendererMessageScan {
    pc98_madou_ars::overlay_text::RendererMessageScan {
        renderer_decoded_offset,
        renderer_logical_offset,
        refs: Vec::new(),
    }
}

fn parse_usize(raw: &str) -> Result<usize> {
    if let Some(hex) = raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
        usize::from_str_radix(hex, 16).with_context(|| format!("parse hex value {raw}"))
    } else {
        raw.parse::<usize>()
            .with_context(|| format!("parse decimal value {raw}"))
    }
}

fn write_json_output(
    output_path: &PathBuf,
    renderer_offset: String,
    load_offset: usize,
    sources: Vec<serde_json::Value>,
) -> Result<()> {
    let value = json!({
        "schema": "pc98_madou_ars.overlay_messages.v1",
        "renderer_logical_offset": renderer_offset,
        "load_offset": format!("0x{load_offset:04X}"),
        "sources": sources,
    });
    if let Some(parent) = output_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("mkdir -p {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(&value).context("serialize overlay messages JSON")?;
    std::fs::write(output_path, bytes).with_context(|| format!("write {}", output_path.display()))
}

fn write_enemy_names_json(output_path: &PathBuf, sources: Vec<serde_json::Value>) -> Result<()> {
    let value = json!({
        "schema": "pc98_madou_ars.enemy_names.v1",
        "sources": sources,
    });
    if let Some(parent) = output_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("mkdir -p {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(&value).context("serialize enemy names JSON")?;
    std::fs::write(output_path, bytes).with_context(|| format!("write {}", output_path.display()))
}

fn print_glyph_stats(stats: &pc98_madou_ars::glyph_stats::GlyphStats, top: usize) {
    println!("=== A.R.S SJIS glyph statistics ===");
    println!("sources: {}", stats.sources.len());
    println!("total pair occurrences: {}", stats.total_pair_occurrences());
    println!("unique pairs: {}", stats.unique_pairs());
    println!("half-width kana bytes: {}", stats.half_kana_bytes);
    println!(
        "free decodable pairs: {} / {}",
        stats.free_pairs(),
        pc98_madou_ars::glyph_stats::total_decodable_sjis_pairs()
    );
    println!();
    println!("Hangul prefix candidate region:");
    for lead in 0xEB..=0xFCu8 {
        let occurrences = stats.prefix_occurrences(lead);
        let mark = if occurrences == 0 { "free" } else { "USED" };
        println!("  0x{lead:02X}: {occurrences:>6} {mark}");
    }
    println!();
    println!("Top {top} pairs:");
    for ((lead, trail), occurrences) in stats.top_pairs(top) {
        let ch = pc98_madou_ars::glyph_stats::decode_sjis_pair(lead, trail).unwrap_or('?');
        println!("  0x{lead:02X} 0x{trail:02X}: {occurrences:>6} {ch}");
    }
}

fn write_glyph_stats_json(
    output_path: &PathBuf,
    stats: &pc98_madou_ars::glyph_stats::GlyphStats,
) -> Result<()> {
    let value = json!({
        "schema": "pc98_madou_ars.glyph_stats.v1",
        "source_count": stats.sources.len(),
        "sources": stats.sources.iter().map(|source| json!({
            "display": source.display,
            "size": source.size,
            "sjis_pairs": source.sjis_pairs,
            "half_kana": source.half_kana,
        })).collect::<Vec<_>>(),
        "total_pair_occurrences": stats.total_pair_occurrences(),
        "unique_pairs": stats.unique_pairs(),
        "half_kana_bytes": stats.half_kana_bytes,
        "total_decodable_pairs": pc98_madou_ars::glyph_stats::total_decodable_sjis_pairs(),
        "free_pairs": stats.free_pairs(),
        "lead_occurrences": stats.lead_counts.iter().map(|(&lead, &occurrences)| {
            (format!("0x{lead:02X}"), occurrences)
        }).collect::<std::collections::BTreeMap<_, _>>(),
        "hangul_prefix_candidates": (0xEB..=0xFCu8).map(|lead| json!({
            "lead": format!("0x{lead:02X}"),
            "occurrences": stats.prefix_occurrences(lead),
            "free": stats.prefix_occurrences(lead) == 0,
        })).collect::<Vec<_>>(),
        "pair_occurrences": stats.pair_counts.iter().map(|(&(lead, trail), &occurrences)| json!({
            "lead": format!("0x{lead:02X}"),
            "trail": format!("0x{trail:02X}"),
            "char": pc98_madou_ars::glyph_stats::decode_sjis_pair(lead, trail)
                .map(|ch| ch.to_string())
                .unwrap_or_default(),
            "occurrences": occurrences,
        })).collect::<Vec<_>>(),
    });
    if let Some(parent) = output_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("mkdir -p {}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(&value).context("serialize glyph stats JSON")?;
    std::fs::write(output_path, bytes).with_context(|| format!("write {}", output_path.display()))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0F) as usize] as char);
    }
    out
}

#[cfg(test)]
mod embedded_sheet_tests {
    use super::*;

    // The embedded default sheet (bin+json) is baked in with include_bytes!/
    // include_str! and drawn by the CURRENT contiguous hook, so a stale sheet from
    // before a capacity bump would silently blank whole rows in-game. Fail the
    // build here instead (there is no build.rs to regenerate it).
    #[test]
    #[ignore = "requires assets/gaiji/kfont.bin and kfont.json"]
    fn embedded_sheet_matches_current_geometry() {
        let meta: serde_json::Value = serde_json::from_str(&KFONT_SHEET_JSON).unwrap();
        assert_eq!(
            meta["capacity"].as_u64(),
            Some(940),
            "embedded kfont.json is not rung-2 (940); regenerate it"
        );
        let rows: Vec<&str> = meta["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(
            rows,
            [
                "0x75", "0x76", "0x77", "0x78", "0x79", "0x7A", "0x7B", "0x7C", "0x7D", "0x7E"
            ],
            "embedded kfont.json rows are not the contiguous 0x75-0x7E band"
        );
        // Sheet bytes must not reach the runtime scratch word.
        let sheet_bytes = KFONT_SHEET.len();
        assert!(
            sheet_bytes <= pc98_madou_ars::hook_geometry::SCRATCH_OFF as usize,
            "embedded kfont.bin {sheet_bytes} bytes reaches the hook scratch"
        );
        // And it must parse under the hardened loader (every code EB-EF, no dups).
        pc98_madou_ars::overlay_reloc::SheetCodes::from_json_str(&KFONT_SHEET_JSON)
            .expect("embedded sheet fails the hardened loader");
    }

    #[test]
    fn sweep_rows_cover_the_current_contiguous_band() {
        assert_eq!(SWEEP_ROWS.len(), 10);
        assert_eq!(SWEEP_ROWS, std::array::from_fn(|index| 0x75 + index as u16));
    }

    #[test]
    fn shipping_translation_status_is_fail_closed() {
        let complete = json!({"ko": "완료", "status": "complete"});
        assert_eq!(
            build_translation(&complete, false, "entry").unwrap(),
            "완료"
        );

        let draft = json!({"ko": "초안", "status": "needs_review"});
        assert!(build_translation(&draft, false, "entry").is_err());
        assert_eq!(build_translation(&draft, true, "entry").unwrap(), "초안");

        let empty = json!({"ko": "", "status": "complete"});
        assert!(build_translation(&empty, false, "entry").is_err());
    }

    #[test]
    fn fixed_slot_rejects_one_byte_overflow_without_mutating() {
        let mut decoded = b"AA\0NEXT\0".to_vec();
        let before = decoded.clone();

        assert!(!patch_in_place_slot(&mut decoded, 0, b"AAA", 3, "test slot").unwrap());
        assert_eq!(decoded, before);
    }
}
