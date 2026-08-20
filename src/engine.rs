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
            let skills_root = package.root.join("skills");
            if skills_root.is_dir() {
                for path in crate::project::files_named(&skills_root, "SKILL.md")? {
                    let payload = crate::project::read_file(&path)?;
                    resources.push(Resource::from_package(
                        package.id.clone(),
                        package.root.clone(),
                        Capability {
                            kind: CapabilityKind::AgentSkill,
                            representation: Representation::Standard,
                            path,
                            payload,
                        },
                    ));
                }
            }
            resources.extend(mcp_resources(&package.id, &package.root)?);
        }
        resources.sort_by_key(|resource| resource.identity());
        Ok(resources)
    }
}

/// Discovers a package's optional root-level `mcp.json` (Agent Plugins 1.0
/// shape: `{"mcpServers": {"<name>": {"command", "args", ...}}}`, the same
/// convention Claude Code's and Codex's own plugin systems already expect —
/// see ADR-007) into one `Resource` per declared server. A package
/// declaring more than one server produces resources that share one
/// `Resource::identity()` (they share the same `mcp.json` path) — an
/// accepted limitation, not solved here; the tracer bullet needs exactly
/// one server per package.
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
        .map(|(_name, config)| {
            let payload = serde_json::to_vec(config)
                .expect("mcp server config re-serialization is infallible");
            Ok(Resource::from_package(
                id.clone(),
                package_root.to_path_buf(),
                Capability {
                    kind: CapabilityKind::Mcp,
                    representation: Representation::Standard,
                    path: manifest_path.clone(),
                    payload,
                },
            ))
        })
        .collect()
}
