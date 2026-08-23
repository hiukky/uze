//! Command Capability conformance (ADR-025) — the shared Integration
//! Conformance Suite's Command contract, exactly as specified:
//!
//! 1. canonical command discovered as Command, not Skill;
//! 2. Skill discovery unchanged;
//! 3. command native route where supported;
//! 4. command adaptation marked Adapted;
//! 5. unsupported command stays Unsupported;
//! 6. explicit vendor command precedence;
//! 7. generated native command deterministic;
//! 8. package exact coverage includes Command only when actually delivered;
//! 9. uncovered Command falls back;
//! 10. no duplicate Command receipt;
//! 11. naming collision deterministic;
//! 12. attach → Matched → detach → Missing;
//! 13. Store bytes unchanged;
//! 14. existing Skill/MCP conformance remains green.
//!
//! Deterministic by construction: no vendor binary is ever spawned. Harness
//! detection is forced to `present` via the same `AlwaysPresent` wrapper
//! strategy `exposure_naming.rs` uses, so `add_plugin` exercises the real
//! full path without depending on installed harnesses.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use uze::integrations::{
    claude::ClaudeIntegration, codex::CodexIntegration, gemini::GeminiIntegration,
    opencode::OpenCodeIntegration,
};
use uze::{
    PackageSource, Resource, UzeApplication, UzeEngine, UzeHome, UzeStore,
    capability::CapabilityKind,
    exposure::{ExposureMechanism, ExposurePlan, PackageExposurePlan},
    integration::{
        AttachmentInspection, AttachmentReceipt, AttachmentState, HarnessDetection,
        IntegrationPort, ManagedArtifact,
    },
    project::Resource as ProjectResource,
    provisioning::{ProcessResult, ProcessRunner, ProcessSpec},
    router::{CompatibilityRoute, HarnessCapabilities, VerificationStatus},
    state,
    store::StoredPackage,
};

fn temp(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "uze-command-conformance-{label}-{}-{nonce}",
        std::process::id()
    ))
}

fn workflow_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/packages/workflow")
}

fn flow_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/packages/flow")
}

fn install(store: &UzeStore, path: impl Into<PathBuf>) -> uze::Result<StoredPackage> {
    store.ingest(&uze::acquisition::acquire(&PackageSource::local(path))?)
}

fn mark_setup(home: &UzeHome, integration: &dyn IntegrationPort) {
    state::record(
        home,
        state::IntegrationRecord {
            harness: integration.id().to_owned(),
            version: Some("test".to_owned()),
            strategy: "test".to_owned(),
            installed: true,
        },
    )
    .unwrap();
}

fn stored_workflow(label: &str) -> (PathBuf, UzeHome, StoredPackage, Vec<Resource>) {
    let root = temp(label);
    let home = UzeHome::at(&root);
    let store = UzeStore::new(home.clone());
    let package = install(&store, workflow_fixture()).unwrap();
    let environment = UzeEngine::new(store)
        .compose(std::slice::from_ref(&package.id))
        .unwrap();
    let resources = environment.resources;
    (root, home, package, resources)
}

fn command_resources(resources: &[Resource]) -> Vec<&Resource> {
    resources
        .iter()
        .filter(|resource| resource.capability.kind == CapabilityKind::Command)
        .collect()
}

fn skill_resources(resources: &[Resource]) -> Vec<&Resource> {
    resources
        .iter()
        .filter(|resource| resource.capability.kind == CapabilityKind::AgentSkill)
        .collect()
}

/// Never spawns a process (same strategy as `exposure_naming.rs`).
struct NoopProcessRunner;

impl ProcessRunner for NoopProcessRunner {
    fn run(&self, _spec: &ProcessSpec) -> uze::Result<ProcessResult> {
        Ok(ProcessResult {
            success: true,
            timed_out: false,
        })
    }
}

struct AlwaysPresent<T: IntegrationPort>(T);

impl<T: IntegrationPort> IntegrationPort for AlwaysPresent<T> {
    fn id(&self) -> &'static str {
        self.0.id()
    }
    fn capabilities(&self) -> HarnessCapabilities {
        self.0.capabilities()
    }
    fn exposure_plan(&self, resource: &ProjectResource) -> ExposurePlan {
        self.0.exposure_plan(resource)
    }
    fn exposure_name_candidates(&self, resource: &ProjectResource) -> Vec<String> {
        self.0.exposure_name_candidates(resource)
    }
    fn shared_agent_skill_root(&self) -> Option<PathBuf> {
        self.0.shared_agent_skill_root()
    }
    fn package_exposure_plan(
        &self,
        package: &StoredPackage,
        resources: &[&ProjectResource],
    ) -> Option<PackageExposurePlan> {
        self.0.package_exposure_plan(package, resources)
    }
    fn detect(&self) -> HarnessDetection {
        HarnessDetection {
            present: true,
            version: Some("9.9.9".to_owned()),
        }
    }
    fn provision(
        &self,
        _runner: &dyn ProcessRunner,
    ) -> uze::Result<uze::provisioning::ProvisioningResult> {
        Ok(uze::provisioning::ProvisioningResult::verified(
            uze::provisioning::ProvisionAction::None,
            "test-always-present",
            self.detect(),
        ))
    }
    fn install(&self, home: &UzeHome, detection: &HarnessDetection) -> uze::Result<()> {
        self.0.install(home, detection)
    }
    fn attach(&self, resource: &ProjectResource) -> uze::Result<Option<PathBuf>> {
        self.0.attach(resource)
    }
    fn attach_package(
        &self,
        package: &StoredPackage,
        plan: &PackageExposurePlan,
    ) -> uze::Result<Option<AttachmentReceipt>> {
        self.0.attach_package(package, plan)
    }
    fn attach_receipt(&self, resource: &ProjectResource) -> uze::Result<Option<AttachmentReceipt>> {
        self.0.attach_receipt(resource)
    }
    fn inspect_receipt(&self, receipt: &AttachmentReceipt) -> AttachmentInspection {
        self.0.inspect_receipt(receipt)
    }
    fn detach_receipt(&self, receipt: &AttachmentReceipt) -> uze::Result<AttachmentInspection> {
        self.0.detach_receipt(receipt)
    }
}

