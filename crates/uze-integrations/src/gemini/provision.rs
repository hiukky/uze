//! Gemini CLI automatic provisioning (install/update via the official npm
//! package) and binary detection.

use std::process::Command;

use uze_core::{
    Result,
    integration::HarnessDetection,
    provisioning::{ProcessRunner, ProcessSpec, ProvisionAction, ProvisioningResult},
};

pub(super) fn provision_npm(
    runner: &dyn ProcessRunner,
    present: bool,
) -> Result<ProvisioningResult> {
    if !cfg!(unix) {
        return Ok(ProvisioningResult::blocked(
            "Gemini automatic provisioning is currently supported on Unix and WSL only",
        ));
    }
    let action = if present {
        ProvisionAction::Update
    } else {
        ProvisionAction::Install
    };
    let outcome = match runner.run(
        &ProcessSpec::new("npm", ["install", "-g", "@google/gemini-cli@latest"])
            .with_inherited_output(),
    ) {
        Ok(outcome) => outcome,
        Err(_) => {
            return Ok(ProvisioningResult::failed(
                action,
                "official-npm",
                "official package installation could not be started",
            ));
        }
    };
    if !outcome.success {
        let reason = if outcome.timed_out {
            "official package installation timed out"
        } else {
            "official package installation exited unsuccessfully"
        };
        return Ok(ProvisioningResult::failed(action, "official-npm", reason));
    }
    let verified = runner.run(&ProcessSpec::new("gemini", ["--version"]));
    if !matches!(verified, Ok(output) if output.success) {
        return Ok(ProvisioningResult::failed(
            action,
            "official-npm",
            "installation finished but the executable could not be verified",
        ));
    }
    Ok(ProvisioningResult::verified(
        action,
        "official-npm",
        detect_binary("gemini"),
    ))
}

pub(super) fn detect_binary(program: &str) -> HarnessDetection {
    match Command::new(program).arg("--version").output() {
        // `gemini --version` prints a bare "0.56.0" — one token either way.
        Ok(output) if output.status.success() => HarnessDetection {
            present: true,
            version: String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .next()
                .map(str::to_owned),
        },
        _ => HarnessDetection::default(),
    }
}
