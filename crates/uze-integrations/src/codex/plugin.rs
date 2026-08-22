//! Codex's native plugin marketplace: the derived, UZE-owned
//! `.agents/plugins/marketplace.json` catalogue every installed package
//! with its own external `.codex-plugin/plugin.json` is republished
//! through, and the `codex plugin`/`codex mcp` inspection JSON parsing.

use std::{fs, path::Path, path::PathBuf, process::Command};

use uze_core::{
    Result, UzeError,
    integration::{AttachmentInspection, AttachmentState},
    store::StoredPackage,
};

/// Name of the local catalogue this integration publishes. A Codex identity,
/// held by the Codex integration.
pub(super) const MARKETPLACE_NAME: &str = "uze-local";

pub(super) fn marketplace_exists(command_home: &Path, root: &Path) -> bool {
    let output = Command::new("codex")
        .env("HOME", command_home)
        .args(["plugin", "marketplace", "list", "--json"])
        .output();
    let Ok(output) = output else {
        return false;
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&output.stdout) else {
        return false;
    };
    value["marketplaces"].as_array().is_some_and(|entries| {
        entries.iter().any(|entry| {
            entry
                .get("root")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|candidate| Path::new(candidate) == root)
        })
    })
}

pub(super) fn run_codex(command_home: &Path, prefix: [&str; 3], path: Option<&Path>) -> Result<()> {
    let mut command = Command::new("codex");
    command.env("HOME", command_home).args(prefix);
    if let Some(path) = path {
        command.arg(path);
    }
    match command.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(UzeError::ExposureUnavailable(format!(
            "`codex {}` exited with {status}",
            prefix.join(" ")
        ))),
        Err(error) => Err(UzeError::ExposureUnavailable(format!(
            "failed to run `codex {}`: {error}",
            prefix.join(" ")
        ))),
    }
}

pub(super) fn inspect_codex_plugin(
    command_home: &Path,
    selector: &str,
    marketplace_root: &Path,
    package_root: &Path,
) -> AttachmentInspection {
    let marketplace = match codex_json(command_home, ["plugin", "marketplace", "list", "--json"]) {
        Ok(value) => value,
        Err(reason) => return blocked(reason),
    };
    let marketplace_name = selector.rsplit_once('@').map(|(_, name)| name);
    let Some(marketplace_name) = marketplace_name else {
        return blocked("plugin receipt selector has no marketplace identity".to_owned());
    };
    let Some(entries) = marketplace
        .get("marketplaces")
        .and_then(serde_json::Value::as_array)
    else {
        return blocked("Codex marketplace JSON has no marketplaces array".to_owned());
    };
    let matching_name = entries.iter().find(|entry| {
        entry
            .get("name")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|name| name == marketplace_name)
    });
    let Some(matching_name) = matching_name else {
        return AttachmentInspection {
            state: AttachmentState::Missing,
            reason: "Codex marketplace is absent".to_owned(),
        };
    };
    if matching_name
        .get("root")
        .and_then(serde_json::Value::as_str)
        .is_none_or(|root| Path::new(root) != marketplace_root)
    {
        return AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "Codex marketplace root differs from receipt".to_owned(),
        };
    }
    let plugins = match codex_json(command_home, ["plugin", "list", "--json"]) {
        Ok(value) => value,
        Err(reason) => return blocked(reason),
    };
    inspect_codex_plugin_value(&plugins, selector, package_root)
}

fn inspect_codex_plugin_value(
    value: &serde_json::Value,
    selector: &str,
    package_root: &Path,
) -> AttachmentInspection {
    let Some(installed) = value.get("installed").and_then(serde_json::Value::as_array) else {
        return blocked("Codex plugin JSON has no installed array".to_owned());
    };
    let Some(plugin) = installed.iter().find(|entry| {
        ["pluginId", "id", "plugin_id", "selector"]
            .iter()
            .filter_map(|field| entry.get(*field).and_then(serde_json::Value::as_str))
            .any(|candidate| candidate == selector)
    }) else {
        return AttachmentInspection {
            state: AttachmentState::Missing,
            reason: "Codex plugin is not installed".to_owned(),
        };
    };
    let Some(enabled) = plugin.get("enabled").and_then(serde_json::Value::as_bool) else {
        return blocked("Codex plugin JSON has no enabled state".to_owned());
    };
    let Some(installed_state) = plugin.get("installed").and_then(serde_json::Value::as_bool) else {
        return blocked("Codex plugin JSON has no installed state".to_owned());
    };
    let Some((_, marketplace_name)) = selector.rsplit_once('@') else {
        return blocked("plugin receipt selector has no marketplace identity".to_owned());
    };
    let Some(actual_marketplace) = plugin
        .get("marketplaceName")
        .or_else(|| plugin.get("marketplace_name"))
        .and_then(serde_json::Value::as_str)
    else {
        return blocked("Codex plugin JSON has no marketplace identity".to_owned());
    };
    let source = plugin
        .get("path")
        .or_else(|| plugin.pointer("/source/path"))
        .and_then(serde_json::Value::as_str);
    let Some(source) = source else {
        return blocked("Codex plugin JSON has no package source path".to_owned());
    };
    if !enabled
        || !installed_state
        || actual_marketplace != marketplace_name
        || Path::new(source) != package_root
    {
        return AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "Codex plugin enabled state or source differs from receipt".to_owned(),
        };
    }
    AttachmentInspection {
        state: AttachmentState::Matched,
        reason: "Codex native plugin matches receipt".to_owned(),
    }
}

