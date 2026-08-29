//! Codex MCP server registration and inspection — the `codex mcp <verb>`
//! CLI surface. No `--scope` flag exists for Codex; global is the only
//! destination.

use std::{path::Path, path::PathBuf, process::Command};

use uze_core::{
    Result, UzeError,
    exposure::{ExposureMechanism, ExposurePlan},
    integration::{AttachmentInspection, AttachmentState, IntegrationPort},
    project::Resource,
    router::{CompatibilityRoute, VerificationStatus},
    state,
};

use super::CodexIntegration;
use super::unsupported;
use crate::shared::process::{capture, failed_message, is_cli_safe_token};

impl CodexIntegration {
    pub(super) fn mcp_exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        if !state::is_installed(&self.uze_home, self.id()) {
            return unsupported(
                resource,
                "Codex has not completed `uze setup`; MCP attachment has no per-session conformance-probe fallback (see ADR-007).",
            );
        }
        let Some(entry_name) = resource
            .resolved_exposure_name
            .clone()
            .or_else(|| self.exposure_name_candidates(resource).into_iter().next())
        else {
            return unsupported(resource, "Resource has no derivable attachment entry name.");
        };
        if !is_cli_safe_token(&entry_name) {
            return unsupported(
                resource,
                "MCP server name would be parsed as a flag by `codex mcp add`, not a name; refusing to attach.",
            );
        }
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
            evidence: "UZE registers the store-owned MCP server once via `codex mcp add`, writing to ~/.codex/config.toml's [mcp_servers.*] (no --scope flag exists; global is the only destination). Available to every future session in any project."
                .to_owned(),
        }
    }
}

pub(super) fn attach_mcp_entry(
    executable: &Path,
    command_home: &Path,
    entry_name: &str,
    command: &Path,
    args: &[String],
) -> Result<Option<PathBuf>> {
    if mcp_entry_exists(executable, command_home, entry_name) {
        return Ok(Some(PathBuf::from(format!("mcp:{entry_name}"))));
    }
    let mut mcp_args: Vec<std::ffi::OsString> = vec![
        std::ffi::OsString::from("mcp"),
        std::ffi::OsString::from("add"),
        std::ffi::OsString::from(entry_name),
        std::ffi::OsString::from("--"),
    ];
    mcp_args.push(command.as_os_str().to_owned());
    mcp_args.extend(args.iter().map(std::ffi::OsString::from));
    let output = capture(executable, command_home, &mcp_args).map_err(|error| {
        UzeError::ExposureUnavailable(format!(
            "failed to run `codex mcp add` for entry `{entry_name}`: {error}"
        ))
    })?;
    if !output.status.success() {
        return Err(UzeError::ExposureUnavailable(failed_message(
            &format!("codex mcp add `{entry_name}`"),
            &output,
        )));
    }
    Ok(Some(PathBuf::from(format!("mcp:{entry_name}"))))
}

/// Idempotently checked before ever calling `codex mcp add` — Codex's
/// overwrite behavior for a colliding, differently-configured name was not
/// confirmed by research, so UZE never relies on it (see ADR-007).
pub(super) fn mcp_entry_exists(executable: &Path, command_home: &Path, entry_name: &str) -> bool {
    Command::new(executable)
        .env("HOME", command_home)
        .args(["mcp", "get", entry_name])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
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
    let output = match Command::new(executable)
        .env("HOME", command_home)
        .args(["mcp", "get", entry_name, "--json"])
        .output()
    {
        Ok(output) => output,
        Err(error) => {
            return super::plugin::blocked(format!(
                "failed to run `codex mcp get --json`: {error}"
            ));
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
        return super::plugin::blocked(format!(
            "`codex mcp get --json` could not verify entry: {}",
            stderr.trim()
        ));
    }
    let value = match serde_json::from_slice::<serde_json::Value>(&output.stdout) {
        Ok(value) => value,
        Err(error) => return super::plugin::blocked(format!("Codex MCP JSON is invalid: {error}")),
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
        return super::plugin::blocked("Codex MCP JSON has no server object".to_owned());
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
        return super::plugin::blocked("Codex MCP JSON has no stdio command".to_owned());
    };
    let Some(actual_args) = transport.get("args").and_then(serde_json::Value::as_array) else {
        return super::plugin::blocked("Codex MCP JSON has no args array".to_owned());
    };
    let actual_args = actual_args
        .iter()
        .map(serde_json::Value::as_str)
        .collect::<Option<Vec<_>>>();
    let Some(actual_args) = actual_args else {
        return super::plugin::blocked("Codex MCP JSON args are not strings".to_owned());
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

/// Removes a UZE-registered MCP entry. Not wired to a CLI verb yet — same
/// precedent as `ExposureMechanism::detach` for Agent Skills. Unused by the
/// `uze` binary for the same reason; exercised directly by
/// `tests/integration_contract.rs`. `command_home` is set explicitly as
/// `HOME` for the same reason `attach_mcp_entry` does — never relies on the
/// calling process's own environment.
#[allow(dead_code)]
pub fn detach_mcp_entry(executable: &Path, command_home: &Path, entry_name: &str) -> Result<()> {
    if !is_cli_safe_token(entry_name) {
        return Err(UzeError::ExposureUnavailable(format!(
            "MCP server name `{entry_name}` would be parsed as a flag by `codex mcp remove`, not a name; refusing to detach."
        )));
    }
    let output =
        capture(executable, command_home, &["mcp", "remove", entry_name]).map_err(|error| {
            UzeError::ExposureUnavailable(format!(
                "failed to run `codex mcp remove` for entry `{entry_name}`: {error}"
            ))
        })?;
    if output.status.success() {
        return Ok(());
    }
    // Already absent is not an error — removal is idempotent.
    if !mcp_entry_exists(executable, command_home, entry_name) {
        return Ok(());
    }
    Err(UzeError::ExposureUnavailable(failed_message(
        &format!("codex mcp remove `{entry_name}`"),
        &output,
    )))
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
