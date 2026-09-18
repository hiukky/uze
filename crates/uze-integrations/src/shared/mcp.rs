//! MCP delivery every harness states the same way: one stdio server read
//! from its canonical payload, the managed config entry planned for it, and
//! — for the harnesses whose own CLI registers servers — the `mcp`
//! add/probe/remove verbs.

use std::{ffi::OsString, path::Path, path::PathBuf};

use uze_core::{
    Result, UzeError,
    capability::Resource,
    exposure::{ExposureMechanism, ExposurePlan},
    integration::ManagedArtifact,
    router::CompatibilityRoute,
};

use crate::shared::process::{capture, failed_message, is_cli_safe_token, succeeds};

/// `{"command": "...", "args": [...]}` from one server's canonical config
/// object, as MCP resource discovery extracts it from `mcp.json`. `None`
/// without a usable `command`; non-string arguments are skipped.
pub(crate) fn stdio_command(payload: &[u8]) -> Option<(PathBuf, Vec<String>)> {
    let value: serde_json::Value = serde_json::from_slice(payload).ok()?;
    let command = value.get("command")?.as_str()?;
    let args = value
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

/// The managed stdio entry for `resource` under `entry_name`, or `None`
/// when its payload carries no usable command.
pub(crate) fn managed_stdio_plan(
    resource: &Resource,
    entry_name: String,
    route: CompatibilityRoute,
    enabled: Option<bool>,
    evidence: &str,
) -> Option<ExposurePlan> {
    let (command, args) = stdio_command(&resource.capability.payload)?;
    Some(ExposurePlan {
        route,
        mechanism: ExposureMechanism::Managed(ManagedArtifact::VendorConfigEntry {
            entry_name,
            transport: "stdio".to_owned(),
            command,
            args,
            cwd: None,
            environment: Vec::new(),
            enabled,
        }),
        evidence: evidence.to_owned(),
    })
}

/// Whether the vendor already knows a server by this name (`mcp get`).
pub(crate) fn cli_exists(executable: &Path, home: &Path, entry_name: &str) -> bool {
    succeeds(executable, home, &["mcp", "get", entry_name])
}

/// Registers `entry_name` through `<add_verb> <entry_name> -- <command>
/// [args...]`. An existing name is left alone: neither vendor's overwrite
/// behavior for a colliding, differently-configured name was confirmed, so
/// UZE never relies on it (ADR-007).
pub(crate) fn cli_add(
    executable: &Path,
    home: &Path,
    vendor: &str,
    add_verb: &[&str],
    entry_name: &str,
    command: &Path,
    args: &[String],
) -> Result<()> {
    if cli_exists(executable, home, entry_name) {
        return Ok(());
    }
    let arguments: Vec<OsString> = add_verb
        .iter()
        .map(OsString::from)
        .chain([OsString::from(entry_name), OsString::from("--")])
        .chain(std::iter::once(command.as_os_str().to_owned()))
        .chain(args.iter().map(OsString::from))
        .collect();
    let output = capture(executable, home, &arguments).map_err(|error| {
        UzeError::HarnessCommand(format!(
            "failed to run `{vendor} mcp add` for entry `{entry_name}`: {error}"
        ))
    })?;
    if output.status.success() {
        return Ok(());
    }
    Err(UzeError::HarnessCommand(failed_message(
        &format!("{vendor} mcp add `{entry_name}`"),
        &output,
    )))
}

/// Removes `entry_name` through `mcp remove`. An entry already absent is not
/// an error — removal is idempotent — and a flag-shaped name is refused
/// before any process is spawned.
pub(crate) fn cli_remove(
    executable: &Path,
    home: &Path,
    vendor: &str,
    entry_name: &str,
) -> Result<()> {
    if !is_cli_safe_token(entry_name) {
        return Err(UzeError::ExposureUnavailable(format!(
            "MCP server name `{entry_name}` would be parsed as a flag by `{vendor} mcp remove`, not a name; refusing to detach."
        )));
    }
    let output = capture(executable, home, &["mcp", "remove", entry_name]).map_err(|error| {
        UzeError::HarnessCommand(format!(
            "failed to run `{vendor} mcp remove` for entry `{entry_name}`: {error}"
        ))
    })?;
    if output.status.success() || !cli_exists(executable, home, entry_name) {
        return Ok(());
    }
    Err(UzeError::HarnessCommand(failed_message(
        &format!("{vendor} mcp remove `{entry_name}`"),
        &output,
    )))
}
