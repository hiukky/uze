// ADR-005: the Core Engine composes peer-harness inputs without named harness rules.
use crate::{
    capability::{Capability, CapabilityKind, Representation},
    error::{Result, UzeError},
    project::{EffectiveEnvironment, Resource, resolve_project_resources},
    store::{PackageId, UzeStore},
};

/// Composes the effective environment owned by the user: project resources
/// remain project-owned and UZE-installed packages remain store-owned.
#[derive(Clone, Debug)]
pub struct UzeEngine {
    store: UzeStore,
}

impl UzeEngine {
    pub fn new(store: UzeStore) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &UzeStore {
        &self.store
    }

    /// Compose every locally installed package with the supplied project's
    /// portable resources. This is the product path used by the CLI.
    pub fn compose_project(
        &self,
        project_root: impl AsRef<std::path::Path>,
    ) -> Result<EffectiveEnvironment> {
        let project = resolve_project_resources(project_root)?;
        let mut resources = project.resources;
        resources.extend(self.package_resources(&self.store.package_ids()?)?);
        resources.sort_by_key(|resource| resource.identity());
        Ok(EffectiveEnvironment {
            root: project.root,
            resources,
        })
    }

    /// Package-only composition remains available for isolated library and
    /// conformance tests. It is not a separate product concept: callers that
    /// have a project should use `compose_project`.
    pub fn compose(&self, packages: &[PackageId]) -> Result<EffectiveEnvironment> {
        let resources = self.package_resources(packages)?;
        Ok(EffectiveEnvironment {
            root: self.store.home().root().to_path_buf(),
            resources,
        })
    }

    fn package_resources(&self, packages: &[PackageId]) -> Result<Vec<Resource>> {
        let mut resources = Vec::new();
        for id in packages {
            let package = self.store.package(id)?;
            resources.extend(package_resources_at(&package.id, &package.root)?);
        }
        resources.sort_by_key(|resource| resource.identity());
        Ok(resources)
    }
}

/// Discovers a package's capabilities from a directory on disk.
///
/// Shared with acquisition, which needs the same reading *before* a package
/// is installed in order to decide trust. Deliberately the same code path, so
/// what an operator authorizes cannot drift from what the Engine later
/// composes.
pub fn package_resources_at(id: &PackageId, root: &std::path::Path) -> Result<Vec<Resource>> {
    let mut resources = Vec::new();
    let skills_root = root.join("skills");
    if skills_root.is_dir() {
        for path in crate::project::files_named(&skills_root, "SKILL.md")? {
            let payload = crate::project::read_file(&path)?;
            resources.push(Resource::from_package(
                id.clone(),
                root.to_path_buf(),
                Capability {
                    kind: CapabilityKind::AgentSkill,
                    representation: Representation::Standard,
                    path,
                    payload,
                },
            ));
        }
    }
    resources.extend(instruction_resources(id, root)?);
    resources.extend(mcp_resources(id, root)?);
    resources.sort_by_key(|resource| resource.identity());
    Ok(resources)
}

/// Discovers a package's optional root-level `AGENTS.md` — the same
/// standard convention `resolve_project` already recognizes at project
/// scope (see `project::discover_instructions`), read here at package scope
/// instead. A package does not ship a whole project instructions file; it
/// ships the portable content a project's own `AGENTS.md` later composes,
/// one delimited region per contributing package.
fn instruction_resources(id: &PackageId, package_root: &std::path::Path) -> Result<Vec<Resource>> {
    let path = package_root.join("AGENTS.md");
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let payload = crate::project::read_file(&path)?;
    Ok(vec![Resource::from_package(
        id.clone(),
        package_root.to_path_buf(),
        Capability {
            kind: CapabilityKind::Instruction,
            representation: Representation::Standard,
            path,
            payload,
        },
    )])
}

/// Discovers a package's optional root-level `mcp.json` (Agent Plugins 1.0
/// shape: `{"mcpServers": {"<name>": {"command", "args", ...}}}`) into one
/// `Resource` per declared server. A package declaring more than one server
/// produces distinct named resources while preserving the original
/// `mcp.json` bytes only once in the Store.
///
/// This module reads the standard, never a harness. Which harnesses already
/// consume that shape is evidence recorded in ADR-007, not a fact the Engine
/// needs or holds.
fn mcp_resources(id: &PackageId, package_root: &std::path::Path) -> Result<Vec<Resource>> {
    let manifest_path = package_root.join("mcp.json");
    if !manifest_path.is_file() {
        return Ok(Vec::new());
    }
    let payload = crate::project::read_file(&manifest_path)?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&payload).map_err(|source| UzeError::Json {
            path: manifest_path.clone(),
            source,
        })?;
    let servers = manifest
        .get("mcpServers")
        .and_then(serde_json::Value::as_object);
    let Some(servers) = servers else {
        return Ok(Vec::new());
    };
    let mut entries: Vec<(&String, &serde_json::Value)> = servers.iter().collect();
    entries.sort_by_key(|(name, _)| name.as_str());
    entries
        .into_iter()
        .map(|(name, config)| {
            let payload = serde_json::to_vec(config)
                .expect("mcp server config re-serialization is infallible");
            Ok(Resource::from_package_named(
                id.clone(),
                package_root.to_path_buf(),
                Capability {
                    kind: CapabilityKind::Mcp,
                    representation: Representation::Standard,
                    path: manifest_path.clone(),
                    payload,
                },
                name.to_owned(),
            ))
        })
        .collect()
}

/// A package's `commands/` directory is no longer a canonical surface
/// (ADR-030): the same explicit-action semantics are carried by a Skill's
/// `invoke:` policy. Discovery below covers only the canonical surfaces
/// that remain: `skills/`, `AGENTS.md`, `mcp.json`.
#[cfg(test)]
mod discovery_tests {
    use super::*;
    use std::fs;
    use std::path::Path;

    fn temp(label: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "uze-discovery-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn a_commands_directory_is_not_a_canonical_surface_anymore() {
        // The physical directory may still exist inside a package (a
        // vendor-authored explicit envelope delivers it natively), but
        // canonical discovery never reads it: there is exactly one
        // Skill family, and its semantics come from invocation policy.
        let root = temp("commands-ignored");
        let pkg = root.join("pkg");
        fs::create_dir_all(pkg.join("commands")).unwrap();
        fs::create_dir_all(pkg.join("skills/review")).unwrap();
        fs::write(pkg.join("commands/review.md"), "legacy command").unwrap();
        fs::write(pkg.join("skills/review/SKILL.md"), "skill").unwrap();
        let id = PackageId::from_plugin_name("demo", Path::new("plugin.json")).unwrap();
        let resources = package_resources_at(&id, &pkg).unwrap();
        assert_eq!(resources.len(), 1);
        assert!(
            resources[0]
                .capability
                .path
                .ends_with("skills/review/SKILL.md")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn skill_policy_is_exposed_by_discovery() {
        let root = temp("policy");
        let pkg = root.join("pkg");
        fs::create_dir_all(pkg.join("skills/review")).unwrap();
        fs::write(
            pkg.join("skills/review/SKILL.md"),
            b"---\ninvoke:\n  model: false\n  user: true\n---\nbody\n",
        )
        .unwrap();
        let id = PackageId::from_plugin_name("demo", Path::new("plugin.json")).unwrap();
        let resources = package_resources_at(&id, &pkg).unwrap();
        assert_eq!(resources.len(), 1);
        assert_eq!(
            resources[0].skill_policy,
            Some(crate::skill::SkillInvocationPolicy::USER_ONLY)
        );
        fs::remove_dir_all(root).unwrap();
    }
}
