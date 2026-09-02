//! North Star v0 fixture (native-projection milestone, spec §10): one
//! vendor-neutral canonical package — `tests/_fixtures/canonical/flow/`, a
//! `plugin.json` plus `skills/commit/SKILL.md`, absolutely no vendor
//! manifests (`.claude-plugin/`, `.codex-plugin/`, `mcp_config.json`,
//! `.opencode/`) — proving the product thesis empirically:
//!
//!   A plugin author should not need to know every harness format.
//!   UZE owns porting and delivery.
//!
//! Claude was the first proven tracer bullet (ADR-013, Generated Native
//! Package) and Codex and Antigravity now close the same gap (ADR-013,
//! Generated Native Package/Plugin): the fixture's single Skill, with no
//! vendor envelope of its own, becomes one native Claude plugin, one native
//! Codex plugin, and one native Antigravity plugin (its canonical
//! `plugin.json` IS the vendor manifest) — the latter two via their own
//! delivery paths, never a Store mutation.
//! OpenCode has no package concept at all and is expected to decompose
//! unconditionally into Native Capability — asymmetry across harnesses is
//! expected there, not a defect (spec §9): it is the one harness with no
//! native package/extension format to synthesize into in the first place.

use std::path::PathBuf;

use uze_core::{UzeEngine, UzeHome, UzeStore, capability::CapabilityKind, exposure::ExposureMechanism, integration::IntegrationPort, router::CompatibilityRoute};

use uze_integrations::{
    antigravity::AntigravityIntegration, claude::ClaudeIntegration, codex::CodexIntegration,
    opencode::OpenCodeIntegration,
};

fn install(
    store: &UzeStore,
    path: impl Into<std::path::PathBuf>,
) -> uze_core::Result<uze_core::StoredPackage> {
    store.ingest(&uze_core::acquisition::acquire(&uze_core::PackageSource::local(
        path,
    ))?)
}

fn fixture() -> PathBuf {
    uze_testkit::fixtures::canonical("flow")
}

fn mark_setup(home: &UzeHome, integration: &dyn IntegrationPort) {
    uze_core::state::record(
        home,
        uze_core::state::IntegrationRecord {
            harness: integration.id().to_owned(),
            version: None,
            strategy: "test".to_owned(),
            installed: true,
        },
    )
    .unwrap();
}

fn temp(label: &str) -> PathBuf {
    uze_testkit::temp::scratch(label)
}

/// The fixture ships literally none of the harnesses' own enhanced envelope
/// formats — the entire point of the North Star fixture. (Antigravity
/// differs: its envelope IS the canonical `plugin.json`, which is why no
/// Antigravity-specific file is asserted absent beyond `mcp_config.json`.)
#[test]
fn fixture_ships_no_vendor_manifest_of_any_kind() {
    let root = fixture();
    assert!(!root.join(".claude-plugin").exists());
    assert!(!root.join(".codex-plugin").exists());
    assert!(!root.join(".opencode").exists());
    // Antigravity-specific: the canonical plugin.json doubles as the vendor
    // manifest, and no other Antigravity file may be required (no
    // mcp_config.json, no .agents/, no hooks.json/rules/).
    assert!(!root.join("mcp_config.json").exists());
    assert!(root.join("plugin.json").is_file());
    assert!(root.join("skills/commit/SKILL.md").is_file());
}

#[test]
fn one_canonical_package_reaches_every_harness_through_its_most_native_safe_representation() {
    let root = temp("cross-harness");
    let home = UzeHome::at(&root);
    let store = UzeStore::new(home.clone());
    let package = install(&store, fixture()).unwrap();
    assert_eq!(package.id.as_str(), "flow");

    let environment = UzeEngine::new(store)
        .compose(std::slice::from_ref(&package.id))
        .unwrap();
    assert_eq!(environment.resources.len(), 1, "exactly the commit Skill");
    let resources: Vec<_> = environment.resources.iter().collect();
    let commit_skill = resources[0];
    assert_eq!(commit_skill.capability.kind, CapabilityKind::AgentSkill);

    // --- Claude: NATIVE PACKAGE (Generated) -----------------------------
    let claude = ClaudeIntegration::new(root.join("claude-home"), home.clone());
    mark_setup(&home, &claude);
    let claude_plan = claude
        .package_exposure_plan(&package, &resources)
        .expect("Claude must synthesize a native envelope for an eligible, envelope-less package");
    assert_eq!(claude_plan.route, CompatibilityRoute::Native);
    assert!(
        claude_plan.provides(commit_skill),
        "the commit Skill must be exactly covered by the generated package"
    );

    // --- Codex: NATIVE PACKAGE (Generated) -------------------------------
    let codex = CodexIntegration::new(root.join("agents-home"), home.clone());
    mark_setup(&home, &codex);
    let codex_plan = codex
        .package_exposure_plan(&package, &resources)
        .expect("Codex must synthesize a native envelope for an eligible, envelope-less package");
    assert_eq!(codex_plan.route, CompatibilityRoute::Native);
    assert!(
        codex_plan.provides(commit_skill),
        "the commit Skill must be exactly covered by the generated package"
    );

    // --- Antigravity: NATIVE PLUGIN (canonical manifest, no envelope) -----
    // The canonical plugin.json IS a valid Antigravity plugin manifest, so
    // the North Star package takes the explicit route straight from the
    // Store — no synthesized envelope of any kind is needed for Skills.
    let antigravity = AntigravityIntegration::new(root.join("agents-home"), home.clone());
    mark_setup(&home, &antigravity);
    let antigravity_plan = antigravity
        .package_exposure_plan(&package, &resources)
        .expect("the canonical plugin.json is itself the native Antigravity plugin manifest");
    assert_eq!(antigravity_plan.route, CompatibilityRoute::Native);
    assert!(
        antigravity_plan.provides(commit_skill),
        "the commit Skill must be exactly covered by the canonical package installed as an Antigravity plugin"
    );

    // --- OpenCode: no package concept at all, by design (spec §9) -------
    let opencode = OpenCodeIntegration::new(
        root.join("agents-home"),
        root.join("opencode-config/opencode.json"),
        home.clone(),
    );
    mark_setup(&home, &opencode);
    assert!(
        opencode
            .package_exposure_plan(&package, &resources)
            .is_none(),
        "OpenCode has no package-level native concept; a fake one must never be fabricated"
    );
    let opencode_plan = opencode.exposure_plan(commit_skill);
    assert_eq!(opencode_plan.route, CompatibilityRoute::Native);
    assert!(matches!(
        opencode_plan.mechanism,
        ExposureMechanism::ManagedUserScopeReference { .. }
    ));

    let _ = std::fs::remove_dir_all(root);
}
