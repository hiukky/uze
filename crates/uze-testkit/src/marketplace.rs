//! Test helper for the product's marketplace contract: a plugin is only
//! ever installed through a marketplace that was added first
//! (`uze market add <dir>` then `uze plugin install <name>@<market>`).
//! The product rejects direct path/Git installs, so tests must stage a
//! single-plugin marketplace to exercise the real user flow.

use std::{fs, path::Path};

/// Stages `package` as a one-plugin marketplace named `test` under `root`
/// and returns the command sequences a test must run, in order:
/// `(["market", "add", <dir>], ["plugin", "install", "<name>@test"])`.
pub fn marketplace_install_args(root: &Path, package: &Path) -> (Vec<String>, Vec<String>) {
    let market = root.join("market");
    let name = package_manifest_name(package);
    let plugins_dir = market.join("plugins");
    fs::create_dir_all(&plugins_dir).unwrap();
    copy_tree(package, &plugins_dir.join(&name));
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
    fs::write(
        market.join("agents.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
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

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.path().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).unwrap();
        }
    }
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
        let agents: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join("market/agents.json")).unwrap())
                .unwrap();
        assert_eq!(agents["name"], "test");
        assert_eq!(agents["plugins"][0]["name"], "demo");
        let _ = fs::remove_dir_all(root);
    }
}
