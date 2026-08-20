//! Opt-in UZE integration conformance:
//!
//! Agent Plugin fixture → one UZE Store installation → one EffectiveEnvironment
//! → peer IntegrationPort implementations → real harnesses.
//!
//! Native discovery is intentionally tested only in
//! `native_harness_conformance.rs`. This suite starts with a clean caller
//! workspace and reports external failures structurally instead of treating
//! them as unsupported capabilities.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use uze::{
    PackageId, Resource, UzeEngine, UzeHome, UzeStore,
    conformance::{ConformanceResult, run_harness},
    exposure::ExposureMechanism,
    integration::IntegrationPort,
};

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

struct SharedStoreFixture {
    root: PathBuf,
    home: UzeHome,
    package_id: PackageId,
    package_path: PathBuf,
    skill_path: PathBuf,
    resource: Resource,
    workspace: PathBuf,
}

impl Drop for SharedStoreFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn package_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/packages/agent-plugin-skill")
}

fn enabled(harness: &str) -> bool {
    env::var("UZE_E2E_UZE_HARNESSES")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .any(|configured| configured == harness)
}

fn harness_timeout() -> Duration {
    let seconds = env::var("UZE_E2E_TIMEOUT_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(90);
    Duration::from_secs(seconds)
}

/// Cheap, overridable model selections for opt-in real-harness probes. They
/// are intentionally irrelevant to deterministic `cargo test` execution.
fn harness_model(variable: &str, default: &str) -> String {
    env::var(variable).unwrap_or_else(|_| default.to_owned())
}

fn temporary_root(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is available")
        .as_nanos();
    std::env::temp_dir().join(format!("uze-{label}-{}-{nonce}", std::process::id()))
}

fn assert_clean_workspace(workspace: &Path) {
    for path in [
        ".agents",
        ".claude",
        ".codex",
        ".cursor",
        ".windsurf",
        ".opencode",
    ] {
        assert!(
            !workspace.join(path).exists(),
            "caller workspace unexpectedly contains {path} before UZE exposure"
        );
    }
}

fn shared_store_fixture(label: &str) -> SharedStoreFixture {
    let root = temporary_root(label);
    let home = UzeHome::at(root.join("uze-home"));
    let store = UzeStore::new(home.clone());
    let installed = store
        .install_agent_plugin(package_fixture())
        .expect("fixture is a valid Agent Plugin 1.0 package");
    assert_eq!(store.registration_count().expect("registry is readable"), 1);

    let workspace = root.join("caller-workspace");
    fs::create_dir_all(&workspace).expect("caller workspace is created");
    let environment = UzeEngine::new(store)
        .compose_project(&workspace)
        .expect("empty caller project composes with the installed package");
    let resource = environment
        .resources
        .into_iter()
        .find(|resource| resource.package_root().is_some())
        .expect("fixture contributes one store-owned skill");
    assert_clean_workspace(&workspace);

    SharedStoreFixture {
        package_id: installed.id,
        package_path: installed.root,
        skill_path: resource.capability.path.clone(),
        root,
        home,
        resource,
        workspace,
    }
}

fn emit_evidence(label: &str, fixture: &SharedStoreFixture, result: &ConformanceResult) {
    eprintln!(
        "UZE {label} conformance evidence:\n{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "uze_home": fixture.home.root(),
            "package_id": fixture.package_id.as_str(),
            "stored_package_path": fixture.package_path,
            "resource_identity": fixture.resource.identity(),
            "stored_skill_path": fixture.skill_path,
            "verification": result.verification,
            "exit_code": result.exit_code,
            "stdout": result.stdout,
            "stderr": result.stderr,
        }))
        .expect("evidence serialization is infallible")
    );
}

#[test]
fn same_store_environment_is_planned_for_claude_and_codex_as_peers() {
    let fixture = shared_store_fixture("same-store-contract");
    let claude = ClaudeIntegration.exposure_plan(&fixture.resource);
    let codex = CodexIntegration.exposure_plan(&fixture.resource);

    assert!(matches!(
        claude.mechanism,
        ExposureMechanism::RuntimeBridge { .. }
    ));
    assert!(matches!(
        codex.mechanism,
        ExposureMechanism::FilesystemProjection { .. }
    ));
    assert!(fixture.skill_path.starts_with(&fixture.package_path));
    assert_eq!(
        fixture.resource.identity(),
        format!(
            "package:{}:skills/uze-e2e/SKILL.md",
            fixture.package_id.as_str()
        )
    );
}

