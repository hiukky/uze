//! Package fixtures shared by the conformance suites.
//!
//! One copy. These builders were byte-identical in
//! `capability_conformance.rs` and `lifecycle_conformance.rs`, which is the
//! shape a fixture drifts from: the copy a change forgets is the one whose
//! suite quietly stops proving what it says it proves.
//!
//! Every fixture lives entirely under a throwaway temp root — never a real
//! `$UZE_HOME/store` — and mirrors a real `Store::ingest` result closely
//! enough for `package_exposure_plan`/`exposure_plan` to behave identically
//! without pulling acquisition machinery in here.

use std::{
    fs,
    path::{Path, PathBuf},
};

use uze_core::{
    acquisition::{PackageSource, Provenance, ResolvedSource},
    capability::{Capability, CapabilityKind, Representation},
    home::UzeHome,
    integration::IntegrationPort,
    project::Resource,
    state,
    store::{PackageId, StoredPackage},
};

pub(crate) fn temp(label: &str) -> PathBuf {
    uze_testkit::temp::scratch(label)
}

/// Writes a canonical `plugin.json` plus every `(relative_path, content)`
/// pair, then builds the `StoredPackage` those bytes describe.
pub(crate) fn build_package(
    label: &str,
    name: &str,
    extra_files: &[(&str, &str)],
) -> (PathBuf, StoredPackage) {
    let root = temp(label);
    let pkg_root = root.join("pkg");
    fs::create_dir_all(&pkg_root).unwrap();
    fs::write(
        pkg_root.join("plugin.json"),
        format!(r#"{{"name":"{name}","version":"1.0.0","description":"Conformance fixture"}}"#),
    )
    .unwrap();
    for (relative, content) in extra_files {
        let path = pkg_root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
    }
    let id = PackageId::from_plugin_name(name, &pkg_root.join("plugin.json")).unwrap();
    let package = StoredPackage {
        active_name: id.plugin_name().to_owned(),
        id,
        root: pkg_root.clone(),
        manifest: pkg_root.join("plugin.json"),
        provenance: Provenance {
            requested: PackageSource::Local {
                path: PathBuf::from("/tmp/fake"),
            },
            resolved: ResolvedSource::Local {
                path: PathBuf::from("/tmp/fake"),
            },
        },
    };
    (root, package)
}

pub(crate) fn skill_resource(package: &StoredPackage, dir: &str, name: &str) -> Resource {
    let path = package.root.join(dir).join(name).join("SKILL.md");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, format!("---\nname: {name}\n---\n\nBody.\n")).unwrap();
    Resource::from_package(
        package.id.clone(),
        package.root.clone(),
        Capability {
            kind: CapabilityKind::AgentSkill,
            representation: Representation::Standard,
            path,
            payload: Vec::new(),
        },
    )
}

pub(crate) fn skill_resource_outside_conventions(package: &StoredPackage, name: &str) -> Resource {
    skill_resource(package, "unconventional-location", name)
}

pub(crate) fn mcp_resource(package: &StoredPackage, name: &str, payload: &str) -> Resource {
    let path = package.root.join("mcp.json");
    Resource::from_package_named(
        package.id.clone(),
        package.root.clone(),
        Capability {
            kind: CapabilityKind::Mcp,
            representation: Representation::Standard,
            path,
            payload: payload.as_bytes().to_vec(),
        },
        name.to_owned(),
    )
}

pub(crate) fn mark_setup(home: &UzeHome, integration: &dyn IntegrationPort) {
    state::record(
        home,
        state::IntegrationRecord {
            harness: integration.id().to_owned(),
            version: None,
            strategy: "conformance-fixture".to_owned(),
            installed: true,
        },
    )
    .unwrap();
}

/// Every regular file under `root`, relative to it, with its bytes — the
/// shape an "these bytes did not change" assertion needs.
pub(crate) fn tree(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    collect(root, root, &mut found);
    found.sort_by(|left, right| left.0.cmp(&right.0));
    found
}

fn collect(root: &Path, current: &Path, into: &mut Vec<(PathBuf, Vec<u8>)>) {
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_symlink() {
            let target = fs::read_link(&path).unwrap_or_default();
            into.push((
                path.strip_prefix(root).unwrap().to_path_buf(),
                target.as_os_str().as_encoded_bytes().to_vec(),
            ));
        } else if path.is_dir() {
            collect(root, &path, into);
        } else if let Ok(bytes) = fs::read(&path) {
            into.push((path.strip_prefix(root).unwrap().to_path_buf(), bytes));
        }
    }
}
