use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use uze::{
    UzeEngine, UzeHome, UzeStore,
    capability::{CapabilityKind, Representation},
    exposure::{ExposureMechanism, ExposurePlan},
    integration::{IntegrationPort, assess_environment},
    router::{CompatibilityRoute, HarnessCapabilities, VerificationStatus},
};

use uze::integrations::{
    claude::{self, ClaudeIntegration},
    codex::{self, CodexIntegration},
    opencode::OpenCodeIntegration,
};

/// The acquisition pipeline every install now goes through: a source is
/// acquired into a materialized package, and only then does the Store ingest
/// it. Spelled out here rather than hidden behind a Store convenience,
/// because the Store deliberately no longer accepts a path.
fn install(
    store: &UzeStore,
    path: impl Into<std::path::PathBuf>,
) -> uze::Result<uze::StoredPackage> {
    store.ingest(&uze::acquisition::acquire(&uze::PackageSource::local(
        path,
    ))?)
}

fn package_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/packages/agent-plugin-skill")
}

fn mcp_package_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/packages/agent-plugin-mcp")
}

fn mcp_stored_environment(label: &str) -> (PathBuf, uze::EffectiveEnvironment) {
    let root = temporary_home(label);
    let store = UzeStore::new(UzeHome::at(&root));
    let package = install(&store, mcp_package_fixture()).unwrap();
    let environment = UzeEngine::new(store).compose(&[package.id]).unwrap();
    (root, environment)
}

fn temporary_home(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("uze-{label}-{}-{nonce}", std::process::id()))
}

fn stored_environment(label: &str) -> (PathBuf, uze::EffectiveEnvironment) {
    let root = temporary_home(label);
    let store = UzeStore::new(UzeHome::at(&root));
    let package = install(&store, package_fixture()).unwrap();
    let environment = UzeEngine::new(store).compose(&[package.id]).unwrap();
    (root, environment)
}

#[test]
fn peer_integrations_choose_exposure_without_converting_one_standard_skill() {
    let (home_root, environment) = stored_environment("integration-contract");
    let claude = ClaudeIntegration::new(home_root.join("claude-home"), UzeHome::at(&home_root));
    let codex = CodexIntegration::new(home_root.join("agents-home"), UzeHome::at(&home_root));
    let opencode = OpenCodeIntegration::new(
        home_root.join("opencode-agents"),
        home_root.join("opencode-config/opencode.json"),
        UzeHome::at(&home_root),
    );

    let resource = environment.resources.first().unwrap();
    assert_eq!(resource.capability.representation, Representation::Standard);
    assert!(resource.package_root().is_some());

    let claude_skill = assess_environment(&environment, &claude).pop().unwrap();
    assert_eq!(claude_skill.decision.route, CompatibilityRoute::Adaptable);
    assert_eq!(
        claude_skill.decision.verification,
        VerificationStatus::Unverified
    );
    assert!(matches!(
        claude_skill.exposure_plan.mechanism,
        ExposureMechanism::RuntimeBridge { .. }
    ));

    let codex_skill = assess_environment(&environment, &codex).pop().unwrap();
    assert_eq!(codex_skill.decision.route, CompatibilityRoute::Adaptable);
    assert_eq!(
        codex_skill.decision.verification,
        VerificationStatus::Unverified
    );
    assert!(matches!(
        codex_skill.exposure_plan.mechanism,
        ExposureMechanism::FilesystemProjection { .. }
    ));

    let opencode_skill = assess_environment(&environment, &opencode).pop().unwrap();
    assert_eq!(opencode_skill.decision.route, CompatibilityRoute::Adaptable);
    assert!(matches!(
        opencode_skill.exposure_plan.mechanism,
        ExposureMechanism::FilesystemProjection { .. }
    ));

    fs::remove_dir_all(home_root).unwrap();
}

struct FakeIntegration {
    id: &'static str,
}

impl IntegrationPort for FakeIntegration {
    fn id(&self) -> &'static str {
        self.id
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            direct_standard: BTreeSet::from([CapabilityKind::AgentSkill]),
            evidence: "fake contract evidence".to_owned(),
            ..HarnessCapabilities::default()
        }
    }

    fn exposure_plan(&self, resource: &uze::Resource) -> ExposurePlan {
        ExposurePlan {
            representation: resource.capability.representation,
            route: CompatibilityRoute::Native,
            verification: VerificationStatus::Unverified,
            mechanism: ExposureMechanism::DirectNative {
                resource_path: resource.capability.path.clone(),
            },
            evidence: "fake direct exposure".to_owned(),
        }
    }
}

