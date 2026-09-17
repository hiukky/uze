//! Codex automatic provisioning (install/update via the official installer
//! script) and binary detection.

use std::process::Command;

use uze_core::integration::HarnessDetection;

pub(super) fn detect_binary(program: &str) -> HarnessDetection {
    let Ok(output) = Command::new(program).arg("--version").output() else {
        return HarnessDetection::default();
    };
    // `codex --version` prints "codex-cli 0.148.0" — the version trails.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = stdout.split_whitespace().last().map(str::to_owned);
    HarnessDetection {
        present: true,
        version,
    }
}
