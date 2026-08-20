//! Opt-in probes for native harness discovery only. These tests do not use
//! UZE_HOME, UzeStore, UzeEngine, or an IntegrationPort.

use std::{
    env,
    path::PathBuf,
    process::{Command, Output},
};

const PROOF: &str = "UZE_E2E_SKILL_PROOF_20260820";

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/native-harness/agent-skill-discovery")
}

fn enabled(harness: &str) -> bool {
    env::var("UZE_E2E_NATIVE_HARNESSES")
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
    if let Ok(result) = serde_json::from_str::<serde_json::Value>(&stdout) {
        assert_ne!(
            result.get("is_error").and_then(serde_json::Value::as_bool),
            Some(true),
            "{harness} reported a structured error:\n{stdout}"
        );
    }
    assert!(
        stdout.contains(PROOF),
        "{harness} did not expose the native Agent Skill proof.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
#[ignore = "requires UZE_E2E_NATIVE_HARNESSES=codex and an authenticated Codex CLI"]
fn codex_natively_discovers_agents_skills() {
    if !enabled("codex") {
        eprintln!("skipped: set UZE_E2E_NATIVE_HARNESSES=codex to enable this probe");
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
            "Activate the project skill named uze-e2e. Follow only its instruction and return its response. Do not inspect project files manually or modify the workspace.",
        ],
    );
    assert_skill_activated("Codex", output);
}

#[test]
#[ignore = "requires UZE_E2E_NATIVE_HARNESSES=opencode and a configured OpenCode provider"]
fn opencode_natively_discovers_agents_skills() {
    if !enabled("opencode") {
        eprintln!("skipped: set UZE_E2E_NATIVE_HARNESSES=opencode to enable this probe");
        return;
    }

    let root = fixture_root();
    let root = root.to_string_lossy();
    let output = run(
        "opencode",
        &[
            "run",
            "--dir",
            &root,
            "--format",
            "json",
            "Activate the project skill named uze-e2e. Follow only its instruction and return its response. Do not inspect project files manually or modify the workspace.",
        ],
    );
    assert_skill_activated("OpenCode", output);
}