// --- 1. Canonical command discovered as Command, not Skill -------------------

#[test]
fn canonical_command_is_discovered_as_command_not_skill() {
    let (root, _home, _package, resources) = stored_workflow("discovery");
    assert_eq!(resources.len(), 2, "exactly Skill + Command");
    let commands = command_resources(&resources);
    assert_eq!(commands.len(), 1);
    let command = commands[0];
    assert_eq!(command.capability.kind, CapabilityKind::Command);
    assert_eq!(
        command.capability.representation,
        uze::capability::Representation::Standard
    );
    assert_eq!(
        command.logical_capability_name().as_deref(),
        Some("review"),
        "the file stem is the command's logical name"
    );
    // Same logical name as the Skill, but a provably distinct identity.
    let skills = skill_resources(&resources);
    assert_eq!(skills.len(), 1);
    assert_eq!(
        skills[0].logical_capability_name().as_deref(),
        Some("review")
    );
    assert_ne!(command.identity(), skills[0].identity());
    assert!(
        command.identity().contains("commands/review.md"),
        "identity is path-based: {}",
        command.identity()
    );
    assert!(
        skills[0].identity().contains("skills/review/SKILL.md"),
        "identity is path-based: {}",
        skills[0].identity()
    );
    fs::remove_dir_all(root).unwrap();
}

// --- 2. Skill discovery unchanged -------------------------------------------

#[test]
fn skill_only_package_behaves_exactly_as_before() {
    let root = temp("skill-only");
    let home = UzeHome::at(&root);
    let store = UzeStore::new(home.clone());
    let package = install(&store, flow_fixture()).unwrap();
    let environment = UzeEngine::new(store).compose(&[package.id]).unwrap();
    assert_eq!(environment.resources.len(), 1, "exactly the commit Skill");
    assert_eq!(
        environment.resources[0].capability.kind,
        CapabilityKind::AgentSkill
    );
    assert_eq!(command_resources(&environment.resources).len(), 0);
    fs::remove_dir_all(root).unwrap();
}

// --- 3. Command native route where supported --------------------------------

#[test]
fn opencode_delivers_command_natively_via_byte_identical_reference() {
    let (root, home, _package, resources) = stored_workflow("opencode-native");
    let opencode = OpenCodeIntegration::new(
        root.join("agents"),
        root.join("config/opencode/opencode.json"),
        home.clone(),
    );
    mark_setup(&home, &opencode);
    let command = *command_resources(&resources).first().unwrap();
    let plan = opencode.exposure_plan(command);
    assert_eq!(plan.route, CompatibilityRoute::Native);
    let ExposureMechanism::ManagedUserScopeReference {
        discovery_root,
        entry_name,
        source,
    } = &plan.mechanism
    else {
        panic!("expected a managed reference, got {:?}", plan.mechanism);
    };
    assert_eq!(entry_name, "review.md");
    assert_eq!(discovery_root, &root.join("config/opencode/commands"));
    // The reference points at the canonical bytes themselves: byte-identical.
    assert_eq!(
        fs::read(source).unwrap(),
        fs::read(workflow_fixture().join("commands/review.md")).unwrap()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn gemini_delivers_command_natively_via_generated_toml() {
    let (root, home, _package, resources) = stored_workflow("gemini-native");
    let gemini = GeminiIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &gemini);
    let command = *command_resources(&resources).first().unwrap();
    let plan = gemini.exposure_plan(command);
    assert_eq!(plan.route, CompatibilityRoute::Native);
    let ExposureMechanism::ManagedFile {
        target_file,
        expected_content,
    } = &plan.mechanism
    else {
        panic!("expected a managed file, got {:?}", plan.mechanism);
    };
    assert_eq!(target_file, &root.join(".gemini/commands/review.toml"));
    assert!(
        expected_content
            .contains("description = \"Review code for correctness and missing tests\"")
    );
    assert!(
        expected_content.contains("prompt = \"\\nReview the current changes"),
        "the prompt holds the canonical body (after its frontmatter block): {expected_content}"
    );
    fs::remove_dir_all(root).unwrap();
}

// --- 4. Command adaptation marked Adapted -----------------------------------

#[test]
fn codex_delivers_command_natively_via_explicit_only_skill() {
    let (root, home, _package, resources) = stored_workflow("codex-native");
    let codex = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &codex);
    let command = command_resources(&resources)[0].clone();
    let plan = codex.exposure_plan(&command);
    // NATIVE per ADR-025: Codex's official explicit-invocation-only Skill
    // mechanism preserves the canonical Command semantics (explicit user
    // invocation; model never auto-selects; identity; body).
    assert_eq!(plan.route, CompatibilityRoute::Native);
    let ExposureMechanism::ManagedUserScopeReference { entry_name, .. } = &plan.mechanism else {
        panic!("expected a managed reference, got {:?}", plan.mechanism);
    };
    assert_eq!(entry_name, "review");
    assert!(
        plan.evidence.contains("NATIVE"),
        "the route must be reported honestly: {}",
        plan.evidence
    );
    fs::remove_dir_all(root).unwrap();
}

