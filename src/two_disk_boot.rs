//! Optional Game-disk transform for the two-drive emulator route.
//!
//! A normal A.R.S launch owns three media: shared Demo plus one character's
//! Game and Data disks. Two-drive emulators cannot mount all three at once.
//! This transform copies `TC.CNS` from the user's exact Demo disk and the
//! character's opening music data from its exact Data disk onto an already
//! composed Game disk. For Rulue/Schezo it also points `AUTOEXEC.BAT` at that
//! disk's own scenario files on drive `A:`. The matching Data disk remains the
//! second floppy for every other Data-owned resource.
//!
//! This is deliberately separate from distributable BPS/package creation:
//! `TC.CNS` and the copied music data are source media, not project-authored
//! patch assets.

use anyhow::{Context, Result, bail, ensure};

use crate::{fat12_add::EnsureReport, read_fat12_file_from_hdm};

const TC_FILE: &str = "TC.CNS";
const AUTOEXEC_FILE: &str = "AUTOEXEC.BAT";

const ARLE_AUTOEXEC: &[u8] = b"FPLAY.COM\r\nBPLAY.COM\r\nBSAMP.COM\r\nREADJS.COM\r\nMAIN.COM /M D:GAOO.OVL A:GAME_A.OVL A:DEMO.OVL 0:OP_A\r\n";
const RULUE_AUTOEXEC: &[u8] = b"FPLAY.COM\r\nBPLAY.COM\r\nBSAMP.COM\r\nREADJS.COM\r\nMAIN.COM /M D:GAOO.OVL A:GAME_R.OVL A:OPENINGR.OVL\r\n";
const SCHEZO_SOURCE_AUTOEXEC: &[u8] = b"FPLAY.COM\r\nBPLAY.COM\r\nBSAMP.COM\r\nREADJS.COM\r\nMAIN.COM /M D:GAOO.OVL S:GAME_S.OVL S:SHEZO_OP.OVL\r\n";
const SCHEZO_AUTOEXEC: &[u8] = b"FPLAY.COM\r\nBPLAY.COM\r\nBSAMP.COM\r\nREADJS.COM\r\nMAIN.COM /M D:GAOO.OVL A:GAME_S.OVL A:SHEZO_OP.OVL\r\n";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TwoDiskBootImage {
    pub character_id: &'static str,
    pub image: Vec<u8>,
    pub tc_cns_added: bool,
    pub opening_music_file: &'static str,
    pub opening_music_added: bool,
    pub autoexec_rewritten: bool,
}

#[derive(Clone, Copy)]
struct BootProfile {
    character_id: &'static str,
    game_overlay: &'static str,
    opening_music_file: &'static str,
    accepted_autoexec: &'static [&'static [u8]],
    target_autoexec: &'static [u8],
}

const BOOT_PROFILES: [BootProfile; 3] = [
    BootProfile {
        character_id: "arle",
        game_overlay: "GAME_A.OVL",
        opening_music_file: "MADO-A2.DAT",
        accepted_autoexec: &[ARLE_AUTOEXEC],
        target_autoexec: ARLE_AUTOEXEC,
    },
    BootProfile {
        character_id: "rulue",
        game_overlay: "GAME_R.OVL",
        opening_music_file: "MADO-R.DAT",
        accepted_autoexec: &[ARLE_AUTOEXEC, RULUE_AUTOEXEC],
        target_autoexec: RULUE_AUTOEXEC,
    },
    BootProfile {
        character_id: "schezo",
        game_overlay: "GAME_S.OVL",
        opening_music_file: "MADO-S.DAT",
        accepted_autoexec: &[SCHEZO_SOURCE_AUTOEXEC, SCHEZO_AUTOEXEC],
        target_autoexec: SCHEZO_AUTOEXEC,
    },
];

