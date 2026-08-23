//! Codex automatic provisioning (install/update via the official installer
//! script) and binary detection.

use std::process::Command;

use uze_core::{
    Result,
    integration::HarnessDetection,
    provisioning::{ProcessRunner, ProcessSpec, ProvisioningResult},
};

use crate::shared::provision::provision_cli as shared_provision_cli;

/// Thin, vendor-labeled call into the shared install/update/verify flow —
/// see `crate::shared::provision` for why this is safe to share with Claude
/// (byte-identical control flow) while `detect_binary`'s own `--version`
/// parsing stays here, unshared (Codex's output trails the version;
/// Claude's leads).
pub(super) fn provision_cli(
    runner: &dyn ProcessRunner,
    executable: &str,
    before: HarnessDetection,
    install: ProcessSpec,
    update: ProcessSpec,
    method: &str,
) -> Result<ProvisioningResult> {
    shared_provision_cli(
        runner,
        executable,
        "Codex",
        before,
        install,
        update,
        method,
        detect_binary,
    )
}

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
