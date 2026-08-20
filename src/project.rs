use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    capability::{Capability, CapabilityKind, Representation},
    error::{Result, UzeError},
    store::PackageId,
};

#[derive(Clone, Debug)]
pub struct EffectiveEnvironment {
    pub root: PathBuf,
    pub resources: Vec<Resource>,
}

pub type ResolvedProject = EffectiveEnvironment;

/// Origin is distinct from representation. A `STANDARD` SKILL.md can be
/// project-owned or package-owned; that fact alone says nothing about how a
/// harness will receive it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResourceOrigin {
    Project { root: PathBuf },
    Package { id: PackageId, root: PathBuf },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Resource {
    pub origin: ResourceOrigin,
    pub capability: Capability,
}

impl Resource {
    pub fn from_project(root: PathBuf, capability: Capability) -> Self {
        Self {
            origin: ResourceOrigin::Project { root },
            capability,
        }
    }

    pub fn from_package(id: PackageId, root: PathBuf, capability: Capability) -> Self {
        Self {
            origin: ResourceOrigin::Package { id, root },
            capability,
        }
    }

    pub fn package_root(&self) -> Option<&Path> {
        match &self.origin {
            ResourceOrigin::Package { root, .. } => Some(root),
            ResourceOrigin::Project { .. } => None,
        }
    }

    /// A stable, namespaced entry name safe to place in a shared, ambient
    /// harness discovery location (e.g. a global skills directory) without
    /// colliding with unrelated pre-existing entries there. `None` for a
    /// project-owned resource, which is not a UZE store package and has no
    /// managed attachment.
    pub fn attachment_entry_name(&self) -> Option<String> {
        let ResourceOrigin::Package { id, .. } = &self.origin else {
            return None;
        };
        match self.capability.kind {
            // A skill's path is `skills/<skill-name>/SKILL.md`: the parent
            // directory name distinguishes multiple skills in one package.
            CapabilityKind::AgentSkill => {
                let skill_name = self.capability.path.parent()?.file_name()?.to_str()?;
                Some(format!("uze-{}-{}", id.as_str(), skill_name))
            }
            // `mcp.json` sits at the package root, so its parent is the
            // package directory itself — using it would just repeat the
            // package id. The package id alone is enough today, since a
            // package declares at most one MCP resource (see ADR-007).
            CapabilityKind::Mcp => Some(format!("uze-{}", id.as_str())),
            _ => None,
        }
    }

    pub fn display_path(&self, environment_root: &Path) -> String {
        self.capability.display_path(environment_root)
    }

    /// A stable operational identity for a resource in one composed
    /// environment. It deliberately retains the external standard payload at
    /// its original path instead of creating a UZE-specific skill format.
    pub fn identity(&self) -> String {
        match &self.origin {
            ResourceOrigin::Project { root } => format!(
                "project:{}",
                self.capability
                    .path
                    .strip_prefix(root)
                    .unwrap_or(&self.capability.path)
                    .display()
            ),
            ResourceOrigin::Package { id, root } => format!(
                "package:{}:{}",
                id.as_str(),
                self.capability
                    .path
                    .strip_prefix(root)
                    .unwrap_or(&self.capability.path)
                    .display()
            ),
        }
    }
}

pub fn resolve_project(root: impl AsRef<Path>) -> Result<EffectiveEnvironment> {
    let root = root.as_ref();
    if !root.exists() {
        return Err(UzeError::MissingPath(root.to_path_buf()));
    }
    if !root.is_dir() {
        return Err(UzeError::NotDirectory(root.to_path_buf()));
    }

    let root = root.canonicalize().map_err(|source| UzeError::Read {
        path: root.to_path_buf(),
        source,
    })?;
    let mut project_resources = Vec::new();

    discover_instructions(&root, &mut project_resources)?;
    discover_skills(&root, &mut project_resources)?;
    discover_mcp(&root, &mut project_resources)?;
    project_resources.sort_by(|left, right| left.path.cmp(&right.path));

    Ok(EffectiveEnvironment {
        root: root.clone(),
        resources: project_resources
            .into_iter()
            .map(|capability| Resource::from_project(root.clone(), capability))
            .collect(),
    })
}

