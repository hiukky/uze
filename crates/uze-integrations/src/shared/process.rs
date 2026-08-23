//! Running vendor CLIs without letting their progress noise reach the
//! terminal.
//!
//! Every mutating vendor command (`plugin marketplace add`, `plugin add`,
//! `plugin remove`, `extensions link`, `mcp add`, ...) used to run with
//! inherited stdio, so Codex/Claude/Gemini progress banners, spinners,
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

use uze_core::{Result, UzeError};

/// Runs `program` with `HOME=home` and the given `args`, stdio captured and
/// stdin null — the opposite of the old inherited-stdio calls whose spinner
/// output interleaved with UZE's own terminal surface.
pub fn capture<S: AsRef<OsStr>>(program: &Path, home: &Path, args: &[S]) -> io::Result<Output> {
    Command::new(program)
        .env("HOME", home)
        .args(args)
        .stdin(Stdio::null())
        .output()
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
    fn failure_message_includes_the_vendor_last_words() {
        let output = Output {
            status: std::process::ExitStatus::from_raw(1),
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
            status: std::process::ExitStatus::from_raw(1),
            stdout: vec![],
            stderr: vec![],
        };
        assert!(failed_message("codex plugin add", &output).contains("exited with"));
    }

    #[test]
    fn an_overlong_tail_is_capped() {
        let output = Output {
            status: std::process::ExitStatus::from_raw(1),
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
}
