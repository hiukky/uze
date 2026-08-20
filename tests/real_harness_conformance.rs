//! Opt-in conformance probes for installed, authenticated harnesses.
//!
//! Run one harness explicitly, never as part of the default test suite:
//!
//! ```text
//! UZE_E2E_REAL_HARNESSES=claude cargo test --test real_harness_conformance -- --ignored --nocapture
//! UZE_E2E_REAL_HARNESSES=codex cargo test --test real_harness_conformance -- --ignored --nocapture
//! UZE_E2E_REAL_HARNESSES=opencode cargo test --test real_harness_conformance -- --ignored --nocapture
//! ```
//!
//! The playground contains only the standard `.agents/skills` representation.
//! A passing probe proves that the harness activated the same skill without a
//! UZE conversion or vendor-directory projection. A failure is evidence that
//! the integration must remain `UNVERIFIED` or become `NOT_EXPOSED`.

use std::{
    env, fs,
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

fn playground_snapshot() -> Vec<(PathBuf, Vec<u8>)> {
    fn visit(
        root: &std::path::Path,
        path: &std::path::Path,
        entries: &mut Vec<(PathBuf, Vec<u8>)>,
    ) {
        let metadata = fs::symlink_metadata(path).unwrap();
        let relative = path.strip_prefix(root).unwrap().to_path_buf();
        if metadata.file_type().is_symlink() {
            entries.push((
                relative,
                fs::read_link(path)
                    .unwrap()
                    .into_os_string()
                    .into_encoded_bytes(),
            ));
        } else if metadata.is_dir() {
            let mut children = fs::read_dir(path)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect::<Vec<_>>();
            children.sort();
            for child in children {
                visit(root, &child, entries);
            }
        } else {
            entries.push((relative, fs::read(path).unwrap()));
        }
    }

    let root = fixture_root();
    let mut entries = Vec::new();
    visit(&root, &root, &mut entries);
    entries
}

fn assert_skill_activated(harness: &str, output: Output) {
    assert_completed_without_api_error(harness, &output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stdout.contains(PROOF),
        "{harness} did not expose the standard Agent Skill proof.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

fn assert_completed_without_api_error(harness: &str, output: &Output) {
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
        assert_ne!(
            result
                .get("terminal_reason")
                .and_then(serde_json::Value::as_str),
            Some("api_error"),
            "{harness} terminated with an API error:\n{stdout}"
        );
        assert!(
            result
                .get("api_error_status")
                .is_none_or(serde_json::Value::is_null),
            "{harness} reported an API status error:\n{stdout}"
        );
    }
}

fn assert_skill_not_exposed(harness: &str, output: Output) {
    assert_completed_without_api_error(harness, &output);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
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

    let before = playground_snapshot();
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
    assert_eq!(
        playground_snapshot(),
        before,
        "Claude Code mutated the playground"
    );
}

#[test]
#[ignore = "requires UZE_E2E_REAL_HARNESSES=codex and an authenticated Codex CLI"]
fn codex_activates_the_standard_skill_without_projection() {
    if !enabled("codex") {
        eprintln!("skipped: set UZE_E2E_REAL_HARNESSES=codex to enable this probe");
        return;
    }

    let before = playground_snapshot();
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
    assert_eq!(
        playground_snapshot(),
        before,
        "Codex mutated the playground"
    );
}

#[test]
#[ignore = "requires UZE_E2E_REAL_HARNESSES=opencode and a configured OpenCode provider"]
fn opencode_activates_the_standard_skill_without_projection() {
    if !enabled("opencode") {
        eprintln!("skipped: set UZE_E2E_REAL_HARNESSES=opencode to enable this probe");
        return;
    }

    let before = playground_snapshot();
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
    assert_eq!(
        playground_snapshot(),
        before,
        "OpenCode mutated the playground"
    );
}
