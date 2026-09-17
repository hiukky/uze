//! Canonical capability identities.
//!
//! # What lives under `capability/`
//!
//! One module per *canonical capability* — a thing a plugin declares once,
//! portably, which UZE then translates into each harness's own encoding.
//! [`skill`] is the canonical capability; [`hook`] is the portable Hook
//! surface (ADR-033). Neither is a harness's own mechanism: they are the
//! vendor-neutral statement of intent that `delivery` projects outward.
pub mod hook;
pub mod skill;

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::{
    skill::{SkillInvocationPolicy, parse_skill_invocation},
    store::PackageId,
};

/// Canonical capability identities. Vendor-neutral: a kind says what a
/// capability *is*, never which harness consumes it or how.
///
/// There is exactly one Skill-family capability kind: `AgentSkill`. Whether
/// a Skill is model-discoverable background knowledge, an explicit
/// user-only action, or both is carried by its invocation policy
/// (`crate::skill::SkillInvocationPolicy`, declared in the Skill's
/// `invoke:` frontmatter block), never by a second capability kind —
/// ADR-030. Vendor resource taxonomy (Claude `commands/`, OpenCode custom
/// commands, Codex explicit-only skills) is a projection detail owned by
/// integrations, not a canonical concept. (`Command` was removed as a
/// canonical capability by ADR-030.)
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityKind {
    Instruction,
    AgentSkill,
    Mcp,
    Agent,
    Hook,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Capability {
    pub kind: CapabilityKind,
    pub path: PathBuf,
    pub payload: Vec<u8>,
}

impl Capability {
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown")
            .to_owned()
    }

    pub fn display_path(&self, root: &Path) -> String {
        self.path
            .strip_prefix(root)
            .unwrap_or(&self.path)
            .to_string_lossy()
            .into_owned()
    }
}

/// One capability an installed package contributes, as delivery sees it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Resource {
    pub package_id: PackageId,
    /// Where the package's bytes live in the Store.
    pub package_root: PathBuf,
    pub capability: Capability,
    /// Standard-defined resource name when several resources share one
    /// manifest path (for example, named MCP servers in `mcp.json`).
    pub resource_name: Option<String>,
    /// The canonical invocation policy of a Skill resource, parsed from its
    /// SKILL.md `invoke:` frontmatter block at discovery time. `None` when
    /// the resource is not a Skill or declares no `invoke:` block — the
    /// canonical default (`model: true, user: true`) then applies (ADR-030).
    pub skill_policy: Option<SkillInvocationPolicy>,
    /// A physical exposure name an Application-layer naming resolution has
    /// already chosen for this resource on one specific integration —
    /// existing-receipt reuse or fresh collision resolution against the
    /// ledger, neither of which `uze-core` performs itself. Set once,
    /// immediately before an attach call, on a short-lived clone of the
    /// resource; never persisted. `None` means no resolution has happened
    /// yet, and an integration's `exposure_plan` falls back to its own
    /// first-choice `exposure_name_candidates` entry — correct for preview
    /// calls that never attach anything.
    pub resolved_exposure_name: Option<String>,
    /// The artifact target (e.g. a shim directory) from an existing
    /// `SymlinkReference` receipt, when `resolved_exposure_name` came from
    /// reusing it, so `exposure_plan` reuses the exact same artifact rather
    /// than materializing a new one at a different path.
    pub resolved_artifact_target: Option<PathBuf>,
}

impl Resource {
    pub fn from_package(
        package_id: PackageId,
        package_root: PathBuf,
        capability: Capability,
    ) -> Self {
        let skill_policy = derive_skill_policy(&capability);
        Self {
            package_id,
            package_root,
            capability,
            resource_name: None,
            skill_policy,
            resolved_exposure_name: None,
            resolved_artifact_target: None,
        }
    }

    pub fn from_package_named(
        package_id: PackageId,
        package_root: PathBuf,
        capability: Capability,
        resource_name: String,
    ) -> Self {
        Self {
            resource_name: Some(resource_name),
            ..Self::from_package(package_id, package_root, capability)
        }
    }

    pub fn name(&self) -> String {
        self.resource_name
            .clone()
            .unwrap_or_else(|| self.capability.name())
    }

    /// The bare, harness-agnostic logical name of this capability within
    /// its package — a Skill's directory name, or a named MCP server's
    /// name. No package qualification and no vendor exposure semantics:
    /// what a harness's discovery location calls it is
    /// `IntegrationPort::exposure_name_candidates`' decision, built from
    /// this name. `None` for a capability kind with no logical name.
    pub fn logical_capability_name(&self) -> Option<String> {
        match self.capability.kind {
            // A skill's path is `skills/<skill-name>/SKILL.md`: the parent
            // directory name is the skill's own logical name.
            CapabilityKind::AgentSkill => {
                let skill_name = self.capability.path.parent()?.file_name()?.to_str()?;
                Some(skill_name.to_owned())
            }
            CapabilityKind::Mcp | CapabilityKind::Hook => self.resource_name.clone(),
            CapabilityKind::Agent => self
                .capability
                .path
                .file_stem()
                .and_then(|name| name.to_str())
                .map(str::to_owned),
            CapabilityKind::Instruction => None,
        }
    }

    /// The canonical invocation policy that governs this resource. A Skill
    /// with no `invoke:` declaration gets the canonical default
    /// (model + user) (ADR-030).
    pub fn skill_invocation(&self) -> SkillInvocationPolicy {
        self.skill_policy
            .unwrap_or(SkillInvocationPolicy::MODEL_AND_USER)
    }

    pub fn display_path(&self, environment_root: &Path) -> String {
        self.capability.display_path(environment_root)
    }

    /// A stable operational identity for a resource. It retains the
    /// external standard payload at its original path instead of creating a
    /// UZE-specific skill format.
    pub fn identity(&self) -> String {
        let path = self
            .capability
            .path
            .strip_prefix(&self.package_root)
            .unwrap_or(&self.capability.path);
        match &self.resource_name {
            Some(name) => format!(
                "package:{}:{}:{name}",
                self.package_id.as_str(),
                path.display()
            ),
            None => format!("package:{}:{}", self.package_id.as_str(), path.display()),
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

#[cfg(test)]
mod resource_tests {
    use super::*;

    fn skill_capability(path: &str) -> Capability {
        Capability {
            kind: CapabilityKind::AgentSkill,
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
    fn mcp_package_resource_logical_name_is_the_server_name() {
        let id = PackageId::from_plugin_name("demo-package", Path::new("plugin.json")).unwrap();
        let resource = Resource::from_package_named(
            id,
            PathBuf::from("/uze-home/store/packages/demo-package"),
            Capability {
                kind: CapabilityKind::Mcp,
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
                path: PathBuf::from("/uze-home/store/packages/demo-package/mcp.json"),
                payload: Vec::new(),
            },
            "github".to_owned(),
        );
        assert_eq!(resource.skill_policy, None);
    }
}
