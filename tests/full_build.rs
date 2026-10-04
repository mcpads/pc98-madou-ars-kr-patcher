use pc98_madou_ars::release_patch::apply_release_patch;
use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "common/mod.rs"]
mod common;

const SCHEZO_GAME_DISK: &str =
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (She-zo Game disk).hdm";
const DEMO_DISK: &str = "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (Demo disk).hdm";
const SCHEZO_DATA_DISK: &str =
    "roms/Madou Monogatari A.R.S [FD]/Madou Monogatari A.R.S (She-zo Data disk).hdm";

struct TestDir(PathBuf);

impl TestDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "pc98-madou-ars-full-build-test-{}-{label}",
            std::process::id(),
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir(&path).expect("create full-build test directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]

#[ignore = "requires original A.R.S HDMs in roms/ and translations in assets/translations/"]
fn full_build_composes_schezo_and_emits_a_self_applying_bps() {
    let Some(source) = common::try_read(SCHEZO_GAME_DISK) else {
        return;
    };
    if common::try_read(DEMO_DISK).is_none() || common::try_read(SCHEZO_DATA_DISK).is_none() {
        return;
    }

    let temp = TestDir::new("character");
    let image = temp.path().join("nested/images/schezo-full.hdm");
    let patch = temp.path().join("nested/patches/schezo-full.bps");
    let output = Command::new(env!("CARGO_BIN_EXE_pc98_madou_ars"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args([
            "build-full-patch",
            "--game-disk",
            SCHEZO_GAME_DISK,
            "--demo-disk",
            DEMO_DISK,
            "--data-disk",
            SCHEZO_DATA_DISK,
            "--translations-dir",
            "assets/translations/needs_review",
            "--allow-needs-review",
            "--image-output",
        ])
        .arg(&image)
        .arg("--bps-output")
        .arg(&patch)
        .output()
        .expect("run build-full-patch");
    assert!(
        output.status.success(),
        "build-full-patch failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let target = std::fs::read(&image).expect("read composed HDM");
    let patch_bytes = std::fs::read(&patch).expect("read BPS");
    let applied = apply_release_patch(&source, &patch_bytes).expect("apply emitted BPS");
    assert_eq!(applied.target, target);

    assert!(
        !pc98_madou_ars::fat12_add::root_file_exists(&target, "TC.CNS")
            .expect("check default Game media boundary"),
        "the default full build must not apply the optional two-disk transform"
    );
    assert_eq!(
        pc98_madou_ars::read_fat12_file_from_hdm(&target, "AUTOEXEC.BAT")
            .expect("read composed AUTOEXEC.BAT"),
        pc98_madou_ars::read_fat12_file_from_hdm(&source, "AUTOEXEC.BAT")
            .expect("read source AUTOEXEC.BAT"),
        "the default full build must preserve the source boot route"
    );

    let launcher = pc98_madou_ars::read_fat12_file_from_hdm(&target, "MADO_S.BAT")
        .expect("read composed Schezo launcher");
    assert!(
        launcher
            .windows(b"MAIN D:GAOO.OVL S:GAME_S.OVL S:SHEZO_OP.OVL".len())
            .any(|bytes| bytes == b"MAIN D:GAOO.OVL S:GAME_S.OVL S:SHEZO_OP.OVL"),
        "the manual Schezo launcher must retain its source command"
    );
    let main = pc98_madou_ars::read_fat12_file_from_hdm(&target, "MAIN.COM")
        .expect("read composed Schezo MAIN.COM");
    assert!(
        main.windows(b"KFONT.BIN\0".len())
            .any(|bytes| bytes == b"KFONT.BIN\0"),
        "direct Game-disk boot must keep MAIN's root-file renderer load"
    );
    let game = pc98_madou_ars::read_fat12_file_from_hdm(&target, "GAME_S.OVL")
        .expect("read composed Schezo overlay");
    let decoded = pc98_madou_ars::overlay_lz::decode_overlay_lz(&game)
        .expect("decode composed Schezo overlay")
        .output;
    assert_eq!(decoded[0], 0xFA, "the source GAME_S entry must stay intact");
    assert_eq!(
        &decoded[0x5D..0x60],
        &[0xE8, 0xFE, 0x1B],
        "the source GAME_S initialization call must stay intact"
    );
    assert!(
        std::fs::read_dir(image.parent().unwrap())
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".pc98-madou-ars-build-")),
        "full-build scratch directory was not removed"
    );
}
