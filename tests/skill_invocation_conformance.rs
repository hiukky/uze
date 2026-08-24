//! Skill Invocation Policy conformance (ADR-030) — the canonical model is
//! one Skill capability whose semantics are *who may invoke it*, declared
//! by the `invoke:` frontmatter block:
//!
//! | `model` | `user` | Meaning | |
//! |---------|--------|---------|-|
//! | `true`  | `true`  | default interactive/discoverable Skill |
//! | `true`  | `false` | background/model-only capability |
//! | `false` | `true`  | explicit user action (previously `Command`) |
//! | `false` | `false` | invalid — nobody can invoke it; never projected |
//!
//! This suite proves, deterministically (no vendor binary is ever spawned),
//! for every harness and every combination:
//!
//! - discovery: one Skill family, invocation policy parsed at the Store
//!   boundary, Store bytes untouched;
//! - route classification (Native / Adaptable / Degraded / Unsupported),
//!   honestly per real vendor semantics;
//! - physical representation: the vendor's own encoding, only where needed;
//! - receipts + lifecycle (attach → Matched → detach → Missing);
//! - backward compatibility: a Skill without `invoke:` behaves exactly as
//!   before.
//!
//! Harness detection is forced to `present` via the same `AlwaysPresent`
//! wrapper strategy `exposure_naming.rs` uses.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use uze::capability::CapabilityKind;
use uze::integrations::{
    antigravity::AntigravityIntegration, claude::ClaudeIntegration, codex::CodexIntegration,
    opencode::OpenCodeIntegration,
};
use uze::{
    PackageSource, Resource, SkillInvocationPolicy, UzeApplication, UzeEngine, UzeHome, UzeStore,
    exposure::{ExposurePlan, PackageExposurePlan},
    integration::{
        AttachmentInspection, AttachmentReceipt, AttachmentState, HarnessDetection,
        IntegrationPort, ManagedArtifact,
    },
    project::Resource as ProjectResource,
    provisioning::{ProcessResult, ProcessRunner, ProcessSpec},
    router::{CompatibilityRoute, HarnessCapabilities},
    state,
    store::StoredPackage,
};

fn temp(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "uze-invocation-conformance-{label}-{}-{nonce}",
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

fn stored_fixture(
    label: &str,
    fixture: PathBuf,
) -> (PathBuf, UzeHome, StoredPackage, Vec<Resource>) {
    let root = temp(label);
    let home = UzeHome::at(&root);
    let store = UzeStore::new(home.clone());
    let package = install(&store, fixture).unwrap();
    let environment = UzeEngine::new(store)
        .compose(std::slice::from_ref(&package.id))
        .unwrap();
    let resources = environment.resources;
    (root, home, package, resources)
}

/// Builds a temp canonical package with ONE skill carrying the given
/// frontmatter body.
fn make_policy_package(
    label: &str,
    skill_name: &str,
    skill_body: &str,
) -> (PathBuf, UzeHome, StoredPackage, Resource) {
    let root = temp(label);
    let home = UzeHome::at(&root);
    let store = UzeStore::new(home.clone());
    let package_root = root.join("pkg");
    fs::create_dir_all(package_root.join("skills").join(skill_name)).unwrap();
    fs::write(
        package_root
            .join("skills")
            .join(skill_name)
            .join("SKILL.md"),
        skill_body,
    )
    .unwrap();
    fs::write(
        package_root.join("plugin.json"),
        r#"{"name":"flow","version":"1.0.0","description":"invocation-policy fixture"}"#,
    )
    .unwrap();
    let package = install(&store, &package_root).unwrap();
    let environment = UzeEngine::new(store)
        .compose(std::slice::from_ref(&package.id))
        .unwrap();
    let resource = environment
        .resources
        .into_iter()
        .find(|resource| resource.capability.kind == CapabilityKind::AgentSkill)
        .expect("fixture ships exactly one Skill");
    (root, home, package, resource)
}

fn user_only_body(name: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: Explicit user action\ninvoke:\n  model: false\n  user: true\n---\n\nExplicit body.\n"
    )
}

fn model_only_body(name: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: Background knowledge\ninvoke:\n  model: true\n  user: false\n---\n\nBody.\n"
    )
}

fn default_body(name: &str) -> String {
    format!("---\nname: {name}\ndescription: Interactive skill\n---\n\nBody.\n")
}

fn invalid_body(name: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: Uninvokable\ninvoke:\n  model: false\n  user: false\n---\n\nBody.\n"
    )
}

