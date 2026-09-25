//! The authoring surface's own contracts: a scaffolded marketplace is born
//! registered and linked, a scaffolded plugin is installable immediately,
//! and the offline check is the same validation an install applies —
//! delivered before one.
//!
//! The scaffold-invariant test lives beside the templates in `uze-core`
//! (`package::authoring::tests`); this suite proves the *orchestration*:
//! what the machine registry and the Store carry once the verbs have run.

use std::{fs, process::Command};

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
    let install = uze(&root)
        .args(["install", "-m", "greet@tools"])
        .output()
        .unwrap();
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
fn a_local_marketplace_is_the_project_itself_and_installs_immediately() {
    let root = uze_testkit::temp::scratch("authoring-cli-local");
    // The project is a real repository with a commit — a marketplace is a
    // Git repository with a commit, and here the project's repo is it.
    let project = root.join("project");
    fs::write(
        root.join(".gitconfig"),
        "[user]\n\tname = Test\n\temail = t@example.invalid\n",
    )
    .unwrap();
    let initialized = Command::new("git")
        .env("GIT_CONFIG_GLOBAL", root.join(".gitconfig"))
        .arg("-C")
        .arg(&root)
        .args([
            "init",
            "-q",
            "-b",
            "main",
            project.to_string_lossy().as_ref(),
        ])
        .output()
        .unwrap();
    assert!(initialized.status.success());
    fs::write(project.join("README.md"), "# demo\n").unwrap();
    let committed = Command::new("git")
        .env("GIT_CONFIG_GLOBAL", root.join(".gitconfig"))
        .arg("-C")
        .arg(&project)
        .args(["add", "-A"])
        .output()
        .unwrap();
    assert!(committed.status.success());
    let commit = Command::new("git")
        .env("GIT_CONFIG_GLOBAL", root.join(".gitconfig"))
        .arg("-C")
        .arg(&project)
        .args(["commit", "-q", "-m", "first"])
        .output()
        .unwrap();
    assert!(commit.status.success());
    let created = uze(&root)
        .current_dir(&project)
        .args(["agent", "market", "create", "tools", "--local"])
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "local create failed: {}",
        String::from_utf8_lossy(&created.stderr)
    );
    // The manifest sits at the project root — the project is the
    // marketplace — and no second commit was fabricated by the scaffold.
    assert!(project.join("marketplace.json").is_file());
    assert!(project.join("plugins").is_dir());
    let commits = Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .arg("-C")
        .arg(&project)
        .args(["rev-list", "--count", "HEAD"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&commits.stdout).trim(),
        "1",
        "the scaffold committed nothing; the project's flow owns it"
    );

    // A plugin authored into it is installable straight away.
    let plugin = uze(&root)
        .current_dir(&project)
        .args(["agent", "plugin", "create", "greet", "--market", "tools"])
        .output()
        .unwrap();
    assert!(
        plugin.status.success(),
        "plugin create failed: {}",
        String::from_utf8_lossy(&plugin.stderr)
    );
    assert!(project.join("plugins/greet/plugin.json").is_file());
    assert!(
        fs::read_to_string(project.join("marketplace.json"))
            .unwrap()
            .contains("\"greet\"")
    );

    let check = uze(&root)
        .current_dir(&project)
        .args(["agent", "plugin", "check"])
        .arg(project.join("plugins/greet"))
        .output()
        .unwrap();
    assert!(check.status.success(), "scaffold must pass its own check");

    // The install reads the project's working tree through the link.
    let install = uze(&root)
        .current_dir(&project)
        .args(["install", "-m", "greet@tools"])
        .output()
        .unwrap();
    assert!(
        install.status.success(),
        "install from the project marketplace failed: {}",
        String::from_utf8_lossy(&install.stderr)
    );

    // A second marketplace in the same project is refused with the fact.
    let again = uze(&root)
        .current_dir(&project)
        .args(["agent", "market", "create", "other", "--local"])
        .output()
        .unwrap();
    assert!(!again.status.success());
    let stderr = String::from_utf8_lossy(&again.stderr);
    assert!(
        stderr.contains("already"),
        "the refusal says the project is already a marketplace: {stderr}"
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

/// A Git repository at `project`, with a commit when `committed`, under
/// this world's own Git identity.
fn git_project(root: &std::path::Path, committed: bool) -> std::path::PathBuf {
    let project = root.join("project");
    let gitconfig = root.join(".gitconfig");
    fs::write(
        &gitconfig,
        "[user]\n\tname = Test\n\temail = t@example.invalid\n",
    )
    .unwrap();
    let git = |arguments: &[&str]| {
        let output = Command::new("git")
            .env("GIT_CONFIG_GLOBAL", &gitconfig)
            .arg("-C")
            .arg(&project)
            .args(arguments)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {arguments:?}");
    };
    fs::create_dir_all(&project).unwrap();
    git(&["init", "-q", "-b", "main"]);
    if committed {
        fs::write(project.join("README.md"), "# demo\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "first"]);
    }
    project
}

fn registry(root: &std::path::Path) -> String {
    fs::read_to_string(uze_core::UzeHome::at(root.join("uze")).marketplaces_path())
        .unwrap_or_default()
}

/// A clone of the project resolves the marketplace it is: the declaration
/// names the project root relative to itself, never this machine's path.
#[test]
fn a_local_marketplace_is_declared_relative_to_the_project() {
    let root = uze_testkit::temp::scratch("authoring-cli-local-declared");
    let project = git_project(&root, true);
    for arguments in [
        &["agent", "market", "create", "tools", "--local"][..],
        &["agent", "plugin", "create", "greet", "--market", "tools"],
        &["greet@tools"],
    ] {
        let output = uze(&root)
            .current_dir(&project)
            .args(arguments)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let declared = fs::read_to_string(project.join("agents.yaml")).unwrap();
    assert!(declared.contains("path: ."), "{declared}");
    assert!(
        !declared.contains(&*project.to_string_lossy()),
        "the declaration names no machine path: {declared}"
    );
}

#[test]
fn a_local_marketplace_in_a_project_with_no_commit_writes_nothing() {
    let root = uze_testkit::temp::scratch("authoring-cli-local-unborn");
    let project = git_project(&root, false);

    let created = uze(&root)
        .current_dir(&project)
        .args(["agent", "market", "create", "tools", "--local"])
        .output()
        .unwrap();

    assert!(!created.status.success());
    assert!(
        String::from_utf8_lossy(&created.stderr).contains("with a commit"),
        "the refusal names the missing commit: {}",
        String::from_utf8_lossy(&created.stderr)
    );
    assert!(!project.join("marketplace.json").exists());
    assert!(!project.join("plugins").exists());
    assert!(!registry(&root).contains("\"tools\""));
}

#[test]
fn a_registered_name_is_refused_before_the_scaffold() {
    let root = uze_testkit::temp::scratch("authoring-cli-taken-name");
    let first = create_marketplace(&root, &root.join("first"))
        .output()
        .unwrap();
    assert!(first.status.success());

    let second_at = root.join("second");
    let second = create_marketplace(&root, &second_at).output().unwrap();

    assert!(!second.status.success());
    assert!(
        !second_at.exists(),
        "the refusal came before the scaffold wrote or committed anything"
    );
}

/// The registry outlives the directory the command ran from, so it records
/// where the marketplace is, not how it was spelled.
#[test]
fn a_relative_at_is_registered_absolute() {
    let root = uze_testkit::temp::scratch("authoring-cli-relative-at");
    let created = uze(&root)
        .current_dir(&root)
        .args(["agent", "market", "create", "tools", "--at", "rel/mk"])
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stderr)
    );
    assert!(
        String::from_utf8_lossy(&created.stdout).ends_with('\n'),
        "the answer ends its last line"
    );

    let registry: serde_json::Value = serde_json::from_str(&registry(&root)).unwrap();
    let mut recorded = Vec::new();
    collect_strings(&registry, &mut recorded);
    let paths: Vec<_> = recorded
        .iter()
        .filter(|value| value.ends_with("rel/mk"))
        .collect();
    assert!(!paths.is_empty(), "the marketplace is recorded: {registry}");
    assert!(
        paths.iter().all(|path| path.starts_with('/')),
        "every recorded path is absolute: {registry}"
    );

    let plugin = uze(&root)
        .args(["agent", "plugin", "create", "greet", "--market", "tools"])
        .output()
        .unwrap();
    assert!(plugin.status.success());
    assert!(String::from_utf8_lossy(&plugin.stdout).ends_with('\n'));
}

fn collect_strings(value: &serde_json::Value, into: &mut Vec<String>) {
    match value {
        serde_json::Value::String(text) => into.push(text.clone()),
        serde_json::Value::Array(values) => values.iter().for_each(|v| collect_strings(v, into)),
        serde_json::Value::Object(entries) => {
            entries.values().for_each(|v| collect_strings(v, into))
        }
        _ => {}
    }
}
