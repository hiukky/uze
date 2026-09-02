//! Effective environment resource model.
//!
//! # What lives under `project/`
//!
//! Everything scoped to a *project directory* rather than to the machine:
//! what a project declares ([`project_lock`], [`worktree`]), where that
//! project begins ([`project_root`], [`workspace`]), the instruction
//! context it carries ([`context`], [`project_context`]), and the
//! mechanism for owning a slice of a file UZE did not write
//! ([`text_region`]).
//!
//! Module file names keep their full public spelling — `project/lock.rs`
//! would read better in the tree but would no longer match
//! `uze_core::project_lock`, and a name that changes between the inside and
//! the outside costs more than the prefix saves.
pub mod context;
pub mod project_context;
pub mod project_lock;
pub mod project_root;
pub mod text_region;
pub mod workspace;
pub mod worktree;

use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    capability::{Capability, CapabilityKind, Representation},
    error::{Result, UzeError},
    skill::{SkillInvocationPolicy, parse_skill_invocation},
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
    /// Standard-defined resource name when several resources share one
    /// manifest path (for example, named MCP servers in `mcp.json`).
    pub resource_name: Option<String>,
    /// The canonical invocation policy of a Skill resource, parsed from its
    /// SKILL.md `invoke:` frontmatter block at discovery time. `None` when
    /// the resource is not a Skill or declares no `invoke:` block — the
    /// canonical default (`model: true, user: true`) applies and the Skill
    /// behaves exactly as it always has (ADR-030, backward-compatibility
    /// gate). Always present on a package/project resource that is an
    /// `AgentSkill`; never a guess for anything else.
    pub skill_policy: Option<SkillInvocationPolicy>,
    /// A physical exposure name an Application-layer naming resolution has
    /// already chosen for this resource on one specific integration —
    /// existing-receipt reuse or fresh collision resolution against the
    /// ledger, neither of which `Resource`/`uze-core` performs itself. Set
    /// once, immediately before an attach call, on a short-lived clone of
    /// the resource; never persisted, never read back. `None` means "no
    /// resolution has happened yet" — an integration's `exposure_plan` then
    /// falls back to its own first-choice `exposure_name_candidates` entry,
    /// which is correct for informational/preview calls that never attach
    /// anything (`uze inspect`, tests, `assess_environment`).
    pub resolved_exposure_name: Option<String>,
    /// The artifact target (e.g., shim directory path) from an existing
    /// receipt, when `resolved_exposure_name` is set via existing-receipt
    /// reuse. This allows the integration's `exposure_plan` to reuse the
    /// exact same artifact rather than materializing a new one at a
    /// different path. Only populated for `ManagedArtifact::SymlinkReference`
    /// receipts; `None` otherwise.
    pub resolved_artifact_target: Option<PathBuf>,
}

impl Resource {
    pub fn from_project(root: PathBuf, capability: Capability) -> Self {
        let skill_policy = derive_skill_policy(&capability);
        Self {
            origin: ResourceOrigin::Project { root },
            capability,
            resource_name: None,
            skill_policy,
            resolved_exposure_name: None,
            resolved_artifact_target: None,
        }
    }

    pub fn from_package(id: PackageId, root: PathBuf, capability: Capability) -> Self {
        let skill_policy = derive_skill_policy(&capability);
        Self {
            origin: ResourceOrigin::Package { id, root },
            capability,
            resource_name: None,
            skill_policy,
            resolved_exposure_name: None,
            resolved_artifact_target: None,
        }
    }

    pub fn from_package_named(
        id: PackageId,
        root: PathBuf,
        capability: Capability,
        resource_name: String,
    ) -> Self {
        let skill_policy = derive_skill_policy(&capability);
        Self {
            origin: ResourceOrigin::Package { id, root },
            capability,
            resource_name: Some(resource_name),
            skill_policy,
            resolved_exposure_name: None,
            resolved_artifact_target: None,
        }
    }

    pub fn name(&self) -> String {
        self.resource_name
            .clone()
            .unwrap_or_else(|| self.capability.name())
    }

    pub fn package_root(&self) -> Option<&Path> {
        match &self.origin {
            ResourceOrigin::Package { root, .. } => Some(root),
            ResourceOrigin::Project { .. } => None,
        }
    }

    /// The bare, harness-agnostic logical name of this capability within
    /// its package — a Skill's directory name, or a named MCP server's
    /// name. This carries no vendor exposure semantics of its own: no
    /// package qualification, no collision-avoidance prefix, nothing a
    /// harness would see. What a harness's shared discovery location
    /// actually gets called is `IntegrationPort::exposure_name_candidates`'
    /// decision, built from this name — never computed here. `None` for a
    /// project-owned resource (no managed attachment at all) or a
    /// capability kind with no logical name of its own.
    pub fn logical_capability_name(&self) -> Option<String> {
        let ResourceOrigin::Package { .. } = &self.origin else {
            return None;
        };
        match self.capability.kind {
            // A skill's path is `skills/<skill-name>/SKILL.md`: the parent
            // directory name is the skill's own logical name.
            CapabilityKind::AgentSkill => {
                let skill_name = self.capability.path.parent()?.file_name()?.to_str()?;
                Some(skill_name.to_owned())
            }
            CapabilityKind::Mcp => self.resource_name.clone(),
            CapabilityKind::Agent => self
                .capability
                .path
                .file_stem()
                .and_then(|name| name.to_str())
                .map(str::to_owned),
            CapabilityKind::Hook => self.resource_name.clone(),
            _ => None,
        }
    }

