//! The official-installer provisioning flow shared, byte-for-byte except a
//! display label, between Claude Code and Codex: both install/update via a
//! shelled installer script, then verify with `<executable> --version`. No
//! vendor knowledge lives here — every vendor-specific fact (the install/
//! update commands themselves, the executable name, the display label used
//! in the platform-support message) is a parameter the caller supplies.
//!
//! Gemini (`npm install -g`, hardcoded package name, no injected
//! `ProcessSpec`) and OpenCode (its own `opencode`/`opencode2` alias
//! resolution) were compared against this shape during the audit that
//! produced this module and found to diverge in real, load-bearing ways —
//! not merely different constants — so neither was folded in here.

use uze_core::{
    Result,
    integration::HarnessDetection,
    provisioning::{ProcessRunner, ProcessSpec, ProvisionAction, ProvisioningResult},
};

/// `detect` re-probes the executable to capture its version string once
/// installation/update has been confirmed successful — deliberately a
/// caller-supplied closure rather than a shared parsing function, because
/// Claude's and Codex's own `--version` output shapes differ (leading vs.
/// trailing version token) and that parsing stays vendor-specific.
#[allow(clippy::too_many_arguments)]
pub(crate) fn provision_cli(
    runner: &dyn ProcessRunner,
    executable: &str,
    label: &str,
    before: HarnessDetection,
    install: ProcessSpec,
    update: ProcessSpec,
    method: &str,
    detect: impl Fn(&str) -> HarnessDetection,
) -> Result<ProvisioningResult> {
    if !cfg!(unix) {
        return Ok(ProvisioningResult::blocked(format!(
            "{label} automatic provisioning is currently supported on Unix and WSL only"
        )));
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
        detect(executable),
    ))
}

#[cfg(test)]
mod provision_cli_tests {
    use std::sync::Mutex;

    use uze_core::provisioning::{ProcessResult, ProcessSpec};

    use super::*;

    struct RecordingRunner {
        commands: Mutex<Vec<ProcessSpec>>,
        succeed: bool,
    }

    impl ProcessRunner for RecordingRunner {
        fn run(&self, spec: &ProcessSpec) -> Result<ProcessResult> {
            self.commands.lock().unwrap().push(spec.clone());
            Ok(ProcessResult {
                success: self.succeed,
                timed_out: false,
            })
        }
    }

    #[test]
    fn absent_before_dispatches_install_then_verifies() {
        let runner = RecordingRunner {
            commands: Mutex::new(Vec::new()),
            succeed: true,
        };
        let result = provision_cli(
            &runner,
            "does-not-exist-on-this-machine",
            "Test Harness",
            HarnessDetection::default(),
            ProcessSpec::new("sh", ["-c", "install"]),
            ProcessSpec::new("sh", ["-c", "update"]),
            "official-test-installer",
            |_| HarnessDetection::default(),
        )
        .unwrap();
        if cfg!(unix) {
            assert_eq!(result.action, ProvisionAction::Install);
            let commands = runner.commands.lock().unwrap();
            assert_eq!(commands[0].arguments, ["-c", "install"]);
            assert_eq!(commands[1].program, "does-not-exist-on-this-machine");
        }
    }

    #[test]
    fn present_before_dispatches_update_not_install() {
        let runner = RecordingRunner {
            commands: Mutex::new(Vec::new()),
            succeed: true,
        };
        let result = provision_cli(
            &runner,
            "does-not-exist-on-this-machine",
            "Test Harness",
            HarnessDetection {
                present: true,
                version: Some("1.0.0".to_owned()),
            },
            ProcessSpec::new("sh", ["-c", "install"]),
            ProcessSpec::new("sh", ["-c", "update"]),
            "official-test-installer",
            |_| HarnessDetection::default(),
        )
        .unwrap();
        if cfg!(unix) {
            assert_eq!(result.action, ProvisionAction::Update);
            let commands = runner.commands.lock().unwrap();
            assert_eq!(commands[0].arguments, ["-c", "update"]);
        }
    }

    #[test]
    fn installer_failure_is_reported_without_a_verification_attempt() {
        let runner = RecordingRunner {
            commands: Mutex::new(Vec::new()),
            succeed: false,
        };
        let result = provision_cli(
            &runner,
            "does-not-exist-on-this-machine",
            "Test Harness",
            HarnessDetection::default(),
            ProcessSpec::new("sh", ["-c", "install"]),
            ProcessSpec::new("sh", ["-c", "update"]),
            "official-test-installer",
            |_| HarnessDetection::default(),
        )
        .unwrap();
        if cfg!(unix) {
            assert_eq!(
                result.status,
                uze_core::provisioning::ProvisionStatus::Failed
            );
            assert_eq!(
                runner.commands.lock().unwrap().len(),
                1,
                "must not attempt --version verification after the installer itself failed"
            );
        }
    }
}
