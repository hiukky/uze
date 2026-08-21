//! Core trust boundary: the one question acquisition made necessary: *should this package be
//! allowed to introduce code a harness will execute?*
//!
//! `uze add ./plugin` has always been able to register an MCP server, but the
//! operator had the directory in front of them. `uze add <url>` removes that,
//! so installing stops meaning "copy some files" and starts meaning
//! "authorize execution of something you have not read". UZE is what performs
//! the introduction, so UZE is where the question belongs.
//!
//! Two things this deliberately is **not**:
//!
//! - It is not a permission system. There is no policy language, no stored
//!   grant history, no per-capability rules. It makes one boundary visible.
//! - It is not a check for executable files. A repository full of scripts
//!   nobody invokes is inert; a single declared MCP `command` is not. The
//!   boundary is *a capability that introduces process execution*, which for
//!   M2 means MCP.

use serde::Serialize;

use crate::{capability::CapabilityKind, project::Resource};

/// One capability that will cause a process to run once a harness picks it
/// up. Carries what a person needs to judge it, and nothing else.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ExecutableCapability {
    pub name: String,
    pub command: String,
    pub arguments: Vec<String>,
}

/// What the operator is being asked to authorize.
///
/// Structured rather than pre-rendered so every front end — CLI now, TUI
/// later — asks the same question from the same facts instead of
/// reimplementing the judgement.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TrustRequest {
    pub package_id: String,
    /// What the operator asked for.
    pub requested_source: String,
    /// What it resolved to — the immutable revision, when there is one.
    pub resolved_source: String,
    pub executable: Vec<ExecutableCapability>,
    /// Set when this package is already installed and the update introduces
    /// execution it did not previously have. Consent is not inherited just
    /// because the package id is unchanged.
    pub previously_trusted: bool,
}

/// The answer, and the reason it is three-valued.
///
/// `Unavailable` is not `Denied`: a CI run that cannot ask must fail with a
/// signal a pipeline can act on, not look like an operator who said no.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrustOutcome {
    Granted,
    Denied,
    Unavailable,
}

/// Whoever can answer the question. The Application asks; it never decides.
pub trait TrustAuthority {
    fn authorize(&self, request: &TrustRequest) -> TrustOutcome;
}

/// Grants without asking. For a caller that has already obtained consent out
/// of band — an explicit `--trust` flag, or a package with nothing executable
/// to authorize.
pub struct AlwaysTrust;

impl TrustAuthority for AlwaysTrust {
    fn authorize(&self, _request: &TrustRequest) -> TrustOutcome {
        TrustOutcome::Granted
    }
}

/// Cannot ask. The correct authority for a non-interactive process: it turns
/// an unanswerable question into a structured failure rather than a silent
/// yes.
pub struct NoTrustAuthority;

impl TrustAuthority for NoTrustAuthority {
    fn authorize(&self, _request: &TrustRequest) -> TrustOutcome {
        TrustOutcome::Unavailable
    }
}

/// Extracts the capabilities that introduce process execution.
///
/// Only MCP servers declaring a `command` qualify today. A Skill is text a
/// model reads; it carries no execution of its own, so it needs no consent
/// beyond installing the package at all.
pub fn executable_capabilities(resources: &[&Resource]) -> Vec<ExecutableCapability> {
    resources
        .iter()
        .filter(|resource| resource.capability.kind == CapabilityKind::Mcp)
        .filter_map(|resource| {
            let config: serde_json::Value =
                serde_json::from_slice(&resource.capability.payload).ok()?;
            let command = config.get("command")?.as_str()?.to_owned();
            let arguments = config
                .get("args")
                .and_then(serde_json::Value::as_array)
                .map(|values| {
                    values
                        .iter()
                        .filter_map(serde_json::Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default();
            Some(ExecutableCapability {
                name: resource.name(),
                command,
                arguments,
            })
        })
        .collect()
}

/// Whether an update introduces execution the installed package did not
/// already have.
///
/// Compares the whole invocation, not just presence: a server whose command
/// or arguments changed materially is a new thing to authorize, even though
/// the package id and the capability name are unchanged. This is not a
/// permission history — it exists so a change cannot pass unseen.
pub fn introduces_new_execution(
    previous: &[ExecutableCapability],
    next: &[ExecutableCapability],
) -> bool {
    next.iter().any(|candidate| !previous.contains(candidate))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::{
        capability::{Capability, Representation},
        store::PackageId,
    };

    fn mcp_resource(name: &str, command: &str, args: &[&str]) -> Resource {
        let payload = serde_json::to_vec(&serde_json::json!({
            "command": command,
            "args": args,
        }))
        .unwrap();
        Resource::from_package_named(
            PackageId::from_plugin_name("demo", &PathBuf::from("plugin.json")).unwrap(),
            PathBuf::from("/store/demo"),
            Capability {
                kind: CapabilityKind::Mcp,
                representation: Representation::Standard,
                path: PathBuf::from("/store/demo/mcp.json"),
                payload,
            },
            name.to_owned(),
        )
    }

    fn skill_resource() -> Resource {
        Resource::from_package(
            PackageId::from_plugin_name("demo", &PathBuf::from("plugin.json")).unwrap(),
            PathBuf::from("/store/demo"),
            Capability {
                kind: CapabilityKind::AgentSkill,
                representation: Representation::Standard,
                path: PathBuf::from("/store/demo/skills/a/SKILL.md"),
                payload: b"body".to_vec(),
            },
        )
    }

    /// A declarative package asks nothing of the operator.
    #[test]
    fn a_skill_only_package_declares_no_executable_capability() {
        let skill = skill_resource();
        assert!(executable_capabilities(&[&skill]).is_empty());
    }

    #[test]
    fn an_mcp_server_with_a_command_is_executable() {
        let mcp = mcp_resource("files", "./bin/server", &["--stdio"]);
        assert_eq!(
            executable_capabilities(&[&mcp]),
            vec![ExecutableCapability {
                name: "files".to_owned(),
                command: "./bin/server".to_owned(),
                arguments: vec!["--stdio".to_owned()],
            }]
        );
    }

    #[test]
    fn an_unchanged_invocation_introduces_nothing_new() {
        let existing = vec![ExecutableCapability {
            name: "files".to_owned(),
            command: "./bin/server".to_owned(),
            arguments: vec!["--stdio".to_owned()],
        }];
        assert!(!introduces_new_execution(&existing, &existing.clone()));
    }

    /// The same capability name pointing at a different command is a new
    /// thing to authorize — this is exactly the case consent must not be
    /// inherited through.
    #[test]
    fn a_changed_command_or_argument_introduces_new_execution() {
        let previous = vec![ExecutableCapability {
            name: "files".to_owned(),
            command: "./bin/server".to_owned(),
            arguments: vec!["--stdio".to_owned()],
        }];
        for (command, argument) in [("./bin/other", "--stdio"), ("./bin/server", "--elevated")] {
            let next = vec![ExecutableCapability {
                name: "files".to_owned(),
                command: command.to_owned(),
                arguments: vec![argument.to_owned()],
            }];
            assert!(introduces_new_execution(&previous, &next));
        }
    }

    #[test]
    fn removing_execution_never_requires_fresh_consent() {
        let previous = vec![ExecutableCapability {
            name: "files".to_owned(),
            command: "./bin/server".to_owned(),
            arguments: Vec::new(),
        }];
        assert!(!introduces_new_execution(&previous, &[]));
    }
}