#[test]
fn a_new_peer_integration_needs_no_core_change() {
    let (home_root, environment) = stored_environment("fake-integration");
    let cursor = FakeIntegration { id: "cursor" };
    let skill = assess_environment(&environment, &cursor).pop().unwrap();

    assert_eq!(skill.decision.route, CompatibilityRoute::Native);
    assert_eq!(skill.integration_id, "cursor");
    assert!(matches!(
        skill.exposure_plan.mechanism,
        ExposureMechanism::DirectNative { .. }
    ));

    fs::remove_dir_all(home_root).unwrap();
}

#[test]
fn package_store_and_effective_environment_preserve_the_same_skill_bytes() {
    let (home_root, environment) = stored_environment("byte-preservation");
    let resource = environment.resources.first().unwrap();
    let packaged_skill = package_fixture().join("skills/uze-e2e/SKILL.md");

    assert_eq!(
        fs::read(&resource.capability.path).unwrap(),
        fs::read(packaged_skill).unwrap()
    );

    fs::remove_dir_all(home_root).unwrap();
}

/// Exercises the real `ClaudeIntegration`/`CodexIntegration` transparent
/// attachment logic end to end, purely through the filesystem and directly
/// recorded integration state — no real `claude`/`codex` binary is spawned,
/// keeping this deterministic per the project's TDD boundary. Real-harness
/// behavioral verification is a separate opt-in conformance concern.
#[test]
fn claude_prefers_managed_attachment_once_setup_state_is_recorded() {
    let (home_root, environment) = stored_environment("claude-managed-attachment");
    let uze_home = UzeHome::at(&home_root);
    let claude_home = home_root.join("claude-home");
    let claude = ClaudeIntegration::new(claude_home.clone(), uze_home.clone());
    let resource = environment.resources.first().unwrap();

    // Before setup: the existing --plugin-dir conformance fallback.
    assert!(matches!(
        claude.exposure_plan(resource).mechanism,
        ExposureMechanism::RuntimeBridge { .. }
    ));
    assert!(claude.attach(resource).unwrap().is_none());

    // Simulate what `uze setup` records, without spawning a real `claude`
    // process.
    uze::state::record(
        &uze_home,
        uze::state::IntegrationRecord {
            harness: claude.id().to_owned(),
            version: Some("2.1.237".to_owned()),
            strategy: "managed-user-scope-skills-dir".to_owned(),
            installed: true,
        },
    )
    .unwrap();

    assert!(matches!(
        claude.exposure_plan(resource).mechanism,
        ExposureMechanism::ManagedUserScopeReference { .. }
    ));

    let attached = claude
        .attach(resource)
        .unwrap()
        .expect("managed attachment path");
    assert!(attached.is_symlink());
    assert_eq!(attached.parent().unwrap(), claude_home.join("skills"));

    let shim_root = fs::read_link(&attached).unwrap();
    assert!(shim_root.join(".claude-plugin/plugin.json").is_file());
    let skill_link = shim_root.join("SKILL.md");
    assert!(skill_link.is_symlink());
    assert_eq!(
        fs::read_link(&skill_link).unwrap(),
        resource.capability.path.parent().unwrap().join("SKILL.md")
    );

    // Idempotent: attaching again resolves to the same entry, no error.
    let attached_again = claude.attach(resource).unwrap().unwrap();
    assert_eq!(attached, attached_again);

    fs::remove_dir_all(home_root).unwrap();
}

#[test]
fn codex_prefers_managed_attachment_once_setup_state_is_recorded() {
    let (home_root, environment) = stored_environment("codex-managed-attachment");
    let uze_home = UzeHome::at(&home_root);
    let agents_home = home_root.join("agents-home");
    let codex = CodexIntegration::new(agents_home.clone(), uze_home.clone());
    let resource = environment.resources.first().unwrap();

    assert!(matches!(
        codex.exposure_plan(resource).mechanism,
        ExposureMechanism::FilesystemProjection { .. }
    ));

    uze::state::record(
        &uze_home,
        uze::state::IntegrationRecord {
            harness: codex.id().to_owned(),
            version: Some("0.148.0".to_owned()),
            strategy: "managed-user-scope-skills-dir".to_owned(),
            installed: true,
        },
    )
    .unwrap();

    assert!(matches!(
        codex.exposure_plan(resource).mechanism,
        ExposureMechanism::ManagedUserScopeReference { .. }
    ));

    let attached = codex
        .attach(resource)
        .unwrap()
        .expect("managed attachment path");
    assert!(attached.is_symlink());
    assert_eq!(
        fs::read_link(&attached).unwrap(),
        resource.capability.path.parent().unwrap().to_path_buf()
    );

    // Idempotent, and independent of Claude's own attachment state.
    let attached_again = codex.attach(resource).unwrap().unwrap();
    assert_eq!(attached, attached_again);
    assert!(!uze::state::is_installed(&uze_home, "claude-code"));

    fs::remove_dir_all(home_root).unwrap();
}