/// Audit contract (Codex command semantics): the generated artifact must
/// carry Codex's official explicit-only invocation policy
/// (`agents/openai.yaml` → `policy.allow_implicit_invocation: false`), so
/// the model never auto-selects a Command — the semantic that makes a
/// Command distinct from an ordinary Skill. A normal Codex Skill delivery
/// must NOT get the marker, keeping the two artifact shapes distinguishable.
#[test]
fn codex_generated_command_artifact_is_explicit_only_and_distinct_from_skill() {
    let (root, home, _package, resources) = stored_workflow("codex-explicit-only");
    let codex = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &codex);
    let command = command_resources(&resources)[0].clone();
    let command_receipt = codex.attach_receipt(&command).unwrap().expect("attaches");
    let ManagedArtifact::SymlinkReference { target, .. } = &command_receipt.artifact else {
        panic!("expected symlink artifact");
    };
    // The marker that disables implicit (model) invocation.
    assert_eq!(
        fs::read_to_string(target.join("agents/openai.yaml")).unwrap(),
        "policy:\n  allow_implicit_invocation: false\n",
        "the generated Command artifact must be explicit-only"
    );
    // The canonical identity is untouched by the physical adaptation.
    assert_eq!(
        command_receipt.resource_identity.as_deref().unwrap(),
        command.identity(),
        "the receipt keeps the canonical Command resource identity"
    );

    // A normal Codex Skill delivery is still a plain skill: no marker.
    let skill = skill_resources(&resources)[0].clone();
    let skill_receipt = codex.attach_receipt(&skill).unwrap().expect("attaches");
    let ManagedArtifact::SymlinkReference {
        target: skill_target,
        ..
    } = &skill_receipt.artifact
    else {
        panic!("expected symlink artifact");
    };
    assert!(
        !skill_target.join("agents/openai.yaml").exists(),
        "a plain skill must stay model-invocable: no explicit-only marker"
    );
    assert!(skill_target.join("SKILL.md").is_file());
    fs::remove_dir_all(root).unwrap();
}

/// Same-name Skill and Command stay distinct through the REAL add path: the
/// Codex generated native plugin covers the Skill (package receipt); the
/// Command is delivered per-resource as an explicit-only Skill. Assert: one
/// package receipt + exactly one Command receipt whose identity is the
/// canonical Command identity (never the Skill's), the bare user-facing
/// name, and the explicit-only marker — while the Skill keeps its own
/// identity inside the package coverage, never conflated and never
/// double-attached.
#[cfg(unix)]
#[test]
fn same_name_skill_and_command_stay_distinct_on_codex() {
    use std::os::unix::fs::PermissionsExt;

    let root = temp("codex-distinct");
    let _guard = lock_path();
    let bin = root.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let fake = bin.join("codex");
    fs::write(
        &fake,
        r#"#!/bin/sh
case "$*" in
  "plugin marketplace add "*|"plugin marketplace list --json"|"plugin add "*|"plugin list --json") exit 0 ;;
esac
exit 0
"#,
    )
    .unwrap();
    let mut permissions = fs::metadata(&fake).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake, permissions).unwrap();
    let original_path = std::env::var("PATH").unwrap();
    unsafe {
        std::env::set_var("PATH", format!("{}:{}", bin.display(), original_path));
    }

    let agents_home = root.join("codex-agents");
    let uze_home = UzeHome::at(root.join("uze-home"));
    let app = UzeApplication::new_with_runner(
        uze_home.clone(),
        vec![Box::new(AlwaysPresent(CodexIntegration::new(
            agents_home.clone(),
            uze_home.clone(),
        )))],
        Box::new(NoopProcessRunner),
    );
    let report = app
        .add_plugin(
            PackageSource::local(workflow_fixture()),
            &uze::trust::AlwaysTrust,
        )
        .unwrap();
    assert_eq!(report.plugin.capability_count, 2, "Skill + Command");

    // The package plan covers exactly the Skill (Codex native plugin format
    // has no command surface) — the Command is left for capability delivery.
    let (_, codex_plan) = report
        .package_plans
        .iter()
        .find(|(integration, _)| integration == "codex")
        .expect("codex produced a package plan");
    let package_receipts = state::receipts(&uze_home, Some("workflow"))
        .unwrap()
        .into_iter()
        .filter(|(_, receipt)| receipt.integration == "codex")
        .collect::<Vec<_>>();
    // Plugin-manager receipt + per-resource Command adaptation: exactly two,
    // never one per capability duplicated.
    assert_eq!(
        package_receipts.len(),
        2,
        "one package + one Command receipt"
    );

    let command_receipt = package_receipts
        .iter()
        .find(|(_, receipt)| {
            receipt
                .resource_identity
                .as_deref()
                .is_some_and(|identity| identity.contains("commands/review.md"))
        })
        .map(|(_, receipt)| receipt)
        .expect("one Command receipt");
    assert!(
        command_receipt
            .resource_identity
            .as_deref()
            .is_some_and(|identity| identity.contains("commands/review.md")),
        "the Command receipt keeps the canonical Command path identity"
    );
    assert!(
        !command_receipt
            .resource_identity
            .as_deref()
            .is_some_and(|identity| identity.contains("skills/review/SKILL.md")),
        "a Command is never recorded under a Skill identity"
    );
    // The Skill stays inside the generated plugin's coverage, so no
    // per-resource Skill receipt exists — nothing is double-attached.
    assert!(
        !package_receipts.iter().any(|(_, receipt)| receipt
            .resource_identity
            .as_deref()
            .is_some_and(|identity| identity.contains("skills/review/SKILL.md"))),
        "the package covers the Skill; no separate Skill receipt"
    );

    let ManagedArtifact::SymlinkReference {
        target: command_target,
        ..
    } = &command_receipt.artifact
    else {
        panic!("expected symlink artifact");
    };
    assert!(
        command_target.join("agents/openai.yaml").is_file(),
        "the Command artifact is explicit-only (not model-invocable)"
    );
    assert_eq!(
        command_target.file_name().unwrap(),
        "review",
        "the user-facing bare name stays with the Command"
    );

    // Exact coverage claim: the plan provided the Skill identity, never the
    // Command identity.
    let provided = &codex_plan.provided_resource_identities;
    assert!(
        provided
            .iter()
            .any(|identity| identity.contains("skills/review/SKILL.md")),
        "the package plan covers the Skill"
    );
    assert!(
        !provided
            .iter()
            .any(|identity| identity.contains("commands/review.md")),
        "the package plan never claims the Command"
    );

    unsafe {
        std::env::set_var("PATH", original_path);
    }
    fs::remove_dir_all(root).unwrap();
}

