//! Canonical capability identities and preserved representations.

use std::path::{Path, PathBuf};

use serde::Serialize;

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
    Policy,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Representation {
    Standard,
    Native,
    Uze,
    Foreign,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Capability {
    pub kind: CapabilityKind,
    pub representation: Representation,
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
