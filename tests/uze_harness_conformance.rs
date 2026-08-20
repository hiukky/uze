//! Opt-in end-to-end conformance for the UZE architecture:
//!
//! Agent Plugin fixture → UZE Store → UZE Engine → EffectiveEnvironment
//! → Integration → real harness.
//!
//! Unlike native_harness_conformance.rs, the package fixture and the caller
//! workspace contain no `.agents`, `.claude`, `.codex`, or manual equivalent.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

use uze::{ExposureMechanism, UzeEngine, UzeHome, UzeStore, integration::IntegrationPort};

#[path = "../src/integrations/claude.rs"]
mod claude;
#[path = "../src/integrations/codex.rs"]
mod codex;
#[path = "../src/integrations/opencode.rs"]
mod opencode;

use claude::ClaudeIntegration;
use codex::CodexIntegration;
use opencode::OpenCodeIntegration;

const PROOF: &str = "UZE_E2E_SKILL_PROOF_20260820";

fn package_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("playground/agent-plugin-package")
}

fn enabled(harness: &str) -> bool {
    env::var("UZE_E2E_UZE_HARNESSES")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .any(|configured| configured == harness)
}

fn temporary_root(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("uze-{label}-{}-{nonce}", std::process::id()))
}

fn assert_clean_workspace(workspace: &Path) {
    for path in [".agents", ".claude", ".codex", ".opencode"] {
        assert!(
            !workspace.join(path).exists(),
            "caller workspace unexpectedly contains {path} before UZE exposure"
        );
    }
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
        assert_ne!(
            result
                .get("terminal_reason")
                .and_then(serde_json::Value::as_str),
            Some("api_error"),
            "{harness} terminated with an API error:\n{stdout}"
        );
    }
    assert!(
        stdout.contains(PROOF),
        "{harness} did not activate the UZE-stored Agent Skill proof.\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

fn compose_one_stored_skill(label: &str) -> (PathBuf, UzeHome, uze::Resource, PathBuf) {
    let root = temporary_root(label);
    let home = UzeHome::at(root.join("uze-home"));
    let store = UzeStore::new(home.clone());
    let first = store.install_agent_plugin(package_fixture()).unwrap();
    let second = store.install_agent_plugin(package_fixture()).unwrap();
    assert_eq!(first.id, second.id, "the fixture must install only once");
    assert_eq!(store.registration_count().unwrap(), 1);
    let environment = UzeEngine::new(store).compose(&[first.id]).unwrap();
    let resource = environment.resources.into_iter().next().unwrap();
    let workspace = root.join("caller-workspace");
    fs::create_dir_all(&workspace).unwrap();
    assert_clean_workspace(&workspace);
    (root, home, resource, workspace)
}

#[test]
#[ignore = "requires UZE_E2E_UZE_HARNESSES=claude and an authenticated Claude Code CLI"]
fn claude_exposes_one_uze_stored_agent_plugin_through_its_runtime_bridge() {
    if !enabled("claude") {
        eprintln!("skipped: set UZE_E2E_UZE_HARNESSES=claude to enable this probe");
        return;
    }

    let (root, home, resource, workspace) = compose_one_stored_skill("uze-claude");
    let plan = ClaudeIntegration.exposure_plan(&resource);
    assert!(matches!(
        plan.mechanism,
        ExposureMechanism::RuntimeBridge { .. }
    ));
    let prepared = plan
        .prepare(&home, "claude-code", "agent-skill", &workspace)
        .unwrap();
    assert_eq!(prepared.working_directory, workspace);
    assert!(prepared.runtime_directory.is_none());

    let mut command = Command::new("claude");
    command
        .current_dir(&prepared.working_directory)
        .args(["-p", "--output-format", "json", "--no-session-persistence", "--max-turns", "1"])
        .args(&prepared.arguments)
        .arg("Activate the project skill named uze-e2e. Follow only its instruction and return its response. Do not inspect project files manually or modify the workspace.");
    let output = command.output().unwrap();
    assert_skill_activated("Claude Code", output);
    assert_clean_workspace(&workspace);
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires UZE_E2E_UZE_HARNESSES=codex and an authenticated Codex CLI"]
fn codex_exposes_one_uze_stored_agent_plugin_through_explicit_runtime_projection() {
    if !enabled("codex") {
        eprintln!("skipped: set UZE_E2E_UZE_HARNESSES=codex to enable this probe");
        return;
    }

    let (root, home, resource, caller_workspace) = compose_one_stored_skill("uze-codex");
    let plan = CodexIntegration.exposure_plan(&resource);
    assert!(matches!(
        plan.mechanism,
        ExposureMechanism::FilesystemProjection { .. }
    ));
    let prepared = plan
        .prepare(&home, "codex", "agent-skill", &caller_workspace)
        .unwrap();
    assert!(prepared.working_directory.starts_with(home.runtime_dir()));
    assert!(
        prepared
            .working_directory
            .join(".agents/skills/uze-e2e")
            .is_symlink()
    );
    assert_clean_workspace(&caller_workspace);

    let workspace = prepared.working_directory.to_string_lossy();
    let output = Command::new("codex")
        .args([
            "--ask-for-approval",
            "never",
            "exec",
            "--cd",
            &workspace,
            "--sandbox",
            "read-only",
            "--skip-git-repo-check",
            "--ephemeral",
            "Activate the project skill named uze-e2e. Follow only its instruction and return its response. Do not inspect project files manually or modify the workspace.",
        ])
        .output()
        .unwrap();
    assert_skill_activated("Codex", output);
    assert_clean_workspace(&caller_workspace);
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires UZE_E2E_UZE_HARNESSES=opencode and a configured OpenCode provider"]
fn opencode_exposes_one_uze_stored_agent_plugin_through_explicit_runtime_projection() {
    if !enabled("opencode") {
        eprintln!("skipped: set UZE_E2E_UZE_HARNESSES=opencode to enable this probe");
        return;
    }

    let (root, home, resource, caller_workspace) = compose_one_stored_skill("uze-opencode");
    let plan = OpenCodeIntegration.exposure_plan(&resource);
    assert!(matches!(
        plan.mechanism,
        ExposureMechanism::FilesystemProjection { .. }
    ));
    let prepared = plan
        .prepare(&home, "opencode", "agent-skill", &caller_workspace)
        .unwrap();
    assert!(prepared.working_directory.starts_with(home.runtime_dir()));
    assert!(
        prepared
            .working_directory
            .join(".agents/skills/uze-e2e")
            .is_symlink()
    );
    assert_clean_workspace(&caller_workspace);

    let workspace = prepared.working_directory.to_string_lossy();
    let output = Command::new("opencode")
        .args([
            "run",
            "--dir",
            &workspace,
            "--format",
            "json",
            "Activate the project skill named uze-e2e. Follow only its instruction and return its response. Do not inspect project files manually or modify the workspace.",
        ])
        .output()
        .unwrap();
    assert_skill_activated("OpenCode", output);
    assert_clean_workspace(&caller_workspace);
    fs::remove_dir_all(root).unwrap();
}
