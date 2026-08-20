use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PortableKind {
    Instruction,
    Skill,
    Mcp,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EnhancementKind {
    Command,
    Hook,
    Subagent,
    Permission,
    VendorDirectory,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "scope", content = "kind")]
pub enum ItemKind {
    Portable(PortableKind),
    Enhancement(EnhancementKind),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectItem {
    pub kind: ItemKind,
    pub path: PathBuf,
    pub payload: Vec<u8>,
}

impl ProjectItem {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Harness {
    ClaudeCode,
    Codex,
    Cursor,
    OpenCode,
}

impl Harness {
    pub const ALL: [Self; 4] = [Self::ClaudeCode, Self::Codex, Self::Cursor, Self::OpenCode];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
            Self::Cursor => "Cursor",
            Self::OpenCode => "OpenCode",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Classification {
    Standard,
    Native,
    Adaptable,
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ClassificationResult {
    pub classification: Classification,
    pub rationale: String,
    pub evidence_source: &'static str,
}

pub fn classify(item: &ProjectItem, harness: Harness) -> ClassificationResult {
    match &item.kind {
        ItemKind::Portable(PortableKind::Instruction) if harness == Harness::ClaudeCode => {
            ClassificationResult {
                classification: Classification::Unsupported,
                rationale: "Claude Code has no verified native AGENTS.md discovery; no safe implicit translation is performed.".to_owned(),
                evidence_source: "research-notes.md §1 (Instructions)",
            }
        }
        ItemKind::Portable(kind) => ClassificationResult {
            classification: Classification::Standard,
            rationale: format!("{} is consumed as a portable open-standard item without UZE transformation.", standard_name(kind)),
            evidence_source: standard_evidence_source(kind),
        },
        ItemKind::Enhancement(kind)
            if path_belongs_to_harness(&item.path, harness)
                && native_enhancement_evidence(harness, kind) =>
        {
            ClassificationResult {
                classification: Classification::Native,
                rationale: format!("{} is a native optional enhancement for {} and does not alter the portable core.", enhancement_name(kind), harness.as_str()),
                evidence_source: "research-notes.md §1 (master capability matrix)",
            }
        }
        ItemKind::Enhancement(kind) => ClassificationResult {
            classification: Classification::Unsupported,
            rationale: format!("{} has no verified safe equivalent for {}; directory generation alone is not treated as adaptation.", enhancement_name(kind), harness.as_str()),
            evidence_source: "research-notes.md §1 (master capability matrix)",
        },
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct HarnessEvidence {
    pub harness: Harness,
    pub instructions: Classification,
    pub skills: Classification,
    pub mcp: Classification,
    pub commands: Classification,
    pub hooks: Classification,
    pub subagents: Classification,
    pub permissions: Classification,
    pub source: &'static str,
}

pub fn harness_evidence() -> [HarnessEvidence; 4] {
    let source = "research-notes.md §1 (master capability matrix)";
    [
        HarnessEvidence {
            harness: Harness::ClaudeCode,
            instructions: Classification::Unsupported,
            skills: Classification::Standard,
            mcp: Classification::Standard,
            commands: Classification::Native,
            hooks: Classification::Native,
            subagents: Classification::Native,
            permissions: Classification::Native,
            source,
        },
        HarnessEvidence {
            harness: Harness::Codex,
            instructions: Classification::Standard,
            skills: Classification::Standard,
            mcp: Classification::Standard,
            commands: Classification::Unsupported,
            hooks: Classification::Unsupported,
            subagents: Classification::Unsupported,
            permissions: Classification::Native,
            source,
        },
        HarnessEvidence {
            harness: Harness::Cursor,
            instructions: Classification::Standard,
            skills: Classification::Standard,
            mcp: Classification::Standard,
            commands: Classification::Unsupported,
            hooks: Classification::Unsupported,
            subagents: Classification::Native,
            permissions: Classification::Native,
            source,
        },
        HarnessEvidence {
            harness: Harness::OpenCode,
            instructions: Classification::Standard,
            skills: Classification::Standard,
            mcp: Classification::Standard,
            commands: Classification::Native,
            hooks: Classification::Unsupported,
            subagents: Classification::Native,
            permissions: Classification::Native,
            source,
        },
    ]
}

fn standard_name(kind: &PortableKind) -> &'static str {
    match kind {
        PortableKind::Instruction => "AGENTS.md",
        PortableKind::Skill => "Agent Skills",
        PortableKind::Mcp => "MCP",
    }
}

fn standard_evidence_source(kind: &PortableKind) -> &'static str {
    match kind {
        PortableKind::Instruction => "research-notes.md §1 (Instructions)",
        PortableKind::Skill => "research-notes.md §1 (Skills)",
        PortableKind::Mcp => "research-notes.md §1 (MCP)",
    }
}

fn enhancement_name(kind: &EnhancementKind) -> &'static str {
    match kind {
        EnhancementKind::Command => "Command",
        EnhancementKind::Hook => "Hook",
        EnhancementKind::Subagent => "Subagent",
        EnhancementKind::Permission => "Permission rule",
        EnhancementKind::VendorDirectory => "Vendor directory",
    }
}

fn path_belongs_to_harness(path: &Path, harness: Harness) -> bool {
    let expected = match harness {
        Harness::ClaudeCode => ".claude",
        Harness::Codex => ".codex",
        Harness::Cursor => ".cursor",
        Harness::OpenCode => ".opencode",
    };

    path.components()
        .any(|component| component.as_os_str() == expected)
}

fn native_enhancement_evidence(harness: Harness, kind: &EnhancementKind) -> bool {
    let evidence = harness_evidence();
    let evidence = evidence
        .iter()
        .find(|entry| entry.harness == harness)
        .expect("every active harness has evidence");
    match kind {
        EnhancementKind::Command => evidence.commands == Classification::Native,
        EnhancementKind::Hook => evidence.hooks == Classification::Native,
        EnhancementKind::Subagent => evidence.subagents == Classification::Native,
        EnhancementKind::Permission => evidence.permissions == Classification::Native,
        EnhancementKind::VendorDirectory => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_hook_is_not_promoted_to_an_opencode_equivalent() {
        let hook = ProjectItem {
            kind: ItemKind::Enhancement(EnhancementKind::Hook),
            path: PathBuf::from(".claude/hooks/pre-tool-use.sh"),
            payload: Vec::new(),
        };

        let result = classify(&hook, Harness::OpenCode);
        assert_eq!(result.classification, Classification::Unsupported);
        assert!(result.rationale.contains("no verified safe equivalent"));
    }
}
