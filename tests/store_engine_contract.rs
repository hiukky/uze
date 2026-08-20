use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use uze::{
    ResourceOrigin, UzeEngine, UzeHome, UzeStore,
    capability::{CapabilityKind, Representation},
};

fn package_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("playground/agent-plugin-package")
}

fn temporary_home(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("uze-{label}-{}-{nonce}", std::process::id()))
}

#[test]
fn uze_home_derives_every_owned_path_from_one_root() {
    let root = temporary_home("paths");
    let home = UzeHome::at(&root);

    assert_eq!(home.root(), root.as_path());
    assert_eq!(home.store_dir(), root.join("store"));
    assert_eq!(home.packages_dir(), root.join("store/packages"));
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
    let first = store.install_agent_plugin(package_fixture()).unwrap();
    let second = store.install_agent_plugin(package_fixture()).unwrap();

    assert_eq!(first.id, second.id);
    assert_eq!(store.registration_count().unwrap(), 1);
    assert_eq!(first.root, home.package_dir(&first.id));
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
fn engine_composes_a_standard_resource_from_the_store() {
    let root = temporary_home("engine");
    let store = UzeStore::new(UzeHome::at(&root));
    let package = store.install_agent_plugin(package_fixture()).unwrap();
    let environment = UzeEngine::new(store)
        .compose(&[package.id.clone()])
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
