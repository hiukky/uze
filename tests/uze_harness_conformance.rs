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

/// Auditable evidence for a transparent-attachment runtime probe: enough to
/// answer "how was this result obtained?" without inventing a persistent
/// struct beyond what the opt-in test itself needs.
#[allow(clippy::too_many_arguments)]
fn emit_transparent_attachment_evidence(
    harness: &str,
    version: Option<&str>,
    setup_strategy: &str,
    // "documented" when the harness's own official docs state the mechanism
    // (e.g. Codex's symlink-following USER scope); "empirical" when it is
    // known only from this project's own controlled experiment against the
    // real CLI (e.g. Claude's skills-dir symlink behavior). See ADR-006.
    // Deliberately a plain string, not an enum, until a second consumer
    // needs one.
    confidence: &str,
    invocation: &[&str],
    cwd: &std::path::Path,
    fixture: &SharedStoreFixture,
    exposure_mechanism: &str,
    result: &ConformanceResult,
) {
    eprintln!(
        "UZE transparent-attachment evidence:\n{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "harness": harness,
            "detected_version": version,
            "setup_strategy": setup_strategy,
            "confidence": confidence,
            "invocation": invocation,
            "cwd": cwd,
            "uze_home": fixture.home.root(),
            "package_id": fixture.package_id.as_str(),
            "resource_id": fixture.resource.identity(),
            "exposure_mechanism": exposure_mechanism,
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
    let claude = ClaudeIntegration::new(fixture.root.join("claude-home"), fixture.home.clone())
        .exposure_plan(&fixture.resource);
    let codex = CodexIntegration::new(fixture.root.join("agents-home"), fixture.home.clone())
        .exposure_plan(&fixture.resource);

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
    let plan = CodexIntegration::new(fixture.root.join("agents-home"), fixture.home.clone())
        .exposure_plan(&fixture.resource);
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
    let claude_plan =
        ClaudeIntegration::new(fixture.root.join("claude-home"), fixture.home.clone())
            .exposure_plan(&fixture.resource);
    let codex_plan = CodexIntegration::new(fixture.root.join("agents-home"), fixture.home.clone())
        .exposure_plan(&fixture.resource);
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

/// Setup phase only: proves `uze setup` produces the expected on-disk state
/// and is idempotent. No real `claude`/`codex` process is spawned here — see
/// `runtime_phase_invokes_the_harness_plainly_after_setup_already_completed`
/// for the separate, structurally distinct proof that a plain invocation
/// then works. A pass here must never be read as runtime-verified.
#[test]
#[ignore = "requires UZE_E2E_UZE_HARNESSES=claude,codex and real CLIs in PATH; always targets an isolated HOME/UZE_HOME, never the operator's real configuration"]
fn setup_phase_is_idempotent_for_detected_harnesses() {
    if !enabled("claude") && !enabled("codex") {
        eprintln!("skipped: set UZE_E2E_UZE_HARNESSES=claude,codex for the setup-phase probe");
        return;
    }
    let fixture = shared_store_fixture("setup-phase");
    let claude_home = fixture.root.join("home/.claude");
    let agents_home = fixture.root.join("home/.agents");

    if enabled("claude") {
        let claude = ClaudeIntegration::new(claude_home.clone(), fixture.home.clone());
        if claude.detect().present {
            claude
                .install(&fixture.home)
                .expect("Claude setup succeeds");
            assert!(claude_home.join("skills").is_dir());
            assert!(uze::state::is_installed(&fixture.home, claude.id()));
            claude
                .install(&fixture.home)
                .expect("second Claude setup is idempotent");
        } else {
            eprintln!("skipped: no `claude` executable found in PATH");
        }
    }

    if enabled("codex") {
        let codex = CodexIntegration::new(agents_home.clone(), fixture.home.clone());
        if codex.detect().present {
            codex.install(&fixture.home).expect("Codex setup succeeds");
            assert!(agents_home.join("skills").is_dir());
            assert!(uze::state::is_installed(&fixture.home, codex.id()));
            codex
                .install(&fixture.home)
                .expect("second Codex setup is idempotent");
        } else {
            eprintln!("skipped: no `codex` executable found in PATH");
        }
    }
}

/// Runtime phase: after setup already completed (mirroring what `uze setup`
/// and `uze add` do, not a same-invocation `prepare()` hack), spawns the
/// real harness with its own plain command and zero UZE-specific arguments.
/// Structurally distinct from the setup-phase test above so a setup-only
/// pass can never be conflated with runtime verification.
#[test]
#[ignore = "requires UZE_E2E_UZE_HARNESSES=claude,codex, real CLIs, and authenticated real CLIs; always targets an isolated HOME/UZE_HOME"]
fn runtime_phase_invokes_the_harness_plainly_after_setup_already_completed() {
    if !enabled("claude") && !enabled("codex") {
        eprintln!("skipped: set UZE_E2E_UZE_HARNESSES=claude,codex for the runtime-phase probe");
        return;
    }
    let fixture = shared_store_fixture("runtime-phase");
    let isolated_home = fixture.root.join("home");
    let claude_home = isolated_home.join(".claude");
    let agents_home = isolated_home.join(".agents");
    let claude = ClaudeIntegration::new(claude_home, fixture.home.clone());
    let codex = CodexIntegration::new(agents_home, fixture.home.clone());

    // Setup phase (already proven separately above): what `uze setup` and
    // `uze add` already did before the user ever ran the harness.
    let claude_detection = claude.detect();
    let claude_ready = enabled("claude") && claude_detection.present;
    if claude_ready {
        claude
            .install(&fixture.home)
            .expect("Claude setup succeeds");
        claude
            .attach(&fixture.resource)
            .expect("Claude managed attachment is created");
    }
    let codex_detection = codex.detect();
    let codex_ready = enabled("codex") && codex_detection.present;
    if codex_ready {
        codex.install(&fixture.home).expect("Codex setup succeeds");
        codex
            .attach(&fixture.resource)
            .expect("Codex managed attachment is created");
    }

    // Runtime phase: plain harness invocation, zero UZE-specific arguments,
    // in the real fixture workspace. Only HOME is redirected, to the same
    // isolated home setup just used — never the operator's real one.
    let prompt = "Activate the project skill named uze-e2e. Follow only its instruction and return its response. Do not inspect project files manually or modify the workspace.";

    if claude_ready {
        let model = harness_model("UZE_E2E_CLAUDE_MODEL", "haiku");
        let claude_args = [
            "-p",
            "--output-format",
            "json",
            "--no-session-persistence",
            "--max-turns",
            "2",
            "--model",
            model.as_str(),
        ];
        let mut command = Command::new("claude");
        command
            .current_dir(&fixture.workspace)
            .env("HOME", &isolated_home)
            .args(claude_args)
            .arg(prompt);
        let result = run_harness(&mut command, PROOF, harness_timeout());
        emit_transparent_attachment_evidence(
            "claude-code",
            claude_detection.version.as_deref(),
            "managed-user-scope-skills-dir",
            "empirical",
            &claude_args,
            &fixture.workspace,
            &fixture,
            "MANAGED_USER_SCOPE_REFERENCE",
            &result,
        );
    }

    if codex_ready {
        let model = harness_model("UZE_E2E_CODEX_MODEL", "gpt-5.6-luna");
        let workspace = fixture.workspace.to_string_lossy();
        let codex_args = [
            "--model",
            model.as_str(),
            "--ask-for-approval",
            "never",
            "exec",
            "--cd",
            workspace.as_ref(),
            "--sandbox",
            "read-only",
            "--skip-git-repo-check",
            "--ephemeral",
        ];
        let mut command = Command::new("codex");
        command
            .env("HOME", &isolated_home)
            .args(codex_args)
            .arg(prompt);
        let result = run_harness(&mut command, PROOF, harness_timeout());
        emit_transparent_attachment_evidence(
            "codex",
            codex_detection.version.as_deref(),
            "managed-user-scope-skills-dir",
            "documented",
            &codex_args,
            &fixture.workspace,
            &fixture,
            "MANAGED_USER_SCOPE_REFERENCE",
            &result,
        );
    }

    // No UZE artifact was ever written into the real project workspace —
    // both attachments live entirely under the isolated home.
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
