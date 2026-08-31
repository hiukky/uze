//! Running vendor CLIs without letting their progress noise reach the
//! terminal.
//!
//! Every mutating vendor command (`plugin marketplace add`, `plugin add`,
//! `plugin remove`, `install`, `mcp add`, ...) used to run with
//! inherited stdio, so Codex/Claude progress banners, spinners,
//! consent narratives and warnings were written straight over UZE's own
//! output — and over the TUI's alternate screen, corrupting its layout.
//!
//! All mutating commands now run with captured stdio and **null stdin**: an
//! accidentally interactive vendor prompt fails fast instead of hanging a
//! captured pipeline. The vendor's own words are surfaced only when the
//! command fails, as the tail of `failed_message`'s error.

use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use uze_core::{
    Result, UzeError,
    subprocess::{kill_process_group, read_bounded, wait_with_timeout, with_process_group},
};

/// Wall-clock budget for any single vendor CLI invocation. A vendor binary
/// that hangs (interactive prompt, stalled network install) must fail rather
/// than hang `uze add`/`remove`/`setup` forever.
pub const VENDOR_CLI_TIMEOUT: Duration = Duration::from_secs(120);

/// Per-stream cap for captured vendor output. A chatty vendor must not be
/// able to exhaust memory; inspection only ever reads the tail anyway.
const VENDOR_OUTPUT_CAP: usize = 256 * 1024;

/// A value is safe to pass as a bare positional argument to a vendor CLI
/// only when it cannot be mistaken for a flag. Package-controlled strings
/// (an MCP server's name in `mcp.json`, a Skill/Command's logical name) flow
/// into vendor invocations like `claude mcp add --scope user --transport
/// stdio <entry_name> -- <command>` with no `--` separator available before
/// `entry_name` — a name starting with `-` would be parsed as a flag by the
/// vendor's own argument parser instead of as the positional value. This is
/// the one guard every such value must pass before it is ever used as one.
pub(crate) fn is_cli_safe_token(value: &str) -> bool {
    !value.is_empty() && !value.starts_with('-')
}

/// Runs `program` with `HOME=home` and the given `args`, stdio captured and
/// stdin null — the opposite of the old inherited-stdio calls whose spinner
/// output interleaved with UZE's own terminal surface. Bounded by
/// `VENDOR_CLI_TIMEOUT` and a per-stream output cap so a hung or chatty
/// vendor cannot hang UZE or exhaust memory.
pub fn capture<S: AsRef<OsStr>>(program: &Path, home: &Path, args: &[S]) -> io::Result<Output> {
    capture_with_timeout(program, home, args, VENDOR_CLI_TIMEOUT)
}

fn capture_with_timeout<S: AsRef<OsStr>>(
    program: &Path,
    home: &Path,
    args: &[S],
    timeout: Duration,
) -> io::Result<Output> {
    let mut command = Command::new(program);
    command
        .env("HOME", home)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = with_process_group(command).spawn()?;
    let Some(mut stdout) = child.stdout.take() else {
        return Err(io::Error::other("captured stdout was not piped"));
    };
    let Some(mut stderr) = child.stderr.take() else {
        return Err(io::Error::other("captured stderr was not piped"));
    };
    let deadline = Instant::now() + timeout;
    let timeout_error = || {
        io::Error::new(
            io::ErrorKind::TimedOut,
            format!(
                "`{}` did not finish within {}s",
                program.display(),
                timeout.as_secs()
            ),
        )
    };
    // Readers report through a channel rather than a bare `JoinHandle`: a
    // descendant the vendor forked can outlive the direct child and keep a
    // pipe open, so waiting on the readers must be bounded by the same
    // deadline the process wait used below — a plain `.join()` would let
    // that case hang past the documented timeout.
    let (stdout_tx, stdout_rx) = mpsc::channel();
    let (stderr_tx, stderr_rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = stdout_tx.send(read_bounded(&mut stdout, VENDOR_OUTPUT_CAP));
    });
    thread::spawn(move || {
        let _ = stderr_tx.send(read_bounded(&mut stderr, VENDOR_OUTPUT_CAP));
    });
    let (status, timed_out) = wait_with_timeout(&mut child, timeout)?;
    if timed_out {
        // The group was already killed; the reader threads exit once the
        // now-closed pipes hit EOF and are never waited on here.
        return Err(timeout_error());
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    let stdout_bytes = match stdout_rx.recv_timeout(remaining) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            // The direct child exited, but a descendant it forked is still
            // holding the stdout pipe open. Sweep the group again (already
            // killed once inside `wait_with_timeout` if it timed out, but
            // that branch was not taken here) before giving up.
            kill_process_group(child.id());
            return Err(timeout_error());
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            return Err(io::Error::other("stdout reader panicked"));
        }
    };
    let stderr_bytes = match stderr_rx.recv_timeout(remaining) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            kill_process_group(child.id());
            return Err(timeout_error());
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            return Err(io::Error::other("stderr reader panicked"));
        }
    };
    Ok(Output {
        status,
        stdout: mark_if_truncated(stdout_bytes),
        stderr: mark_if_truncated(stderr_bytes),
    })
}

