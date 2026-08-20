// ADR-005: the Engine composes peer-harness inputs without named harness rules.
use crate::{
    capability::{Capability, CapabilityKind, Representation},
    error::Result,
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
        resources.sort_by_key(|resource| resource.identity());
        Ok(resources)
    }
}
