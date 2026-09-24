use std::{fs, path::PathBuf};

use uze_core::{UzeHome, UzeStore, capability::CapabilityKind};

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

fn package_fixture() -> PathBuf {
    uze_testkit::fixtures::canonical("skill-plugin")
}

fn mcp_package_fixture() -> PathBuf {
    uze_testkit::fixtures::canonical("mcp-plugin")
}

fn temporary_home(label: &str) -> PathBuf {
    uze_testkit::temp::scratch(label)
}

/// Counts registry entries by reading `packages.json` itself, so the Store's
/// own bookkeeping is never the witness for its own claim.
fn registered(home: &UzeHome) -> usize {
    let Ok(text) = fs::read_to_string(home.registry_path()) else {
        return 0;
    };
    serde_json::from_str::<serde_json::Value>(&text)
        .expect("the registry is valid JSON")["packages"]
        .as_object()
        .expect("the registry carries a packages map")
        .len()
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
    assert_eq!(home.runtime_projects_dir(), root.join("runtime/projects"));
    assert_eq!(
        home.runtime_project_dir("abc123"),
        root.join("runtime/projects/abc123")
    );
    assert_eq!(
        home.runtime_projection_dir("fake", "abc123"),
        root.join("runtime/projects/abc123/fake")
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
    assert_eq!(registered(&home), 1);
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

/// ADR-036's Store layout is unchanged: two same-named plugins from distinct
/// marketplaces can always coexist as *bytes*, each under its own
/// `store/plugins/<marketplace>/<plugin>` directory and registry entry.
/// ADR-036 adds a second, independent layer on top: at most one of them may
/// be *active* under that bare name at a time (the name a harness actually
/// invokes) — ingesting a second one under the same default name is refused,
/// not silently allowed to shadow the first.
#[test]
fn store_keeps_same_named_plugins_from_distinct_marketplaces_separate_but_only_one_active() {
    let root = temporary_home("marketplace-qualified-identity");
    let home = UzeHome::at(&root);
    let store = UzeStore::new(home.clone());
    let materialized =
        uze_core::acquisition::acquire(&uze_core::PackageSource::local(package_fixture())).unwrap();

    let from_alpha = store.ingest(&materialized, "alpha", None).unwrap();
    assert_eq!(from_alpha.id.as_str(), "uze-agent-skill-conformance@alpha");
    assert_eq!(from_alpha.active_name, "uze-agent-skill-conformance");

    // A second marketplace's plugin sharing the same bare name is refused by
    // default — not because its bytes can't coexist (they can, and do, once
    // resolved), but because it would silently shadow `alpha`'s claim on
    // every harness's `/uze-agent-skill-conformance:*` invocation.
    let collision = store.ingest(&materialized, "beta", None).unwrap_err();
    assert!(matches!(
        collision,
        uze_core::UzeError::PluginNameCollision { existing, requested, .. }
            if existing == "uze-agent-skill-conformance@alpha"
                && requested == "uze-agent-skill-conformance@beta"
    ));
    // The refused install must not have written anything.
    assert_eq!(registered(&home), 1);

    // Resolved with an explicit alias, `beta`'s copy installs and coexists —
    // its own bytes, its own registration, active under the chosen name.
    let from_beta = store
        .ingest(
            &materialized,
            "beta",
            Some("uze-agent-skill-conformance-beta"),
        )
        .unwrap();
    assert_eq!(from_beta.id.as_str(), "uze-agent-skill-conformance@beta");
    assert_eq!(from_beta.active_name, "uze-agent-skill-conformance-beta");
    assert_ne!(from_alpha.root, from_beta.root);
    assert_eq!(
        from_alpha.root,
        root.join("store/plugins/alpha/uze-agent-skill-conformance")
    );
    assert_eq!(
        from_beta.root,
        root.join("store/plugins/beta/uze-agent-skill-conformance")
    );
    assert_eq!(registered(&home), 2);
    assert_eq!(
        store
            .find_by_active_name("uze-agent-skill-conformance")
            .unwrap(),
        Some(from_alpha.id.clone())
    );
    assert_eq!(
        store
            .find_by_active_name("uze-agent-skill-conformance-beta")
            .unwrap(),
        Some(from_beta.id.clone())
    );
}

#[test]
fn store_rejects_an_invalid_marketplace_name_before_writing_plugin_bytes() {
    let root = temporary_home("invalid-marketplace");
    let home = UzeHome::at(&root);
    let store = UzeStore::new(home.clone());
    let materialized =
        uze_core::acquisition::acquire(&uze_core::PackageSource::local(package_fixture())).unwrap();

    assert!(
        store
            .ingest(&materialized, "not/a-marketplace", None)
            .is_err()
    );
    assert!(!home.plugins_dir().join("not/a-marketplace").exists());
    assert_eq!(registered(&home), 0);
}

#[test]
fn engine_composes_a_standard_resource_from_the_store() {
    let root = temporary_home("engine");
    let store = UzeStore::new(UzeHome::at(&root));
    let package = install(&store, package_fixture()).unwrap();
    let resources = uze_core::engine::package_resources(&package).unwrap();

    assert_eq!(resources.len(), 1);
    let resource = &resources[0];
    assert_eq!(resource.capability.kind, CapabilityKind::AgentSkill);
    assert_eq!(resource.package_id, package.id);
    assert!(resource.capability.path.starts_with(&package.root));

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

    let resources = uze_core::engine::package_resources(&package).unwrap();
    assert_eq!(resources.len(), 1);
    let resource = &resources[0];
    assert_eq!(resource.capability.kind, CapabilityKind::Mcp);
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
    let resources = uze_core::engine::package_resources(&package).unwrap();
    assert_eq!(resources.len(), 2);
    let identities = resources
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
    let names = resources
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

/// An install interrupted between the copy and the registration leaves a
/// plugin directory nothing in `packages.json` names. `create_dir` refused
/// it forever after, and the refusal was a dead end: `remove` answers only
/// to registered ids, so there was no command that could clear it. The next
/// attempt now clears the debris and succeeds.
#[test]
fn an_install_interrupted_mid_copy_never_blocks_the_next_attempt() {
    let root = temporary_home("store-partial-install");
    let home = UzeHome::at(root.join("uze"));
    let store = UzeStore::new(home.clone());
    let source = package_fixture();

    // The state an interrupted install leaves: a partial directory where
    // the package's bytes go, and no registration for it.
    let installed = install(&store, &source).unwrap();
    let plugin_dir = installed.root.clone();
    fs::write(home.registry_path(), r#"{"packages":{}}"#).unwrap();
    for entry in fs::read_dir(&plugin_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().unwrap() != "plugin.json" {
            let _ = fs::remove_file(&path).or_else(|_| fs::remove_dir_all(&path));
        }
    }
    fs::write(plugin_dir.join("half-written"), "truncated").unwrap();
    assert_eq!(
        registered(&home),
        0,
        "the interrupted install registered nothing"
    );

    let reinstalled = install(&store, &source).expect("a second attempt must not be refused");

    assert_eq!(reinstalled.root, plugin_dir);
    assert_eq!(registered(&home), 1);
    assert!(
        !plugin_dir.join("half-written").exists(),
        "the partial tree survived into the reinstalled package"
    );
    assert!(
        plugin_dir.join("skills").exists(),
        "the package was not copied in full"
    );
    fs::remove_dir_all(root).unwrap();
}

/// The mirror: an ingest that fails after the bytes are copied leaves the
/// Store as it found it, so the failure is one the operator can simply
/// retry rather than the state the test above describes.
#[cfg(unix)]
#[test]
fn a_failed_ingest_leaves_no_directory_behind() {
    use std::os::unix::fs::PermissionsExt;

    if unsafe { libc::geteuid() } == 0 {
        return; // root writes into a read-only directory anyway
    }
    let root = temporary_home("store-failed-ingest");
    let home = UzeHome::at(root.join("uze"));
    let store = UzeStore::new(home.clone());
    home.ensure_layout().unwrap();
    // The registry cannot be written, so the ingest fails at its last step —
    // after `copy_tree` has already landed the package's bytes.
    fs::set_permissions(home.state_dir(), fs::Permissions::from_mode(0o555)).unwrap();

    let outcome = install(&store, package_fixture());

    fs::set_permissions(home.state_dir(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        outcome.is_err(),
        "an unregistrable package must not be reported as installed"
    );
    assert!(
        !home
            .plugins_dir()
            .join("local")
            .join("uze-agent-skill-conformance")
            .exists(),
        "a failed ingest left a directory the next attempt would trip over"
    );
    assert_eq!(registered(&home), 0);
    fs::remove_dir_all(root).unwrap();
}
