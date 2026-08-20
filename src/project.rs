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
