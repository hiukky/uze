//! Codex MCP server registration and inspection — the `codex mcp <verb>`
//! CLI surface. No `--scope` flag exists for Codex; global is the only
//! destination.

use std::path::Path;

use uze_core::{
    Result,
    capability::Resource,
    exposure::ExposurePlan,
    integration::{AttachmentInspection, AttachmentState, IntegrationPort},
    router::CompatibilityRoute,
    state,
};

use super::CodexIntegration;
use crate::shared::mcp::{cli_add, cli_remove, managed_stdio_plan};
use crate::shared::plan::{blocked, unsupported};
use crate::shared::process::{capture, is_cli_safe_token};

impl CodexIntegration {
    pub(super) fn mcp_exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        if !state::is_installed(&self.uze_home, self.id()) {
            return unsupported(
                "Codex has not completed `uze setup`; run `uze setup` so UZE can attach this MCP server (see ADR-007).",
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
                "MCP server name would be parsed as a flag by `codex mcp add`, not a name; refusing to attach.",
            );
        }
        managed_stdio_plan(
            resource,
            entry_name,
            CompatibilityRoute::Adaptable,
            None,
            "UZE registers the store-owned MCP server once via `codex mcp add`, writing to ~/.codex/config.toml's [mcp_servers.*] (no --scope flag exists; global is the only destination). Available to every future session in any project.",
        )
        .unwrap_or_else(|| unsupported("mcp.json server entry is missing a usable `command` field."))
    }
}

/// Registers the server globally — Codex has no scope flag.
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
        "codex",
        &["mcp", "add"],
        entry_name,
        command,
        args,
    )
}

/// Inspects `codex mcp get --json`, the documented structured Codex surface.
/// No TOML is read or written by UZE; an unavailable or malformed response is
/// deliberately BLOCKED rather than interpreted as absence.
#[allow(clippy::too_many_arguments)]
pub(super) fn inspect_codex_mcp(
    executable: &Path,
    command_home: &Path,
    entry_name: &str,
    transport: &str,
    command: &Path,
    args: &[String],
    cwd: Option<&Path>,
    environment: &[uze_core::exposure::McpEnvironmentReference],
    enabled: Option<bool>,
) -> AttachmentInspection {
    let output = match capture(
        executable,
        command_home,
        &["mcp", "get", entry_name, "--json"],
    ) {
        Ok(output) => output,
        Err(error) => {
            return blocked(format!("failed to run `codex mcp get --json`: {error}"));
        }
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Current Codex identifies an absent name with exit 1 and this
        // stable diagnostic. Any other non-zero result is not positive
        // absence evidence, so removal must remain blocked.
        if output.status.code() == Some(1)
            && stderr.contains("No MCP server named")
            && stderr.contains(entry_name)
        {
            return AttachmentInspection {
                state: AttachmentState::Missing,
                reason: "Codex MCP entry is absent".to_owned(),
            };
        }
        return blocked(format!(
            "`codex mcp get --json` could not verify entry: {}",
            stderr.trim()
        ));
    }
    let value = match serde_json::from_slice::<serde_json::Value>(&output.stdout) {
        Ok(value) => value,
        Err(error) => return blocked(format!("Codex MCP JSON is invalid: {error}")),
    };
    inspect_codex_mcp_value(
        &value,
        entry_name,
        transport,
        command,
        args,
        cwd,
        environment,
        enabled,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn inspect_codex_mcp_value(
    value: &serde_json::Value,
    entry_name: &str,
    expected_transport: &str,
    command: &Path,
    args: &[String],
    expected_cwd: Option<&Path>,
    expected_environment: &[uze_core::exposure::McpEnvironmentReference],
    expected_enabled: Option<bool>,
) -> AttachmentInspection {
    let object = value
        .as_object()
        .or_else(|| value.get("server")?.as_object());
    let Some(object) = object else {
        return blocked("Codex MCP JSON has no server object");
    };
    if let Some(name) = object
        .get("name")
        .or_else(|| object.get("id"))
        .and_then(serde_json::Value::as_str)
        && name != entry_name
    {
        return AttachmentInspection {
            state: AttachmentState::Conflict,
            reason: "Codex MCP JSON identifies a different entry".to_owned(),
        };
    }
    if expected_enabled.is_some()
        && object.get("enabled").and_then(serde_json::Value::as_bool) != expected_enabled
    {
        return AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "Codex MCP entry is disabled".to_owned(),
        };
    }
    let transport = object
        .get("transport")
        .and_then(serde_json::Value::as_object)
        .unwrap_or(object);
    if transport
        .get("type")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|actual| actual != expected_transport)
    {
        return AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "Codex MCP transport differs from stdio receipt".to_owned(),
        };
    }
    let Some(actual_command) = transport.get("command").and_then(serde_json::Value::as_str) else {
        return blocked("Codex MCP JSON has no stdio command");
    };
    let Some(actual_args) = transport.get("args").and_then(serde_json::Value::as_array) else {
        return blocked("Codex MCP JSON has no args array");
    };
    let actual_args = actual_args
        .iter()
        .map(serde_json::Value::as_str)
        .collect::<Option<Vec<_>>>();
    let Some(actual_args) = actual_args else {
        return blocked("Codex MCP JSON args are not strings");
    };
    if let Some(expected_cwd) = expected_cwd
        && transport.get("cwd").and_then(serde_json::Value::as_str)
            != Some(expected_cwd.to_string_lossy().as_ref())
    {
        return AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "Codex MCP cwd differs from receipt".to_owned(),
        };
    }
    if !expected_environment.is_empty() {
        let actual = transport
            .get("env_vars")
            .and_then(serde_json::Value::as_array)
            .and_then(|values| {
                values
                    .iter()
                    .map(serde_json::Value::as_str)
                    .collect::<Option<Vec<_>>>()
            });
        let expected = expected_environment
            .iter()
            .map(|reference| reference.name.as_str())
            .collect::<Vec<_>>();
        if actual.as_deref() != Some(expected.as_slice()) {
            return AttachmentInspection {
                state: AttachmentState::Drifted,
                reason: "Codex MCP environment references differ from receipt".to_owned(),
            };
        }
    }
    if actual_command == command.to_string_lossy() && actual_args == args {
        AttachmentInspection {
            state: AttachmentState::Matched,
            reason: "Codex MCP entry matches receipt".to_owned(),
        }
    } else {
        AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "Codex MCP command or args differ from receipt".to_owned(),
        }
    }
}

