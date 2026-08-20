//! Opt-in conformance probes for installed, authenticated harnesses.
//!
//! Run one harness explicitly, never as part of the default test suite:
//!
//! ```text
//! UZE_E2E_REAL_HARNESSES=claude cargo test --test real_harness_conformance -- --ignored --nocapture
//! UZE_E2E_REAL_HARNESSES=codex cargo test --test real_harness_conformance -- --ignored --nocapture
//! ```
//!
//! The playground contains only the standard `.agents/skills` representation.
//! A passing probe proves that the harness activated the same skill without a
//! UZE conversion or vendor-directory projection. A failure is evidence that
//! the integration must remain `UNVERIFIED` or become `NOT_EXPOSED`.

use std::{
    env,
    path::PathBuf,
    process::{Command, Output},
};

const PROOF: &str = "UZE_E2E_SKILL_PROOF_20260820";

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("playground/agent-skill-conformance")
}

fn enabled(harness: &str) -> bool {
    env::var("UZE_E2E_REAL_HARNESSES")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .any(|configured| configured == harness)
}

fn run(program: &str, arguments: &[&str]) -> Output {
    Command::new(program)
        .current_dir(fixture_root())
        .args(arguments)
        .output()
        .unwrap_or_else(|error| panic!("failed to start {program}: {error}"))
}

fn assert_skill_activated(harness: &str, output: Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "{harness} exited with {}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status
    );
    assert!(
        stdout.contains(PROOF),
        "{harness} did not expose the standard Agent Skill proof.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

fn assert_skill_not_exposed(harness: &str, output: Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "{harness} exited with {}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        output.status
    );
    assert!(
        !stdout.contains(PROOF),
        "{harness} unexpectedly exposed the standard Agent Skill; update its integration capability declaration.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
#[ignore = "requires UZE_E2E_REAL_HARNESSES=claude and an authenticated Claude Code CLI"]
fn claude_does_not_expose_the_standard_skill_without_projection() {
    if !enabled("claude") {
        eprintln!("skipped: set UZE_E2E_REAL_HARNESSES=claude to enable this probe");
        return;
    }

    let output = run(
        "claude",
        &[
            "-p",
            "--output-format",
            "json",
            "--no-session-persistence",
            "--max-turns",
            "1",
            "--tools=",
            "Activate the project skill named `uze-e2e`. Follow only its instruction and return its response. Do not inspect project files manually or use tools.",
        ],
    );
    assert_skill_not_exposed("Claude Code", output);
}

#[test]
#[ignore = "requires UZE_E2E_REAL_HARNESSES=codex and an authenticated Codex CLI"]
fn codex_activates_the_standard_skill_without_projection() {
    if !enabled("codex") {
        eprintln!("skipped: set UZE_E2E_REAL_HARNESSES=codex to enable this probe");
        return;
    }

    let root = fixture_root();
    let root = root.to_string_lossy();
    let output = run(
        "codex",
        &[
            "--ask-for-approval",
            "never",
            "exec",
            "--cd",
            &root,
            "--sandbox",
            "read-only",
            "--ephemeral",
            "Activate the project skill named `uze-e2e`. Follow only its instruction and return its response. Do not inspect project files manually or modify the workspace.",
        ],
    );
    assert_skill_activated("Codex", output);
}
