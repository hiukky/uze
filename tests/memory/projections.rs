//! L1 contract: one Store installation, the resources it contributes, and the
//! exposure each peer `IntegrationPort` plans from it.
//!
//! Deterministic by construction. No harness binary is spawned, no model is
//! invoked, no credential is read, and nothing here is gated behind an opt-in
//! environment variable — `cargo test` must be a complete gate for this tier.
//!
//! Real-harness evidence lives in the conformance lab under `conformance/`, which
//! runs the actual CLIs inside a disposable container and classifies
//! attachment, discovery and behavior as separate, non-conflatable tiers.
//! Earlier revisions of this file welded opt-in harness probes onto the
//! assertions below; a model declining to use a capability then presented as
//! a failure of the planning contract, which is exactly the conflation the
//! tier split exists to prevent.

use std::{
    fs,
    path::{Path, PathBuf},
};

use uze_core::{
    PackageId, Resource, UzeHome, UzeStore,
    exposure::ExposureMechanism,
    integration::{IntegrationPort, default_exposure_name_candidates},
};

use uze_integrations::{
    claude::ClaudeIntegration, codex::CodexIntegration, opencode::OpenCodeIntegration,
};

/// The acquisition pipeline every install now goes through: a source is
/// acquired into a materialized package, and only then does the Store ingest
/// it. Spelled out here rather than hidden behind a Store convenience,
/// because the Store deliberately no longer accepts a path.
fn install(
    store: &UzeStore,
    path: impl Into<std::path::PathBuf>,
) -> uze_core::Result<uze_core::StoredPackage> {
    store.ingest(
        &uze_core::acquisition::acquire(&uze_core::PackageSource::local(path))?,
        "local",
        None,
    )
}

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
    uze_testkit::fixtures::canonical("skill-plugin")
}

fn mcp_package_fixture() -> PathBuf {
    uze_testkit::fixtures::canonical("mcp-plugin")
}

fn temporary_root(label: &str) -> PathBuf {
    uze_testkit::temp::scratch(label)
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
            "caller workspace unexpectedly contains {path}"
        );
    }
}

fn shared_store_fixture(label: &str) -> SharedStoreFixture {
    let root = temporary_root(label);
    let home = UzeHome::at(root.join("uze-home"));
    let store = UzeStore::new(home.clone());
    let installed =
        install(&store, package_fixture()).expect("fixture is a valid Agent Plugin 1.0 package");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(
            &fs::read_to_string(home.registry_path()).expect("the registry was written")
        )
        .expect("the registry is valid JSON")["packages"]
            .as_object()
            .expect("the registry carries a packages map")
            .len(),
        1
    );

    let workspace = root.join("caller-workspace");
    fs::create_dir_all(&workspace).expect("caller workspace is created");
    let resources = uze_core::engine::package_resources(&installed)
        .expect("empty caller project composes with the installed package");
    let resource = resources
        .into_iter()
        .next()
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

/// Before `uze setup` no peer has a managed attachment to reach, so each
/// plans the same store resource as Unsupported and says how to get one —
/// never a per-session projection into the caller's workspace.
#[test]
fn every_peer_plans_one_store_resource_as_setup_required_without_touching_the_workspace() {
    let fixture = shared_store_fixture("peer-planning");
    let plans = [
        (
            "claude",
            ClaudeIntegration::new(fixture.root.join("claude-home"), fixture.home.clone())
                .exposure_plan(&fixture.resource),
        ),
        (
            "codex",
            CodexIntegration::new(fixture.root.join("agents-home"), fixture.home.clone())
                .exposure_plan(&fixture.resource),
        ),
        (
            "opencode",
            OpenCodeIntegration::new(
                fixture.home.root().join("opencode-agents"),
                fixture.home.root().join("opencode-config/opencode.json"),
                fixture.home.clone(),
            )
            .exposure_plan(&fixture.resource),
        ),
    ];

    for (id, plan) in plans {
        let ExposureMechanism::Unsupported { rationale } = &plan.mechanism else {
            panic!(
                "{id}: expected an Unsupported plan, got {:?}",
                plan.mechanism
            );
        };
        assert!(rationale.contains("uze setup"), "{id}: {rationale}");
    }
    assert!(fixture.skill_path.starts_with(&fixture.package_path));
    assert_eq!(
        fixture.resource.identity(),
        format!(
            "package:{}:skills/uze-e2e/SKILL.md",
            fixture.package_id.as_str()
        )
    );
    assert_clean_workspace(&fixture.workspace);
}

/// A harness composes the final tool-call function name as
/// `<entry_name>_<tool_name>`, and providers cap that at 64 characters —
/// OpenAI rejects a longer one with `Invalid
/// 'messages[..].tool_calls[0].function.name': string too long`. UZE
/// controls only the entry-name half, so it must leave usable room.
///
/// Guarded here as well as in the conformance lab because the failure is
/// invisible to every cheaper signal: attachment reconciles, the harness
/// lists the server, and it connects. Only an actual tool call fails.
#[test]
fn a_derived_mcp_entry_name_leaves_room_for_a_tool_name() {
    /// OpenAI's documented cap on a tool-call function name.
    const PROVIDER_LIMIT: usize = 64;
    /// Room for the `_<tool_name>` suffix the MCP server contributes.
    const TOOL_NAME_RESERVE: usize = 16;

    let root = temporary_root("mcp-entry-name");
    let home = UzeHome::at(root.join("uze-home"));
    let store = UzeStore::new(home.clone());
    let installed = install(&store, mcp_package_fixture())
        .expect("MCP fixture is a valid Agent Plugin 1.0 package");
    let workspace = root.join("caller-workspace");
    fs::create_dir_all(&workspace).expect("caller workspace is created");
    let resources =
        uze_core::engine::package_resources(&installed).expect("MCP-only package composes");
    let resource = resources
        .into_iter()
        .next()
        .expect("fixture contributes one store-owned MCP resource");

    let entry_name = default_exposure_name_candidates(&resource)
        .into_iter()
        .next()
        .expect("an MCP resource has a derivable exposure name candidate");
    let budget = PROVIDER_LIMIT - TOOL_NAME_RESERVE;
    assert!(
        entry_name.len() <= budget,
        "derived entry name {entry_name} is {} characters; over {budget} leaves no room for a \
         tool name inside the provider's {PROVIDER_LIMIT}-character limit (package {})",
        entry_name.len(),
        installed.id.as_str()
    );

    let _ = fs::remove_dir_all(root);
}
