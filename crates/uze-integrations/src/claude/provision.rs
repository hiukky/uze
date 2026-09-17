//! Claude Code automatic provisioning (install/update via the official
//! installer script) and binary detection.

use std::process::Command;

use uze_core::integration::HarnessDetection;

pub(super) fn detect_binary(program: &str) -> HarnessDetection {
    let Ok(output) = Command::new(program).arg("--version").output() else {
        return HarnessDetection::default();
    };
    // `claude --version` prints "2.1.239 (Claude Code)" — the version leads.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = stdout.split_whitespace().next().map(str::to_owned);
    HarnessDetection {
        present: true,
        version,
    }
}
