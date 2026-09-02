//! Harness-agnostic process and outcome contracts for explicit provisioning.
//!
//! Concrete integrations own their documented vendor commands. This module
//! deliberately models only how UZE invokes and records an opaque command.

use std::{
    process::{Command, Stdio},
    time::Duration,
};

use serde::{Deserialize, Serialize};

use crate::{
    error::{Result, UzeError},
    integration::HarnessDetection,
    subprocess::{wait_with_timeout, with_process_group},
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
        // `Quiet` runs (verification probes, background installs) are never
        // watched by a user, so isolating into a new process group buys a
        // reliable whole-tree kill on timeout — the installer can fork
        // helpers (curl | bash), and the timeout must reach all of them.
        //
        // `Inherit` runs are the opposite: an operator is watching the
        // terminal and expects Ctrl-C to reach the child directly. A new
        // process group is detached from the terminal's foreground group,
        // so the terminal's SIGINT would stop reaching only `uze` itself,
        // leaving the child running invisibly in the background — worse
        // than the single-process kill this mode falls back to on timeout.
        let mut command = match spec.output {
            ProcessOutput::Quiet => with_process_group(command),
            ProcessOutput::Inherit => command,
        };
        let mut child = command.spawn().map_err(|source| UzeError::Process {
            program: spec.program.clone(),
            source,
        })?;
        let (status, timed_out) =
            wait_with_timeout(&mut child, spec.timeout).map_err(|source| UzeError::Process {
                program: spec.program.clone(),
                source,
            })?;
        Ok(ProcessResult {
            success: status.success() && !timed_out,
            timed_out,
        })
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

    #[cfg(unix)]
    mod process_group_tests {
        use super::*;
        use std::time::Instant;

        fn stat_field(pid: u32, index: usize) -> Option<u32> {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
            let after_comm = stat.rsplit_once(')')?.1;
            after_comm.split_whitespace().nth(index)?.parse().ok()
        }

        fn pgid_of(pid: u32) -> u32 {
            stat_field(pid, 2).expect("process must still be alive under /proc")
        }

        /// Finds the live child spawned as `/bin/sleep <marker>`, matched by
        /// exact argv (never a substring) so a concurrently running test's
        /// own spawned `sleep` — a different marker — can never be mistaken
        /// for this one.
        fn find_marked_sleep_child(marker: &str, deadline: Instant) -> u32 {
            loop {
                if let Ok(entries) = std::fs::read_dir("/proc") {
                    for entry in entries.flatten() {
                        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
                            continue;
                        };
                        let Ok(cmdline) = std::fs::read(entry.path().join("cmdline")) else {
                            continue;
                        };
                        let args: Vec<&str> = cmdline
                            .split(|&byte| byte == 0)
                            .filter_map(|part| std::str::from_utf8(part).ok())
                            .filter(|part| !part.is_empty())
                            .collect();
                        if args == ["/bin/sleep", marker] {
                            return pid;
                        }
                    }
                }
                assert!(
                    Instant::now() < deadline,
                    "the marked `sleep {marker}` child never appeared under /proc"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        #[test]
        fn quiet_output_isolates_the_child_into_its_own_process_group() {
            std::thread::spawn(|| {
                let _ = SystemProcessRunner.run(&ProcessSpec::new("/bin/sleep", ["2"]));
            });
            let child_pid = find_marked_sleep_child("2", Instant::now() + Duration::from_secs(5));
            assert_eq!(
                pgid_of(child_pid),
                child_pid,
                "a Quiet-output child must become its own process-group leader so a timeout can \
                 reliably kill the whole tree it may fork"
            );
        }

        #[test]
        fn inherited_output_child_stays_in_the_callers_process_group() {
            let our_pgid = pgid_of(std::process::id());
            std::thread::spawn(|| {
                let _ = SystemProcessRunner
                    .run(&ProcessSpec::new("/bin/sleep", ["3"]).with_inherited_output());
            });
            let child_pid = find_marked_sleep_child("3", Instant::now() + Duration::from_secs(5));
            assert_eq!(
                pgid_of(child_pid),
                our_pgid,
                "an Inherit-output child must stay in the caller's process group, or a terminal \
                 SIGINT (Ctrl-C) would stop reaching it once it only reaches uze's own group"
            );
        }
    }
}
