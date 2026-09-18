//! Test helper for the product's marketplace contract: a plugin is only
//! ever installed through a marketplace that was added first
//! (`uze market add <dir>` then `uze plugin install <name>@<market>`).
//! The product rejects direct path/Git installs, so tests must stage a
//! single-plugin marketplace to exercise the real user flow.

use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::fixtures::copy_tree;

/// Writes a marketplace repository at `directory`: its `marketplace.json`,
/// each plugin directory copied to `plugins/<name>`, all of it committed —
/// a marketplace is a Git repository, a staged one included. Answers with
/// the commit.
pub fn stage(directory: &Path, marketplace_json: &str, plugins: &[(String, PathBuf)]) -> String {
    fs::create_dir_all(directory)
        .unwrap_or_else(|error| panic!("create {}: {error}", directory.display()));
    fs::write(directory.join("marketplace.json"), marketplace_json)
        .unwrap_or_else(|error| panic!("write marketplace.json: {error}"));
    for (name, source) in plugins {
        copy_tree(source, &directory.join("plugins").join(name));
    }
    crate::git::commit_everything_in(directory)
}

/// Stages `package` as a one-plugin marketplace named `test` under `root`
/// and returns the command sequences a test must run, in order:
/// `(["market", "add", <dir>], ["plugin", "install", "<name>@test"])`.
pub fn marketplace_install_args(root: &Path, package: &Path) -> (Vec<String>, Vec<String>) {
    let market = root.join("market");
    let name = package_manifest_name(package);
    let manifest = serde_json::json!({
        "name": "test",
        "description": "Test marketplace staged by uze-testkit.",
        "plugins": [
            {
                "name": name,
                "source": format!("./plugins/{name}"),
                "description": "Test plugin.",
            }
        ],
    });
    stage(
        &market,
        &serde_json::to_string_pretty(&manifest).unwrap(),
        &[(name.clone(), package.to_path_buf())],
    );
    (
        vec![
            "market".to_owned(),
            "add".to_owned(),
            market.to_string_lossy().into_owned(),
        ],
        vec![
            "plugin".to_owned(),
            "install".to_owned(),
            format!("{name}@test"),
        ],
    )
}

/// The `name` field of the package's `plugin.json` — the marketplace
/// resolves plugins by this name.
pub fn package_manifest_name(package: &Path) -> String {
    let manifest = fs::read_to_string(package.join("plugin.json"))
        .unwrap_or_else(|error| panic!("package plugin.json: {error}"));
    let parsed: serde_json::Value = serde_json::from_str(&manifest)
        .unwrap_or_else(|error| panic!("package plugin.json: {error}"));
    parsed
        .get("name")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| panic!("package plugin.json has no name: {}", package.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stages_a_marketplace_and_returns_market_and_install_args() {
        let root = std::env::temp_dir().join(format!("uze-testkit-market-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let package = root.join("pkg");
        fs::create_dir_all(&package).unwrap();
        fs::write(
            package.join("plugin.json"),
            r#"{"name":"demo","skills":{}}"#,
        )
        .unwrap();
        let (market, install) = marketplace_install_args(&root, &package);
        assert_eq!(market[0], "market");
        assert_eq!(market[1], "add");
        assert!(market[2].ends_with("market"));
        assert_eq!(install, vec!["plugin", "install", "demo@test"]);
        let marketplace: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(root.join("market/marketplace.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(marketplace["name"], "test");
        assert_eq!(marketplace["plugins"][0]["name"], "demo");
        let _ = fs::remove_dir_all(root);
    }
}