/// Removes a UZE-registered MCP entry. Wired to the remove lifecycle
/// (`detach_receipt`) and exercised directly by `tests/integrations/
/// contract.rs`; `command_home` is always set as `HOME`, never inherited.
pub fn detach_mcp_entry(executable: &Path, command_home: &Path, entry_name: &str) -> Result<()> {
    cli_remove(executable, command_home, "codex", entry_name)
}

#[cfg(test)]
mod mcp_tests {
    use std::path::PathBuf;

    use uze_core::integration::AttachmentState;

    use super::inspect_codex_mcp_value;

    #[test]
    fn structured_mcp_inspection_distinguishes_match_drift_and_conflict() {
        let expected_command = PathBuf::from("/bin/example");
        let expected_args = vec!["--serve".to_owned()];
        let exact = serde_json::json!({
            "name": "uze-example",
            "command": "/bin/example",
            "args": ["--serve"],
            "unrelated": {"future": true}
        });
        assert_eq!(
            inspect_codex_mcp_value(
                &exact,
                "uze-example",
                "stdio",
                &expected_command,
                &expected_args,
                None,
                &[],
                None
            )
            .state,
            AttachmentState::Matched
        );
        let official_shape = serde_json::json!({
            "name": "uze-example", "enabled": true,
            "transport": {"type":"stdio", "command":"/bin/example", "args":["--serve"], "env":null}
        });
        assert_eq!(
            inspect_codex_mcp_value(
                &official_shape,
                "uze-example",
                "stdio",
                &expected_command,
                &expected_args,
                None,
                &[],
                None,
            )
            .state,
            AttachmentState::Matched
        );
        let changed =
            serde_json::json!({"name":"uze-example", "command":"/bin/changed", "args":["--serve"]});
        assert_eq!(
            inspect_codex_mcp_value(
                &changed,
                "uze-example",
                "stdio",
                &expected_command,
                &expected_args,
                None,
                &[],
                None
            )
            .state,
            AttachmentState::Drifted
        );
        let foreign =
            serde_json::json!({"name":"foreign", "command":"/bin/example", "args":["--serve"]});
        assert_eq!(
            inspect_codex_mcp_value(
                &foreign,
                "uze-example",
                "stdio",
                &expected_command,
                &expected_args,
                None,
                &[],
                None
            )
            .state,
            AttachmentState::Conflict
        );
        assert_eq!(
            inspect_codex_mcp_value(
                &serde_json::json!({}),
                "uze-example",
                "stdio",
                &expected_command,
                &expected_args,
                None,
                &[],
                None,
            )
            .state,
            AttachmentState::Blocked
        );
    }
}
