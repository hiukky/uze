//! MCP delivery every harness states the same way: one stdio server read
//! from its canonical payload, the managed config entry planned for it, and
//! — for the harnesses whose own CLI registers servers — the `mcp`
//! add/probe/remove verbs.

use std::{ffi::OsString, fs, path::Path, path::PathBuf};

use uze_core::{
    Result, UzeError,
    capability::Resource,
    exposure::{ExposureMechanism, ExposurePlan},
    integration::ManagedArtifact,
    router::CompatibilityRoute,
    store::StoredPackage,
};

use crate::shared::process::{capture, failed_message, is_cli_safe_token, succeeds};

/// How a canonical `mcp.json` names the root of its own package — the token
/// the portable hook contract already speaks.
const PACKAGE_ROOT_TOKEN: &str = "${PLUGIN_ROOT}";

/// Resolves [`PACKAGE_ROOT_TOKEN`] to `package_root` in every string a
/// server declaration carries. No harness expands the portable token, and
/// the ones that stage their own copy of a plugin do not follow the
/// symlinks an envelope is made of, so the one path that holds in every
/// harness is the Store's.
pub(crate) fn resolve_package_root(
    value: &serde_json::Value,
    package_root: &Path,
) -> serde_json::Value {
    match value {
        serde_json::Value::String(text) => serde_json::Value::String(
            text.replace(PACKAGE_ROOT_TOKEN, &package_root.to_string_lossy()),
        ),
        serde_json::Value::Array(items) => serde_json::Value::Array(
            items
                .iter()
                .map(|item| resolve_package_root(item, package_root))
                .collect(),
        ),
        serde_json::Value::Object(entries) => serde_json::Value::Object(
            entries
                .iter()
                .map(|(key, value)| (key.clone(), resolve_package_root(value, package_root)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// The `mcpServers` value of the package's canonical `mcp.json`, resolved
/// into the grammar every harness runs: what an envelope carries.
pub(crate) fn delivered_mcp_servers(package: &StoredPackage) -> Option<serde_json::Value> {
    fs::read(package.root.join("mcp.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|value| value.get("mcpServers").cloned())
        .map(|servers| resolve_package_root(&servers, &package.root))
}

/// `{"command": "...", "args": [...]}` from one server's canonical config
/// object, as MCP resource discovery extracts it from `mcp.json`, with the
/// package root resolved. `None` without a usable `command`; non-string
/// arguments are skipped.
pub(crate) fn stdio_command(payload: &[u8], package_root: &Path) -> Option<(PathBuf, Vec<String>)> {
    let value = resolve_package_root(&serde_json::from_slice(payload).ok()?, package_root);
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
    let (command, args) = stdio_command(&resource.capability.payload, &resource.package_root)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_package_root_resolves_in_every_string_a_server_carries() {
        let root = Path::new("/store/plugins/mk/pm");
        let declared = serde_json::json!({
            "command": "${PLUGIN_ROOT}/bin/server",
            "args": ["--data", "${PLUGIN_ROOT}/data", 3],
            "env": { "HOME_OF": "${PLUGIN_ROOT}" },
        });
        assert_eq!(
            resolve_package_root(&declared, root),
            serde_json::json!({
                "command": "/store/plugins/mk/pm/bin/server",
                "args": ["--data", "/store/plugins/mk/pm/data", 3],
                "env": { "HOME_OF": "/store/plugins/mk/pm" },
            })
        );
    }

    #[test]
    fn a_managed_entry_runs_the_resolved_command() {
        let (command, args) = stdio_command(
            br#"{"command":"python3","args":["${PLUGIN_ROOT}/scripts/server.py"]}"#,
            Path::new("/store/pm"),
        )
        .unwrap();
        assert_eq!(command, PathBuf::from("python3"));
        assert_eq!(args, vec!["/store/pm/scripts/server.py".to_owned()]);
    }
}
