//! Antigravity CLI MCP server registration and inspection — `agy mcp add
//! <name> <command> [args...]` (global scope; flags must precede the name
//! in 1.1.19), with `agy mcp list` being human-readable only, so inspection
//! reads the one expected `mcpServers.<name>` entry directly out of
//! `~/.gemini/config/mcp_config.json` — the vendor's dedicated, sparse
//! MCP profile (Antigravity separately MCP from settings.json; legacy
//! inline declarations are gone).

use std::{fs, path::Path, path::PathBuf};

use uze_core::{
    Result, UzeError,
    exposure::{ExposureMechanism, ExposurePlan},
    integration::{AttachmentInspection, AttachmentState, IntegrationPort},
    project::Resource,
    router::{CompatibilityRoute, VerificationStatus},
    state,
};

use super::AntigravityIntegration;
use super::unsupported;
use crate::shared::process::{capture, failed_message, is_cli_safe_token};

impl AntigravityIntegration {
    pub(super) fn mcp_exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        let Some(entry_name) = resource
            .resolved_exposure_name
            .clone()
            .or_else(|| self.exposure_name_candidates(resource).into_iter().next())
        else {
            return unsupported(resource, "MCP resource has no derivable entry name.");
        };
        if !is_cli_safe_token(&entry_name) {
            return unsupported(
                resource,
                "MCP server name would be parsed as a flag by `agy mcp add`, not a name; refusing to attach.",
            );
        }
        if !state::is_installed(&self.uze_home, self.id()) {
            return unsupported(
                resource,
                "Antigravity setup has not completed, so no managed MCP entry exists yet.",
            );
        }
        let Some((command, args)) = stdio_command(resource) else {
            return unsupported(
                resource,
                "Antigravity MCP attachment is only modeled for a stdio command/args server.",
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
            evidence: "UZE registers the store-owned MCP server once via `agy mcp add <name> <command> [args...]`, writing to ~/.gemini/config/mcp_config.json's mcpServers. The Antigravity MCP runtime remains native."
                .to_owned(),
        }
    }
}

pub(super) fn attach_mcp_entry(
    executable: &str,
    command_home: &Path,
    entry_name: &str,
    command: &Path,
    args: &[String],
) -> Result<Option<PathBuf>> {
    // Checked before ever calling `mcp add`: Antigravity's verb is
    // add-or-update (help text: "Add or update an MCP server
    // configuration"), so a colliding, differently-configured name would be
    // silently overwritten — UZE never relies on that (same discipline as
    // ADR-007 for the other peers).
    if mcp_entry_exists(command_home, entry_name) {
        return Err(UzeError::ExposureUnavailable(format!(
            "Antigravity already has an MCP server named `{entry_name}` that UZE does not own; refusing to overwrite it"
        )));
    }
    let mut mcp_args: Vec<std::ffi::OsString> = vec![
        std::ffi::OsString::from("mcp"),
        std::ffi::OsString::from("add"),
        std::ffi::OsString::from(entry_name),
    ];
    mcp_args.push(command.as_os_str().to_owned());
    mcp_args.extend(args.iter().map(std::ffi::OsString::from));
    let output = capture(Path::new(executable), command_home, &mcp_args).map_err(|error| {
        UzeError::ExposureUnavailable(format!(
            "failed to run `agy mcp add` for entry `{entry_name}`: {error}"
        ))
    })?;
    if !output.status.success() {
        return Err(UzeError::ExposureUnavailable(failed_message(
            &format!("agy mcp add `{entry_name}`"),
            &output,
        )));
    }
    Ok(Some(PathBuf::from(format!("mcp:{entry_name}"))))
}

fn mcp_config_path(command_home: &Path) -> PathBuf {
    command_home.join(".gemini/config/mcp_config.json")
}

fn mcp_entry_exists(command_home: &Path, entry_name: &str) -> bool {
    read_mcp_entry(&mcp_config_path(command_home), entry_name).is_some()
}

fn read_mcp_entry(path: &Path, entry_name: &str) -> Option<serde_json::Value> {
    let bytes = fs::read(path).ok()?;
    let config: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    config
        .get("mcpServers")
        .and_then(serde_json::Value::as_object)
        .and_then(|servers| servers.get(entry_name))
        .cloned()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn inspect_antigravity_mcp(
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
        return blocked(
            "Antigravity MCP receipt requests state this integration cannot verify safely"
                .to_owned(),
        );
    }
    if !path.exists() {
        return AttachmentInspection {
            state: AttachmentState::Missing,
            reason: "Antigravity MCP config is missing".to_owned(),
        };
    }
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => return blocked(error.to_string()),
    };
    if serde_json::from_slice::<serde_json::Value>(&bytes).is_err() {
        return blocked("Antigravity MCP config is malformed".to_owned());
    }
    let Some(entry) = read_mcp_entry(path, entry_name) else {
        return AttachmentInspection {
            state: AttachmentState::Missing,
            reason: "Antigravity MCP entry is absent".to_owned(),
        };
    };
    inspect_antigravity_mcp_value(&entry, command, args)
}

