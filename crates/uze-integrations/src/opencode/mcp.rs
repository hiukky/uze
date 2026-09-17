//! OpenCode V2 MCP exposure — UZE writes the standard stdio command/args
//! into the global `mcp.servers.<name>` entry of `opencode.json` directly,
//! the same file inspection and detach read; the OpenCode MCP runtime
//! remains native.

use std::path::Path;

use uze_core::{
    Result, UzeError,
    capability::Resource,
    exposure::ExposurePlan,
    integration::{AttachmentInspection, AttachmentState, IntegrationPort},
    router::CompatibilityRoute,
    state,
};

use super::OpenCodeIntegration;
use crate::shared::json_config;
use crate::shared::mcp::managed_stdio_plan;
use crate::shared::plan::unsupported;

impl OpenCodeIntegration {
    pub(super) fn mcp_plan(&self, resource: &Resource) -> ExposurePlan {
        if !state::is_installed(&self.uze_home, self.id()) {
            return unsupported(
                "OpenCode has not completed `uze setup`; its managed global MCP config is not yet enabled.",
            );
        }
        let Some(entry_name) = resource
            .resolved_exposure_name
            .clone()
            .or_else(|| self.exposure_name_candidates(resource).into_iter().next())
        else {
            return unsupported("Resource has no derivable attachment entry name.");
        };
        managed_stdio_plan(
            resource,
            entry_name,
            CompatibilityRoute::Native,
            Some(true),
            "UZE writes the store-owned MCP server into opencode.json's mcp.servers.<name> entry (type local, command array) — the one file attach, inspection and detach all read, with foreign entries left untouched; OpenCode MCP runtime remains native.",
        )
        .unwrap_or_else(|| unsupported("mcp.json server entry is missing a usable `command` field."))
    }
}

/// Where the server lives in `opencode.json`.
fn entry_path(entry_name: &str) -> [&str; 3] {
    ["mcp", "servers", entry_name]
}

/// Writes the managed entry, leaving an identical one alone and refusing to
/// replace one UZE did not write.
pub(super) fn attach_mcp_config(
    config_path: &Path,
    entry_name: &str,
    command: &Path,
    args: &[String],
) -> Result<()> {
    let mut config = if config_path.exists() {
        json_config::read_object(config_path).map_err(UzeError::HarnessConfig)?
    } else {
        serde_json::json!({ "$schema": "https://opencode.ai/config.json" })
    };
    let command_values: Vec<serde_json::Value> =
        std::iter::once(command.to_string_lossy().into_owned())
            .chain(args.iter().cloned())
            .map(serde_json::Value::String)
            .collect();
    let desired = serde_json::json!({ "type": "local", "command": command_values });
    match json_config::get_path(&config, &entry_path(entry_name)) {
        Some(current) if current == &desired => return Ok(()),
        Some(_) => {
            return Err(UzeError::ExposureUnavailable(format!(
                "OpenCode MCP entry `{entry_name}` already exists and is not owned by this UZE plan"
            )));
        }
        None => {}
    }
    json_config::set_path(&mut config, &entry_path(entry_name), desired).map_err(|reason| {
        UzeError::HarnessConfig(format!("cannot attach OpenCode MCP entry: {reason}"))
    })?;
    json_config::write_object(config_path, &config)
}

/// The managed entry's state in a read `opencode.json`.
#[allow(clippy::too_many_arguments)]
pub(super) fn inspect_mcp_entry(
    config: &serde_json::Value,
    entry_name: &str,
    transport: &str,
    command: &Path,
    args: &[String],
    cwd: Option<&Path>,
    environment: &[uze_core::exposure::McpEnvironmentReference],
    enabled: Option<bool>,
) -> AttachmentInspection {
    match json_config::get_path(config, &entry_path(entry_name)) {
        Some(current) => {
            inspect_opencode_mcp_value(current, transport, command, args, cwd, environment, enabled)
        }
        None => AttachmentInspection {
            state: AttachmentState::Missing,
            reason: "OpenCode MCP entry is missing".to_owned(),
        },
    }
}

/// Removes exactly the managed entry once the file it is read from still
/// matches the receipt.
#[allow(clippy::too_many_arguments)]
pub(super) fn detach_mcp_config(
    config_path: &Path,
    entry_name: &str,
    transport: &str,
    command: &Path,
    args: &[String],
    cwd: Option<&Path>,
    environment: &[uze_core::exposure::McpEnvironmentReference],
    enabled: Option<bool>,
) -> Result<AttachmentInspection> {
    let mut config = json_config::read_object(config_path).map_err(UzeError::HarnessConfig)?;
    let inspection = inspect_mcp_entry(
        &config,
        entry_name,
        transport,
        command,
        args,
        cwd,
        environment,
        enabled,
    );
    if inspection.state != AttachmentState::Matched {
        return Ok(inspection);
    }
    json_config::remove_path(&mut config, &entry_path(entry_name));
    json_config::write_object(config_path, &config)?;
    Ok(AttachmentInspection {
        state: AttachmentState::Missing,
        reason: "OpenCode managed MCP entry detached".to_owned(),
    })
}

fn inspect_opencode_mcp_value(
    current: &serde_json::Value,
    transport: &str,
    command: &Path,
    args: &[String],
    cwd: Option<&Path>,
    environment: &[uze_core::exposure::McpEnvironmentReference],
    enabled: Option<bool>,
) -> AttachmentInspection {
    let expected_command = std::iter::once(command.to_string_lossy().into_owned())
        .chain(args.iter().cloned())
        .collect::<Vec<_>>();
    let matches = transport == "stdio"
        && current.get("type").and_then(serde_json::Value::as_str) == Some("local")
        && enabled.is_none_or(|expected| {
            current
                .get("disabled")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
                == !expected
        })
        && current
            .get("command")
            .and_then(serde_json::Value::as_array)
            .and_then(|values| {
                values
                    .iter()
                    .map(serde_json::Value::as_str)
                    .collect::<Option<Vec<_>>>()
            })
            .is_some_and(|actual| actual == expected_command)
        && cwd.is_none_or(|expected| {
            current.get("cwd").and_then(serde_json::Value::as_str)
                == Some(expected.to_string_lossy().as_ref())
        })
        && (environment.is_empty()
            || current
                .get("environment")
                .and_then(serde_json::Value::as_object)
                .is_some_and(|env| {
                    environment
                        .iter()
                        .all(|reference| env.contains_key(&reference.name))
                }));
    if matches {
        AttachmentInspection {
            state: AttachmentState::Matched,
            reason: "OpenCode V2 MCP entry matches receipt".to_owned(),
        }
    } else {
        AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "OpenCode V2 MCP entry differs from receipt".to_owned(),
        }
    }
}

#[cfg(test)]
mod mcp_tests {
    use std::path::{Path, PathBuf};

    use uze_core::integration::AttachmentState;

    use super::inspect_opencode_mcp_value;

    #[test]
    fn managed_cwd_and_environment_reference_drift_are_detected() {
        let expected = PathBuf::from("/bin/example");
        let args = vec!["--serve".to_owned()];
        let current = serde_json::json!({
            "type": "local",
            "command": ["/bin/example", "--serve"],
            "disabled": false,
            "cwd": "/other",
            "environment": {"OTHER": "opaque"}
        });
        assert_eq!(
            inspect_opencode_mcp_value(
                &current,
                "stdio",
                &expected,
                &args,
                Some(Path::new("/expected")),
                &[uze_core::exposure::McpEnvironmentReference {
                    name: "TOKEN".to_owned()
                }],
                Some(true),
            )
            .state,
            AttachmentState::Drifted
        );
    }
}
