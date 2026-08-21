// ADR-005: the Engine composes peer-harness inputs without named harness rules.
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
    resources.extend(mcp_resources(id, root)?);
    resources.sort_by_key(|resource| resource.identity());
    Ok(resources)
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
