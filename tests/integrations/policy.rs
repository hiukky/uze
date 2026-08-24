//! Shared invocation-policy harness helpers + canonical policy semantics
//! (ADR-030): the single Skill-capability model, discovery at the Store
//! boundary, invalid-policy rejection, and cross-harness default behavior.
//!
//! The per-harness semantic tests live in `harness/{claude,codex,opencode,
//! antigravity}.rs` (migrated verbatim from the former
//! `tests/skill_invocation_conformance.rs`).

use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) use std::fs;
pub(crate) use std::path::{Path, PathBuf};

use uze::capability::CapabilityKind;
pub(crate) use uze::integrations::{
    antigravity::AntigravityIntegration, claude::ClaudeIntegration, codex::CodexIntegration,
    opencode::OpenCodeIntegration,
};
pub(crate) use uze::{
    PackageSource, Resource, SkillInvocationPolicy, UzeEngine, UzeHome, UzeStore,
    integration::{AttachmentState, IntegrationPort, ManagedArtifact},
    router::CompatibilityRoute,
    state,
    store::StoredPackage,
};
pub(crate) fn temp(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "uze-invocation-conformance-{label}-{}-{nonce}",
        std::process::id()
    ))
}

pub(crate) fn workflow_fixture() -> PathBuf {
    uze_testkit::fixtures::canonical("workflow")
}

pub(crate) fn flow_fixture() -> PathBuf {
    uze_testkit::fixtures::canonical("flow")
}

pub(crate) fn install(store: &UzeStore, path: impl Into<PathBuf>) -> uze::Result<StoredPackage> {
    store.ingest(&uze::acquisition::acquire(&PackageSource::local(path))?)
}

pub(crate) fn mark_setup(home: &UzeHome, integration: &dyn IntegrationPort) {
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

pub(crate) fn stored_fixture(
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
pub(crate) fn make_policy_package(
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

pub(crate) fn user_only_body(name: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: Explicit user action\ninvoke:\n  model: false\n  user: true\n---\n\nExplicit body.\n"
    )
}

pub(crate) fn model_only_body(name: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: Background knowledge\ninvoke:\n  model: true\n  user: false\n---\n\nBody.\n"
    )
}

pub(crate) fn default_body(name: &str) -> String {
    format!("---\nname: {name}\ndescription: Interactive skill\n---\n\nBody.\n")
}

pub(crate) fn invalid_body(name: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: Uninvokable\ninvoke:\n  model: false\n  user: false\n---\n\nBody.\n"
    )
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
