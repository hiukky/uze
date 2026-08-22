//! Codex automatic provisioning (install/update via the official installer
//! script) and binary detection.

use std::process::Command;

use uze_core::{
    Result,
    integration::HarnessDetection,
    provisioning::{ProcessRunner, ProcessSpec, ProvisionAction, ProvisioningResult},
};

pub(super) fn provision_cli(
    runner: &dyn ProcessRunner,
    executable: &str,
    before: HarnessDetection,
    install: ProcessSpec,
    update: ProcessSpec,
    method: &str,
) -> Result<ProvisioningResult> {
    if !cfg!(unix) {
        return Ok(ProvisioningResult::blocked(
            "Codex automatic provisioning is currently supported on Unix and WSL only",
        ));
    }
    let action = if before.present {
        ProvisionAction::Update
    } else {
        ProvisionAction::Install
    };
    let command = if before.present { update } else { install };
    let outcome = match runner.run(&command) {
        Ok(outcome) => outcome,
        Err(_) => {
            return Ok(ProvisioningResult::failed(
                action,
                method,
                "official installer could not be started",
            ));
        }
    };
    if !outcome.success {
        let reason = if outcome.timed_out {
            "official installer timed out"
        } else {
            "official installer exited unsuccessfully"
        };
        return Ok(ProvisioningResult::failed(action, method, reason));
    }
    let verified = runner.run(&ProcessSpec::new(executable, ["--version"]));
    if !matches!(verified, Ok(output) if output.success) {
        return Ok(ProvisioningResult::failed(
            action,
            method,
            "installer finished but the executable could not be verified",
        ));
    }
    Ok(ProvisioningResult::verified(
        action,
        method,
        detect_binary(executable),
    ))
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