fn codex_json<const N: usize>(
    command_home: &Path,
    args: [&str; N],
) -> std::result::Result<serde_json::Value, String> {
    let output = Command::new("codex")
        .env("HOME", command_home)
        .args(args)
        .output()
        .map_err(|error| format!("failed to run `codex`: {error}"))?;
    if !output.status.success() {
        return Err(format!("`codex` inspection exited with {}", output.status));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("Codex JSON is invalid: {error}"))
}

pub(super) fn remove_plugin(command_home: &Path, selector: &str) -> Result<()> {
    match Command::new("codex")
        .env("HOME", command_home)
        .args(["plugin", "remove", selector])
        .status()
    {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(UzeError::ExposureUnavailable(format!(
            "`codex plugin remove` exited with {status} for `{selector}`"
        ))),
        Err(error) => Err(UzeError::ExposureUnavailable(format!(
            "failed to run `codex plugin remove` for `{selector}`: {error}"
        ))),
    }
}

pub(super) fn blocked(reason: String) -> AttachmentInspection {
    AttachmentInspection {
        state: AttachmentState::Blocked,
        reason,
    }
}

/// Packages carrying the Codex-native envelope. Deciding which packages
/// belong in the catalogue is Codex policy, so it lives here rather than in
/// the Store.
pub(super) fn publishable(packages: &[StoredPackage]) -> Vec<&StoredPackage> {
    packages
        .iter()
        .filter(|package| package.root.join(".codex-plugin/plugin.json").is_file())
        .collect()
}

/// The catalogue document, derived purely from the installed package set.
/// Nothing here exists only in the catalogue: delete the file and this
/// rebuilds it byte for byte from the Store.
pub(super) fn catalogue_document(packages: &[StoredPackage]) -> serde_json::Value {
    let plugins: Vec<serde_json::Value> = publishable(packages)
        .into_iter()
        .map(|package| {
            serde_json::json!({
                "name": package.id.as_str(),
                // Relative to the catalogue root by necessity — see
                // `CodexIntegration::catalogue_root`.
                "source": { "source": "local", "path": format!("./packages/{}", package.id.as_str()) },
                "policy": { "installation": "AVAILABLE", "authentication": "ON_INSTALL" },
                "category": "Developer tools"
            })
        })
        .collect();
    serde_json::json!({
        "name": MARKETPLACE_NAME,
        "interface": { "displayName": "UZE Local" },
        "plugins": plugins,
    })
}

pub(super) fn write_catalogue(path: &Path, packages: &[StoredPackage]) -> Result<()> {
    let parent = path.parent().expect("catalogue path has a parent");
    fs::create_dir_all(parent).map_err(|source| UzeError::Write {
        path: parent.to_path_buf(),
        source,
    })?;
    uze_core::persistence::write_atomic(
        path,
        &serde_json::to_vec_pretty(&catalogue_document(packages))
            .expect("catalogue is serializable"),
    )
}

/// Reads one integration-defined detail out of an opaque receipt payload.
/// Only this integration interprets these keys.
pub(super) fn detail_path(
    detail: &std::collections::BTreeMap<String, serde_json::Value>,
    key: &str,
) -> Option<PathBuf> {
    detail
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
}

#[cfg(test)]
mod plugin_tests {
    use std::path::Path;

    use uze_core::integration::AttachmentState;

    use super::inspect_codex_plugin_value;

    #[test]
    fn native_plugin_receipt_requires_installed_identity_and_expected_source() {
        let package = Path::new("/uze/store/example");
        let exact = serde_json::json!({
            "installed": [{"pluginId":"example@uze-local", "enabled":true, "installed":true, "marketplaceName":"uze-local", "source":{"path":"/uze/store/example"}}]
        });
        assert_eq!(
            inspect_codex_plugin_value(&exact, "example@uze-local", package).state,
            AttachmentState::Matched
        );
        let changed = serde_json::json!({
            "installed": [{"id":"example@uze-local", "enabled":false, "installed":true, "marketplaceName":"uze-local", "path":"/uze/store/example"}]
        });
        assert_eq!(
            inspect_codex_plugin_value(&changed, "example@uze-local", package).state,
            AttachmentState::Drifted
        );
        let absent = serde_json::json!({"installed": []});
        assert_eq!(
            inspect_codex_plugin_value(&absent, "example@uze-local", package).state,
            AttachmentState::Missing
        );
    }
}
