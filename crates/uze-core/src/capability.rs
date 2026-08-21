//! Canonical capability identities and preserved representations.

use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityKind {
    Instruction,
    AgentSkill,
    Mcp,
    Agent,
    Action,
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
