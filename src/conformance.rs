//! Opt-in process boundary for real-harness conformance.
//!
//! This module deliberately classifies environmental failures separately from
//! resource compatibility. It is not used by deterministic core tests.

use std::{
    io,
    process::{Command, Output},
    thread,
    time::{Duration, Instant},
};

use serde::Serialize;

use crate::router::VerificationStatus;

#[derive(Debug, Serialize)]
pub struct ConformanceResult {
    pub verification: VerificationStatus,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
}

/// Runs one harness command and returns structured evidence. A missing binary,
/// authentication/quota response, temporary service issue, or timeout is
/// `BLOCKED_BY_ENVIRONMENT`, never `UNSUPPORTED`.
pub fn run_harness(
    command: &mut Command,
    proof_token: &str,
    timeout: Duration,
) -> ConformanceResult {
    let child = match command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => return launch_failure(error),
    };
    let mut child = child;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
            Ok(None) => {
                let _ = child.kill();
                return match child.wait_with_output() {
                    Ok(output) => classify_output(output, proof_token, Some("process timed out")),
                    Err(error) => launch_failure(error),
                };
            }
            Err(error) => return launch_failure(error),
        }
    }
    match child.wait_with_output() {
        Ok(output) => classify_output(output, proof_token, None),
        Err(error) => launch_failure(error),
    }
}

fn launch_failure(error: io::Error) -> ConformanceResult {
    let reason = match error.kind() {
        io::ErrorKind::NotFound => "harness executable unavailable".to_owned(),
        _ => format!("could not launch harness: {error}"),
    };
    ConformanceResult {
        verification: VerificationStatus::BlockedByEnvironment { reason },
        stdout: String::new(),
        stderr: error.to_string(),
        exit_code: None,
    }
}

fn classify_output(output: Output, proof_token: &str, timeout: Option<&str>) -> ConformanceResult {
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let combined = format!("{stdout}\n{stderr}").to_ascii_lowercase();
    let verification = if let Some(reason) = timeout {
        VerificationStatus::BlockedByEnvironment {
            reason: reason.to_owned(),
        }
    } else if is_environment_block(&combined) {
        VerificationStatus::BlockedByEnvironment {
            reason: "harness reported an authentication, quota, rate-limit, or temporary service condition".to_owned(),
        }
    } else if !output.status.success() {
        VerificationStatus::Failed {
            reason: format!("harness exited with {}", output.status),
        }
    } else if stdout.contains(proof_token) || stderr.contains(proof_token) {
        VerificationStatus::Verified
    } else {
        VerificationStatus::NotExposed
    };
    ConformanceResult {
        verification,
        stdout,
        stderr,
        exit_code: output.status.code(),
    }
}

fn is_environment_block(output: &str) -> bool {
    [
        "429",
        "rate limit",
        "quota",
        "usage limit",
        "authentication",
        "unauthorized",
        "api_error",
        "temporarily unavailable",
    ]
    .iter()
    .any(|needle| output.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_executable_is_blocked_not_unsupported() {
        let mut command = Command::new("uze-definitely-no-such-harness");
        let result = run_harness(&mut command, "proof", Duration::from_secs(1));
        assert!(matches!(
            result.verification,
            VerificationStatus::BlockedByEnvironment { .. }
        ));
    }

    #[test]
    fn proof_token_is_behavioral_verification() {
        let mut command = Command::new("sh");
        command.args(["-c", "printf proof-token"]);
        let result = run_harness(&mut command, "proof-token", Duration::from_secs(1));
        assert_eq!(result.verification, VerificationStatus::Verified);
    }

    #[test]
    fn quota_response_is_blocked_by_environment() {
        let mut command = Command::new("sh");
        command.args(["-c", "printf 'HTTP 429 rate limit' >&2; exit 1"]);
        let result = run_harness(&mut command, "proof", Duration::from_secs(1));
        assert!(matches!(
            result.verification,
            VerificationStatus::BlockedByEnvironment { .. }
        ));
    }
}