/// Resolves only resources owned by a project. `UzeEngine` is responsible for
/// combining this source with UZE-installed package resources into the one
/// effective environment used by the product.
pub fn resolve_project_resources(root: impl AsRef<Path>) -> Result<EffectiveEnvironment> {
    resolve_project(root)
}

fn discover_instructions(root: &Path, items: &mut Vec<Capability>) -> Result<()> {
    for path in files_named(root, "AGENTS.md")? {
        push_file(items, path, CapabilityKind::Instruction)?;
    }
    Ok(())
}

fn discover_skills(root: &Path, items: &mut Vec<Capability>) -> Result<()> {
    let skills_root = root.join(".agents/skills");
    if !skills_root.is_dir() {
        return Ok(());
    }

    for path in files_named(&skills_root, "SKILL.md")? {
        push_file(items, path, CapabilityKind::AgentSkill)?;
    }
    Ok(())
}

fn discover_mcp(root: &Path, items: &mut Vec<Capability>) -> Result<()> {
    for name in ["mcp.json", ".mcp.json"] {
        let path = root.join(name);
        if path.is_file() {
            let payload = read_file(&path)?;
            serde_json::from_slice::<serde_json::Value>(&payload).map_err(|source| {
                UzeError::Json {
                    path: path.clone(),
                    source,
                }
            })?;
            items.push(Capability {
                kind: CapabilityKind::Mcp,
                representation: Representation::Standard,
                path,
                payload,
            });
        }
    }
    Ok(())
}

pub fn files_named(root: &Path, expected_name: &str) -> Result<Vec<PathBuf>> {
    let mut pending = vec![root.to_path_buf()];
    let mut matches = Vec::new();
    while let Some(directory) = pending.pop() {
        let mut entries = fs::read_dir(&directory)
            .map_err(|source| UzeError::Read {
                path: directory.clone(),
                source,
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|source| UzeError::Read {
                path: directory.clone(),
                source,
            })?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.file_name().and_then(|name| name.to_str()) == Some(expected_name) {
                matches.push(path);
            }
        }
    }
    matches.sort();
    Ok(matches)
}

pub fn read_file(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|source| UzeError::Read {
        path: path.to_path_buf(),
        source,
    })
}

pub(crate) fn push_file(
    items: &mut Vec<Capability>,
    path: PathBuf,
    kind: CapabilityKind,
) -> Result<()> {
    items.push(Capability {
        kind,
        representation: Representation::Standard,
        payload: read_file(&path)?,
        path,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::PackageId;

    fn skill_capability(path: &str) -> Capability {
        Capability {
            kind: CapabilityKind::AgentSkill,
            representation: Representation::Standard,
            path: PathBuf::from(path),
            payload: Vec::new(),
        }
    }

    #[test]
    fn package_resource_has_a_namespaced_attachment_entry_name() {
        let id = PackageId::from_plugin_name("demo-package", Path::new("plugin.json")).unwrap();
        let resource = Resource::from_package(
            id,
            PathBuf::from("/uze-home/store/packages/demo-package"),
            skill_capability("/uze-home/store/packages/demo-package/skills/demo-skill/SKILL.md"),
        );
        assert_eq!(
            resource.attachment_entry_name().as_deref(),
            Some("uze-demo-package-demo-skill")
        );
    }

    #[test]
    fn project_resource_has_no_attachment_entry_name() {
        let resource = Resource::from_project(
            PathBuf::from("/project"),
            skill_capability("/project/.agents/skills/demo-skill/SKILL.md"),
        );
        assert_eq!(resource.attachment_entry_name(), None);
    }

    #[test]
    fn mcp_package_resource_uses_the_package_id_alone() {
        let id = PackageId::from_plugin_name("demo-package", Path::new("plugin.json")).unwrap();
        let resource = Resource::from_package(
            id,
            PathBuf::from("/uze-home/store/packages/demo-package"),
            Capability {
                kind: CapabilityKind::Mcp,
                representation: Representation::Standard,
                path: PathBuf::from("/uze-home/store/packages/demo-package/mcp.json"),
                payload: Vec::new(),
            },
        );
        assert_eq!(
            resource.attachment_entry_name().as_deref(),
            Some("uze-demo-package")
        );
    }
}