#[test]
fn projection_keeps_real_project_cwd_and_cleans_its_managed_artifact() {
    let fixture = shared_store_fixture("projection-lifecycle");
    let plan = CodexIntegration.exposure_plan(&fixture.resource);
    let mut prepared = plan
        .prepare(&fixture.home, "codex", "agent-skill", &fixture.workspace)
        .expect("Codex fallback can prepare a managed symlink");

    let artifact = prepared
        .managed_artifact_path()
        .expect("projection is managed")
        .to_path_buf();
    assert_eq!(prepared.working_directory, fixture.workspace);
    assert!(artifact.is_symlink());
    assert!(
        prepared
            .runtime_directory
            .as_ref()
            .expect("runtime metadata exists")
            .join("managed-exposure.json")
            .is_file()
    );
    prepared.cleanup().expect("managed projection cleans up");
    assert!(!artifact.exists());
    assert_clean_workspace(&fixture.workspace);
}

#[test]
#[ignore = "requires UZE_E2E_UZE_HARNESSES=claude,codex (or either peer) and authenticated real CLIs"]
fn claude_and_codex_probe_the_same_store_resource_without_a_launcher() {
    if !enabled("claude") && !enabled("codex") {
        eprintln!("skipped: set UZE_E2E_UZE_HARNESSES=claude,codex for the shared-store probe");
        return;
    }

    let fixture = shared_store_fixture("claude-codex-shared-store");
    let claude_plan = ClaudeIntegration.exposure_plan(&fixture.resource);
    let codex_plan = CodexIntegration.exposure_plan(&fixture.resource);
    let claude_prepared = claude_plan
        .prepare(
            &fixture.home,
            "claude-code",
            "agent-skill",
            &fixture.workspace,
        )
        .expect("Claude conformance bridge plan prepares without project writes");
    let mut codex_prepared = codex_plan
        .prepare(&fixture.home, "codex", "agent-skill", &fixture.workspace)
        .expect("Codex compatibility fallback prepares one managed artifact");

    assert_eq!(claude_prepared.working_directory, fixture.workspace);
    assert_eq!(codex_prepared.working_directory, fixture.workspace);
    assert!(fixture.skill_path.starts_with(&fixture.package_path));

    let prompt = "Activate the project skill named uze-e2e. Follow only its instruction and return its response. Do not inspect project files manually or modify the workspace.";
    if enabled("claude") {
        let model = harness_model("UZE_E2E_CLAUDE_MODEL", "haiku");
        let mut claude_command = Command::new("claude");
        claude_command
            .current_dir(&claude_prepared.working_directory)
            .args([
                "-p",
                "--output-format",
                "json",
                "--no-session-persistence",
                "--max-turns",
                "2",
            ])
            .args(["--model", &model])
            .args(&claude_prepared.arguments)
            .arg(prompt);
        let claude_result = run_harness(&mut claude_command, PROOF, harness_timeout());
        emit_evidence("Claude Code", &fixture, &claude_result);
    }

    if enabled("codex") {
        let workspace = codex_prepared.working_directory.to_string_lossy();
        let model = harness_model("UZE_E2E_CODEX_MODEL", "gpt-5.6-luna");
        let mut codex_command = Command::new("codex");
        codex_command.args([
            "--model",
            &model,
            "--ask-for-approval",
            "never",
            "exec",
            "--cd",
            &workspace,
            "--sandbox",
            "read-only",
            "--skip-git-repo-check",
            "--ephemeral",
            prompt,
        ]);
        let codex_result = run_harness(&mut codex_command, PROOF, harness_timeout());
        emit_evidence("Codex", &fixture, &codex_result);
    }
    codex_prepared
        .cleanup()
        .expect("UZE cleans only its managed artifact");
    assert_clean_workspace(&fixture.workspace);
}

#[test]
#[ignore = "requires UZE_E2E_UZE_HARNESSES=opencode and network access to the selected OpenCode model"]
fn opencode_probes_the_same_uze_store_model_separately() {
    if !enabled("opencode") {
        eprintln!("skipped: set UZE_E2E_UZE_HARNESSES=opencode to enable this probe");
        return;
    }
    let fixture = shared_store_fixture("opencode-store");
    let plan = OpenCodeIntegration.exposure_plan(&fixture.resource);
    let mut prepared = plan
        .prepare(&fixture.home, "opencode", "agent-skill", &fixture.workspace)
        .expect("OpenCode fallback can prepare one managed artifact");
    let workspace = prepared.working_directory.to_string_lossy();
    let model = harness_model("UZE_E2E_OPENCODE_MODEL", "opencode/deepseek-v4-flash-free");
    let mut command = Command::new("opencode");
    command.args([
        "run",
        "--dir",
        &workspace,
        "--format",
        "json",
        "--model",
        &model,
        "Activate the project skill named uze-e2e. Follow only its instruction and return its response. Do not inspect project files manually or modify the workspace.",
    ]);
    let result = run_harness(&mut command, PROOF, harness_timeout());
    emit_evidence("OpenCode", &fixture, &result);
    prepared
        .cleanup()
        .expect("UZE cleans only its managed artifact");
    assert_clean_workspace(&fixture.workspace);
}
