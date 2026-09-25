use std::fs;
use std::path::PathBuf;
use std::process::Command;

use super::*;
use crate::package::acquisition::marketplace;

/// A process-scoped Git identity the scaffold's commit can use, isolated
/// from the ambient configuration the way every test fixture here is.
fn git_identity() -> uze_testkit::env::ProcessEnvGuard<'static> {
    let configuration = uze_testkit::temp::scratch("authoring-gitconfig").join("gitconfig");
    fs::write(
        &configuration,
        "[user]\n\tname = Test\n\temail = t@example.invalid\n",
    )
    .unwrap();
    let mut environment = uze_testkit::env::scope();
    environment.set("GIT_CONFIG_GLOBAL", &configuration);
    environment.set("GIT_CONFIG_SYSTEM", &configuration);
    environment
}

fn scratch(label: &str) -> PathBuf {
    uze_testkit::temp::scratch(label)
}

/// The load-bearing invariant: **every layout this module writes passes its
/// own check** — every combination of capability flags, and a description
/// carrying what plain YAML would misread. The templates' commented field
/// documentation cannot drift from what the parsers accept, because a drift
/// is this test failing.
#[test]
fn every_scaffold_passes_its_own_check() -> Result<()> {
    let _git_identity = git_identity();
    let description = r#"Deploys: things "fast" — it's #1"#;
    for flags in 0..16_u8 {
        let caps = ScaffoldCapabilities {
            hook: flags & 1 != 0,
            mcp: flags & 2 != 0,
            agent: flags & 4 != 0,
            instructions: flags & 8 != 0,
        };
        let root = scratch(&format!("authoring-scaffold-{flags}"));
        let market = scaffold_marketplace("tools", Some("Test tools"), &root.join("market"))?;
        let plugin = scaffold_plugin(&market, "greet", Some(description), &caps)?;

        let plugin_report = check_plugin(&plugin)?;
        assert!(
            plugin_report.is_clean(),
            "{caps:?}: {:?}",
            plugin_report.findings
        );
        let market_report = check_marketplace(&market)?;
        assert!(
            market_report.is_clean(),
            "{caps:?}: {:?}",
            market_report.findings
        );
        assert!(market_report.delivers.iter().any(|d| d == "greet"));

        // The marketplace manifest the scaffold wrote parses by the same
        // rule an install-time catalogue read uses — and by now names the
        // plugin the scaffold added.
        let manifest = marketplace::parse_manifest(
            &fs::read(root.join("market/marketplace.json")).expect("the manifest is there"),
        )?;
        assert_eq!(manifest.plugins.len(), 1);
        fs::remove_dir_all(&root).expect("teardown");
    }
    Ok(())
}

#[test]
fn the_skill_description_survives_yaml_verbatim() -> Result<()> {
    let _git_identity = git_identity();
    let root = scratch("authoring-description");
    let market = scaffold_marketplace("tools", None, &root.join("market"))?;
    let description = r#"Deploys: things "fast" — it's #1"#;
    let plugin = scaffold_plugin(
        &market,
        "greet",
        Some(description),
        &ScaffoldCapabilities::default(),
    )?;
    let skill = fs::read_to_string(plugin.join("skills/greet/SKILL.md")).unwrap();
    let head = skill
        .strip_prefix("---\n")
        .and_then(|rest| rest.split("\n---\n").next())
        .unwrap();
    let frontmatter: serde_yaml::Value =
        from_str_with_config(head, &ParserConfig::serde_yaml_compat()).unwrap();
    assert_eq!(
        frontmatter
            .get("description")
            .and_then(serde_yaml::Value::as_str),
        Some(description)
    );
    fs::remove_dir_all(&root).expect("teardown");
    Ok(())
}