    /// The canonical invocation policy that governs this resource. A Skill
    /// with no `invoke:` declaration gets the canonical default
    /// (model + user): existing Skills behave exactly as before (ADR-030).
    pub fn skill_invocation(&self) -> SkillInvocationPolicy {
        self.skill_policy
            .unwrap_or(SkillInvocationPolicy::MODEL_AND_USER)
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
            ResourceOrigin::Package { id, root } => {
                let path = self
                    .capability
                    .path
                    .strip_prefix(root)
                    .unwrap_or(&self.capability.path);
                match &self.resource_name {
                    Some(name) => format!("package:{}:{}:{name}", id.as_str(), path.display()),
                    None => format!("package:{}:{}", id.as_str(), path.display()),
                }
            }
        }
    }
}

/// One derivation point for every `Resource` constructor. A Skill without
/// an `invoke:` block yields `None` (canonical default applies); an
/// explicit declaration — even invalid — is preserved so consumers can
/// refuse to project it instead of silently changing its meaning.
fn derive_skill_policy(capability: &Capability) -> Option<SkillInvocationPolicy> {
    if capability.kind != CapabilityKind::AgentSkill {
        return None;
    }
    parse_skill_invocation(&capability.payload)
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
    discover_agents(&root, &mut project_resources)?;
    discover_mcp(&root, &mut project_resources)?;
    discover_hooks(&root, &mut project_resources)?;
    project_resources.sort_by(|left, right| left.path.cmp(&right.path));

    Ok(EffectiveEnvironment {
        root: root.clone(),
        resources: project_resources
            .into_iter()
            .map(|capability| Resource::from_project(root.clone(), capability))
            .collect(),
    })
}

fn discover_agents(root: &Path, items: &mut Vec<Capability>) -> Result<()> {
    let agents_root = root.join(".agents/agents");
    if !agents_root.is_dir() {
        return Ok(());
    }
    for path in files_with_extension(&agents_root, "md")? {
        push_file(items, path, CapabilityKind::Agent)?;
    }
    Ok(())
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

fn discover_hooks(root: &Path, items: &mut Vec<Capability>) -> Result<()> {
    let path = root.join("hooks.json");
    if !path.is_file() {
        return Ok(());
    }
    let payload = read_file(&path)?;
    // Validate now, just as project MCP discovery validates its manifest.
    // Project resources deliberately keep the authored bytes as one local
    // capability; package delivery expands groups into stable named resources.
    crate::hook::parse_manifest(&path, &payload)?;
    items.push(Capability {
        kind: CapabilityKind::Hook,
        representation: Representation::Standard,
        path,
        payload,
    });
    Ok(())
}

/// Walks `root` for files with an exact name, **never descending into a
/// symlinked directory**.
///
/// That single rule is what makes this traversal cycle-free by construction:
/// a directory cycle needs at least one symlink in the loop, and no symlink
/// is ever entered. It costs nothing and needs no visited-set or repeated
/// canonicalization.
///
/// The rule matters because discovery runs over package content. A package
/// is already required to be self-contained, but self-contained is not the
/// same as acyclic — `a -> b`, `b -> a` never leaves the package root and
/// would still spin here forever. With remote acquisition that stops being a
/// strange local package and becomes a repository able to hang the Engine.
///
/// Consequence worth stating: a file reachable *only* through a symlinked
/// directory is not discovered. The symlink itself is still preserved
/// verbatim in the Store as package content — this changes what discovery
/// walks, not what a package may contain.
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
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            // `symlink_metadata` deliberately, not `is_dir()`: the latter
            // follows the link and is exactly how a cycle gets entered.
            let metadata = fs::symlink_metadata(&path).map_err(|source| UzeError::Read {
                path: path.clone(),
                source,
            })?;
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                pending.push(path);
            } else if path.file_name().and_then(|name| name.to_str()) == Some(expected_name) {
                matches.push(path);
            }
        }
    }
    matches.sort();
    Ok(matches)
}

