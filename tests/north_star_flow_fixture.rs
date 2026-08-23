//! North Star v0 fixture (native-projection milestone, spec §10): one
//! vendor-neutral canonical package — `tests/fixtures/packages/flow/`, a
//! `plugin.json` plus `skills/commit/SKILL.md`, absolutely no vendor
//! manifests (`.claude-plugin/`, `.codex-plugin/`, `gemini-extension.json`,
//! `.opencode/`) — proving the product thesis empirically:
//!
//!   A plugin author should not need to know every harness format.
//!   UZE owns porting and delivery.
//!
//! Claude is the proven tracer bullet (ADR-020, Generated Native Package):
//! the fixture's single Skill, with no envelope of its own, still becomes
//! one native Claude plugin via a UZE-synthesized envelope. Codex and
//! Gemini generated-native delivery are NOT_IMPLEMENTED in this milestone
//! (see the final report) — both correctly fall back to Native Capability
//! (direct Skill decomposition), never a fabricated package. OpenCode has
//! no package concept at all and is expected to decompose unconditionally
//! — asymmetry across harnesses is expected, not a defect (spec §9).

use std::path::PathBuf;

use uze::{
    UzeEngine, UzeHome, UzeStore, capability::CapabilityKind, exposure::ExposureMechanism,
    integration::IntegrationPort, router::CompatibilityRoute,
};

use uze::integrations::{
    claude::ClaudeIntegration, codex::CodexIntegration, gemini::GeminiIntegration,
    opencode::OpenCodeIntegration,
};

fn install(
    store: &UzeStore,
    path: impl Into<std::path::PathBuf>,
) -> uze::Result<uze::StoredPackage> {
    store.ingest(&uze::acquisition::acquire(&uze::PackageSource::local(
        path,
    ))?)
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/packages/flow")
}

fn mark_setup(home: &UzeHome, integration: &dyn IntegrationPort) {
    uze::state::record(
        home,
        uze::state::IntegrationRecord {
            harness: integration.id().to_owned(),
            version: None,
            strategy: "test".to_owned(),
            installed: true,
        },
    )
    .unwrap();
}

fn temp(label: &str) -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "uze-north-star-{label}-{}-{nonce}",
        std::process::id()
    ))
}

/// The fixture ships literally none of the four harnesses' own envelope
/// formats — the entire point of the North Star fixture.
#[test]
fn fixture_ships_no_vendor_manifest_of_any_kind() {
    let root = fixture();
    assert!(!root.join(".claude-plugin").exists());
    assert!(!root.join(".codex-plugin").exists());
    assert!(!root.join("gemini-extension.json").exists());
    assert!(!root.join(".opencode").exists());
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

    // --- Codex: NOT_IMPLEMENTED this milestone → Native Capability ------
    // No `.codex-plugin/plugin.json` and Codex generated-native delivery
    // is out of scope for this milestone (spec §15 Phase C) — Codex must
    // still deliver the Skill, just at the capability level, never
    // silently drop it.
    let codex = CodexIntegration::new(root.join("agents-home"), home.clone());
    mark_setup(&home, &codex);
    assert!(
        codex.package_exposure_plan(&package, &resources).is_none(),
        "Codex generated-native delivery is NOT_IMPLEMENTED this milestone"
    );
    let codex_plan = codex.exposure_plan(commit_skill);
    assert_ne!(codex_plan.route, CompatibilityRoute::Unsupported);
    assert!(matches!(
        codex_plan.mechanism,
        ExposureMechanism::ManagedUserScopeReference { .. }
    ));

    // --- Gemini: NOT_IMPLEMENTED this milestone → Native Capability -----
    let gemini = GeminiIntegration::new(root.join("agents-home"), home.clone());
    mark_setup(&home, &gemini);
    assert!(
        gemini.package_exposure_plan(&package, &resources).is_none(),
        "Gemini generated-native delivery is NOT_IMPLEMENTED this milestone"
    );
    let gemini_plan = gemini.exposure_plan(commit_skill);
    assert_ne!(gemini_plan.route, CompatibilityRoute::Unsupported);

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