#[test]
fn authoring_scaffold_meets_the_budget() -> Result<()> {
    // Held while Git is isolated: the scaffold's own Git work is not the
    // budget being measured, the file writes are. The global config points
    // at an empty file for the *read* side too, so `missing_git_identity`
    // is asked inside a world, not on the developer's machine.
    let mut environment = uze_testkit::env::scope();
    let configuration = uze_testkit::temp::scratch("authoring-budget-gitconfig").join("gitconfig");
    fs::write(
        &configuration,
        "[user]\n\tname = T\n\temail = t@example.invalid\n",
    )
    .unwrap();
    environment.set("GIT_CONFIG_GLOBAL", &configuration);
    environment.set("GIT_CONFIG_SYSTEM", &configuration);

    let root = scratch("authoring-scaffold-budget");
    let market = scaffold_marketplace("tools", None, &root.join("market"))?;
    let started = std::time::Instant::now();
    scaffold_plugin(&market, "greet", None, &ScaffoldCapabilities::default())?;
    let elapsed = started.elapsed();
    assert!(
        elapsed < std::time::Duration::from_millis(200),
        "a plugin scaffold is file writes plus one marketplace manifest read: {elapsed:?}"
    );
    fs::remove_dir_all(&root).expect("teardown");
    Ok(())
}

#[test]
fn the_local_marketplace_is_the_project_itself() -> Result<()> {
    let root = scratch("authoring-local");
    // A project: its own repository is the marketplace's repository.
    let project = root.join("project");
    fs::create_dir_all(project.join(".git")).unwrap();
    let (root, plugins) =
        scaffold_local_marketplace("tools", Some("Project tools"), &project, "plugins")?;
    assert!(root.join("marketplace.json").is_file());
    assert!(plugins.is_dir());

    // No Git was touched by the scaffold: the project's repository is the
    // identity, and the commit is the project's own flow's to make — the
    // unborn-HEAD state a fresh fixture starts in stays as it was.
    let manifest = fs::read_to_string(project.join("marketplace.json")).unwrap();
    assert!(manifest.contains("\"name\": \"tools\""));
    let no_commit = Command::new("git")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .arg("-C")
        .arg(&project)
        .args(["rev-parse", "--verify", "HEAD"])
        .output()
        .unwrap();
    assert!(
        !no_commit.status.success(),
        "the scaffold fabricated a commit; the project's flow owns it"
    );

    // A project that already is a marketplace is refused, saying so.
    assert!(
        scaffold_local_marketplace("other", None, &project, "plugins").is_err(),
        "a second manifest would be two marketplaces in one repository"
    );

    // The renameable plugins directory: same manifest rule, other folder.
    let other = scratch("authoring-local-rename");
    let project_two = other.join("project");
    fs::create_dir_all(project_two.join(".git")).unwrap();
    let (_, plugins) = scaffold_local_marketplace("tools", None, &project_two, "tools-plugins")?;
    assert_eq!(plugins, project_two.join("tools-plugins"));
    fs::remove_dir_all(other).expect("teardown");
    fs::remove_dir_all(root).expect("teardown");
    Ok(())
}

#[test]
fn create_refuses_to_collide() -> Result<()> {
    let _git_identity = git_identity();
    let root = scratch("authoring-refusal");
    let market = scaffold_marketplace("tools", None, &root.join("market"))?;
    // The same name again from a different directory is refused only at the
    // registration layer (the machine registry owns the name); what the
    // scaffold itself refuses is a target that already holds something.
    let occupied = root.join("occupied");
    fs::create_dir_all(occupied.join("something")).unwrap();
    assert!(scaffold_marketplace("other", None, &occupied).is_err());
    assert!(
        occupied.join("something").is_dir(),
        "the refusal wrote nothing into the directory"
    );

    // An existing plugin is refused, never overwritten.
    scaffold_plugin(&market, "greet", None, &ScaffoldCapabilities::default())?;
    assert!(scaffold_plugin(&market, "greet", None, &ScaffoldCapabilities::default()).is_err());
    // A name outside the charset is refused by the same rule an id is held to.
    assert!(scaffold_plugin(&market, "-flag", None, &ScaffoldCapabilities::default()).is_err());
    fs::remove_dir_all(&root).expect("teardown");
    Ok(())
}

