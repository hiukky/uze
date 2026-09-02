//! Canonical Agent capability routes remain explicit across every harness.

use uze_core::{
    capability::{Capability, CapabilityKind, Representation},
    home::UzeHome,
    integration::IntegrationPort,
    project::Resource,
    router::CompatibilityRoute,
    store::PackageId,
};
use uze_integrations::{
    antigravity::AntigravityIntegration, claude::ClaudeIntegration, codex::CodexIntegration,
    opencode::OpenCodeIntegration,
};

fn agent(root: &std::path::Path) -> Resource {
    let package_root = root.join("store/flow");
    let id = PackageId::from_plugin_name("flow", &package_root.join("plugin.json")).unwrap();
    Resource::from_package(
        id,
        package_root.clone(),
        Capability {
            kind: CapabilityKind::Agent,
            representation: Representation::Standard,
            path: package_root.join("agents/reviewer.md"),
            payload: b"---\nname: reviewer\n---\nReview.\n".to_vec(),
        },
    )
}

#[test]
fn canonical_agent_routes_natively_for_every_harness() {
    let root = uze_testkit::temp::scratch("agent-routes");
    let home = UzeHome::at(root.join("uze"));
    let resource = agent(&root);
    let claude = ClaudeIntegration::new(root.join("claude"), home.clone());
    let codex = CodexIntegration::new(root.join("agents"), home.clone());
    let opencode = OpenCodeIntegration::new(
        root.join("agents"),
        root.join("config/opencode.json"),
        home.clone(),
    );
    let antigravity = AntigravityIntegration::new(root.join("agents"), home);

    assert_eq!(
        claude.exposure_plan(&resource).route,
        CompatibilityRoute::Native
    );
    assert_eq!(
        opencode.exposure_plan(&resource).route,
        CompatibilityRoute::Native
    );
    assert_eq!(
        antigravity.exposure_plan(&resource).route,
        CompatibilityRoute::Native
    );
    assert_eq!(
        codex.exposure_plan(&resource).route,
        CompatibilityRoute::Native
    );
}

#[test]
fn codex_generates_the_documented_custom_agent_toml_before_exposure() {
    let root = uze_testkit::temp::scratch("codex-agent");
    let home = UzeHome::at(root.join("uze"));
    let codex = CodexIntegration::new(root.join("home/.agents"), home);
    let resource = agent(&root);

    let receipt = codex
        .attach_receipt(&resource)
        .expect("Codex agent attachment succeeds")
        .expect("Codex agent attachment has a receipt");
    let uze_core::integration::ManagedArtifact::SymlinkReference { path, target } =
        receipt.artifact
    else {
        panic!("Codex native agent is a receipt-owned reference");
    };
    assert_eq!(path, root.join("home/.codex/agents/reviewer.toml"));
    assert!(path.is_symlink());
    let toml = std::fs::read_to_string(target).expect("generated native TOML exists");
    assert!(toml.contains("name = \"reviewer\""));
    assert!(toml.contains("description = \"Portable UZE custom agent.\""));
    assert!(toml.contains("developer_instructions = \"Review.\""));
}

#[test]
fn claude_attaches_an_agent_without_treating_its_markdown_as_a_skill_plugin() {
    let root = uze_testkit::temp::scratch("claude-agent");
    let home = UzeHome::at(root.join("uze"));
    let claude = ClaudeIntegration::new(root.join("home/.claude"), home);
    let resource = agent(&root);

    let receipt = claude
        .attach_receipt(&resource)
        .expect("Claude agent attachment succeeds")
        .expect("Claude agent attachment has a receipt");
    let uze_core::integration::ManagedArtifact::SymlinkReference { path, target } =
        receipt.artifact
    else {
        panic!("Claude native agent is a receipt-owned reference");
    };
    assert_eq!(path, root.join("home/.claude/agents/reviewer.md"));
    assert_eq!(target, resource.capability.path);
    assert!(path.is_symlink());
}
