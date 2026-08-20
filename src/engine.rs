use crate::{
    capability::{Capability, CapabilityKind, Representation},
    error::Result,
    project::{EffectiveEnvironment, Resource},
    store::{PackageId, UzeStore},
};

/// Composes an effective environment from packages already registered in the
/// UZE store. Project discovery remains a separate, native-discovery concern.
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

    pub fn compose(&self, packages: &[PackageId]) -> Result<EffectiveEnvironment> {
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
        resources.sort_by(|left, right| left.capability.path.cmp(&right.capability.path));
        Ok(EffectiveEnvironment {
            root: self.store.home().root().to_path_buf(),
            resources,
        })
    }
}