#[test]
fn check_reports_what_install_would_refuse() -> Result<()> {
    let _git_identity = git_identity();
    let root = scratch("authoring-check-fail");
    let market = scaffold_marketplace("tools", None, &root.join("market"))?;
    let plugin = scaffold_plugin(&market, "greet", None, &ScaffoldCapabilities::default())?;

    // A name the PackageId rule refuses is named before any install ran.
    fs::write(
        plugin.join("plugin.json"),
        r#"{ "name": "-flag", "description": "x" }"#,
    )
    .unwrap();
    let report = check_plugin(&plugin)?;
    assert!(!report.is_clean(), "an invalid name must be a finding");
    assert!(report.findings.iter().any(|f| f.contains("name")));
    fs::write(
        plugin.join("plugin.json"),
        r#"{ "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name": "greet", "description": "x" }"#,
    )
    .unwrap();

    // A marketplace entry pointing outside itself is located by name.
    fs::write(
        root.join("market/marketplace.json"),
        r#"{ "name": "tools", "plugins": [ { "name": "escape", "source": "../outside" } ] }"#,
    )
    .unwrap();
    let market_report = check_marketplace(&root.join("market"))?;
    assert!(
        market_report
            .findings
            .iter()
            .any(|finding| finding.starts_with("escape")),
        "the finding locates the plugin that escapes: {:?}",
        market_report.findings
    );
    fs::remove_dir_all(&root).expect("teardown");
    Ok(())
}

#[test]
fn the_marketplace_check_covers_its_plugins() -> Result<()> {
    let _git_identity = git_identity();
    let root = scratch("authoring-market-check");
    let market = scaffold_marketplace("tools", None, &root.join("market"))?;
    let plugin = scaffold_plugin(
        &market,
        "greet",
        None,
        &ScaffoldCapabilities {
            hook: true,
            ..ScaffoldCapabilities::default()
        },
    )?;
    // A hook manifest violating the handler contract (out-of-bounds timeout)
    // is a finding located in that plugin.
    fs::write(
        plugin.join("hooks.json"),
        r#"{ "hooks": { "PreToolUse": [ { "id": "x", "matcher": "shell", "effect": "deny", "hooks": [ { "type": "command", "command": "${PLUGIN_ROOT}/scripts/guard", "timeout": 301 } ] } ] } }"#,
    )
    .unwrap();
    let report = check_marketplace(&market)?;
    assert!(
        report
            .findings
            .iter()
            .any(|finding| finding.starts_with("greet") && finding.contains("between 1 and 300")),
        "an out-of-bounds handler timeout is found in its plugin, not delivered: {:?}",
        report.findings
    );
    fs::remove_dir_all(&root).expect("teardown");
    Ok(())
}

#[test]
fn an_empty_at_directory_is_accepted() -> Result<()> {
    let _git_identity = git_identity();
    let root = scratch("authoring-empty-at");
    let at = root.join("market");
    fs::create_dir_all(&at).unwrap();
    scaffold_marketplace("tools", None, &at)?;
    assert!(at.join("marketplace.json").is_file());
    fs::remove_dir_all(&root).expect("teardown");
    Ok(())
}

#[test]
fn a_missing_git_identity_is_refused_before_any_write() {
    let mut environment = uze_testkit::env::scope();
    let empty = scratch("authoring-no-identity-config").join("gitconfig");
    fs::write(&empty, "").unwrap();
    environment.set("GIT_CONFIG_GLOBAL", &empty);
    environment.set("GIT_CONFIG_SYSTEM", &empty);
    let root = scratch("authoring-no-identity");
    let at = root.join("market");

    let refused = scaffold_marketplace("tools", None, &at);

    assert!(refused.is_err(), "no identity, no commit, no scaffold");
    assert!(!at.exists(), "the refusal wrote nothing");
    fs::remove_dir_all(&root).expect("teardown");
}

#[test]
fn a_name_the_manifest_already_carries_is_refused_before_any_write() -> Result<()> {
    let _git_identity = git_identity();
    let root = scratch("authoring-ghost");
    let market = scaffold_marketplace("tools", None, &root.join("market"))?;
    fs::write(
        market.join("marketplace.json"),
        r#"{ "name": "tools", "plugins": [ { "name": "ghost", "source": "./elsewhere" } ] }"#,
    )
    .unwrap();

    let refused = scaffold_plugin(&market, "ghost", None, &ScaffoldCapabilities::default());

    assert!(refused.is_err());
    assert!(
        !market.join("plugins/ghost").exists(),
        "the refusal wrote nothing"
    );
    fs::remove_dir_all(&root).expect("teardown");
    Ok(())
}