fn always_present<T: IntegrationPort>(integration: T) -> AlwaysPresent<T> {
    AlwaysPresent(integration)
}

/// Never spawns a process.
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
    fn status(&self, home: &UzeHome) -> uze::integration::IntegrationStatus {
        self.0.status(home)
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

// --- 1. Discovery: one Skill family, invocation policy at the boundary ----

#[test]
fn user_only_fixture_is_discovered_as_one_skill_with_policy() {
    let (root, _home, _package, resources) = stored_fixture("workflow", workflow_fixture());
    let skills: Vec<&Resource> = resources
        .iter()
        .filter(|resource| resource.capability.kind == CapabilityKind::AgentSkill)
        .collect();
    assert_eq!(skills.len(), 1, "exactly one Skill resource");
    let skill = skills[0];
    assert_eq!(
        skill.skill_policy,
        Some(SkillInvocationPolicy::USER_ONLY),
        "the canonical invocation policy is parsed at discovery time"
    );
    assert_eq!(skill.skill_invocation(), SkillInvocationPolicy::USER_ONLY);
    assert_eq!(skill.logical_capability_name().as_deref(), Some("review"));
    assert!(
        skill.identity().contains("skills/review/SKILL.md"),
        "identity is path-based under skills/: {}",
        skill.identity()
    );
    // `commands/` is not part of the canonical model anymore: the fixture
    // ships only `skills/review/SKILL.md`.
    assert!(
        !_package.root.join("commands").exists(),
        "the fixture has no commands/ directory"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn absent_invoke_block_defaults_to_model_and_user_and_behaves_as_before() {
    let (root, _home, _package, resources) = stored_fixture("flow", flow_fixture());
    let skills: Vec<&Resource> = resources
        .iter()
        .filter(|resource| resource.capability.kind == CapabilityKind::AgentSkill)
        .collect();
    assert_eq!(skills.len(), 1);
    assert_eq!(
        skills[0].skill_policy, None,
        "no invoke block → canonical default, never re-attached as Some"
    );
    assert_eq!(
        skills[0].skill_invocation(),
        SkillInvocationPolicy::MODEL_AND_USER
    );
    fs::remove_dir_all(root).unwrap();
}

// --- 2. Route matrix ------------------------------------------------------

#[test]
fn codex_routes_every_combination_honestly() {
    // A. model+user → Native
    let (root, home, _package, r) =
        make_policy_package("codex-a", "commit", &default_body("commit"));
    let codex = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &codex);
    assert_eq!(
        codex.exposure_plan(&r).route,
        CompatibilityRoute::Native,
        "default Skill is a normal model-discoverable Skill on Codex"
    );
    fs::remove_dir_all(root).unwrap();

    // B. user-only → Native (explicit-only policy sidecar)
    let (root, home, _package, r) =
        make_policy_package("codex-b", "review", &user_only_body("review"));
    let codex = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &codex);
    assert_eq!(codex.exposure_plan(&r).route, CompatibilityRoute::Native);
    fs::remove_dir_all(root).unwrap();

    // C. model-only → Degraded (Codex cannot hide explicit `$skill`)
    let (root, home, _package, r) =
        make_policy_package("codex-c", "legacy", &model_only_body("legacy"));
    let codex = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &codex);
    assert_eq!(
        codex.exposure_plan(&r).route,
        CompatibilityRoute::Degraded,
        "user=false cannot be enforced on Codex — honest degradation, never invented Native"
    );
    fs::remove_dir_all(root).unwrap();

    // D. invalid → Unsupported, never silently projected
    let (root, home, _package, r) = make_policy_package("codex-d", "dead", &invalid_body("dead"));
    let codex = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &codex);
    assert_eq!(
        codex.exposure_plan(&r).route,
        CompatibilityRoute::Unsupported
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn opencode_routes_every_combination_natively() {
    let combinations = [
        (
            "default",
            default_body("commit"),
            CompatibilityRoute::Native,
        ),
        (
            "user-only",
            user_only_body("review"),
            CompatibilityRoute::Native,
        ),
        (
            "model-only",
            model_only_body("legacy"),
            CompatibilityRoute::Native,
        ),
        (
            "invalid",
            invalid_body("dead"),
            CompatibilityRoute::Unsupported,
        ),
    ];
    for (label, body, expected) in combinations {
        let (root, home, _package, r) =
            make_policy_package(&format!("opencode-{label}"), "test", &body);
        let opencode = OpenCodeIntegration::new(
            root.join("agents"),
            root.join("config/opencode.json"),
            home.clone(),
        );
        mark_setup(&home, &opencode);
        assert_eq!(
            opencode.exposure_plan(&r).route,
            expected,
            "OpenCode V2 preserves every combination natively ({label})"
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn antigravity_routes_every_combination_honestly() {
    // A. model+user → Native
    let (root, home, _package, r) = make_policy_package("agy-a", "commit", &default_body("commit"));
    let agy = AntigravityIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &agy);
    assert_eq!(agy.exposure_plan(&r).route, CompatibilityRoute::Native);
    fs::remove_dir_all(root).unwrap();

    // B. user-only → Adapted, degradation explicit (no model-hiding exists)
    let (root, home, _package, r) =
        make_policy_package("agy-b", "review", &user_only_body("review"));
    let agy = AntigravityIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &agy);
    let plan = agy.exposure_plan(&r);
    assert_eq!(
        plan.route,
        CompatibilityRoute::Adaptable,
        "Antigravity cannot hide a Skill from the model — Adapted, honestly"
    );
    assert!(
        plan.evidence
            .contains("invoke.model=false cannot be enforced"),
        "the degradation must be stated, never hidden: {}",
        plan.evidence
    );
    fs::remove_dir_all(root).unwrap();

    // C. model-only → Adapted
    let (root, home, _package, r) =
        make_policy_package("agy-c", "legacy", &model_only_body("legacy"));
    let agy = AntigravityIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &agy);
    assert_eq!(agy.exposure_plan(&r).route, CompatibilityRoute::Adaptable);
    fs::remove_dir_all(root).unwrap();

    // D. invalid → Unsupported
    let (root, home, _package, r) = make_policy_package("agy-d", "dead", &invalid_body("dead"));
    let agy = AntigravityIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &agy);
    assert_eq!(agy.exposure_plan(&r).route, CompatibilityRoute::Unsupported);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn claude_routes_every_combination_at_capability_level() {
    let (root, home, _package, r) =
        make_policy_package("claude-a", "commit", &default_body("commit"));
    let claude = ClaudeIntegration::new(root.join("claude"), home.clone());
    mark_setup(&home, &claude);
    assert_eq!(
        claude.exposure_plan(&r).route,
        CompatibilityRoute::Adaptable
    );
    fs::remove_dir_all(root).unwrap();

    let (root, home, _package, r) =
        make_policy_package("claude-b", "review", &user_only_body("review"));
    let claude = ClaudeIntegration::new(root.join("claude"), home.clone());
    mark_setup(&home, &claude);
    assert_eq!(
        claude.exposure_plan(&r).route,
        CompatibilityRoute::Adaptable
    );
    fs::remove_dir_all(root).unwrap();

    let (root, home, _package, r) = make_policy_package("claude-d", "dead", &invalid_body("dead"));
    let claude = ClaudeIntegration::new(root.join("claude"), home.clone());
    mark_setup(&home, &claude);
    assert_eq!(
        claude.exposure_plan(&r).route,
        CompatibilityRoute::Unsupported
    );
    fs::remove_dir_all(root).unwrap();
}

// --- 3. Physical representation -------------------------------------------

#[test]
fn codex_user_only_wrapper_carries_the_policy_sidecar_and_never_touches_store() {
    let (root, home, package, r) =
        make_policy_package("codex-physical", "review", &user_only_body("review"));
    let store_bytes = fs::read(package.root.join("skills/review/SKILL.md")).unwrap();
    let codex = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &codex);
    let receipt = codex
        .attach_receipt(&r)
        .unwrap()
        .expect("user-only Skill attaches on Codex");
    let ManagedArtifact::SymlinkReference { path, target } = &receipt.artifact else {
        panic!("expected a managed symlink reference");
    };
    assert_eq!(path.file_name().unwrap().to_str(), Some("flow:review"));
    assert_eq!(fs::read_link(path).unwrap(), *target);
    let wrapper = fs::read_to_string(target.join("SKILL.md")).unwrap();
    assert!(
        wrapper.starts_with("---\nname: flow:review\n"),
        "the wrapper carries the stable namespaced label: {wrapper}"
    );
    assert_eq!(
        fs::read_to_string(target.join("agents/openai.yaml")).unwrap(),
        "policy:\n  allow_implicit_invocation: false\n",
        "model=false is translated into Codex's own policy sidecar"
    );
    assert_eq!(
        fs::read(package.root.join("skills/review/SKILL.md")).unwrap(),
        store_bytes,
        "Store bytes are never rewritten"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn opencode_user_only_wrapper_carries_autoinvoke_metadata() {
    let (root, home, package, r) =
        make_policy_package("oc-physical", "review", &user_only_body("review"));
    let store_bytes = fs::read(package.root.join("skills/review/SKILL.md")).unwrap();
    let opencode = OpenCodeIntegration::new(
        root.join("agents"),
        root.join("config/opencode.json"),
        home.clone(),
    );
    mark_setup(&home, &opencode);
    let receipt = opencode
        .attach_receipt(&r)
        .unwrap()
        .expect("user-only Skill attaches on OpenCode");
    let ManagedArtifact::SymlinkReference { target, .. } = &receipt.artifact else {
        panic!("expected a managed symlink reference");
    };
    let wrapper = fs::read_to_string(target.join("SKILL.md")).unwrap();
    assert!(
        wrapper.contains("metadata:\n  opencode/autoinvoke: false\n"),
        "model=false is translated into OpenCode's own control: {wrapper}"
    );
    assert!(
        !wrapper.contains("slash: false"),
        "user invocation stays enabled for a user-only Skill"
    );
    assert_eq!(
        fs::read(package.root.join("skills/review/SKILL.md")).unwrap(),
        store_bytes,
        "the canonical bytes stay untouched"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn opencode_model_only_wrapper_carries_slash_false() {
    let (root, home, _package, r) =
        make_policy_package("oc-model-only", "legacy", &model_only_body("legacy"));
    let opencode = OpenCodeIntegration::new(
        root.join("agents"),
        root.join("config/opencode.json"),
        home.clone(),
    );
    mark_setup(&home, &opencode);
    let receipt = opencode
        .attach_receipt(&r)
        .unwrap()
        .expect("model-only Skill attaches on OpenCode");
    let ManagedArtifact::SymlinkReference { target, .. } = &receipt.artifact else {
        panic!("expected a managed symlink reference");
    };
    let wrapper = fs::read_to_string(target.join("SKILL.md")).unwrap();
    assert!(
        wrapper.contains("slash: false\n"),
        "user=false is translated into OpenCode's catalog-hiding field: {wrapper}"
    );
    assert!(
        !wrapper.contains("opencode/autoinvoke"),
        "model discovery stays enabled for a model-only Skill"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn claude_user_only_shim_carries_the_disable_model_marker() {
    let (root, home, package, r) =
        make_policy_package("claude-physical", "review", &user_only_body("review"));
    let store_bytes = fs::read(package.root.join("skills/review/SKILL.md")).unwrap();
    let claude = ClaudeIntegration::new(root.join("claude"), home.clone());
    mark_setup(&home, &claude);
    let receipt = claude
        .attach_receipt(&r)
        .unwrap()
        .expect("user-only Skill attaches on Claude");
    let ManagedArtifact::SymlinkReference { target, .. } = &receipt.artifact else {
        panic!("expected a managed symlink reference");
    };
    let shim_skill = fs::read_to_string(target.join("SKILL.md")).unwrap();
    assert!(
        shim_skill.contains("disable-model-invocation: true\n"),
        "model=false is translated into Claude's own marker: {shim_skill}"
    );
    assert_eq!(
        fs::read(package.root.join("skills/review/SKILL.md")).unwrap(),
        store_bytes,
        "the canonical bytes stay untouched"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn antigravity_user_only_wrapper_has_no_forced_policy() {
    let (root, home, _package, r) =
        make_policy_package("agy-physical", "review", &user_only_body("review"));
    let agy = AntigravityIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &agy);
    let plan = agy.exposure_plan(&r);
    let receipt = agy
        .attach_receipt(&r)
        .unwrap()
        .expect("user-only Skill attaches on Antigravity");
    let ManagedArtifact::SymlinkReference { target, .. } = &receipt.artifact else {
        panic!("expected a managed symlink reference");
    };
    let wrapper = fs::read_to_string(target.join("SKILL.md")).unwrap();
    assert!(
        !wrapper.contains("disable-model-invocation") && !target.join("agents").exists(),
        "Antigravity has no explicit-only mechanism — UZE must not invent one"
    );
    assert_eq!(plan.route, CompatibilityRoute::Adaptable);
    fs::remove_dir_all(root).unwrap();
}

// --- 4. Lifecycle + receipts ----------------------------------------------

#[test]
fn user_only_skill_lifecycle_attach_matched_detach_missing_on_codex() {
    let (root, home, _package, r) =
        make_policy_package("codex-life", "review", &user_only_body("review"));
    let codex = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &codex);
    let receipt = codex.attach_receipt(&r).unwrap().expect("attaches");
    assert_eq!(
        receipt.resource_identity.as_deref(),
        Some(r.identity().as_str()),
        "receipt identity stays the canonical Skill resource identity"
    );
    assert_eq!(
        codex.inspect_receipt(&receipt).state,
        AttachmentState::Matched
    );
    let detached = codex.detach_receipt(&receipt).unwrap();
    assert_eq!(detached.state, AttachmentState::Missing);
    assert_eq!(
        codex.inspect_receipt(&receipt).state,
        AttachmentState::Missing
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn user_only_skill_lifecycle_attach_matched_detach_missing_on_opencode() {
    let (root, home, _package, r) =
        make_policy_package("oc-life", "review", &user_only_body("review"));
    let opencode = OpenCodeIntegration::new(
        root.join("agents"),
        root.join("config/opencode.json"),
        home.clone(),
    );
    mark_setup(&home, &opencode);
    let receipt = opencode.attach_receipt(&r).unwrap().expect("attaches");
    assert_eq!(
        opencode.inspect_receipt(&receipt).state,
        AttachmentState::Matched
    );
    assert_eq!(
        opencode.detach_receipt(&receipt).unwrap().state,
        AttachmentState::Missing
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn invalid_policy_never_creates_a_receipt_anywhere() {
    for integration in ["claude", "codex", "opencode", "antigravity"] {
        let (root, home, _package, r) = make_policy_package(
            &format!("invalid-{integration}"),
            "dead",
            &invalid_body("dead"),
        );
        let receipt = match integration {
            "claude" => ClaudeIntegration::new(root.join("claude"), home.clone())
                .attach_receipt(&r)
                .unwrap(),
            "codex" => CodexIntegration::new(root.join("agents"), home.clone())
                .attach_receipt(&r)
                .unwrap(),
            "opencode" => OpenCodeIntegration::new(
                root.join("agents"),
                root.join("config/opencode.json"),
                home.clone(),
            )
            .attach_receipt(&r)
            .unwrap(),
            "antigravity" => AntigravityIntegration::new(root.join("agents"), home.clone())
                .attach_receipt(&r)
                .unwrap(),
            _ => unreachable!(),
        };
        assert!(
            receipt.is_none(),
            "a Skill nobody may invoke must never be projected on {integration}"
        );
        fs::remove_dir_all(root).unwrap();
    }
}

// --- 5. Package-level coverage is semantic-aware ---------------------------

#[test]
fn claude_generated_package_covers_a_user_only_skill_and_materializes_the_marker() {
    let (root, home, package, r) =
        make_policy_package("claude-envelope", "review", &user_only_body("review"));
    let claude = ClaudeIntegration::new(root.join("claude"), home.clone());
    let plan = claude
        .package_exposure_plan(&package, &[&r])
        .expect("generated route applies");
    assert!(
        plan.provided_resource_identities.contains(&r.identity()),
        "Claude preserves user-only semantics in the generated envelope (marker injection)"
    );
    // `republish_packages` materializes the generated envelope (the only
    // path that rebuilds derived artifact directories).
    claude.republish_packages(&[package]).unwrap();
    // The generated envelope materializes the marker file, not a symlink of
    // the raw canonical bytes.
    let generated_root = home.state_dir().join("attachments/claude/generated");
    let generated_skill = generated_root.join("flow/skills/review/SKILL.md");
    assert!(
        generated_skill.is_file(),
        "materialized SKILL.md expected at {}",
        generated_skill.display()
    );
    let content = fs::read_to_string(&generated_skill).unwrap();
    assert!(content.contains("disable-model-invocation: true\n"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn codex_generated_package_never_claims_a_model_only_skill() {
    let (root, home, package, r) =
        make_policy_package("codex-envelope", "legacy", &model_only_body("legacy"));
    let codex = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &codex);
    let plan = codex
        .package_exposure_plan(&package, &[&r])
        .expect("generated route applies via the Skill");
    assert!(
        !plan.provided_resource_identities.contains(&r.identity()),
        "Codex cannot preserve user=false in the envelope — never claim it"
    );
    let fallback = codex.exposure_plan(&r);
    assert_eq!(
        fallback.route,
        CompatibilityRoute::Degraded,
        "the capability-level fallback reports the honest degradation"
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn antigravity_generated_package_never_claims_a_user_only_skill() {
    let (root, home, package, r) =
        make_policy_package("agy-envelope", "review", &user_only_body("review"));
    let agy = AntigravityIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &agy);
    let plan = agy
        .package_exposure_plan(&package, &[&r])
        .expect("generated route applies via the Skill");
    assert!(
        !plan.provided_resource_identities.contains(&r.identity()),
        "Antigravity cannot hide the Skill from the model — never claim native coverage"
    );
    let fallback = agy.exposure_plan(&r);
    assert_eq!(fallback.route, CompatibilityRoute::Adaptable);
    fs::remove_dir_all(root).unwrap();
}

// --- 6. Compatibility: old fixtures unchanged ------------------------------

#[test]
fn default_skill_package_installs_cleanly_on_every_harness_as_before() {
    // The `flow` fixture ships a Skill with NO `invoke:` block — it must
    // install exactly as it always has, on every harness, with the same
    // routes and artifacts.
    let (root, home, _package, resources) = stored_fixture("compat", flow_fixture());
    let resource = resources[0].clone();
    let codex = CodexIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &codex);
    assert_eq!(
        codex.exposure_plan(&resource).route,
        CompatibilityRoute::Native
    );
    let opencode = OpenCodeIntegration::new(
        root.join("agents"),
        root.join("config/opencode.json"),
        home.clone(),
    );
    mark_setup(&home, &opencode);
    assert_eq!(
        opencode.exposure_plan(&resource).route,
        CompatibilityRoute::Native
    );
    let agy = AntigravityIntegration::new(root.join("agents"), home.clone());
    mark_setup(&home, &agy);
    assert_eq!(
        agy.exposure_plan(&resource).route,
        CompatibilityRoute::Native
    );
    let claude = ClaudeIntegration::new(root.join("claude"), home.clone());
    mark_setup(&home, &claude);
    assert_eq!(
        claude.exposure_plan(&resource).route,
        CompatibilityRoute::Adaptable
    );

    // And the default Skill wrapper on Codex carries NO policy sidecar.
    let receipt = codex.attach_receipt(&resource).unwrap().expect("attaches");
    let ManagedArtifact::SymlinkReference { target, .. } = &receipt.artifact else {
        panic!("expected a managed symlink reference");
    };
    assert!(
        !target.join("agents/openai.yaml").exists(),
        "no invocation policy was declared → no policy sidecar, exactly as before"
    );
    fs::remove_dir_all(root).unwrap();
}

// --- 7. Shared-root: one canonical Skill, one deterministic entry ----------

/// A fake `codex` that answers every plugin CLI call successfully (the
/// generated-envelope `attach_package` shells out via `Command::new`).
#[cfg(unix)]
fn fake_codex_bin_dir(root: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let dir = root.join("fake-bin");
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("codex");
    fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions).unwrap();
    dir
}

/// Serializes tests that mutate the process-global `PATH`
/// (`with_fake_codex`) against tests that spawn the real `codex`.
fn lock_path() -> std::sync::MutexGuard<'static, ()> {
    static PATH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    PATH_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(unix)]
fn with_fake_codex(root: &Path, f: impl FnOnce()) {
    let _guard = lock_path();
    let fake_bin = fake_codex_bin_dir(root);
    let original_path = std::env::var("PATH").unwrap();
    unsafe {
        std::env::set_var("PATH", format!("{}:{}", fake_bin.display(), original_path));
    }
    f();
    unsafe {
        std::env::set_var("PATH", original_path);
    }
}

#[test]
#[cfg(unix)]
fn codex_and_opencode_reuse_one_physical_entry_for_a_default_skill() {
    with_fake_codex(&temp("shared-default"), || {
        let root = temp("shared-default");
        let agents_home = root.join("agents-home");
        let uze_home = UzeHome::at(root.join("uze-home"));
        let application = UzeApplication::new_with_runner(
            uze_home.clone(),
            vec![
                Box::new(always_present(CodexIntegration::new(
                    agents_home.clone(),
                    uze_home.clone(),
                ))),
                Box::new(always_present(OpenCodeIntegration::new(
                    agents_home.clone(),
                    root.join("opencode-config.json"),
                    uze_home.clone(),
                ))),
            ],
            Box::new(NoopProcessRunner),
        );
        application
            .add_plugin(
                PackageSource::local(flow_fixture()),
                &uze::trust::AlwaysTrust,
            )
            .expect("a default Skill installs cleanly with both harnesses");
        let entry = agents_home.join("skills/flow:commit");
        assert!(entry.is_symlink());
        let target = fs::read_link(&entry).unwrap();
        assert!(
            target.is_dir() && target.join("SKILL.md").exists(),
            "one physical entry owned by the first attacher, reused by both"
        );
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
#[cfg(unix)]
fn model_only_skill_shared_root_detects_cross_integration_policy_loss() {
    // A model-only canonical Skill is NOT covered by Codex's generated
    // package envelope (user=false cannot be enforced there), so Codex
    // attaches it capability-level into `~/.agents/skills/flow:legacy`.
    // OpenCode then REUSES that shared entry but needs its own `slash:
    // false` encoding; silently reusing Codex's wrapper would drop the
    // policy, so the second attacher must fail deterministically with a
    // ProjectionConflict — never a silent semantic degradation (ADR-030
    // §25).
    with_fake_codex(&temp("shared-model-only"), || {
        let root = temp("shared-model-only");
        let agents_home = root.join("agents-home");
        let uze_home = UzeHome::at(root.join("uze-home"));
        let application = UzeApplication::new_with_runner(
            uze_home.clone(),
            vec![
                Box::new(always_present(CodexIntegration::new(
                    agents_home.clone(),
                    uze_home.clone(),
                ))),
                Box::new(always_present(OpenCodeIntegration::new(
                    agents_home.clone(),
                    root.join("opencode-config.json"),
                    uze_home.clone(),
                ))),
            ],
            Box::new(NoopProcessRunner),
        );
        let fixture_root = root.join("fixture");
        fs::create_dir_all(fixture_root.join("skills/legacy")).unwrap();
        fs::write(
            fixture_root.join("skills/legacy/SKILL.md"),
            model_only_body("legacy"),
        )
        .unwrap();
        fs::write(
            fixture_root.join("plugin.json"),
            r#"{"name":"flow","version":"1.0.0","description":"model-only fixture"}"#,
        )
        .unwrap();

        let result = application.add_plugin(
            PackageSource::local(&fixture_root),
            &uze::trust::AlwaysTrust,
        );
        match result {
            Err(uze::UzeError::ProjectionConflict(details)) => {
                assert!(
                    details.entry.ends_with("skills/flow:legacy"),
                    "the conflict is about the shared physical entry: {}",
                    details.entry.display()
                );
                assert!(
                    details.requested_integration == "opencode"
                        || details.existing_integration == "opencode",
                    "OpenCode is the integration whose invocation encoding is at stake"
                );
            }
            Err(other) => panic!("expected a deterministic ProjectionConflict, got {other:#?}"),
            Ok(_) => panic!(
                "reusing an incompatible wrapper must not silently drop the invocation policy"
            ),
        }
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
#[cfg(unix)]
fn user_only_skill_installs_cleanly_with_codex_and_opencode() {
    // A user-only Skill is covered by Codex's generated package envelope
    // (policy sidecar) and attached by OpenCode as a wrapper with
    // `metadata.opencode/autoinvoke: false` — both harnesses get their
    // native encoding, and the shared root carries exactly one entry.
    with_fake_codex(&temp("shared-user-only"), || {
        let root = temp("shared-user-only");
        let agents_home = root.join("agents-home");
        let uze_home = UzeHome::at(root.join("uze-home"));
        let application = UzeApplication::new_with_runner(
            uze_home.clone(),
            vec![
                Box::new(always_present(CodexIntegration::new(
                    agents_home.clone(),
                    uze_home.clone(),
                ))),
                Box::new(always_present(OpenCodeIntegration::new(
                    agents_home.clone(),
                    root.join("opencode-config.json"),
                    uze_home.clone(),
                ))),
            ],
            Box::new(NoopProcessRunner),
        );
        let fixture_root = root.join("fixture");
        fs::create_dir_all(fixture_root.join("skills/review")).unwrap();
        fs::write(
            fixture_root.join("skills/review/SKILL.md"),
            user_only_body("review"),
        )
        .unwrap();
        fs::write(
            fixture_root.join("plugin.json"),
            r#"{"name":"flow","version":"1.0.0","description":"user-only fixture"}"#,
        )
        .unwrap();

        application
            .add_plugin(
                PackageSource::local(&fixture_root),
                &uze::trust::AlwaysTrust,
            )
            .expect("each harness gets its own native encoding for a user-only Skill");
        let entry = agents_home.join("skills/flow:review");
        assert!(entry.is_symlink(), "OpenCode projects the shared entry");
        let target = fs::read_link(&entry).unwrap();
        let wrapper = fs::read_to_string(target.join("SKILL.md")).unwrap();
        assert!(
            wrapper.contains("opencode/autoinvoke: false"),
            "the shared entry carries OpenCode's invocation encoding"
        );
        fs::remove_dir_all(root).unwrap();
    });
}

// --- 8. Real Codex dogfood (zero model calls) ------------------------------

/// Runs `codex debug prompt-input` against an isolated HOME.
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
/// delivery: a canonical user-only Skill is attached through
/// `CodexIntegration` into an isolated `~/.agents/skills` (with its
/// `agents/openai.yaml` policy), a default Skill sits beside it, and the
/// model-visible prompt is rendered with `codex debug prompt-input`.
/// Expected: the default Skill is offered to the model, the user-only Skill
/// is not. A malformed-metadata control restores the listing, proving the
/// exclusion is caused by the policy file being genuinely read. Skips when
/// `codex` is not on PATH, so CI stays deterministic.
#[test]
fn real_codex_dogfood_user_only_skill_is_hidden_from_the_model() {
    let probe = std::process::Command::new("codex")
        .arg("--version")
        .output();
    if probe.is_err() || probe.as_ref().is_ok_and(|o| !o.status.success()) {
        eprintln!("codex not available on PATH; skipping real-Codex dogfood");
        return;
    }
    let _guard = lock_path();
    let root = temp("real-codex-dogfood");
    let uze_home = UzeHome::at(root.join("uze"));
    let store = UzeStore::new(uze_home.clone());
    let package = install(&store, workflow_fixture()).unwrap();
    let environment = UzeEngine::new(store)
        .compose(std::slice::from_ref(&package.id))
        .unwrap();
    let resource = &environment.resources[0];
    assert_eq!(
        resource.skill_invocation(),
        SkillInvocationPolicy::USER_ONLY
    );

    let codex_home = root.join("codex-home");
    let agents_home = codex_home.join(".agents");
    let codex = CodexIntegration::new(agents_home.clone(), uze_home.clone());
    mark_setup(&uze_home, &codex);
    let receipt = codex.attach_receipt(resource).unwrap().expect("attaches");
    let ManagedArtifact::SymlinkReference { target, .. } = &receipt.artifact else {
        panic!("expected symlink artifact");
    };
    assert_eq!(
        fs::read_to_string(target.join("agents/openai.yaml")).unwrap(),
        "policy:\n  allow_implicit_invocation: false\n"
    );

    // A default Skill beside it stays implicitly discoverable.
    fs::create_dir_all(agents_home.join("skills/normal")).unwrap();
    fs::write(
        agents_home.join("skills/normal/SKILL.md"),
        "---\nname: normal\ndescription: Run normal tasks N.\n---\n\nNormal body.\n",
    )
    .unwrap();

    let before_store_bytes = fs::read(package.root.join("skills/review/SKILL.md")).unwrap();
    let valid = run_codex_prompt_input(&codex_home).expect("codex prompt-input runs");
    assert!(
        valid.contains("normal: Run normal tasks N"),
        "a default Skill stays implicitly discoverable"
    );
    assert!(
        !valid.contains("workflow:review") && !valid.contains("review: Review code"),
        "the user-only Skill must not be offered to the model"
    );

    // Control: malformed policy restores the listing (the exclusion is
    // caused by the policy file being read).
    fs::write(target.join("agents/openai.yaml"), "policy: [broken yaml\n").unwrap();
    let malformed = run_codex_prompt_input(&codex_home).expect("codex prompt-input runs");
    assert!(
        malformed.contains("workflow:review"),
        "control: malformed policy metadata must not suppress the user-only Skill"
    );

    assert_eq!(
        fs::read(package.root.join("skills/review/SKILL.md")).unwrap(),
        before_store_bytes,
        "the canonical Store bytes are untouched throughout"
    );
    fs::remove_dir_all(root).unwrap();
}

// --- 9. No Command capability anywhere ------------------------------------

#[test]
fn the_canonical_kind_set_has_no_command() {
    let kinds: BTreeSet<CapabilityKind> = [
        CapabilityKind::Instruction,
        CapabilityKind::AgentSkill,
        CapabilityKind::Mcp,
        CapabilityKind::Agent,
        CapabilityKind::Hook,
        CapabilityKind::Policy,
    ]
    .into_iter()
    .collect();
    assert_eq!(kinds.len(), 6);
    // The single Skill-family kind is exactly AgentSkill.
    assert!(kinds.contains(&CapabilityKind::AgentSkill));
}