/// Build a Game image that boots with only its matching Data disk in drive 2.
///
/// `game_disk` may be an exact source image or a composed Korean Game product.
/// `demo_disk` and `data_disk` must be the exact project-supported Demo and
/// matching character Data images. The operation is transactional and
/// idempotent: existing byte-identical source files and an already rewritten
/// boot script are reused.
pub fn build_game_disk(
    game_disk: &[u8],
    demo_disk: &[u8],
    data_disk: &[u8],
) -> Result<TwoDiskBootImage> {
    let manifest = crate::media_identity::load_build_media()?;
    crate::media_identity::validate_disk(demo_disk, &manifest.demo)
        .context("two-disk boot Demo identity")?;

    let profile = identify_boot_profile(game_disk)?;
    let character_media = manifest
        .characters
        .iter()
        .find(|character| character.id == profile.character_id)
        .with_context(|| format!("missing {} media profile", profile.character_id))?;
    crate::media_identity::validate_disk(data_disk, &character_media.data)
        .with_context(|| format!("two-disk boot {} Data identity", profile.character_id))?;
    let source_autoexec = read_fat12_file_from_hdm(game_disk, AUTOEXEC_FILE)
        .context("read Game AUTOEXEC.BAT before two-disk transform")?;
    if !profile
        .accepted_autoexec
        .iter()
        .any(|accepted| source_autoexec == *accepted)
    {
        bail!(
            "{} Game AUTOEXEC.BAT is neither the exact source script nor the supported two-disk script",
            profile.character_id
        );
    }

    let tc = read_fat12_file_from_hdm(demo_disk, TC_FILE)
        .context("read exact Demo TC.CNS for two-disk boot")?;
    let tc_meta = crate::fat12_add::root_file_meta(demo_disk, TC_FILE)
        .context("read exact Demo TC.CNS metadata")?;
    let opening_music = read_fat12_file_from_hdm(data_disk, profile.opening_music_file)
        .with_context(|| format!("read exact Data {}", profile.opening_music_file))?;
    let opening_music_meta =
        crate::fat12_add::root_file_meta(data_disk, profile.opening_music_file)
            .with_context(|| format!("read exact Data {} metadata", profile.opening_music_file))?;

    let mut image = game_disk.to_vec();
    let tc_report = crate::fat12_add::ensure_root_file(&mut image, TC_FILE, &tc, tc_meta)
        .context("stage exact Demo TC.CNS on Game disk")?;
    let opening_music_report = crate::fat12_add::ensure_root_file(
        &mut image,
        profile.opening_music_file,
        &opening_music,
        opening_music_meta,
    )
    .with_context(|| {
        format!(
            "stage exact Data {} on Game disk",
            profile.opening_music_file
        )
    })?;
    let autoexec_rewritten = source_autoexec != profile.target_autoexec;
    if autoexec_rewritten {
        crate::fat12_replace::replace_file_in_place(
            &mut image,
            AUTOEXEC_FILE,
            profile.target_autoexec,
        )
        .with_context(|| format!("rewrite {} Game AUTOEXEC.BAT", profile.character_id))?;
    }

    ensure!(
        read_fat12_file_from_hdm(&image, TC_FILE)? == tc,
        "two-disk Game TC.CNS readback differs from exact Demo source"
    );
    ensure!(
        read_fat12_file_from_hdm(&image, profile.opening_music_file)? == opening_music,
        "two-disk Game {} readback differs from exact Data source",
        profile.opening_music_file
    );
    ensure!(
        read_fat12_file_from_hdm(&image, AUTOEXEC_FILE)? == profile.target_autoexec,
        "two-disk Game AUTOEXEC.BAT readback differs from the selected character route"
    );

    Ok(TwoDiskBootImage {
        character_id: profile.character_id,
        image,
        tc_cns_added: matches!(tc_report, EnsureReport::Added(_)),
        opening_music_file: profile.opening_music_file,
        opening_music_added: matches!(opening_music_report, EnsureReport::Added(_)),
        autoexec_rewritten,
    })
}

fn identify_boot_profile(game_disk: &[u8]) -> Result<&'static BootProfile> {
    let mut matches = Vec::new();
    for profile in &BOOT_PROFILES {
        if crate::fat12_add::root_file_exists(game_disk, profile.game_overlay)? {
            matches.push(profile);
        }
    }
    match matches.as_slice() {
        [profile] => Ok(profile),
        [] => bail!("Game disk has no recognized GAME_A/R/S overlay"),
        _ => bail!("Game disk contains more than one GAME_A/R/S overlay"),
    }
}
