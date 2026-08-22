//! OpenCode MCP exposure — UZE adapts the standard stdio command/args into
//! OpenCode's documented global `mcp.<name>.command` array in
//! `opencode.json`; the OpenCode MCP runtime remains native.

use std::{fs, path::Path, path::PathBuf};

use uze_core::{
    Result, UzeError,
    exposure::{ExposureMechanism, ExposurePlan},
    integration::{AttachmentInspection, AttachmentState, IntegrationPort},
    project::Resource,
    router::{CompatibilityRoute, VerificationStatus},
    state,
};

use super::OpenCodeIntegration;
use super::unsupported;

impl OpenCodeIntegration {
    pub(super) fn mcp_plan(&self, resource: &Resource) -> ExposurePlan {
        if !state::is_installed(&self.uze_home, self.id()) {
            return unsupported(
                resource,
                "OpenCode has not completed `uze setup`; its managed global MCP config is not yet enabled.",
            );
        }
        let Some(entry_name) = resource
            .resolved_exposure_name
            .clone()
            .or_else(|| self.exposure_name_candidates(resource).into_iter().next())
        else {
            return unsupported(resource, "Resource has no derivable attachment entry name.");
        };
        let Some((command, args)) = parse_mcp(&resource.capability.payload) else {
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
                enabled: Some(true),
            },
            evidence: "UZE adapts the standard stdio command/args into OpenCode's documented global `mcp.<name>.command` array in opencode.json; the OpenCode MCP runtime remains native."
                .to_owned(),
        }
    }
}

pub(super) fn attach_mcp_config(
    config_path: &Path,
    entry_name: &str,
    command: &Path,
    args: &[String],
) -> Result<Option<PathBuf>> {
    let mut config = if config_path.exists() {
        serde_json::from_slice(&fs::read(config_path).map_err(|source| UzeError::Read {
            path: config_path.to_path_buf(),
            source,
        })?)
        .map_err(|source| UzeError::Json {
            path: config_path.to_path_buf(),
            source,
        })?
    } else {
        serde_json::json!({ "$schema": "https://opencode.ai/config.json" })
    };
    let root = config.as_object_mut().ok_or_else(|| {
        UzeError::ExposureUnavailable("OpenCode config root must be a JSON object".to_owned())
    })?;
    let mcp = root
        .entry("mcp")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| {
            UzeError::ExposureUnavailable("OpenCode config `mcp` must be an object".to_owned())
        })?;
    let command_values: Vec<serde_json::Value> =
        std::iter::once(command.to_string_lossy().into_owned())
            .chain(args.iter().cloned())
            .map(serde_json::Value::String)
            .collect();
    let desired =
        serde_json::json!({ "type": "local", "command": command_values, "enabled": true });
    match mcp.get(entry_name) {
        Some(current) if current == &desired => return Ok(Some(config_path.to_path_buf())),
        Some(_) => {
            return Err(UzeError::ExposureUnavailable(format!(
                "OpenCode MCP entry `{entry_name}` already exists and is not owned by this UZE plan"
            )));
        }
        None => {
            mcp.insert(entry_name.to_owned(), desired);
        }
    }
    let parent = config_path.parent().expect("config path has a parent");
    fs::create_dir_all(parent).map_err(|source| UzeError::Write {
        path: parent.to_path_buf(),
        source,
    })?;
    fs::write(
        config_path,
        serde_json::to_vec_pretty(&config).expect("config serializable"),
    )
    .map_err(|source| UzeError::Write {
        path: config_path.to_path_buf(),
        source,
    })?;
    Ok(Some(config_path.to_path_buf()))
}

fn parse_mcp(payload: &[u8]) -> Option<(PathBuf, Vec<String>)> {
    let value: serde_json::Value = serde_json::from_slice(payload).ok()?;
    Some((
        PathBuf::from(value.get("command")?.as_str()?),
        value
            .get("args")
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
    ))
}

pub(super) fn inspect_opencode_mcp_value(
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
            current.get("enabled").and_then(serde_json::Value::as_bool) == Some(expected)
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
                .get("env")
                .and_then(serde_json::Value::as_object)
                .is_some_and(|env| {
                    environment
                        .iter()
                        .all(|reference| env.contains_key(&reference.name))
                }));
    if matches {
        AttachmentInspection {
            state: AttachmentState::Matched,
            reason: "OpenCode MCP entry matches receipt".to_owned(),
        }
    } else {
        AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "OpenCode MCP entry differs from receipt".to_owned(),
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
            "enabled": true,
            "cwd": "/other",
            "env": {"OTHER": "opaque"}
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