/// Appends a truncation notice when `read_bounded` hit `VENDOR_OUTPUT_CAP`,
/// so a cap hit is visible to whatever later stringifies the output (error
/// messages, `--version` parsing) instead of silently presenting truncated
/// output as complete.
fn mark_if_truncated((mut bytes, overflow): (Vec<u8>, bool)) -> Vec<u8> {
    if overflow {
        bytes.extend_from_slice(
            format!("\n...[output truncated at {VENDOR_OUTPUT_CAP} bytes]").as_bytes(),
        );
    }
    bytes
}

/// Runs a mutating vendor command quietly: captured output is discarded on
/// success; on failure the error carries the vendor's own last words, so
/// the cause stays actionable without the noise.
pub fn run_quiet<S: AsRef<OsStr>>(
    program: &Path,
    home: &Path,
    label: &str,
    args: &[S],
) -> Result<()> {
    let output = capture(program, home, args).map_err(|error| {
        UzeError::ExposureUnavailable(format!("failed to run `{label}`: {error}"))
    })?;
    if output.status.success() {
        return Ok(());
    }
    Err(UzeError::ExposureUnavailable(failed_message(
        label, &output,
    )))
}

/// Formats a vendor failure: `label` plus `ExitStatus`, and — when the
/// vendor said anything — its own last words, capped so an unbounded error
/// output still fits one diagnostic line.
pub fn failed_message(label: &str, output: &Output) -> String {
    let message = format!("`{label}` exited with {}", output.status);
    match output_tail(output) {
        Some(tail) => format!("{message}: {tail}"),
        None => message,
    }
}

