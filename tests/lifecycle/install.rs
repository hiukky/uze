//! One external plugin -> one store installation -> one effective environment
//! -> peer and adversarial delivery plans. See ADR-013.

use std::{fs, path::PathBuf};

use uze::{
    UzeEngine, UzeHome, UzeStore, capability::CapabilityKind, exposure::ExposureMechanism,
    integration::IntegrationPort, router::CompatibilityRoute,
};

use uze::integrations::{
    claude::ClaudeIntegration, codex::CodexIntegration, opencode::OpenCodeIntegration,
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

fn fixture() -> PathBuf {
    uze_testkit::fixtures::foreign("codex", "native-plugin")
}
fn temp(label: &str) -> PathBuf {
    uze_testkit::temp::scratch(label)
}
fn installed(home: &UzeHome) -> (uze::StoredPackage, uze::EffectiveEnvironment) {
    let store = UzeStore::new(home.clone());
    let package = install(&store, fixture()).unwrap();
    let environment = UzeEngine::new(store)
        .compose(std::slice::from_ref(&package.id))
        .unwrap();
    (package, environment)
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

#[test]
fn one_plugin_install_is_planned_once_for_native_and_decomposed_harnesses() {
    let root = temp("shared-store");
    let home = UzeHome::at(&root);
    let (package, environment) = installed(&home);
    assert_eq!(package.id.as_str(), "uze-plugin-first-conformance@local");
    assert_eq!(UzeStore::new(home.clone()).registration_count().unwrap(), 1);
    assert_eq!(environment.resources.len(), 2);
    assert!(
        environment
            .resources
            .iter()
            .any(|r| r.capability.kind == CapabilityKind::AgentSkill)
    );
    assert!(
        environment
            .resources
            .iter()
            .any(|r| r.capability.kind == CapabilityKind::Mcp)
    );
    assert!(
        environment
            .resources
            .iter()
            .all(|r| r.package_root() == Some(package.root.as_path()))
    );
    assert!(
        package.root.join(".codex-plugin/plugin.json").is_file(),
        "original native envelope is preserved"
    );
    assert!(
        package.root.join(".mcp.json").is_file(),
        "original native MCP document is preserved"
    );
    assert!(
        !home
            .store_dir()
            .join(".agents/plugins/marketplace.json")
            .exists(),
        "the Store publishes no harness-owned view of its own accord"
    );

    let claude = ClaudeIntegration::new(root.join("claude"), home.clone());
    let codex = CodexIntegration::new(root.join("agents"), home.clone());
    let opencode = OpenCodeIntegration::new(
        root.join("agents"),
        root.join("config/opencode/opencode.json"),
        home.clone(),
    );
    mark_setup(&home, &claude);
    mark_setup(&home, &opencode);

    let resources: Vec<_> = environment.resources.iter().collect();
    let codex_package = codex
        .package_exposure_plan(&package, &resources)
        .expect("Codex consumes source-provided native envelope");
    assert_eq!(codex_package.route, CompatibilityRoute::Native);
    assert_eq!(codex_package.provided_resource_identities.len(), 2);
    assert!(
        resources
            .iter()
            .all(|resource| codex_package.provides(resource)),
        "no capability is also individually attached for Codex"
    );

    // Claude has no envelope of its own (only Codex's `.codex-plugin/
    // plugin.json` is present), but ADR-013 §3 makes
    // envelope-less packages eligible for a UZE-GENERATED native envelope
    // rather than falling straight to capability decomposition: the
    // package's Skill lives under a conventional `skills/` directory and
    // its MCP server is declared in `.mcp.json`, so both are safely
    // representable. This is a deliberate, accepted behavior change from
    // this fixture's pre-generation expectation.
    let claude_package = claude
        .package_exposure_plan(&package, &resources)
        .expect("no Claude envelope, but UZE can safely generate one");
    assert_eq!(claude_package.route, CompatibilityRoute::Native);
    assert_eq!(
        claude_package.provided_resource_identities.len(),
        2,
        "both the Skill and the MCP server are safely representable"
    );
    assert!(
        resources
            .iter()
            .all(|resource| claude_package.provides(resource)),
        "no capability is also individually attached for Claude once generated"
    );

    // Per-resource `exposure_plan` is unaffected by package-level
    // generation — it is Application orchestration's job to skip
    // individually attaching a resource the package plan already covers,
    // not `exposure_plan`'s. The capability-level fallback mechanism must
    // still exist and still be correct on its own terms.
    let claude_routes: Vec<_> = resources.iter().map(|r| claude.exposure_plan(r)).collect();
    assert!(
        claude_routes
            .iter()
            .all(|plan| plan.route == CompatibilityRoute::Adaptable)
    );
    assert!(matches!(
        claude_routes
            .iter()
            .find(|p| matches!(
                p.mechanism,
                ExposureMechanism::ManagedUserScopeReference { .. }
            ))
            .unwrap()
            .mechanism,
        ExposureMechanism::ManagedUserScopeReference { .. }
    ));
    assert!(
        claude_routes
            .iter()
            .any(|p| matches!(p.mechanism, ExposureMechanism::ManagedVendorConfig { .. }))
    );

    assert!(
        opencode
            .package_exposure_plan(&package, &resources)
            .is_none(),
        "OpenCode decomposes the unknown envelope"
    );
    let skill = resources
        .iter()
        .find(|r| r.capability.kind == CapabilityKind::AgentSkill)
        .unwrap();
    let mcp = resources
        .iter()
        .find(|r| r.capability.kind == CapabilityKind::Mcp)
        .unwrap();
    assert_eq!(
        opencode.exposure_plan(skill).route,
        CompatibilityRoute::Native
    );
    assert!(matches!(
        opencode.exposure_plan(skill).mechanism,
        ExposureMechanism::ManagedUserScopeReference { .. }
    ));
    assert_eq!(
        opencode.exposure_plan(mcp).route,
        CompatibilityRoute::Native
    );
    assert!(matches!(
        opencode.exposure_plan(mcp).mechanism,
        ExposureMechanism::ManagedVendorConfig { .. }
    ));

    // Prove the adversarial delivery writes one native OpenCode config entry
    // without materializing a proprietary OpenCode plugin.
    opencode.attach(skill).unwrap();
    opencode.attach(mcp).unwrap();
    let config: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("config/opencode/opencode.json")).unwrap())
            .unwrap();
    // MCP stays fully qualified (package-logical) since its physical name
    // is not user-visible; Skills get their stable namespaced invocation
    // label (ADR-026) — the path-derived OpenCode V2 slash ID, user-visible
    // like Claude's.
    assert_eq!(
        config["mcp"]["servers"]["uze-plugin-first-conformance@local-conformance"]["type"],
        "local"
    );
    assert_eq!(
        config["mcp"]["servers"]["uze-plugin-first-conformance@local-conformance"]["command"][0],
        "__UZE_MCP_FIXTURE_BINARY__"
    );
    assert!(
        root.join("agents/skills/uze-plugin-first-conformance:uze-plugin-first")
            .is_symlink(),
        "OpenCode V2 should expose the skill under its stable namespaced label"
    );
    fs::remove_dir_all(root).unwrap();
}
