//! One external plugin -> one store installation -> one effective environment
//! -> peer and adversarial delivery plans. See ADR-008.

use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

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
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("e2e/fixtures/plugin-first-conformance")
}
fn temp(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "uze-plugin-first-{label}-{}-{nonce}",
        std::process::id()
    ))
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
    assert_eq!(package.id.as_str(), "uze-plugin-first-conformance");
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

    assert!(
        claude.package_exposure_plan(&package, &resources).is_none(),
        "Agent Plugin/Codex envelope is not claimed native for Claude"
    );
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
        CompatibilityRoute::Adaptable
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
    // No "uze-" collision-avoidance prefix any more (it never participated
    // in ownership). MCP stays fully qualified (package-logical) since its
    // physical name is not user-visible; Skills now try bare logical first
    // then qualified fallback — OpenCode V2's slash ID is path-derived, like
    // Claude, so short names are user-visible there as well.
    assert_eq!(
        config["mcp"]["uze-plugin-first-conformance-conformance"]["type"],
        "local"
    );
    assert_eq!(
        config["mcp"]["uze-plugin-first-conformance-conformance"]["command"][0],
        "__UZE_MCP_FIXTURE_BINARY__"
    );
    assert!(
        root.join("agents/skills/uze-plugin-first").is_symlink(),
        "OpenCode V2 should expose the skill as bare logical first"
    );
    fs::remove_dir_all(root).unwrap();
}
