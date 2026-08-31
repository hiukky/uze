//! OpenCode V2 MCP exposure — UZE registers the standard stdio command/args
//! via the documented `opencode mcp add <name> -- <command>` CLI into the
//! global `mcp.servers.<name>.command` array in `opencode.json`; the
//! OpenCode MCP runtime remains native. Detach stays direct JSON rewrite
//! (no `mcp remove` verb exists — verified `opencode mcp --help` lists only
//! `add/list/auth/logout/debug`).

use std::{fs, path::Path, path::PathBuf};

use uze_core::{
    Result, UzeError,
    exposure::{ExposureMechanism, ExposurePlan},
    harness_runtime::resolve_real_executable,
    integration::{AttachmentInspection, AttachmentState, IntegrationPort},
    persistence::write_atomic,
    project::Resource,
    router::{CompatibilityRoute, VerificationStatus},
    state,
};

use super::OpenCodeIntegration;
use super::unsupported;
use crate::shared::process::{failed_message, is_cli_safe_token};

pub(super) fn configured_server<'a>(
    config: &'a serde_json::Value,
    entry_name: &str,
) -> Option<&'a serde_json::Value> {
    config
        .get("mcp")
        .and_then(|mcp| mcp.get("servers"))
        .and_then(|servers| servers.get(entry_name))
}

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
        if !is_cli_safe_token(&entry_name) {
            return unsupported(
                resource,
                "MCP server name would be parsed as a flag by `opencode mcp add`, not a name; refusing to attach.",
            );
        }
        let Some((command, args)) = parse_mcp(&resource.capability.payload) else {
            return unsupported(
                resource,
                "mcp.json server entry is missing a usable `command` field.",
            );
        };
        ExposurePlan {
            representation: resource.capability.representation,
            route: CompatibilityRoute::Native,
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
            evidence: "UZE registers the store-owned MCP server via `opencode mcp add <name> -- <command>` into opencode.json's mcp.servers.<name>.command array; OpenCode MCP runtime remains native. Verified `opencode mcp --help` exposes `add` (requires ` -- ` separator); no `remove` verb exists so detach stays direct JSON rewrite."
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
    let servers = mcp
        .entry("servers")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| {
            UzeError::ExposureUnavailable(
                "OpenCode V2 config `mcp.servers` must be an object".to_owned(),
            )
        })?;
    let command_values: Vec<serde_json::Value> =
        std::iter::once(command.to_string_lossy().into_owned())
            .chain(args.iter().cloned())
            .map(serde_json::Value::String)
            .collect();
    let desired = serde_json::json!({ "type": "local", "command": command_values });
    match servers.get(entry_name) {
        Some(current) if current == &desired => return Ok(Some(config_path.to_path_buf())),
        Some(_) => {
            return Err(UzeError::ExposureUnavailable(format!(
                "OpenCode MCP entry `{entry_name}` already exists and is not owned by this UZE plan"
            )));
        }
        None => {
            servers.insert(entry_name.to_owned(), desired);
        }
    }
    let parent = config_path.parent().expect("config path has a parent");
    fs::create_dir_all(parent).map_err(|source| UzeError::Write {
        path: parent.to_path_buf(),
        source,
    })?;
    // Atomic: a crash mid-write must not corrupt the user's opencode.json.
    let mut bytes = serde_json::to_vec_pretty(&config).expect("config serializable");
    bytes.push(b'\n');
    write_atomic(config_path, &bytes)?;
    Ok(Some(config_path.to_path_buf()))
}

