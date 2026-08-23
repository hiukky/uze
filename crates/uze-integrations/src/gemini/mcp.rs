//! Gemini CLI MCP server registration and inspection — `gemini mcp add
//! --scope user`, with `gemini mcp list` having no machine-readable output,
//! so inspection reads the one expected `mcpServers.<name>` entry directly
//! out of `~/.gemini/settings.json`.

use std::{fs, path::Path, path::PathBuf};

use uze_core::{
    Result, UzeError,
    exposure::{ExposureMechanism, ExposurePlan},
    integration::{AttachmentInspection, AttachmentState, IntegrationPort},
    project::Resource,
    router::{CompatibilityRoute, VerificationStatus},
    state,
};

use super::GeminiIntegration;
use super::{blocked, unsupported};
use crate::shared::process::{capture, failed_message};

impl GeminiIntegration {
    pub(super) fn mcp_exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        let Some(entry_name) = resource
            .resolved_exposure_name
            .clone()
            .or_else(|| self.exposure_name_candidates(resource).into_iter().next())
        else {
            return unsupported(resource, "MCP resource has no derivable entry name.");
        };
        if !state::is_installed(&self.uze_home, self.id()) {
            return unsupported(
                resource,
                "Gemini setup has not completed, so no managed MCP entry exists yet.",
            );
        }
        let Some((command, args)) = stdio_command(resource) else {
            return unsupported(
                resource,
                "Gemini MCP attachment is only modeled for a stdio command/args server.",
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
            evidence: "UZE registers the store-owned MCP server once via `gemini mcp add --scope user --transport stdio`, writing to ~/.gemini/settings.json's mcpServers. The Gemini MCP runtime remains native."
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
    // Checked before ever calling `mcp add`: Gemini's overwrite behavior for
    // a colliding, differently-configured name is unconfirmed, so UZE never
    // relies on it (same discipline as ADR-007 for the other peers).
    if mcp_entry_exists(command_home, entry_name) {
        return Ok(Some(PathBuf::from(format!("mcp:{entry_name}"))));
    }
    let mut mcp_args: Vec<std::ffi::OsString> = vec![
        std::ffi::OsString::from("mcp"),
        std::ffi::OsString::from("add"),
        std::ffi::OsString::from("--scope"),
        std::ffi::OsString::from("user"),
        std::ffi::OsString::from("--transport"),
        std::ffi::OsString::from("stdio"),
        std::ffi::OsString::from(entry_name),
    ];
    mcp_args.push(command.as_os_str().to_owned());
    mcp_args.extend(args.iter().map(std::ffi::OsString::from));
    let output = capture(Path::new(executable), command_home, &mcp_args).map_err(|error| {
        UzeError::ExposureUnavailable(format!(
            "failed to run `gemini mcp add` for entry `{entry_name}`: {error}"
        ))
    })?;
    if !output.status.success() {
        return Err(UzeError::ExposureUnavailable(failed_message(
            &format!("gemini mcp add `{entry_name}`"),
            &output,
        )));
    }
    Ok(Some(PathBuf::from(format!("mcp:{entry_name}"))))
}

fn mcp_entry_exists(command_home: &Path, entry_name: &str) -> bool {
    read_user_mcp_entry(&command_home.join(".gemini/settings.json"), entry_name).is_some()
}

fn read_user_mcp_entry(path: &Path, entry_name: &str) -> Option<serde_json::Value> {
    let bytes = fs::read(path).ok()?;
    let config: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    config
        .get("mcpServers")
        .and_then(serde_json::Value::as_object)
        .and_then(|servers| servers.get(entry_name))
        .cloned()
}

/// `gemini mcp list` has no machine-readable output, so inspection reads the
/// one expected `mcpServers.<name>` entry out of user settings. Attachment
/// and removal still go through the official CLI verbs.
#[allow(clippy::too_many_arguments)]
pub(super) fn inspect_gemini_mcp(
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
            "Gemini MCP receipt requests state this integration cannot verify safely".to_owned(),
        );
    }
    if !path.exists() {
        return AttachmentInspection {
            state: AttachmentState::Missing,
            reason: "Gemini user settings are missing".to_owned(),
        };
    }
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => return blocked(error.to_string()),
    };
    if serde_json::from_slice::<serde_json::Value>(&bytes).is_err() {
        return blocked("Gemini user settings are malformed".to_owned());
    }
    let Some(entry) = read_user_mcp_entry(path, entry_name) else {
        return AttachmentInspection {
            state: AttachmentState::Missing,
            reason: "Gemini MCP entry is absent".to_owned(),
        };
    };
    inspect_gemini_mcp_value(&entry, command, args)
}

fn inspect_gemini_mcp_value(
    entry: &serde_json::Value,
    command: &Path,
    args: &[String],
) -> AttachmentInspection {
    let actual_command = entry.get("command").and_then(serde_json::Value::as_str);
    if actual_command != command.to_str() {
        return AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "Gemini MCP command differs from receipt".to_owned(),
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
            reason: "Gemini MCP args differ from receipt".to_owned(),
        };
    }
    // The receipt declares no env, cwd or explicit enabled state, so any of
    // them present is state UZE did not create and cannot claim.
    for unexpected in ["env", "cwd"] {
        if entry.get(unexpected).is_some_and(|value| !value.is_null()) {
            return AttachmentInspection {
                state: AttachmentState::Drifted,
                reason: format!("Gemini MCP entry carries an unexpected `{unexpected}`"),
            };
        }
    }
    AttachmentInspection {
        state: AttachmentState::Matched,
        reason: "Gemini MCP entry matches receipt".to_owned(),
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

#[cfg(test)]
mod mcp_tests {
    use std::path::Path;

    use uze_core::integration::AttachmentState;

    use super::inspect_gemini_mcp_value;

    fn entry(command: &str, args: &[&str]) -> serde_json::Value {
        serde_json::json!({ "command": command, "args": args })
    }

    #[test]
    fn a_matching_mcp_entry_is_matched() {
        assert_eq!(
            inspect_gemini_mcp_value(
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
            inspect_gemini_mcp_value(
                &entry("/bin/other", &["--serve"]),
                Path::new("/bin/server"),
                &["--serve".to_owned()],
            )
            .state,
            AttachmentState::Drifted
        );
        assert_eq!(
            inspect_gemini_mcp_value(
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
            inspect_gemini_mcp_value(&value, Path::new("/bin/server"), &[]).state,
            AttachmentState::Drifted
        );
    }
}