fn output_tail(output: &Output) -> Option<String> {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let text = if stderr.trim().is_empty() {
        String::from_utf8_lossy(&output.stdout)
    } else {
        stderr
    };
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if lines.is_empty() {
        return None;
    }
    let joined = lines
        .iter()
        .rev()
        .take(2)
        .rev()
        .copied()
        .collect::<Vec<&str>>()
        .join(" | ");
    let mut chars = joined.chars();
    let mut capped: String = chars.by_ref().take(240).collect();
    if chars.next().is_some() {
        capped.push('…');
    }
    Some(capped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_leading_dash_is_never_a_safe_cli_token() {
        assert!(!is_cli_safe_token("-h"));
        assert!(!is_cli_safe_token("--scope"));
        assert!(!is_cli_safe_token("--transport"));
        assert!(!is_cli_safe_token(""));
    }

    #[test]
    fn an_ordinary_name_is_a_safe_cli_token() {
        assert!(is_cli_safe_token("github"));
        assert!(is_cli_safe_token("flow:review"));
        assert!(is_cli_safe_token("my-server_1"));
    }

    /// A non-success exit status without touching vendor installs — the
    /// platform's own `false` command is the canonical source.
    fn failing_status() -> std::process::ExitStatus {
        std::process::Command::new("false")
            .status()
            .expect("`false` must exist on every supported platform")
    }

    #[test]
    fn failure_message_includes_the_vendor_last_words() {
        let output = Output {
            status: failing_status(),
            stdout: b"".to_vec(),
            stderr: b"first line\nsecond line\nthird line\n".to_vec(),
        };
        let message = failed_message("codex plugin add `flow@ai`", &output);
        assert!(message.contains("exited with"), "got: {message}");
        assert!(
            message.contains("second line") && message.contains("third line"),
            "tail should carry the vendor's own words, got: {message}"
        );
    }

    #[test]
    fn a_silent_failure_still_names_the_status() {
        let output = Output {
            status: failing_status(),
            stdout: vec![],
            stderr: vec![],
        };
        assert!(failed_message("codex plugin add", &output).contains("exited with"));
    }

    #[test]
    fn an_overlong_tail_is_capped() {
        let output = Output {
            status: failing_status(),
            stdout: vec![],
            stderr: vec![b'x'; 5000],
        };
        let message = failed_message("codex plugin add", &output);
        assert!(
            message.len() < 400,
            "tail must be capped, got {} chars",
            message.len()
        );
    }

    #[test]
    fn capture_times_out_on_a_hung_vendor() {
        let started = std::time::Instant::now();
        let error = capture_with_timeout(
            Path::new("/bin/sleep"),
            Path::new("/tmp"),
            &["30"],
            Duration::from_millis(500),
        )
        .expect_err("a hung vendor must fail, not hang the caller");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "timeout must actually bound the wait, not outlive the child"
        );
    }

    #[cfg(unix)]
    #[test]
    fn capture_collects_output_and_status_of_a_successful_vendor() {
        let output = capture_with_timeout(
            Path::new("/bin/printf"),
            Path::new("/tmp"),
            &["hello"],
            Duration::from_secs(10),
        )
        .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"hello");
    }

    #[cfg(unix)]
    #[test]
    fn capture_collects_stderr_of_a_failing_vendor() {
        let output = capture_with_timeout(
            Path::new("/bin/sh"),
            Path::new("/tmp"),
            &["-c", "echo bad >&2; exit 3"],
            Duration::from_secs(10),
        )
        .unwrap();
        assert_eq!(output.status.code(), Some(3));
        assert_eq!(output.stderr, b"bad\n");
    }

    #[cfg(unix)]
    #[test]
    fn capture_bounds_the_wait_even_when_a_backgrounded_descendant_holds_the_pipe_open() {
        let started = std::time::Instant::now();
        // The direct shell exits immediately, but the backgrounded `sleep`
        // it forked inherits (and keeps open) the stdout pipe. A bare
        // `.join()` on the reader thread would hang for the sleep's whole
        // 30s lifetime even though the direct child already exited well
        // inside the 500ms budget.
        let error = capture_with_timeout(
            Path::new("/bin/sh"),
            Path::new("/tmp"),
            &["-c", "echo done; (sleep 30 &) ; exit 0"],
            Duration::from_millis(500),
        )
        .expect_err("a descendant holding the pipe open must not let this outlive the timeout");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the wait for the readers must be bounded by the same deadline as the process wait"
        );
    }

    #[cfg(unix)]
    #[test]
    fn capture_appends_a_truncation_notice_when_the_output_cap_is_hit() {
        let output = capture_with_timeout(
            Path::new("/bin/sh"),
            Path::new("/tmp"),
            &["-c", "yes x | head -c 300000"],
            Duration::from_secs(10),
        )
        .unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        let marker = format!("[output truncated at {VENDOR_OUTPUT_CAP} bytes]");
        assert!(
            text.ends_with(&marker),
            "truncated output must say so instead of silently dropping bytes, got tail: {:?}",
            &text[text.len().saturating_sub(80)..]
        );
    }
}
