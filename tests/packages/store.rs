use std::{fs, path::PathBuf};

use uze::{
    ResourceOrigin, UzeEngine, UzeHome, UzeStore,
    capability::{CapabilityKind, Representation},
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

fn package_fixture() -> PathBuf {
    uze_testkit::fixtures::canonical("skill-plugin")
}

fn mcp_package_fixture() -> PathBuf {
    uze_testkit::fixtures::canonical("mcp-plugin")
}

fn temporary_home(label: &str) -> PathBuf {
    uze_testkit::temp::scratch(label)
}

#[test]
fn uze_home_derives_every_owned_path_from_one_root() {
    let root = temporary_home("paths");
    let home = UzeHome::at(&root);

    assert_eq!(home.root(), root.as_path());
    assert_eq!(home.store_dir(), root.join("store"));
    assert_eq!(home.plugins_dir(), root.join("store/plugins"));
    assert_eq!(home.state_dir(), root.join("state"));
    assert_eq!(home.cache_dir(), root.join("cache"));
    assert_eq!(home.runtime_dir(), root.join("runtime"));
    assert_eq!(
        home.runtime_session_dir("fake", "session"),
        root.join("runtime/fake/session")
    );
}

#[test]
fn store_installs_one_agent_plugin_once_without_a_uze_manifest() {
    let root = temporary_home("store");
    let home = UzeHome::at(&root);
    let store = UzeStore::new(home.clone());
    let first = install(&store, package_fixture()).unwrap();
    let second = install(&store, package_fixture()).unwrap();

    assert_eq!(first.id, second.id);
    assert_eq!(store.registration_count().unwrap(), 1);
    assert_eq!(first.root, home.plugin_dir(&first.id));
    assert!(first.manifest.is_file());
    assert!(home.registry_path().is_file());
    assert!(home.cache_dir().is_dir());
    assert!(home.runtime_dir().is_dir());
    assert!(!first.root.join(".agents").exists());
    assert_eq!(
        fs::read(first.root.join("skills/uze-e2e/SKILL.md")).unwrap(),
        fs::read(package_fixture().join("skills/uze-e2e/SKILL.md")).unwrap()
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn store_keeps_same_named_plugins_from_distinct_marketplaces_separate() {
    let root = temporary_home("marketplace-qualified-identity");
    let home = UzeHome::at(&root);
    let store = UzeStore::new(home.clone());
    let materialized =
        uze::acquisition::acquire(&uze::PackageSource::local(package_fixture())).unwrap();

    let from_alpha = store
        .ingest_from_marketplace(&materialized, "alpha")
        .unwrap();
    let from_beta = store
        .ingest_from_marketplace(&materialized, "beta")
        .unwrap();

    assert_eq!(from_alpha.id.as_str(), "uze-agent-skill-conformance@alpha");
    assert_eq!(from_beta.id.as_str(), "uze-agent-skill-conformance@beta");
    assert_ne!(from_alpha.root, from_beta.root);
    assert_eq!(
        from_alpha.root,
        root.join("store/plugins/alpha/uze-agent-skill-conformance")
    );
    assert_eq!(
        from_beta.root,
        root.join("store/plugins/beta/uze-agent-skill-conformance")
    );
    assert_eq!(store.registration_count().unwrap(), 2);
}

#[test]
fn store_rejects_an_invalid_marketplace_name_before_writing_plugin_bytes() {
    let root = temporary_home("invalid-marketplace");
    let home = UzeHome::at(&root);
    let store = UzeStore::new(home.clone());
    let materialized =
        uze::acquisition::acquire(&uze::PackageSource::local(package_fixture())).unwrap();

    assert!(
        store
            .ingest_from_marketplace(&materialized, "not/a-marketplace")
            .is_err()
    );
    assert!(!home.plugins_dir().join("not/a-marketplace").exists());
    assert_eq!(store.registration_count().unwrap(), 0);
}

#[test]
fn engine_composes_a_standard_resource_from_the_store() {
    let root = temporary_home("engine");
    let store = UzeStore::new(UzeHome::at(&root));
    let package = install(&store, package_fixture()).unwrap();
    let environment = UzeEngine::new(store)
        .compose(std::slice::from_ref(&package.id))
        .unwrap();

    assert_eq!(environment.resources.len(), 1);
    let resource = &environment.resources[0];
    assert_eq!(resource.capability.kind, CapabilityKind::AgentSkill);
    assert_eq!(resource.capability.representation, Representation::Standard);
    assert!(matches!(
        resource.origin,
        ResourceOrigin::Package { ref id, .. } if id == &package.id
    ));
    assert!(resource.capability.path.starts_with(&package.root));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn engine_composes_project_and_store_sources_into_one_effective_environment() {
    let root = temporary_home("combined-environment");
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    fs::write(project.join("AGENTS.md"), "# Project-owned instructions\n").unwrap();
    let store = UzeStore::new(UzeHome::at(root.join("uze-home")));
    let package = install(&store, package_fixture()).unwrap();

    let environment = UzeEngine::new(store).compose_project(&project).unwrap();
    assert_eq!(environment.root, project.canonicalize().unwrap());
    assert_eq!(environment.resources.len(), 2);
    assert!(
        environment
            .resources
            .iter()
            .any(|resource| matches!(resource.origin, ResourceOrigin::Project { .. }))
    );
    assert!(environment.resources.iter().any(|resource| {
        matches!(resource.origin, ResourceOrigin::Package { ref id, .. } if id == &package.id)
    }));

    fs::remove_dir_all(root).unwrap();
}

/// A package with only `mcp.json` (no `skills/`) composes into one `Mcp`
/// resource, independently of Skill discovery — the two code paths never
/// interfere with each other (see ADR-007 / design.md Non-Goals on why this
/// fixture is deliberately separate from `agent-plugin-skill`).
#[test]
fn store_and_engine_compose_an_mcp_only_package_into_one_mcp_resource() {
    let root = temporary_home("mcp-store");
    let store = UzeStore::new(UzeHome::at(&root));
    let package = install(&store, mcp_package_fixture()).unwrap();

    assert!(package.root.join("mcp.json").is_file());
    assert!(!package.root.join("skills").exists());
    assert_eq!(
        fs::read(package.root.join("mcp.json")).unwrap(),
        fs::read(mcp_package_fixture().join("mcp.json")).unwrap()
    );

    let environment = UzeEngine::new(store)
        .compose(std::slice::from_ref(&package.id))
        .unwrap();
    assert_eq!(environment.resources.len(), 1);
    let resource = &environment.resources[0];
    assert_eq!(resource.capability.kind, CapabilityKind::Mcp);
    assert_eq!(resource.capability.representation, Representation::Standard);
    assert_eq!(resource.capability.path, package.root.join("mcp.json"));

    let config: serde_json::Value = serde_json::from_slice(&resource.capability.payload).unwrap();
    assert_eq!(
        config.get("command").and_then(|value| value.as_str()),
        Some("__UZE_MCP_FIXTURE_BINARY__")
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn one_package_with_two_mcp_servers_produces_two_named_resources() {
    let home = UzeHome::at(temporary_home("multi-mcp"));
    let store = UzeStore::new(home.clone());
    let fixture = uze_testkit::fixtures::canonical("multi-mcp-plugin");
    let package = install(&store, fixture).unwrap();
    let environment = UzeEngine::new(store)
        .compose(std::slice::from_ref(&package.id))
        .unwrap();
    assert_eq!(environment.resources.len(), 2);
    let identities = environment
        .resources
        .iter()
        .map(|resource| resource.identity())
        .collect::<Vec<_>>();
    assert_ne!(identities[0], identities[1]);
    assert!(
        identities
            .iter()
            .any(|identity| identity.ends_with(":filesystem"))
    );
    assert!(
        identities
            .iter()
            .any(|identity| identity.ends_with(":github"))
    );
    // Logical capability names are bare — no package prefix, no "uze-"
    // collision-avoidance prefix. Physical exposure naming (with
    // qualification when needed) is an Integration/Application decision
    // now, not something a Resource computes for itself.
    let names = environment
        .resources
        .iter()
        .map(|resource| resource.logical_capability_name().unwrap())
        .collect::<Vec<_>>();
    assert!(names.contains(&"filesystem".to_owned()));
    assert!(names.contains(&"github".to_owned()));
    fs::remove_dir_all(home.root()).unwrap();
}

#[cfg(unix)]
#[test]
fn store_preserves_plugin_symlinks_and_executable_permissions() {
    use std::os::unix::{fs::PermissionsExt, fs::symlink};

    let root = temporary_home("store-fidelity");
    let source = root.join("source");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("plugin.json"), r#"{"name":"fidelity"}"#).unwrap();
    let executable = source.join("bin/run");
    fs::create_dir_all(executable.parent().unwrap()).unwrap();
    fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    symlink("run", source.join("bin/current")).unwrap();

    let store = UzeStore::new(UzeHome::at(root.join("uze")));
    let package = install(&store, &source).unwrap();
    let copied = package.root.join("bin/run");
    assert!(package.root.join("bin/current").is_symlink());
    assert_eq!(
        fs::read_link(package.root.join("bin/current")).unwrap(),
        PathBuf::from("run")
    );
    assert_ne!(
        fs::metadata(copied).unwrap().permissions().mode() & 0o111,
        0
    );
    fs::remove_dir_all(root).unwrap();
}