fn inspect_antigravity_mcp_value(
    entry: &serde_json::Value,
    command: &Path,
    args: &[String],
) -> AttachmentInspection {
    let actual_command = entry.get("command").and_then(serde_json::Value::as_str);
    if actual_command != command.to_str() {
        return AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "Antigravity MCP command differs from receipt".to_owned(),
        };
    }
    let actual_args: Vec<&str> = entry
        .get("args")
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect()
        })
        .unwrap_or_default();
    if actual_args != args.iter().map(String::as_str).collect::<Vec<_>>() {
        return AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "Antigravity MCP args differ from receipt".to_owned(),
        };
    }
    // The receipt declares no env or cwd, so any of them present is state
    // UZE did not create and cannot claim.
    for unexpected in ["env", "cwd", "headers"] {
        if entry.get(unexpected).is_some_and(|value| !value.is_null()) {
            return AttachmentInspection {
                state: AttachmentState::Drifted,
                reason: format!("Antigravity MCP entry carries an unexpected `{unexpected}`"),
            };
        }
    }
    // `disabled: true` is a user preference on an entry UZE still owns
    // (same rationale as every vendor's enablement field); a non-boolean
    // `disabled` is state this integration cannot interpret.
    if let Some(disabled) = entry.get("disabled") {
        match disabled.as_bool() {
            Some(_) => {}
            None => {
                return AttachmentInspection {
                    state: AttachmentState::Blocked,
                    reason: "Antigravity MCP entry has a non-boolean `disabled`".to_owned(),
                };
            }
        }
    }
    AttachmentInspection {
        state: AttachmentState::Matched,
        reason: "Antigravity MCP entry matches receipt".to_owned(),
    }
}

pub(super) fn stdio_command(resource: &Resource) -> Option<(PathBuf, Vec<String>)> {
    let config: serde_json::Value = serde_json::from_slice(&resource.capability.payload).ok()?;
    let command = config.get("command").and_then(serde_json::Value::as_str)?;
    let args = config
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
    Some((PathBuf::from(command), args))
}

fn blocked(reason: String) -> AttachmentInspection {
    AttachmentInspection {
        state: AttachmentState::Blocked,
        reason,
    }
}

#[cfg(test)]
mod mcp_tests {
    use std::path::Path;

    use uze_core::integration::AttachmentState;

    use super::inspect_antigravity_mcp_value;

    fn entry(command: &str, args: &[&str]) -> serde_json::Value {
        serde_json::json!({ "command": command, "args": args })
    }

    #[test]
    fn a_matching_mcp_entry_is_matched() {
        assert_eq!(
            inspect_antigravity_mcp_value(
                &entry("/bin/server", &["--serve"]),
                Path::new("/bin/server"),
                &["--serve".to_owned()],
            )
            .state,
            AttachmentState::Matched
        );
    }

    #[test]
    fn a_changed_command_or_args_is_drift_not_a_match() {
        assert_eq!(
            inspect_antigravity_mcp_value(
                &entry("/bin/other", &["--serve"]),
                Path::new("/bin/server"),
                &["--serve".to_owned()],
            )
            .state,
            AttachmentState::Drifted
        );
        assert_eq!(
            inspect_antigravity_mcp_value(
                &entry("/bin/server", &["--different"]),
                Path::new("/bin/server"),
                &["--serve".to_owned()],
            )
            .state,
            AttachmentState::Drifted
        );
    }

    #[test]
    fn state_uze_never_created_is_drift() {
        let mut value = entry("/bin/server", &[]);
        value["env"] = serde_json::json!({ "TOKEN": "x" });
        assert_eq!(
            inspect_antigravity_mcp_value(&value, Path::new("/bin/server"), &[]).state,
            AttachmentState::Drifted
        );
    }

    #[test]
    fn a_user_disable_is_a_preference_not_an_ownership_signal() {
        let mut value = entry("/bin/server", &[]);
        value["disabled"] = serde_json::json!(true);
        assert_eq!(
            inspect_antigravity_mcp_value(&value, Path::new("/bin/server"), &[]).state,
            AttachmentState::Matched
        );
    }

    #[test]
    fn a_non_boolean_disabled_is_blocked_not_guessed() {
        let mut value = entry("/bin/server", &[]);
        value["disabled"] = serde_json::json!("yes");
        assert_eq!(
            inspect_antigravity_mcp_value(&value, Path::new("/bin/server"), &[]).state,
            AttachmentState::Blocked
        );
    }
}
