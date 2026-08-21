//! Harness-agnostic process and outcome contracts for explicit provisioning.
//!
//! Concrete integrations own their documented vendor commands. This module
//! deliberately models only how UZE invokes and records an opaque command.

use std::{
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use crate::{
    error::{Result, UzeError},
    integration::HarnessDetection,
};

/// One integration-owned command. `program` and `arguments` are never
/// persisted: they can change with vendor installers and may include paths
/// that do not belong in durable UZE state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessSpec {
    pub program: String,
    pub arguments: Vec<String>,
    pub timeout: Duration,
    pub output: ProcessOutput,
}

impl ProcessSpec {
    pub fn new(
        program: impl Into<String>,
        arguments: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            program: program.into(),
            arguments: arguments.into_iter().map(Into::into).collect(),
            timeout: Duration::from_secs(300),
            output: ProcessOutput::Quiet,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Lets an explicit operator-visible action (such as an official vendor
    /// installer) report progress directly to the terminal. Verification
    /// probes remain quiet by default, and neither mode persists output.
    pub fn with_inherited_output(mut self) -> Self {
        self.output = ProcessOutput::Inherit;
        self
    }
}

/// Where a process may write. This is transient UI behavior, never durable
/// provisioning state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessOutput {
    Quiet,
    Inherit,
}

/// Transient process observation. It is intentionally not serializable: UZE
/// never stores installer output, which can contain vendor or environment
/// details not relevant to future ownership decisions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProcessResult {
    pub success: bool,
    pub timed_out: bool,
}

pub trait ProcessRunner: Send + Sync {
    fn run(&self, spec: &ProcessSpec) -> Result<ProcessResult>;
}

/// Production command runner. The bounded polling loop makes installer hangs
/// diagnosable without adding an async runtime to UZE Core.
#[derive(Default)]
pub struct SystemProcessRunner;

impl ProcessRunner for SystemProcessRunner {
    fn run(&self, spec: &ProcessSpec) -> Result<ProcessResult> {
        let mut command = Command::new(&spec.program);
        command.args(&spec.arguments).stdin(Stdio::null());
        match spec.output {
            ProcessOutput::Quiet => {
                command.stdout(Stdio::null()).stderr(Stdio::null());
            }
            ProcessOutput::Inherit => {
                command.stdout(Stdio::inherit()).stderr(Stdio::inherit());
            }
        }
        let mut child = command.spawn().map_err(|source| UzeError::Process {
            program: spec.program.clone(),
            source,
        })?;
        let started = Instant::now();
        loop {
            if let Some(status) = child.try_wait().map_err(|source| UzeError::Process {
                program: spec.program.clone(),
                source,
            })? {
                return Ok(ProcessResult {
                    success: status.success(),
                    timed_out: false,
                });
            }
            if started.elapsed() >= spec.timeout {
                child.kill().map_err(|source| UzeError::Process {
                    program: spec.program.clone(),
                    source,
                })?;
                let _ = child.wait();
                return Ok(ProcessResult {
                    success: false,
                    timed_out: true,
                });
            }
            thread::sleep(Duration::from_millis(25));
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProvisionAction {
    None,
    Install,
    Update,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProvisionStatus {
    Verified,
    Failed,
    Blocked,
}

/// Secret-free, product-facing outcome for one explicit `uze setup` action.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProvisioningResult {
    pub action: ProvisionAction,
    pub status: ProvisionStatus,
    pub detection: HarnessDetection,
    pub method: String,
    pub reason: Option<String>,
}

impl ProvisioningResult {
    pub fn blocked(reason: impl Into<String>) -> Self {
        Self {
            action: ProvisionAction::None,
            status: ProvisionStatus::Blocked,
            detection: HarnessDetection::default(),
            method: "unsupported".to_owned(),
            reason: Some(reason.into()),
        }
    }

    pub fn failed(
        action: ProvisionAction,
        method: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            action,
            status: ProvisionStatus::Failed,
            detection: HarnessDetection::default(),
            method: method.into(),
            reason: Some(reason.into()),
        }
    }

    pub fn verified(
        action: ProvisionAction,
        method: impl Into<String>,
        detection: HarnessDetection,
    ) -> Self {
        Self {
            action,
            status: ProvisionStatus::Verified,
            detection,
            method: method.into(),
            reason: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_installer_commands_can_inherit_progress_without_affecting_probes() {
        let installer = ProcessSpec::new("installer", ["install"]).with_inherited_output();
        let probe = ProcessSpec::new("tool", ["--version"]);
        assert_eq!(installer.output, ProcessOutput::Inherit);
        assert_eq!(probe.output, ProcessOutput::Quiet);
    }
}
