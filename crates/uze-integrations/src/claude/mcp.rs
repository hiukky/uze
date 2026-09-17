//! Claude Code MCP server registration, inspection, and detachment — the
//! `claude mcp <verb>` CLI surface, plus `~/.claude.json`'s `mcpServers`
//! read path used for read-only inspection.

use std::path::Path;

use uze_core::{
    Result,
    capability::Resource,
    exposure::ExposurePlan,
    integration::{AttachmentInspection, AttachmentState, IntegrationPort},
    router::CompatibilityRoute,
    state,
};

use super::ClaudeIntegration;
use crate::shared::json_config;
use crate::shared::mcp::{cli_add, cli_remove, managed_stdio_plan};
use crate::shared::plan::{blocked, unsupported};
use crate::shared::process::is_cli_safe_token;

impl ClaudeIntegration {
    pub(super) fn mcp_exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        if !state::is_installed(&self.uze_home, self.id()) {
            return unsupported(
                "Claude Code has not completed `uze setup`; run `uze setup` so UZE can attach this MCP server (see ADR-007).",
            );
        }
        let Some(entry_name) = resource
            .resolved_exposure_name
            .clone()
            .or_else(|| self.exposure_name_candidates(resource).into_iter().next())
        else {
            return unsupported("Resource has no derivable attachment entry name.");
        };
        if !is_cli_safe_token(&entry_name) {
            return unsupported(
                "MCP server name would be parsed as a flag by `claude mcp add`, not a name; refusing to attach.",
            );
        }
        managed_stdio_plan(
            resource,
            entry_name,
            CompatibilityRoute::Adaptable,
            None,
            "UZE registers the store-owned MCP server once via `claude mcp add --scope user --transport stdio`, writing to ~/.claude.json's mcpServers. Available to every future session in any project with no --plugin-dir-style flag.",
        )
        .unwrap_or_else(|| unsupported("mcp.json server entry is missing a usable `command` field."))
    }
}

/// Registers the server at user scope (`--scope user`), where every future
/// session in any project reads it.
pub(super) fn attach_mcp_entry(
    executable: &Path,
    command_home: &Path,
    entry_name: &str,
    command: &Path,
    args: &[String],
) -> Result<()> {
    cli_add(
        executable,
        command_home,
        "claude",
        &["mcp", "add", "--scope", "user", "--transport", "stdio"],
        entry_name,
        command,
        args,
    )
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
    let config = match json_config::read_object(path) {
        Ok(config) => config,
        Err(reason) => return blocked(reason),
    };
    let Some(entry) = json_config::get_path(&config, &["mcpServers", entry_name]) else {
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

/// Removes a UZE-registered MCP entry. Wired to the remove lifecycle
/// (`detach_receipt`) and exercised directly by `tests/integrations/
/// contract.rs`; `command_home` is always set as `HOME`, never inherited.
pub fn detach_mcp_entry(executable: &Path, command_home: &Path, entry_name: &str) -> Result<()> {
    cli_remove(executable, command_home, "claude", entry_name)
}