/// The plugins directory a local marketplace chose is where every later
/// plugin goes — recorded in the manifest while it has no entry to say so.
#[test]
fn a_plugin_goes_where_the_marketplace_keeps_its_plugins() -> Result<()> {
    let root = scratch("authoring-plugins-dir");
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    scaffold_local_marketplace("tools", None, &project, "tools-plugins")?;

    let first = scaffold_plugin(&project, "greet", None, &ScaffoldCapabilities::default())?;
    assert_eq!(first, project.join("tools-plugins/greet"));

    // With an entry to read, the recorded key is no longer the only answer.
    let mut manifest = read_json(&project.join("marketplace.json"))?;
    manifest
        .as_object_mut()
        .unwrap()
        .remove(PLUGINS_DIRECTORY_KEY);
    write_json(&project.join("marketplace.json"), &manifest)?;
    let second = scaffold_plugin(&project, "wave", None, &ScaffoldCapabilities::default())?;
    assert_eq!(second, project.join("tools-plugins/wave"));

    let report = check_marketplace(&project)?;
    assert!(report.is_clean(), "{:?}", report.findings);
    assert_eq!(report.delivers, ["greet", "wave"]);
    fs::remove_dir_all(&root).expect("teardown");
    Ok(())
}

#[test]
fn check_reports_a_skill_a_harness_could_not_read() -> Result<()> {
    let _git_identity = git_identity();
    let root = scratch("authoring-check-skill");
    let market = scaffold_marketplace("tools", None, &root.join("market"))?;
    let plugin = scaffold_plugin(&market, "greet", None, &ScaffoldCapabilities::default())?;
    let skill = plugin.join("skills/greet/SKILL.md");

    for (body, fault) in [
        ("Just prose, no frontmatter.\n", "no frontmatter"),
        (
            "---\nname: greet\ndescription: Deploys: things fast\n---\nbody\n",
            "not valid YAML",
        ),
        ("---\nname: greet\n---\nbody\n", "no `description`"),
    ] {
        fs::write(&skill, body).unwrap();
        let report = check_plugin(&plugin)?;
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.contains("SKILL.md") && finding.contains(fault)),
            "{fault}: {:?}",
            report.findings
        );
    }
    fs::remove_dir_all(&root).expect("teardown");
    Ok(())
}

#[test]
fn check_reports_a_reference_outside_the_plugin() -> Result<()> {
    let _git_identity = git_identity();
    let root = scratch("authoring-check-escape");
    let market = scaffold_marketplace("tools", None, &root.join("market"))?;
    let plugin = scaffold_plugin(
        &market,
        "greet",
        None,
        &ScaffoldCapabilities {
            hook: true,
            mcp: true,
            ..ScaffoldCapabilities::default()
        },
    )?;

    for (file, body, reference) in [
        (
            "hooks.json",
            r#"{ "hooks": { "PreToolUse": [ { "id": "x", "matcher": "shell", "effect": "deny", "hooks": [ { "type": "command", "command": "${PLUGIN_ROOT}/../guard" } ] } ] } }"#,
            "${PLUGIN_ROOT}/../guard",
        ),
        (
            "hooks.json",
            r#"{ "hooks": { "PreToolUse": [ { "id": "x", "matcher": "shell", "effect": "deny", "hooks": [ { "type": "command", "command": "sh /opt/guard" } ] } ] } }"#,
            "/opt/guard",
        ),
        (
            "mcp.json",
            r#"{ "mcpServers": { "example": { "command": "python3", "args": ["../server.py"] } } }"#,
            "../server.py",
        ),
    ] {
        let original = fs::read(plugin.join(file)).unwrap();
        fs::write(plugin.join(file), body).unwrap();
        let report = check_plugin(&plugin)?;
        assert!(
            report
                .findings
                .iter()
                .any(|finding| finding.contains(file) && finding.contains(reference)),
            "{reference}: {:?}",
            report.findings
        );
        fs::write(plugin.join(file), original).unwrap();
    }
    fs::remove_dir_all(&root).expect("teardown");
    Ok(())
}
