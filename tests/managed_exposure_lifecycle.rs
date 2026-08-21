//! L1 contract: one Store installation, one `EffectiveEnvironment`, and the
//! managed exposure each peer `IntegrationPort` prepares from it.
//!
//! Deterministic by construction. No harness binary is spawned, no model is
//! invoked, no credential is read, and nothing here is gated behind an opt-in
//! environment variable — `cargo test` must be a complete gate for this tier.
//!
//! Real-harness evidence lives in the conformance lab under `e2e/`, which
//! runs the actual CLIs inside a disposable container and classifies
//! attachment, discovery and behavior as separate, non-conflatable tiers.
//! Earlier revisions of this file welded opt-in harness probes onto the
//! assertions below; a model declining to use a capability then presented as
//! a failure of the planning contract, which is exactly the conflation the
//! tier split exists to prevent.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use uze::{
    PackageId, Resource, UzeEngine, UzeHome, UzeStore, exposure::ExposureMechanism,
    integration::IntegrationPort,
};

use uze::integrations::{
    claude::ClaudeIntegration, codex::CodexIntegration, opencode::OpenCodeIntegration,
};

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
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/packages/agent-plugin-skill")
}

fn mcp_package_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/packages/agent-plugin-mcp")
}

fn temporary_root(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is available")
        .as_nanos();
    std::env::temp_dir().join(format!("uze-{label}-{}-{nonce}", std::process::id()))
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
    let installed = store
        .install_agent_plugin(package_fixture())
        .expect("fixture is a valid Agent Plugin 1.0 package");
    assert_eq!(store.registration_count().expect("registry is readable"), 1);

    let workspace = root.join("caller-workspace");
    fs::create_dir_all(&workspace).expect("caller workspace is created");
    let environment = UzeEngine::new(store)
        .compose_project(&workspace)
        .expect("empty caller project composes with the installed package");
    let resource = environment
        .resources
        .into_iter()
        .find(|resource| resource.package_root().is_some())
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

#[test]
fn same_store_environment_is_planned_for_claude_and_codex_as_peers() {
    let fixture = shared_store_fixture("same-store-contract");
    let claude = ClaudeIntegration::new(fixture.root.join("claude-home"), fixture.home.clone())
        .exposure_plan(&fixture.resource);
    let codex = CodexIntegration::new(fixture.root.join("agents-home"), fixture.home.clone())
        .exposure_plan(&fixture.resource);

    assert!(matches!(
        claude.mechanism,
        ExposureMechanism::RuntimeBridge { .. }
    ));
    assert!(matches!(
        codex.mechanism,
        ExposureMechanism::FilesystemProjection { .. }
    ));
    assert!(fixture.skill_path.starts_with(&fixture.package_path));
    assert_eq!(
        fixture.resource.identity(),
        format!(
            "package:{}:skills/uze-e2e/SKILL.md",
            fixture.package_id.as_str()
        )
    );
}

#[test]
fn projection_keeps_real_project_cwd_and_cleans_its_managed_artifact() {
    let fixture = shared_store_fixture("projection-lifecycle");
    let plan = CodexIntegration::new(fixture.root.join("agents-home"), fixture.home.clone())
        .exposure_plan(&fixture.resource);
    let mut prepared = plan
        .prepare(&fixture.home, "codex", "agent-skill", &fixture.workspace)
        .expect("Codex fallback can prepare a managed symlink");

    let artifact = prepared
        .managed_artifact_path()
        .expect("projection is managed")
        .to_path_buf();
    assert_eq!(prepared.working_directory, fixture.workspace);
    assert!(artifact.is_symlink());
    assert!(
        prepared
            .runtime_directory
            .as_ref()
            .expect("runtime metadata exists")
            .join("managed-exposure.json")
            .is_file()
    );
    prepared.cleanup().expect("managed projection cleans up");
    assert!(!artifact.exists());
    assert_clean_workspace(&fixture.workspace);
}

/// Every peer prepares the *same* store resource, in the caller's real
/// working directory, and leaves nothing of its own behind. This is the
/// plugin-first invariant: no harness-specific copy is created in the
/// project, and cleanup restores the workspace exactly.
///
/// Sequential by necessity, not by convenience — see
/// `a_second_peer_refuses_to_clobber_an_existing_projection`.
#[test]
fn every_peer_prepares_one_store_resource_without_touching_the_caller_workspace() {
    let fixture = shared_store_fixture("peer-preparation");
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
        let mut prepared = plan
            .prepare(&fixture.home, id, "agent-skill", &fixture.workspace)
            .unwrap_or_else(|error| panic!("{id} prepares the shared store resource: {error:?}"));
        assert_eq!(
            prepared.working_directory, fixture.workspace,
            "{id} redirected the caller's working directory"
        );
        prepared
            .cleanup()
            .unwrap_or_else(|error| panic!("{id} cleans only its managed artifact: {error:?}"));
        assert_clean_workspace(&fixture.workspace);
    }
}

/// Codex and OpenCode both project into `.agents/skills/`, so their managed
/// artifacts occupy the same path for the same resource. UZE must refuse the
/// second preparation rather than overwrite an artifact it did not create —
/// silently clobbering would make the first peer's `cleanup()` delete the
/// second peer's projection, or leave a dangling one behind.
#[test]
fn a_second_peer_refuses_to_clobber_an_existing_projection() {
    let fixture = shared_store_fixture("projection-collision");
    let mut codex = CodexIntegration::new(fixture.root.join("agents-home"), fixture.home.clone())
        .exposure_plan(&fixture.resource)
        .prepare(&fixture.home, "codex", "agent-skill", &fixture.workspace)
        .expect("Codex prepares first");

    let collision = OpenCodeIntegration::new(
        fixture.home.root().join("opencode-agents"),
        fixture.home.root().join("opencode-config/opencode.json"),
        fixture.home.clone(),
    )
    .exposure_plan(&fixture.resource)
    .prepare(&fixture.home, "opencode", "agent-skill", &fixture.workspace);

    assert!(
        collision.is_err(),
        "a peer overwrote another peer's managed projection instead of refusing"
    );

    codex.cleanup().expect("the first peer still cleans up");
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
    let installed = store
        .install_agent_plugin(mcp_package_fixture())
        .expect("MCP fixture is a valid Agent Plugin 1.0 package");
    let workspace = root.join("caller-workspace");
    fs::create_dir_all(&workspace).expect("caller workspace is created");
    let environment = UzeEngine::new(store)
        .compose_project(&workspace)
        .expect("MCP-only package composes");
    let resource = environment
        .resources
        .into_iter()
        .find(|resource| resource.package_root().is_some())
        .expect("fixture contributes one store-owned MCP resource");

    let entry_name = resource
        .attachment_entry_name()
        .expect("an MCP resource has a derivable entry name");
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
