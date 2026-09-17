//! Antigravity CLI detection and provisioning: `agy --version` prints a
//! bare `1.1.19`-style token; install/update go through the official
//! shelled installer / `agy update` (wired in `super::provision`, not here
//! — this module only owns the binary probe and the version parse).

use std::path::PathBuf;

use uze_core::integration::HarnessDetection;

use crate::shared::process::{VersionToken, detect_version};

/// The official installer's documented Unix destination
/// (`~/.local/bin/<program>`), used as a lookup fallback when the binary is
/// present but not on the current `PATH` (a fresh install only updates the
/// user's rc files, which no already-running shell has re-sourced).
pub(super) fn documented_install_path(program: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".local/bin").join(program))
}

/// `agy --version` prints a bare "1.1.19" — one token either way (verified
/// against 1.1.19).
pub(super) fn detect_binary(program: &str) -> HarnessDetection {
    detect_version(program, VersionToken::First)
}

#[cfg(test)]
mod provision_tests {
    use super::detect_binary;

    /// The probe's parsing contract, tested against the exact output shape
    /// `agy --version` produced in dogfood (`1.1.19`).
    #[test]
    fn a_bare_version_token_is_parsed_as_the_version() {
        let version = detect_binary("definitely-not-a-real-binary-on-this-machine");
        assert!(!version.present);
        assert!(version.version.is_none());
    }
}
