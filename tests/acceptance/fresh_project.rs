//! Acceptance scenarios A1-A3 + the golden environment health signal:
//! fresh machine with a canonical plugin, fresh clone with `agents.lock`,
//! marketplace + consumer, and `golden_environment_is_healthy`.

use std::path::PathBuf;

use uze_testkit::fixtures;
use uze_testkit::scenario::Scenario;
use uze_testkit::temp::TestEnvironment;

use crate::util::{install_fake_harnesses, marketplace_json, uze_bin};

/// A1 — fresh machine: empty HOME/UZE_HOME, install a canonical plugin,
/// Store correct, projection correct, inspect healthy.
#[test]
fn fresh_machine_installs_canonical_plugin_and_inspects_healthy() {
    let env = TestEnvironment::isolated();
    let _harnesses = install_fake_harnesses(&env);

    // The default `uze` plugin is seeded even on an empty machine.
    let (market_args, install_args) = uze_testkit::marketplace::marketplace_install_args(
        &env.home,
        &fixtures::canonical("skill-plugin"),
    );
    env.run_ok(
        uze_bin(),
        &market_args.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    let mut with_json = install_args.clone();
    with_json.push("--format".to_owned());
    with_json.push("json".to_owned());
    let install = env.run_ok(
        uze_bin(),
        &with_json.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    let report: serde_json::Value = serde_json::from_slice(&install.stdout).expect("json report");
    assert_eq!(report["plugin"]["id"], "uze-agent-skill-conformance@test");
    let store_path = PathBuf::from(report["plugin"]["store_path"].as_str().unwrap());
    assert!(
        store_path.starts_with(&env.uze_home),
        "store path must live under the isolated UZE_HOME, got {}",
        store_path.display()
    );

    let inspect = env.run_ok(
        uze_bin(),
        &[
            "plugin",
            "inspect",
            "uze-agent-skill-conformance",
            "--format",
            "json",
        ],
    );
    let report: serde_json::Value = serde_json::from_slice(&inspect.stdout).expect("json report");
    let capabilities = report["capabilities"]
        .as_array()
        .expect("capabilities array");
    assert_eq!(capabilities.len(), 1, "one canonical Skill resource");
    assert_eq!(capabilities[0]["kind"], "agent_skill");

    let doctor = env.run_ok(uze_bin(), &["doctor"]);
    let stdout = String::from_utf8_lossy(&doctor.stdout);
    assert!(stdout.contains("UZE Home"), "doctor must report its home");
}

/// A2 — fresh clone + `agents.lock`: dependencies absent, `uze status`
/// reports missing, `uze install` makes the environment ready.
#[test]
fn fresh_clone_with_lock_install_marks_environment_ready() {
    let env = TestEnvironment::isolated();
    let _harnesses = install_fake_harnesses(&env);
    let scenario = Scenario::new()
        .marketplace("ai", &marketplace_json("ai", "flow"))
        .marketplace_plugin("flow", fixtures::canonical("flow"))
        .lock_plugin_from_market("ai", "flow")
        .project_file("AGENTS.md", "# Project\n")
        .materialize(&env);
    env.run_ok(
        uze_bin(),
        &[
            "market",
            "add",
            scenario.marketplace.as_ref().unwrap().to_str().unwrap(),
        ],
    );

    let before = env.run_ok(uze_bin(), &["status"]);
    let before = String::from_utf8_lossy(&before.stdout);
    assert!(
        before.contains("flow") && before.contains("missing (run `uze install`)"),
        "status must call out the missing locked plugin, got: {before}"
    );

    let install = env.run_ok(uze_bin(), &["install"]);
    assert!(
        String::from_utf8_lossy(&install.stdout).contains("Installed"),
        "install must report success"
    );

    let after = env.run_ok(uze_bin(), &["status"]);
    let after = String::from_utf8_lossy(&after.stdout);
    assert!(
        after.contains("flow") && after.contains("installed"),
        "status must show the plugin installed, got: {after}"
    );
    assert!(
        after.contains("no issues"),
        "a reconciled fresh project must be healthy, got: {after}"
    );
}

/// A3 — marketplace + consumer: register a marketplace, add a plugin by
/// shorthand, install from the lock.
#[test]
fn marketplace_resolution_installs_plugin_by_shorthand() {
    let env = TestEnvironment::isolated();
    let _harnesses = install_fake_harnesses(&env);
    let scenario = Scenario::new()
        .marketplace("ai", &marketplace_json("ai", "flow"))
        .marketplace_plugin("flow", fixtures::canonical("flow"))
        .project_file("AGENTS.md", "# Project\n")
        .project_file("agents.lock", "version: 1\n")
        .materialize(&env);
    let market = scenario.marketplace.as_ref().unwrap();
    env.run_ok(uze_bin(), &["market", "add", market.to_str().unwrap()]);

    let list = env.run_ok(uze_bin(), &["market", "list", "--format", "json"]);
    let json: serde_json::Value = serde_json::from_slice(&list.stdout).expect("json list");
    let marketplaces = json.as_array().expect("json list array");
    assert!(
        marketplaces
            .iter()
            .any(|m| m["name"].as_str() == Some("ai")),
        "registered marketplace must appear in market list: {json}"
    );

    // `<plugin>@<market>` is the project-scoped shorthand: it writes the
    // lock and installs.
    let add = env.run_ok(uze_bin(), &["flow@ai"]);
    assert!(
        String::from_utf8_lossy(&add.stdout).contains("flow"),
        "add must name the flow plugin, got: {}",
        String::from_utf8_lossy(&add.stdout)
    );
    assert!(
        env.project.join("agents.lock").is_file(),
        "shorthand must write agents.lock"
    );

    let status = env.run_ok(uze_bin(), &["status"]);
    let status = String::from_utf8_lossy(&status.stdout);
    assert!(
        status.contains("flow") && status.contains("installed"),
        "got: {status}"
    );
}

/// The release signal: the golden marketplace + project install clean, and
/// doctor reports zero missing, zero drifted, zero conflicts.
#[test]
fn golden_environment_is_healthy() {
    let env = TestEnvironment::isolated();
    let _harnesses = install_fake_harnesses(&env);
    let golden = fixtures::golden();
    let golden_market = golden.join("marketplace");
    let marketplace = std::fs::read_to_string(golden_market.join("marketplace.json"))
        .expect("golden marketplace marketplace.json");
    let project_readme = std::fs::read_to_string(golden.join("project/AGENTS.md"))
        .expect("golden project AGENTS.md");

    let scenario = Scenario::new()
        .marketplace("golden", &marketplace)
        .marketplace_plugin("flow", golden_market.join("plugins/flow"))
        .lock_plugin_from_market("golden", "flow")
        .project_file("AGENTS.md", project_readme)
        .materialize(&env);
    let market = scenario.marketplace.as_ref().unwrap();
    env.run_ok(uze_bin(), &["market", "add", market.to_str().unwrap()]);
    env.run_ok(uze_bin(), &["install"]);

    let doctor = env.run_ok(uze_bin(), &["doctor"]);
    let stdout = String::from_utf8_lossy(&doctor.stdout);
    assert!(
        stdout.contains("0 missing"),
        "golden environment must have nothing missing, got: {stdout}"
    );
    assert!(
        stdout.contains("0 drifted"),
        "golden environment must have nothing drifted, got: {stdout}"
    );
    assert!(
        stdout.contains("0 conflicts"),
        "golden environment must have nothing conflicting, got: {stdout}"
    );

    let status = env.run_ok(uze_bin(), &["status"]);
    let status = String::from_utf8_lossy(&status.stdout);
    assert!(
        status.contains("flow") && status.contains("no issues"),
        "golden project must be healthy end-to-end, got: {status}"
    );
}
