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
/// own check.** The templates' commented field documentation cannot drift
/// from what the parsers accept, because a drift is this test failing.
#[test]
fn every_scaffold_passes_its_own_check() -> Result<()> {
    let _git_identity = git_identity();
    for (hook, mcp, instructions) in [
        (false, false, false),
        (true, false, false),
        (false, true, false),
        (true, true, true),
    ] {
        let label = format!("authoring-scaffold-{hook}{mcp}{instructions}");
        let root = scratch(&label);
        let market = scaffold_marketplace("tools", Some("Test tools"), &root.join("market"))?;
        let plugin = scaffold_plugin(
            &market,
            "greet",
            Some("Says hello"),
            hook,
            mcp,
            instructions,
        )?;

        let plugin_report = check_plugin(&plugin)?;
        assert!(
            plugin_report.is_clean(),
            "hook={hook} mcp={mcp} instructions={instructions}: {:?}",
            plugin_report.findings
        );
        let market_report = check_marketplace(&market)?;
        assert!(
            market_report.is_clean(),
            "hook={hook} mcp={mcp}: {:?}",
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
    scaffold_plugin(&market, "greet", None, false, false, false)?;
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
    scaffold_plugin(&market, "greet", None, false, false, false)?;
    assert!(scaffold_plugin(&market, "greet", None, false, false, false).is_err());
    // A name outside the charset is refused by the same rule an id is held to.
    assert!(scaffold_plugin(&market, "-flag", None, false, false, false).is_err());
    fs::remove_dir_all(&root).expect("teardown");
    Ok(())
}

#[test]
fn check_reports_what_install_would_refuse() -> Result<()> {
    let _git_identity = git_identity();
    let root = scratch("authoring-check-fail");
    let market = scaffold_marketplace("tools", None, &root.join("market"))?;
    let plugin = scaffold_plugin(&market, "greet", None, false, false, false)?;

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
    let plugin = scaffold_plugin(&market, "greet", None, true, false, false)?;
    // A hook manifest violating the handler contract (out-of-bounds timeout)
    // is a finding located in that plugin.
    fs::write(
        plugin.join("hooks.json"),
        r#"{ "hooks": { "PreToolUse": [ { "id": "x", "matcher": "shell", "effect": "deny", "hooks": [ { "type": "command", "command": "${PLUGIN_ROOT}/scripts/guard", "timeout": 99999 } ] } ] } }"#,
    )
    .unwrap();
    let report = check_marketplace(&market)?;
    assert!(
        !report.is_clean(),
        "an out-of-bounds handler timeout must be found, not delivered: {:?}",
        report.delivers
    );
    fs::remove_dir_all(&root).expect("teardown");
    Ok(())
}
