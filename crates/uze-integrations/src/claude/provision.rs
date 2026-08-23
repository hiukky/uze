//! Claude Code automatic provisioning (install/update via the official
//! installer script) and binary detection.

use std::process::Command;

use uze_core::{
    Result,
    integration::HarnessDetection,
    provisioning::{ProcessRunner, ProcessSpec, ProvisioningResult},
};

use crate::shared::provision::provision_cli as shared_provision_cli;

/// Thin, vendor-labeled call into the shared install/update/verify flow —
/// see `crate::shared::provision` for why this is safe to share with Codex
/// (byte-identical control flow) while `detect_binary`'s own `--version`
/// parsing stays here, unshared (Claude's output leads with the version;
/// Codex's trails).
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
        "Claude Code",
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
    // `claude --version` prints "2.1.239 (Claude Code)" — the version leads.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = stdout.split_whitespace().next().map(str::to_owned);
    HarnessDetection {
        present: true,
        version,
    }
}
