//! Locally supplied inputs for tests that the public tree cannot run alone.
//!
//! Tests that read these inputs are marked `#[ignore = "requires ..."]` and run
//! with `cargo test -- --ignored` once the named files are in place. A missing
//! input fails the test instead of skipping it.
#![allow(dead_code)]

pub const ROMS_REQUIRED_ENV: &str = "PC98_MADOU_ARS_ROMS_REQUIRED";

/// Inputs are always required; there is no skip mode.
pub fn roms_required() -> bool {
    true
}

pub fn try_read(path: &str) -> Option<Vec<u8>> {
    match std::fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(err) => panic!("required test input {path} is unavailable: {err}"),
    }
}
