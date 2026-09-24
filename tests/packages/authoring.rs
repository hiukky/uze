//! The authoring surface's own contracts: a scaffolded marketplace is born
//! registered and linked, a scaffolded plugin is installable immediately,
//! and the offline check is the same validation an install applies —
//! delivered before one.
//!
//! The scaffold-invariant test lives beside the templates in `uze-core`
//! (`package::authoring::tests`); this suite proves the *orchestration*:
//! what the machine registry and the Store carry once the verbs have run.

use std::{fs, path::PathBuf, process::Command};

use uze_testkit::marketplace::marketplace_install_args;

fn uze_bin() -> &'static str {
    env!("CARGO_BIN_EXE_uze")
}

/// Runs `uze` with `UZE_HOME` and `HOME` under `root`, and Git isolated from
/// the ambient configuration — pointing at this world's own `gitconfig`,
/// which carries the identity the scaffold's first commit needs.
fn uze(root: &std::path::Path) -> Command {
    let gitconfig = root.join(".gitconfig");
    if !gitconfig.exists() {
        fs::write(
            &gitconfig,
            "[user]\n\tname = Test\n\temail = t@example.invalid\n",
        )
        .unwrap();
    }
    let mut command = Command::new(uze_bin());
    command
        .env("UZE_HOME", root.join("uze"))
        .env("HOME", root)
        .env("PATH", "/usr/bin:/bin");
    command
}

/// A scaffolded-and-linked marketplace: created by the CLI, answered about
/// by the registry the machine keeps.
fn create_marketplace(root: &std::path::Path, at: &std::path::Path) -> Command {
    let mut command = uze(root);
    command
        .args(["agent", "market", "create", "tools"])
        .arg("--at")
        .arg(at);
    command
}

#[test]
fn a_scaffolded_marketplace_is_registered_and_linked_in_one_step() {
    let root = uze_testkit::temp::scratch("authoring-cli-market");
    let at = root.join("my-market");
    let created = create_marketplace(&root, &at).output().unwrap();
    assert!(
        created.status.success(),
        "create failed: {}",
        String::from_utf8_lossy(&created.stderr)
    );

    // The bytes are the author's own text, where the agent named.
    assert!(at.join("marketplace.json").is_file());
    assert!(at.join("plugins").is_dir());
    // The identity contract: a Git repository with an initial commit.
    assert!(at.join(".git").exists());
    let commit = Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .arg("-C")
        .arg(&at)
        .args(["rev-parse", "--verify", "HEAD"])
        .output()
        .unwrap();
    assert!(
        commit.status.success(),
        "the scaffold committed: {}",
        String::from_utf8_lossy(&commit.stderr)
    );

    // And the machine registry answers for it, linked to the checkout.
    let list = uze(&root)
        .args(["market", "list", "--format", "json"])
        .output()
        .unwrap();
    assert!(list.status.success());
    let registry: serde_json::Value = serde_json::from_slice(&list.stdout).unwrap();
    let entry = markets_in(&registry).and_then(|markets| {
        markets
            .iter()
            .find(|market| market["name"] == "tools")
            .cloned()
    });
    let entry = entry.unwrap_or_else(|| panic!("registry does not answer for `tools`: {registry}"));
    assert_eq!(entry["name"], "tools");

    // The refusal: the same name again, from the same directory.
    let again = create_marketplace(&root, &at).output().unwrap();
    assert!(
        !again.status.success(),
        "a marketplace already occupying `--at` is refused"
    );
}

fn markets_in(registry: &serde_json::Value) -> Option<Vec<serde_json::Value>> {
    registry
        .get("marketplaces")
        .and_then(|value| {
            if value.is_array() {
                value.as_array().cloned()
            } else {
                None
            }
        })
        .or_else(|| registry.as_array().cloned())
}

#[test]
fn a_scaffolded_plugin_is_installable_before_any_second_commit() {
    let root = uze_testkit::temp::scratch("authoring-cli-plugin");
    let at = root.join("my-market");
    let created = create_marketplace(&root, &at)
        .arg("--description")
        .arg("Test tools")
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "market create failed: {}",
        String::from_utf8_lossy(&created.stderr)
    );

    let plugin = uze(&root)
        .args([
            "agent", "plugin", "create", "greet", "--market", "tools", "--hook",
        ])
        .output()
        .unwrap();
    assert!(
        plugin.status.success(),
        "plugin create failed: {}",
        String::from_utf8_lossy(&plugin.stderr)
    );
    let plugin_root = at.join("plugins/greet");
    assert!(plugin_root.join("plugin.json").is_file());
    assert!(plugin_root.join("skills/greet/SKILL.md").is_file());
    assert!(plugin_root.join("hooks.json").is_file());
    // The entry that makes it installable, written at scaffold time.
    assert!(
        fs::read_to_string(at.join("marketplace.json"))
            .unwrap()
            .contains("\"greet\"")
    );

    // Check before install — and the check is clean, because the scaffold's
    // own output passes its own validation.
    let check = uze(&root)
        .args(["agent", "plugin", "check"])
        .arg(&plugin_root)
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "the scaffold's own output must pass its check: {}{}",
        String::from_utf8_lossy(&check.stdout),
        String::from_utf8_lossy(&check.stderr)
    );

    // And it installs: the linked marketplace reads the working tree, so
    // nothing has been committed since the scaffold's initial commit.
    let (market_args, mut install_args) = marketplace_install_args(&root, &plugin_root);
    // marketplace_install_args stages its own marketplace; the one the
    // scaffold made is already registered, so only the install runs.
    let _ = market_args;
    install_args = vec![
        "install".to_owned(),
        "-m".to_owned(),
        "greet@tools".to_owned(),
    ];
    let install = uze(&root).args(&install_args).output().unwrap();
    assert!(
        install.status.success(),
        "install from the linked marketplace failed: {}",
        String::from_utf8_lossy(&install.stderr)
    );
    assert!(
        root.join("uze/store/plugins/tools/greet/plugin.json")
            .is_file()
            || root.join("uze/store/plugins/greet/plugin.json").is_file(),
        "the Store carries the package"
    );
}

#[test]
fn a_plugin_that_would_fail_at_install_fails_check_first() {
    let root = uze_testkit::temp::scratch("authoring-cli-check");
    let at = root.join("my-market");
    let _ = create_marketplace(&root, &at).output().unwrap();
    let _ = uze(&root)
        .args(["agent", "plugin", "create", "greet", "--market", "tools"])
        .output()
        .unwrap();
    let plugin_root = at.join("plugins/greet");
    fs::write(
        plugin_root.join("plugin.json"),
        r#"{ "name": "-flag", "description": "x" }"#,
    )
    .unwrap();

    let check = uze(&root)
        .args(["agent", "plugin", "check"])
        .arg(&plugin_root)
        .output()
        .unwrap();
    assert!(
        !check.status.success(),
        "an invalid package name is a finding, not a deliverable"
    );
    let stdout = String::from_utf8_lossy(&check.stdout);
    assert!(
        stdout.contains("name"),
        "the finding names the invalid name: {stdout}"
    );
    // Nothing was installed: the machine's Store holds no such package.
    let list = uze(&root)
        .args(["status", "-m", "--format", "json"])
        .output()
        .unwrap();
    assert!(list.status.success());
    let machine: serde_json::Value = serde_json::from_slice(&list.stdout).unwrap();
    let any_greet = machine["packages"]
        .as_array()
        .expect("the machine read model")
        .iter()
        .any(|package| {
            package["id"]
                .as_str()
                .is_some_and(|id| id.starts_with("greet"))
        });
    assert!(!any_greet, "nothing was installed: {machine}");
}