/// Walks a non-symlinked directory tree for ordinary files with an extension.
/// This shares `files_named`'s cycle-safety contract while allowing the
/// canonical Agent surface to retain its meaningful Markdown filenames.
pub fn files_with_extension(root: &Path, extension: &str) -> Result<Vec<PathBuf>> {
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
        entries.sort_by_key(std::fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|source| UzeError::Read {
                path: path.clone(),
                source,
            })?;
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                pending.push(path);
            } else if path.extension().and_then(|value| value.to_str()) == Some(extension) {
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
    fn package_skill_resource_has_a_bare_logical_capability_name() {
        let id = PackageId::from_plugin_name("demo-package", Path::new("plugin.json")).unwrap();
        let resource = Resource::from_package(
            id,
            PathBuf::from("/uze-home/store/packages/demo-package"),
            skill_capability("/uze-home/store/packages/demo-package/skills/demo-skill/SKILL.md"),
        );
        // Bare — no package qualification, no "uze-" prefix. Physical
        // exposure naming (with qualification when needed) is decided by
        // `IntegrationPort::exposure_name_candidates`, not here.
        assert_eq!(
            resource.logical_capability_name().as_deref(),
            Some("demo-skill")
        );
    }

    #[test]
    fn project_resource_has_no_logical_capability_name() {
        let resource = Resource::from_project(
            PathBuf::from("/project"),
            skill_capability("/project/.agents/skills/demo-skill/SKILL.md"),
        );
        assert_eq!(resource.logical_capability_name(), None);
    }

    #[test]
    fn mcp_package_resource_logical_name_is_the_server_name() {
        let id = PackageId::from_plugin_name("demo-package", Path::new("plugin.json")).unwrap();
        let resource = Resource::from_package_named(
            id,
            PathBuf::from("/uze-home/store/packages/demo-package"),
            Capability {
                kind: CapabilityKind::Mcp,
                representation: Representation::Standard,
                path: PathBuf::from("/uze-home/store/packages/demo-package/mcp.json"),
                payload: Vec::new(),
            },
            "github".to_owned(),
        );
        assert_eq!(
            resource.logical_capability_name().as_deref(),
            Some("github")
        );
    }

    #[test]
    fn agent_package_resource_logical_name_is_its_markdown_stem() {
        let id = PackageId::from_plugin_name("demo-package", Path::new("plugin.json")).unwrap();
        let resource = Resource::from_package(
            id,
            PathBuf::from("/uze-home/store/packages/demo-package"),
            Capability {
                kind: CapabilityKind::Agent,
                representation: Representation::Standard,
                path: PathBuf::from("/uze-home/store/packages/demo-package/agents/reviewer.md"),
                payload: Vec::new(),
            },
        );
        assert_eq!(
            resource.logical_capability_name().as_deref(),
            Some("reviewer")
        );
    }

    #[test]
    fn skill_policy_is_derived_from_the_skill_payload_at_construction() {
        let id = PackageId::from_plugin_name("demo-package", Path::new("plugin.json")).unwrap();
        let capability = Capability {
            kind: CapabilityKind::AgentSkill,
            representation: Representation::Standard,
            path: PathBuf::from("/uze-home/store/packages/demo-package/skills/demo-skill/SKILL.md"),
            payload: b"---\ninvoke:\n  model: false\n  user: true\n---\nbody\n".to_vec(),
        };
        let resource = Resource::from_package(
            id,
            PathBuf::from("/uze-home/store/packages/demo-package"),
            capability,
        );
        assert_eq!(
            resource.skill_policy,
            Some(crate::skill::SkillInvocationPolicy::USER_ONLY)
        );
        assert_eq!(
            resource.skill_invocation(),
            crate::skill::SkillInvocationPolicy::USER_ONLY
        );
    }

    #[test]
    fn skill_without_invocation_block_defaults_and_is_not_reattached() {
        let id = PackageId::from_plugin_name("demo-package", Path::new("plugin.json")).unwrap();
        let capability = Capability {
            kind: CapabilityKind::AgentSkill,
            representation: Representation::Standard,
            path: PathBuf::from("/uze-home/store/packages/demo-package/skills/demo-skill/SKILL.md"),
            payload: b"---\nname: demo-skill\n---\nbody\n".to_vec(),
        };
        let resource = Resource::from_package(
            id,
            PathBuf::from("/uze-home/store/packages/demo-package"),
            capability,
        );
        assert_eq!(resource.skill_policy, None);
        assert_eq!(
            resource.skill_invocation(),
            crate::skill::SkillInvocationPolicy::MODEL_AND_USER,
            "absent policy keeps the exact prior default behavior"
        );
    }

    #[test]
    fn non_skill_resources_have_no_policy() {
        let id = PackageId::from_plugin_name("demo-package", Path::new("plugin.json")).unwrap();
        let resource = Resource::from_package_named(
            id,
            PathBuf::from("/uze-home/store/packages/demo-package"),
            Capability {
                kind: CapabilityKind::Mcp,
                representation: Representation::Standard,
                path: PathBuf::from("/uze-home/store/packages/demo-package/mcp.json"),
                payload: Vec::new(),
            },
            "github".to_owned(),
        );
        assert_eq!(resource.skill_policy, None);
    }

    #[test]
    fn a_project_hook_manifest_is_discovered_without_rewriting_its_bytes() {
        let root = uze_testkit::temp::scratch("project-hook");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bytes = br#"{"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"scripts/check"}]}]}}"#;
        std::fs::write(root.join("hooks.json"), bytes).unwrap();
        let environment = resolve_project(&root).unwrap();
        let hook = environment
            .resources
            .iter()
            .find(|resource| resource.capability.kind == CapabilityKind::Hook)
            .unwrap();
        assert_eq!(hook.capability.payload, bytes);
        std::fs::remove_dir_all(root).unwrap();
    }
}
