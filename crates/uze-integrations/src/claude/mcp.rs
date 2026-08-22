//! Claude Code MCP server registration, inspection, and detachment — the
//! `claude mcp <verb>` CLI surface, plus `~/.claude.json`'s `mcpServers`
//! read path used for read-only inspection.

use std::{fs, path::Path, path::PathBuf, process::Command};

use uze_core::{
    Result, UzeError,
    exposure::{ExposureMechanism, ExposurePlan},
    integration::{AttachmentInspection, AttachmentState, IntegrationPort},
    project::Resource,
    router::{CompatibilityRoute, VerificationStatus},
    state,
};

use super::ClaudeIntegration;
use super::unsupported;

impl ClaudeIntegration {
    pub(super) fn mcp_exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        if !state::is_installed(&self.uze_home, self.id()) {
            return unsupported(
                resource,
                "Claude Code has not completed `uze setup`; MCP attachment has no per-session conformance-probe fallback (see ADR-007).",
            );
        }
        let Some(entry_name) = resource
            .resolved_exposure_name
            .clone()
            .or_else(|| self.exposure_name_candidates(resource).into_iter().next())
        else {
            return unsupported(resource, "Resource has no derivable attachment entry name.");
        };
        let Some((command, args)) = parse_mcp_server_config(&resource.capability.payload) else {
            return unsupported(
                resource,
                "mcp.json server entry is missing a usable `command` field.",
            );
        };
        ExposurePlan {
            representation: resource.capability.representation,
            route: CompatibilityRoute::Adaptable,
            verification: VerificationStatus::Unverified,
            mechanism: ExposureMechanism::ManagedVendorConfig {
                entry_name,
                transport: "stdio".to_owned(),
                command,
                args,
                cwd: None,
                environment: Vec::new(),
                enabled: None,
            },
            evidence: "UZE registers the store-owned MCP server once via `claude mcp add --scope user --transport stdio`, writing to ~/.claude.json's mcpServers. Available to every future session in any project with no --plugin-dir-style flag."
                .to_owned(),
        }
    }
}

pub(super) fn attach_mcp_entry(
    command_home: &Path,
    entry_name: &str,
    command: &Path,
    args: &[String],
) -> Result<Option<PathBuf>> {
    if mcp_entry_exists(command_home, entry_name) {
        return Ok(Some(PathBuf::from(format!("mcp:{entry_name}"))));
    }
    let status = Command::new("claude")
        .env("HOME", command_home)
        .args([
            "mcp",
            "add",
            "--scope",
            "user",
            "--transport",
            "stdio",
            entry_name,
            "--",
        ])
        .arg(command)
        .args(args)
        .status();
    match status {
        Ok(status) if status.success() => Ok(Some(PathBuf::from(format!("mcp:{entry_name}")))),
        Ok(status) => Err(UzeError::ExposureUnavailable(format!(
            "`claude mcp add` exited with {status} for entry `{entry_name}`"
        ))),
        Err(error) => Err(UzeError::ExposureUnavailable(format!(
            "failed to run `claude mcp add` for entry `{entry_name}`: {error}"
        ))),
    }
}

/// Idempotently checked before ever calling `claude mcp add` — Claude's
/// overwrite behavior for a colliding, differently-configured name was not
/// confirmed by research, so UZE never relies on it (see ADR-007).
pub(super) fn mcp_entry_exists(command_home: &Path, entry_name: &str) -> bool {
    Command::new("claude")
        .env("HOME", command_home)
        .args(["mcp", "get", entry_name])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Claude has no structured `mcp get` output. This is deliberately read-only:
/// attachment/removal still go through the official CLI, while inspection
/// reads only the one expected `mcpServers.<name>` entry.
#[allow(clippy::too_many_arguments)]
pub(super) fn inspect_claude_mcp(
    path: &Path,
    entry_name: &str,
    transport: &str,
    command: &Path,
    args: &[String],
    cwd: Option<&Path>,
    environment: &[uze_core::exposure::McpEnvironmentReference],
    enabled: Option<bool>,
) -> AttachmentInspection {
    if transport != "stdio" || cwd.is_some() || !environment.is_empty() || enabled.is_some() {
        return AttachmentInspection {
            state: AttachmentState::Blocked,
            reason: "Claude MCP receipt requests state this integration cannot verify safely"
                .to_owned(),
        };
    }
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return AttachmentInspection {
                state: AttachmentState::Missing,
                reason: "Claude config is missing".to_owned(),
            };
        }
        Err(error) => {
            return AttachmentInspection {
                state: AttachmentState::Blocked,
                reason: error.to_string(),
            };
        }
    };
    let config: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => {
            return AttachmentInspection {
                state: AttachmentState::Blocked,
                reason: "Claude config is malformed".to_owned(),
            };
        }
    };
    let Some(entry) = config
        .get("mcpServers")
        .and_then(serde_json::Value::as_object)
        .and_then(|servers| servers.get(entry_name))
    else {
        return AttachmentInspection {
            state: AttachmentState::Missing,
            reason: "Claude MCP entry is missing".to_owned(),
        };
    };
    let command_matches = entry.get("command").and_then(serde_json::Value::as_str)
        == Some(command.to_string_lossy().as_ref());
    let args_match = entry
        .get("args")
        .and_then(serde_json::Value::as_array)
        .map(|actual| {
            actual
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
                == args.iter().map(String::as_str).collect::<Vec<_>>()
        })
        .unwrap_or(args.is_empty());
    if command_matches && args_match {
        AttachmentInspection {
            state: AttachmentState::Matched,
            reason: "Claude MCP entry matches receipt".to_owned(),
        }
    } else {
        AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "Claude MCP command or args differ from receipt".to_owned(),
        }
    }
}

/// Removes a UZE-registered MCP entry. Not wired to a CLI verb yet — same
/// precedent as `ExposureMechanism::detach` for Agent Skills. Unused by the
/// `uze` binary for the same reason; exercised directly by
/// `tests/integration_contract.rs`. `command_home` is set explicitly as
/// `HOME` for the same reason `attach_mcp_entry` does — never relies on the
/// calling process's own environment.
#[allow(dead_code)]
pub fn detach_mcp_entry(command_home: &Path, entry_name: &str) -> Result<()> {
    let status = Command::new("claude")
        .env("HOME", command_home)
        .args(["mcp", "remove", entry_name])
        .status();
    match status {
        Ok(status) if status.success() => Ok(()),
        // Already absent is not an error — removal is idempotent.
        Ok(_) if !mcp_entry_exists(command_home, entry_name) => Ok(()),
        Ok(status) => Err(UzeError::ExposureUnavailable(format!(
            "`claude mcp remove` exited with {status} for entry `{entry_name}`"
        ))),
        Err(error) => Err(UzeError::ExposureUnavailable(format!(
            "failed to run `claude mcp remove` for entry `{entry_name}`: {error}"
        ))),
    }
}

/// Parses `{"command": "...", "args": [...]}` from a payload produced by
/// `UzeEngine`'s MCP resource discovery (one server's config object,
/// already extracted from `mcp.json`'s `mcpServers` map).
fn parse_mcp_server_config(payload: &[u8]) -> Option<(PathBuf, Vec<String>)> {
    let value: serde_json::Value = serde_json::from_slice(payload).ok()?;
    let command = value.get("command")?.as_str()?.to_owned();
    let args = value
        .get("args")
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Some((PathBuf::from(command), args))
}