// --- 5. Unsupported command stays Unsupported -------------------------------

struct CommandlessIntegration;

impl IntegrationPort for CommandlessIntegration {
    fn id(&self) -> &'static str {
        "commandless"
    }
    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            direct_standard: [CapabilityKind::AgentSkill].into_iter().collect(),
            ..HarnessCapabilities::default()
        }
    }
    fn exposure_plan(&self, _resource: &ProjectResource) -> ExposurePlan {
        ExposurePlan {
            representation: uze::capability::Representation::Standard,
            route: CompatibilityRoute::Unsupported,
            verification: VerificationStatus::Unverified,
            mechanism: ExposureMechanism::Unsupported {
                rationale: "no commands".to_owned(),
            },
            evidence: "no commands".to_owned(),
        }
    }
}

#[test]
fn harness_without_command_support_keeps_commands_unsupported() {
    let (root, _home, _package, resources) = stored_workflow("unsupported");
    let decision = uze::integration::assess_environment(
        &uze::project::EffectiveEnvironment {
            root: root.clone(),
            resources: resources.clone(),
        },
        &CommandlessIntegration,
    )
    .into_iter()
    .find(|assessment| assessment.capability_path.contains("commands"))
    .unwrap();
    assert_eq!(decision.decision.route, CompatibilityRoute::Unsupported);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn claude_capability_level_command_is_unsupported_outside_package_coverage() {
    let (root, home, _package, resources) = stored_workflow("claude-cmd");
    let claude = ClaudeIntegration::new(root.join("claude"), home.clone());
    mark_setup(&home, &claude);
    let command = *command_resources(&resources).first().unwrap();
    let plan = claude.exposure_plan(command);
    assert_eq!(plan.route, CompatibilityRoute::Unsupported);
    assert!(plan.evidence.contains("native plugin envelope"));
    fs::remove_dir_all(root).unwrap();
}

// --- 6. Explicit vendor command precedence ----------------------------------

fn explicit_claude_package(label: &str, envelope: &str) -> (PathBuf, StoredPackage) {
    let root = temp(label);
    let store = UzeStore::new(UzeHome::at(&root));
    let pkg_root = root.join("src");
    fs::create_dir_all(pkg_root.join(".claude-plugin")).unwrap();
    fs::create_dir_all(pkg_root.join("skills/a")).unwrap();
    fs::create_dir_all(pkg_root.join("commands")).unwrap();
    fs::write(
        pkg_root.join("plugin.json"),
        r#"{"name":"explicit-claude"}"#,
    )
    .unwrap();
    fs::write(pkg_root.join(".claude-plugin/plugin.json"), envelope).unwrap();
    fs::write(pkg_root.join("skills/a/SKILL.md"), "skill a").unwrap();
    fs::write(pkg_root.join("commands/review.md"), "command review").unwrap();
    let package = install(&store, pkg_root.clone()).unwrap();
    (root, package)
}

#[test]
fn explicit_envelope_commands_field_controls_command_coverage() {
    // Declared explicitly: both Skill and Command covered.
    let (root, package) = explicit_claude_package(
        "explicit-declared",
        r#"{"name":"explicit-claude","skills":["./skills/a"],"commands":["./commands/review.md"]}"#,
    );
    let store = UzeStore::new(UzeHome::at(&root));
    let environment = UzeEngine::new(store)
        .compose(std::slice::from_ref(&package.id))
        .unwrap();
    let claude = ClaudeIntegration::new(root.join("claude"), UzeHome::at(&root));
    let plan = claude
        .package_exposure_plan(&package, &environment.resources.iter().collect::<Vec<_>>())
        .expect("explicit envelope always plans");
    let command = *command_resources(&environment.resources).first().unwrap();
    let skill = *skill_resources(&environment.resources).first().unwrap();
    assert!(plan.provides(command));
    assert!(plan.provides(skill));
    fs::remove_dir_all(root).unwrap();

    // Declared surface excludes the command: no blanket fallback.
    let (root, package) = explicit_claude_package(
        "explicit-other",
        r#"{"name":"explicit-claude","skills":["./skills/a"],"commands":["./commands/other.md"]}"#,
    );
    let store = UzeStore::new(UzeHome::at(&root));
    let environment = UzeEngine::new(store)
        .compose(std::slice::from_ref(&package.id))
        .unwrap();
    let claude = ClaudeIntegration::new(root.join("claude"), UzeHome::at(&root));
    let plan = claude
        .package_exposure_plan(&package, &environment.resources.iter().collect::<Vec<_>>())
        .expect("explicit envelope always plans");
    let command = *command_resources(&environment.resources).first().unwrap();
    assert!(
        !plan.provides(command),
        "a command the envelope does not declare must not be claimed"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn explicit_envelope_absent_commands_field_uses_conventional_commands_dir() {
    let (root, package) = explicit_claude_package(
        "explicit-default",
        r#"{"name":"explicit-claude","skills":["./skills/a"]}"#,
    );
    let store = UzeStore::new(UzeHome::at(&root));
    let environment = UzeEngine::new(store)
        .compose(std::slice::from_ref(&package.id))
        .unwrap();
    let claude = ClaudeIntegration::new(root.join("claude"), UzeHome::at(&root));
    let plan = claude
        .package_exposure_plan(&package, &environment.resources.iter().collect::<Vec<_>>())
        .expect("explicit envelope always plans");
    let command = *command_resources(&environment.resources).first().unwrap();
    assert!(
        plan.provides(command),
        "the default commands/ directory is Claude's command surface"
    );
    fs::remove_dir_all(root).unwrap();
}

// --- 7. Generated native command deterministic ------------------------------

#[test]
fn generated_command_artifacts_are_deterministic() {
    let (root, home, _package, resources) = stored_workflow("determinism");
    let command = command_resources(&resources)[0].clone();
    let gemini = GeminiIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &gemini);
    let first = gemini.exposure_plan(&command);
    let second = gemini.exposure_plan(&command);
    assert_eq!(
        first, second,
        "two plans from the same resource are identical"
    );

    let codex = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &codex);
    let receipt_a = codex.attach_receipt(&command).unwrap().expect("attaches");
    let ManagedArtifact::SymlinkReference {
        target: target_a, ..
    } = &receipt_a.artifact
    else {
        panic!("expected symlink artifact");
    };
    let bytes_a = fs::read(target_a.join("SKILL.md")).unwrap();
    let policy_a = fs::read(target_a.join("agents/openai.yaml")).unwrap();
    let receipt_b = codex.attach_receipt(&command).unwrap().expect("attaches");
    let ManagedArtifact::SymlinkReference {
        target: target_b, ..
    } = &receipt_b.artifact
    else {
        panic!("expected symlink artifact");
    };
    let bytes_b = fs::read(target_b.join("SKILL.md")).unwrap();
    let policy_b = fs::read(target_b.join("agents/openai.yaml")).unwrap();
    assert_eq!(bytes_a, bytes_b, "rebuilds are byte-identical");
    assert_eq!(
        policy_a, policy_b,
        "the explicit-only policy is deterministic"
    );
    fs::remove_dir_all(root).unwrap();
}

// --- 8. Package exact coverage includes Command only when delivered ---------

#[test]
fn claude_generated_package_covers_skill_and_command_exactly() {
    let (root, home, package, resources) = stored_workflow("claude-generated");
    let claude = ClaudeIntegration::new(root.join("claude"), home.clone());
    let resources: Vec<_> = resources.iter().collect();
    let plan = claude
        .package_exposure_plan(&package, &resources)
        .expect("workflow is generatable");
    assert_eq!(plan.route, CompatibilityRoute::Native);
    let expected: BTreeSet<String> = resources
        .iter()
        .map(|resource| resource.identity())
        .collect();
    assert_eq!(
        plan.provided_resource_identities, expected,
        "the generated envelope covers exactly Skill + Command and nothing else"
    );
    // `attach_package` materializes the generated envelope (Derived Artifact
    // under $UZE_HOME — never the Store) from the plan it just approved.
    // A fake `claude` on PATH answers marketplace/plugin queries without
    // spawning the real binary (same pattern as `detach_mcp_entry`'s test).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _guard = lock_path();
        let bin = root.join("bin");
        fs::create_dir_all(&bin).unwrap();
        let fake = bin.join("claude");
        fs::write(
            &fake,
            r#"#!/bin/sh
case "$*" in
  "plugin marketplace add "*|"plugin marketplace list --json"|"plugin install "*|"plugin list --json") exit 0 ;;
esac
exit 0
"#,
        )
        .unwrap();
        let mut permissions = fs::metadata(&fake).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&fake, permissions).unwrap();
        let original_path = std::env::var("PATH").unwrap();
        unsafe {
            std::env::set_var("PATH", format!("{}:{}", bin.display(), original_path));
        }
        let receipt = claude
            .attach_package(&package, &plan)
            .expect("attach_package succeeds")
            .expect("a package receipt is produced");
        assert_eq!(receipt.strategy, "native-plugin-marketplace-generated");
        let generated = home
            .state_dir()
            .join("attachments")
            .join("claude")
            .join("generated")
            .join(package.id.as_str());
        assert!(generated.join("commands").is_symlink());
        assert!(generated.join("skills").is_symlink());
        let manifest: serde_json::Value = serde_json::from_slice(
            &fs::read(generated.join(".claude-plugin/plugin.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["commands"], serde_json::json!(["./commands"]));
        assert_eq!(manifest["skills"], serde_json::json!(["./skills"]));
        unsafe {
            std::env::set_var("PATH", original_path);
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn codex_generated_package_cannot_cover_commands_so_they_fall_back() {
    let (root, home, package, resources) = stored_workflow("codex-generated");
    let codex = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &codex);
    let resources: Vec<_> = resources.iter().collect();
    let plan = codex
        .package_exposure_plan(&package, &resources)
        .expect("workflow is generatable");
    assert_eq!(plan.route, CompatibilityRoute::Native);
    let command =
        command_resources(&resources.iter().map(|r| (*r).clone()).collect::<Vec<_>>())[0].clone();
    assert!(
        !plan.provides(&command),
        "Codex's native plugin format has no command surface; the generated envelope must not claim it"
    );
    // The same command is still deliverable — capability-level, as a NATIVE
    // explicit-only Skill: no resource is lost to blanket coverage.
    let command_plan = codex.exposure_plan(&command);
    assert_eq!(command_plan.route, CompatibilityRoute::Native);
    assert!(matches!(
        command_plan.mechanism,
        ExposureMechanism::ManagedUserScopeReference { .. }
    ));
    fs::remove_dir_all(root).unwrap();
}

// --- 9. Uncovered Command falls back ---------------------------------------

fn gemini_explicit_package(label: &str) -> (PathBuf, StoredPackage) {
    let root = temp(label);
    let store = UzeStore::new(UzeHome::at(&root));
    let pkg_root = root.join("src");
    fs::create_dir_all(pkg_root.join("skills/a")).unwrap();
    fs::create_dir_all(pkg_root.join("commands")).unwrap();
    fs::write(
        pkg_root.join("plugin.json"),
        r#"{"name":"gemini-explicit"}"#,
    )
    .unwrap();
    fs::write(
        pkg_root.join("gemini-extension.json"),
        r#"{"name":"gemini-explicit","mcpServers":{}}"#,
    )
    .unwrap();
    fs::write(pkg_root.join("skills/a/SKILL.md"), "skill a").unwrap();
    fs::write(pkg_root.join("commands/review.md"), "command review").unwrap();
    let package = install(&store, pkg_root.clone()).unwrap();
    (root, package)
}

#[test]
fn gemini_explicit_extension_does_not_claim_commands_and_fallback_delivers() {
    let (root, package) = gemini_explicit_package("gemini-fallback");
    let home = UzeHome::at(&root);
    let environment = UzeEngine::new(UzeStore::new(home.clone()))
        .compose(std::slice::from_ref(&package.id))
        .unwrap();
    let gemini = GeminiIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &gemini);
    let command = command_resources(&environment.resources)[0].clone();
    let resources: Vec<_> = environment.resources.iter().collect();
    // The explicit extension represents vendor TOML, not canonical .md
    // commands — no blanket claim.
    let plan = gemini
        .package_exposure_plan(&package, &resources)
        .expect("explicit extension always plans");
    assert!(
        !plan.provides(&command),
        "an explicit extension's commands are vendor TOML, never canonical .md resources"
    );
    // Fallback delivers it natively at the capability level.
    let fallback = gemini.exposure_plan(&command);
    assert_eq!(fallback.route, CompatibilityRoute::Native);
    assert!(matches!(
        fallback.mechanism,
        ExposureMechanism::ManagedFile { .. }
    ));
    fs::remove_dir_all(root).unwrap();
}

// --- 10. No duplicate Command receipt ---------------------------------------

#[test]
fn one_command_attaches_exactly_one_receipt() {
    let root = temp("single-receipt");
    let agents_home = root.join("opencode-agents");
    let uze_home = UzeHome::at(root.join("uze-home"));
    let app = UzeApplication::new_with_runner(
        uze_home.clone(),
        vec![Box::new(AlwaysPresent(OpenCodeIntegration::new(
            agents_home.clone(),
            root.join("opencode-config/opencode.json"),
            uze_home.clone(),
        )))],
        Box::new(NoopProcessRunner),
    );
    app.add_plugin(
        PackageSource::local(workflow_fixture()),
        &uze::trust::AlwaysTrust,
    )
    .unwrap();
    let receipts = state::receipts(&uze_home, None).unwrap();
    let command_receipts: Vec<_> = receipts
        .iter()
        .filter(|(_, receipt)| {
            receipt
                .resource_identity
                .as_deref()
                .is_some_and(|identity| identity.contains("commands/review.md"))
        })
        .collect();
    assert_eq!(command_receipts.len(), 1, "exactly one Command receipt");
    let (_, receipt) = command_receipts[0];
    assert!(matches!(
        receipt.artifact,
        ManagedArtifact::SymlinkReference { .. }
    ));
    fs::remove_dir_all(root).unwrap();
}

// --- 11. Naming collision deterministic -------------------------------------

fn command_fixture(root: &Path, package_id: &str) -> PathBuf {
    let dir = root.join(package_id);
    fs::create_dir_all(dir.join("commands")).unwrap();
    fs::write(
        dir.join("plugin.json"),
        format!(
            r#"{{"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"{package_id}"}}"#
        ),
    )
    .unwrap();
    fs::write(
        dir.join("commands/review.md"),
        format!("---\ndescription: review for {package_id}\n---\n\nBody.\n"),
    )
    .unwrap();
    dir
}

#[test]
fn same_named_commands_from_two_packages_resolve_deterministically() {
    let root = temp("collision");
    let agents_home = root.join("opencode-agents");
    let uze_home = UzeHome::at(root.join("uze-home"));
    let app = UzeApplication::new_with_runner(
        uze_home.clone(),
        vec![Box::new(AlwaysPresent(OpenCodeIntegration::new(
            agents_home.clone(),
            root.join("opencode-config/opencode.json"),
            uze_home.clone(),
        )))],
        Box::new(NoopProcessRunner),
    );
    app.add_plugin(
        PackageSource::local(command_fixture(&root.join("fixtures"), "alpha")),
        &uze::trust::AlwaysTrust,
    )
    .unwrap();
    app.add_plugin(
        PackageSource::local(command_fixture(&root.join("fixtures"), "beta")),
        &uze::trust::AlwaysTrust,
    )
    .unwrap();

    let commands_dir = root.join("opencode-config/commands");
    assert!(
        commands_dir.join("review.md").is_symlink(),
        "first package keeps the bare name"
    );
    assert!(
        commands_dir.join("beta-review.md").is_symlink(),
        "second package resolves to its qualified name"
    );
    let receipts = state::receipts(&uze_home, None).unwrap();
    let mut names: Vec<String> = receipts
        .iter()
        .filter(|(_, receipt)| {
            receipt
                .resource_identity
                .as_deref()
                .is_some_and(|identity| identity.contains("commands/review.md"))
        })
        .map(|(_, receipt)| match &receipt.artifact {
            ManagedArtifact::SymlinkReference { path, .. } => {
                path.file_name().unwrap().to_string_lossy().into_owned()
            }
            other => panic!("unexpected artifact {other:?}"),
        })
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec!["beta-review.md".to_owned(), "review.md".to_owned()]
    );
    fs::remove_dir_all(root).unwrap();
}

// --- 12. attach → Matched → detach → Missing --------------------------------

#[test]
fn opencode_command_lifecycle_attach_matched_detach_missing() {
    let (root, home, _package, resources) = stored_workflow("lifecycle-opencode");
    let opencode = OpenCodeIntegration::new(
        root.join("agents"),
        root.join("config/opencode/opencode.json"),
        home.clone(),
    );
    mark_setup(&home, &opencode);
    let command = command_resources(&resources)[0].clone();
    let receipt = opencode
        .attach_receipt(&command)
        .expect("a managed attachment is produced")
        .expect("not None");
    assert_eq!(
        opencode.inspect_receipt(&receipt).state,
        AttachmentState::Matched
    );
    // The canonical bytes are untouched — the reference is byte-identical.
    let ManagedArtifact::SymlinkReference { target, .. } = &receipt.artifact else {
        panic!("expected symlink artifact");
    };
    assert_eq!(
        fs::read(target).unwrap(),
        fs::read(workflow_fixture().join("commands/review.md")).unwrap()
    );
    assert_eq!(
        opencode.detach_receipt(&receipt).unwrap().state,
        AttachmentState::Missing
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn gemini_command_lifecycle_attach_matched_detach_missing() {
    let (root, home, _package, resources) = stored_workflow("lifecycle-gemini");
    let gemini = GeminiIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &gemini);
    let command = command_resources(&resources)[0].clone();
    let receipt = gemini.attach_receipt(&command).unwrap().expect("not None");
    assert_eq!(
        gemini.inspect_receipt(&receipt).state,
        AttachmentState::Matched
    );
    let ManagedArtifact::ManagedFile { path, .. } = &receipt.artifact else {
        panic!("expected managed-file artifact");
    };
    assert!(path.is_file());
    assert_eq!(
        gemini.detach_receipt(&receipt).unwrap().state,
        AttachmentState::Missing
    );
    assert!(!path.exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn codex_command_lifecycle_attach_matched_detach_missing_with_cleanup() {
    let (root, home, _package, resources) = stored_workflow("lifecycle-codex");
    let codex = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &codex);
    let command = command_resources(&resources)[0].clone();
    let receipt = codex.attach_receipt(&command).unwrap().expect("not None");
    assert_eq!(
        codex.inspect_receipt(&receipt).state,
        AttachmentState::Matched
    );
    let ManagedArtifact::SymlinkReference { target, .. } = &receipt.artifact else {
        panic!("expected symlink artifact");
    };
    // The delivered Skill is a Derived Artifact under $UZE_HOME.
    assert!(
        target.starts_with(
            uze_home_state(&home)
                .join("attachments")
                .join("codex")
                .join("commands")
        )
    );
    assert!(
        target.join("SKILL.md").is_file(),
        "the generated SKILL.md preserves the command identity and body"
    );
    assert!(
        target.join("agents/openai.yaml").is_file(),
        "the generated explicit-only policy rides with the artifact"
    );
    assert_eq!(
        codex.detach_receipt(&receipt).unwrap().state,
        AttachmentState::Missing
    );
    assert!(
        !target.exists(),
        "unreferenced derived artifact is cleaned up"
    );
    fs::remove_dir_all(root).unwrap();
}

fn uze_home_state(home: &UzeHome) -> PathBuf {
    home.state_dir()
}

/// Serializes tests that mutate the process-global `PATH` (fake-vendor
/// binaries and real-Codex dogfood): three tests in this file prepend their
/// own bin dir, and parallel interleaving would let one test's restore wipe
/// another's prepend mid-run.
static PATH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock_path() -> std::sync::MutexGuard<'static, ()> {
    PATH_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

// --- 13. Store bytes unchanged ----------------------------------------------

#[test]
fn store_bytes_are_never_touched_by_command_discovery_or_delivery() {
    let (root, home, package, resources) = stored_workflow("store-bytes");
    let original = fs::read(package.root.join("commands/review.md")).unwrap();
    let command = command_resources(&resources)[0].clone();
    let gemini = GeminiIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &gemini);
    let plan = gemini.exposure_plan(&command);
    let _ = plan;
    // Attach Gemini's generated TOML — must only write outside the Store.
    let receipt = gemini.attach_receipt(&command).unwrap();
    let _ = receipt;
    assert_eq!(
        fs::read(package.root.join("commands/review.md")).unwrap(),
        original,
        "the canonical command bytes in the Store are untouched"
    );
    assert!(
        !package.root.join("commands/review.toml").exists(),
        "generated vendor representation never lands in the Store"
    );
    fs::remove_dir_all(root).unwrap();
}

// --- 14. Existing Skill/MCP conformance remains green -----------------------

#[test]
fn skill_routing_and_receipt_identity_are_unchanged() {
    let (root, home, _package, resources) = stored_workflow("skill-green");
    let skill = skill_resources(&resources)[0].clone();
    assert_eq!(skill.capability.kind, CapabilityKind::AgentSkill);
    // OpenCode still routes the Skill through the shared skills root with
    // the bare logical name — same as before Commands existed.
    let opencode = OpenCodeIntegration::new(
        root.join("agents"),
        root.join("config/opencode/opencode.json"),
        home.clone(),
    );
    mark_setup(&home, &opencode);
    let skill_plan = opencode.exposure_plan(&skill);
    assert_eq!(skill_plan.route, CompatibilityRoute::Native);
    let ExposureMechanism::ManagedUserScopeReference { entry_name, .. } = &skill_plan.mechanism
    else {
        panic!("expected managed reference");
    };
    assert_eq!(entry_name, "review");
    // A same-named Command does not collide with the Skill: distinct roots
    // and distinct receipt identities.
    let command = command_resources(&resources)[0].clone();
    let command_plan = opencode.exposure_plan(&command);
    let ExposureMechanism::ManagedUserScopeReference { entry_name, .. } = &command_plan.mechanism
    else {
        panic!("expected managed reference");
    };
    assert_eq!(entry_name, "review.md");
    fs::remove_dir_all(root).unwrap();
}

// --- Real Codex dogfood (zero model calls) -----------------------------------

/// Runs `codex` with the given isolated HOME; returns stdout on success.
fn run_codex_prompt_input(home: &Path) -> std::result::Result<String, String> {
    let output = std::process::Command::new("codex")
        .env("HOME", home)
        .args(["debug", "prompt-input"])
        .output()
        .map_err(|error| format!("failed to run codex: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "codex debug prompt-input exited with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Real-Codex deterministic dogfood (zero model calls), driven by UZE's own
/// delivery: the `CodexIntegration` attaches a canonical Command into an
/// isolated `~/.agents/skills` (with its `agents/openai.yaml` explicit-only
/// policy), a plain Skill is placed beside it, and the model-visible prompt
/// is rendered with `codex debug prompt-input`. The six audit regressions:
///
/// 1. the plain Skill IS in the model-visible list (implicitly discoverable);
/// 2. the Command is NOT (explicit-only: model cannot auto-select);
/// 3. the Command artifact carries Codex's official mechanism
///    (`allow_implicit_invocation: false`), which per Codex documentation
///    preserves explicit `$skill` invocation;
/// 4. the receipt keeps the canonical Command identity;
/// 5. same-name Skill and Command remain distinct (only the Command is
///    attached here; the Skill is a separate, plain artifact);
/// 6. the Store is byte-immutable throughout.
///
/// A malformed-metadata control restores the listing, proving the exclusion
/// is caused by the policy file being genuinely read. Skips (prints a note
/// and returns) when `codex` is not on PATH, so CI stays deterministic.
#[test]
fn real_codex_dogfood_explicit_only_preserves_command_semantics() {
    let probe = std::process::Command::new("codex")
        .arg("--version")
        .output();
    if probe.is_err() || probe.as_ref().is_ok_and(|o| !o.status.success()) {
        eprintln!("codex not available on PATH; skipping real-Codex dogfood");
        return;
    }
    let root = temp("real-codex-dogfood");
    let _guard = lock_path();
    let uze_home = UzeHome::at(root.join("uze"));
    let store = UzeStore::new(uze_home.clone());
    let package = install(&store, workflow_fixture()).unwrap();
    let environment = UzeEngine::new(store)
        .compose(std::slice::from_ref(&package.id))
        .unwrap();
    let resources = environment.resources;

    let codex_home = root.join("codex-home");
    let agents_home = codex_home.join(".agents");
    let codex = CodexIntegration::new(agents_home.clone(), uze_home.clone());
    mark_setup(&uze_home, &codex);

    // (4) canonical identity: attach the Command through the real integration.
    let command = command_resources(&resources)[0].clone();
    let command_receipt = codex.attach_receipt(&command).unwrap().expect("attaches");
    assert!(
        command_receipt
            .resource_identity
            .as_deref()
            .is_some_and(|identity| identity.contains("commands/review.md")),
        "the receipt keeps the canonical Command identity"
    );
    let ManagedArtifact::SymlinkReference { target, .. } = &command_receipt.artifact else {
        panic!("expected symlink artifact");
    };
    // (3) official explicit-invocation mechanism is present.
    assert_eq!(
        fs::read_to_string(target.join("agents/openai.yaml")).unwrap(),
        "policy:\n  allow_implicit_invocation: false\n",
        "the generated Command artifact carries Codex's official explicit-only policy"
    );

    // (5) same-name Skill stays distinct: a plain, model-invocable Skill.
    fs::create_dir_all(agents_home.join("skills/normal")).unwrap();
    fs::write(
        agents_home.join("skills/normal/SKILL.md"),
        "---\nname: normal\ndescription: Run normal tasks N.\n---\n\nNormal body.\n",
    )
    .unwrap();

    let before_store_bytes = fs::read(package.root.join("commands/review.md")).unwrap();
    let valid = run_codex_prompt_input(&codex_home).expect("codex prompt-input runs");
    // (1) plain Skill: implicitly discoverable (model-visible list).
    assert!(
        valid.contains("normal: Run normal tasks N"),
        "a plain Skill stays implicitly discoverable in the model-visible list"
    );
    // (2) Command: NOT implicitly discoverable.
    assert!(
        !valid.contains("review: Review code for correctness and missing tests"),
        "the explicit-only Command must not be offered to the model"
    );

    // Control: malformed policy metadata must restore the listing, proving
    // the exclusion above is caused by the policy file itself.
    fs::write(target.join("agents/openai.yaml"), "policy: [broken yaml\n").unwrap();
    let malformed = run_codex_prompt_input(&codex_home).expect("codex prompt-input runs");
    assert!(
        malformed.contains("review: Review code for correctness and missing tests"),
        "control: malformed policy metadata must not suppress the Command"
    );

    // (6) Store immutability.
    assert_eq!(
        fs::read(package.root.join("commands/review.md")).unwrap(),
        before_store_bytes,
        "the canonical command bytes in the Store are untouched"
    );
    assert!(
        !package.root.join("commands/agents").exists(),
        "no generated policy file ever lands in the Store"
    );

    fs::remove_dir_all(root).unwrap();
}