/// Deterministic MCP routing: exercises `exposure_plan` only (no `attach`,
/// no real `claude`/`codex` process — see `tests/cli.rs` for the
/// attach-exercising fake-harness suite). MCP has no per-session
/// conformance-probe fallback (unlike Skills' `--plugin-dir`), so
/// pre-setup routing must be `Unsupported`, not a fabricated mechanism.
#[test]
fn mcp_resource_is_unsupported_before_setup_for_both_harnesses() {
    let (home_root, environment) = mcp_stored_environment("mcp-unsupported-before-setup");
    let uze_home = UzeHome::at(&home_root);
    let claude = ClaudeIntegration::new(home_root.join("claude-home"), uze_home.clone());
    let codex = CodexIntegration::new(home_root.join("agents-home"), uze_home.clone());
    let resource = environment.resources.first().unwrap();
    assert_eq!(resource.capability.kind, CapabilityKind::Mcp);

    assert!(matches!(
        claude.exposure_plan(resource).mechanism,
        ExposureMechanism::Unsupported { .. }
    ));
    assert!(matches!(
        codex.exposure_plan(resource).mechanism,
        ExposureMechanism::Unsupported { .. }
    ));

    fs::remove_dir_all(home_root).unwrap();
}

#[test]
fn mcp_resource_routes_to_managed_vendor_config_once_setup_state_is_recorded() {
    let (home_root, environment) = mcp_stored_environment("mcp-managed-vendor-config");
    let uze_home = UzeHome::at(&home_root);
    let claude = ClaudeIntegration::new(home_root.join("claude-home"), uze_home.clone());
    let codex = CodexIntegration::new(home_root.join("agents-home"), uze_home.clone());
    let resource = environment.resources.first().unwrap();

    for harness in [claude.id(), codex.id()] {
        uze::state::record(
            &uze_home,
            uze::state::IntegrationRecord {
                harness: harness.to_owned(),
                version: Some("0.0.0".to_owned()),
                strategy: "managed-user-scope-skills-dir".to_owned(),
                installed: true,
            },
        )
        .unwrap();
    }

    let claude_plan = claude.exposure_plan(resource);
    let ExposureMechanism::ManagedVendorConfig {
        entry_name,
        command,
        args,
        ..
    } = &claude_plan.mechanism
    else {
        panic!(
            "expected ManagedVendorConfig, got {:?}",
            claude_plan.mechanism
        );
    };
    assert_eq!(entry_name, "uze-mcp-conformance-uze-conformance");
    assert_eq!(command.to_str().unwrap(), "__UZE_MCP_FIXTURE_BINARY__");
    assert!(args.is_empty());

    let codex_plan = codex.exposure_plan(resource);
    let ExposureMechanism::ManagedVendorConfig {
        entry_name: codex_entry_name,
        ..
    } = &codex_plan.mechanism
    else {
        panic!(
            "expected ManagedVendorConfig, got {:?}",
            codex_plan.mechanism
        );
    };
    // Same package, same entry-naming rule, independent of which harness asks.
    assert_eq!(codex_entry_name, entry_name);

    fs::remove_dir_all(home_root).unwrap();
}

/// `detach_mcp_entry` — not wired to any CLI verb yet (see ADR-007), so it
/// is exercised directly here rather than through `tests/cli.rs`. Uses a
/// minimal fake `claude`/`codex` on `PATH` that tracks one marker file per
/// registered entry name.
#[test]
#[cfg(unix)]
fn detach_mcp_entry_removes_a_registered_entry_idempotently() {
    use std::os::unix::fs::PermissionsExt;

    let dir = temporary_home("detach-mcp-entry");
    fs::create_dir_all(&dir).unwrap();
    let marker = dir.join("registered");
    fs::write(&marker, "").unwrap();
    for name in ["claude", "codex"] {
        let path = dir.join(name);
        fs::write(
            &path,
            format!(
                "#!/bin/sh\ncase \"$2\" in\n  remove) rm -f '{marker}' ;;\nesac\nexit 0\n",
                marker = marker.display()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).unwrap();
    }
    let original_path = std::env::var("PATH").unwrap();
    // SAFETY: this test does not run concurrently with other PATH-sensitive
    // tests in this process (cargo test runs each `#[test]` in the same
    // process but the harness's default parallelism only matters here in
    // that PATH is process-global; restored immediately after use).
    unsafe {
        std::env::set_var("PATH", format!("{}:{}", dir.display(), original_path));
    }

    assert!(marker.exists());
    claude::detach_mcp_entry(&dir, "uze-example").unwrap();
    assert!(!marker.exists(), "claude mcp remove should have run");

    // Idempotent: removing an already-absent entry is not an error.
    claude::detach_mcp_entry(&dir, "uze-example").unwrap();
    codex::detach_mcp_entry(&dir, "uze-example").unwrap();

    unsafe {
        std::env::set_var("PATH", original_path);
    }
    fs::remove_dir_all(dir).unwrap();
}