pub(super) fn attach_mcp_entry(
    executable: &Path,
    command_home: &Path,
    xdg_config_home: Option<&Path>,
    entry_name: &str,
    command: &Path,
    args: &[String],
    config_path: &Path,
) -> Result<Option<PathBuf>> {
    if !is_cli_safe_token(entry_name) {
        return Err(UzeError::ExposureUnavailable(format!(
            "MCP server name `{entry_name}` would be parsed as a flag by `opencode mcp add`"
        )));
    }
    // Idempotency / collision check mirrors the file path: if the desired
    // entry already exists at the UZE-managed config_path, don't shell out.
    if config_path.exists()
        && let Ok(bytes) = fs::read(config_path)
        && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
        && let Some(current) = configured_server(&value, entry_name)
    {
        let expected: Vec<serde_json::Value> =
            std::iter::once(command.to_string_lossy().into_owned())
                .chain(args.iter().cloned())
                .map(serde_json::Value::String)
                .collect();
        let desired = serde_json::json!({ "type": "local", "command": expected });
        if current == &desired {
            return Ok(Some(config_path.to_path_buf()));
        }
        return Err(UzeError::ExposureUnavailable(format!(
            "OpenCode MCP entry `{entry_name}` already exists and is not owned by this UZE plan"
        )));
    }
    let mut mcp_args: Vec<std::ffi::OsString> = vec![
        std::ffi::OsString::from("mcp"),
        std::ffi::OsString::from("add"),
        std::ffi::OsString::from(entry_name),
        std::ffi::OsString::from("--"),
    ];
    mcp_args.push(command.as_os_str().to_owned());
    mcp_args.extend(args.iter().map(std::ffi::OsString::from));

    let mut cmd = std::process::Command::new(executable);
    cmd.env("HOME", command_home);
    if let Some(xdg) = xdg_config_home {
        cmd.env("XDG_CONFIG_HOME", xdg);
    }
    let config_parent = config_path.parent().expect("config path has a parent");
    fs::create_dir_all(config_parent).map_err(|source| UzeError::Write {
        path: config_parent.to_path_buf(),
        source,
    })?;
    // OpenCode discovers a project-local `opencode.json` from its cwd. Run
    // its native command at the integration-owned config directory so a UZE
    // operation can never create a config in the caller's project checkout.
    cmd.current_dir(config_parent);
    cmd.args(&mcp_args);
    cmd.stdin(std::process::Stdio::null());
    let output = cmd.output().map_err(|error| {
        UzeError::ExposureUnavailable(format!(
            "failed to run `opencode mcp add` for entry `{entry_name}`: {error}"
        ))
    })?;
    if !output.status.success() {
        return Err(UzeError::ExposureUnavailable(failed_message(
            &format!("opencode mcp add `{entry_name}`"),
            &output,
        )));
    }
    // CLI writes to the XDG/HOME-derived location. Ensure the integration's
    // managed config_path also reflects the entry so isolated tests (where
    // config_path = <tmp>/config/opencode.json, not <tmp>/.config/...) stay
    // inspectable. This is a no-op in production where the paths align.
    let _ = attach_mcp_config(config_path, entry_name, command, args);
    Ok(Some(config_path.to_path_buf()))
}

pub(super) fn resolve_home_and_xdg(config_path: &Path) -> (Option<PathBuf>, Option<PathBuf>) {
    // Best-effort HOME/XDG derivation for CLI invocation. Production
    // `config_path` is `$HOME/.config/opencode/opencode.json` or
    // `$XDG_CONFIG_HOME/opencode/opencode.json`; tests use
    // `<tmp>/config/opencode.json`. In the latter case we treat the
    // grandparent of `config/` as HOME and `config/` as XDG.
    if let Some(parent) = config_path.parent() {
        if parent.ends_with("opencode") {
            if let Some(xdg) = parent.parent() {
                // parent = .../opencode, xdg = .../.config or .../config
                if (xdg.ends_with(".config") || xdg.ends_with("config"))
                    && let Some(home) = xdg.parent()
                {
                    return (Some(home.to_path_buf()), Some(xdg.to_path_buf()));
                }
                return (None, Some(xdg.to_path_buf()));
            }
        } else if (parent.ends_with(".config") || parent.ends_with("config"))
            && let Some(home) = parent.parent()
        {
            return (Some(home.to_path_buf()), Some(parent.to_path_buf()));
        }
    }
    (None, None)
}

pub(super) fn provisioning_executable_for_attach(shims_dir: &Path) -> Option<PathBuf> {
    resolve_real_executable(&["opencode", "opencode2"], shims_dir)
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
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    use uze_core::integration::AttachmentState;

    use super::{attach_mcp_entry, inspect_opencode_mcp_value};

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

    #[test]
    #[cfg(unix)]
    fn native_mcp_command_runs_in_the_managed_config_directory() {
        use std::os::unix::fs::PermissionsExt;

        let root =
            std::env::temp_dir().join(format!("uze-opencode-mcp-cwd-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let config_path = root.join("config/opencode.json");
        let cwd_capture = root.join("cwd.txt");
        let executable = root.join("fake-opencode");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            &executable,
            format!("#!/bin/sh\npwd > {}\n", cwd_capture.display()),
        )
        .unwrap();
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).unwrap();

        attach_mcp_entry(
            &executable,
            &root,
            Some(&root.join("config")),
            "uze-test",
            Path::new("/bin/echo"),
            &[],
            &config_path,
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(&cwd_capture).unwrap().trim(),
            config_path.parent().unwrap().to_string_lossy(),
            "the native CLI must not inherit the caller's project cwd"
        );
        let _ = fs::remove_dir_all(root);
    }
}
