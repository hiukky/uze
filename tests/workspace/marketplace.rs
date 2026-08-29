//! Workspace negatives: malformed external inputs are reported cleanly,
//! never panicked on (L2 CLI evidence, isolated env).

use uze_testkit::fixtures;
use uze_testkit::scenario::Scenario;
use uze_testkit::temp::TestEnvironment;

use crate::util::uze_bin;

fn marketplace_json(name: &str, plugin: &str) -> String {
    format!(
        r#"{{"name":"{name}","plugins":[{{"name":"{plugin}","source":"./plugins/{plugin}"}}]}}"#
    )
}

/// A `marketplace.json` with a plugin missing required fields: `market add`
/// must fail with a parse error naming the file + field, and must leave
/// the registry functional (no partial state).
#[test]
fn malformed_marketplace_is_reported_not_panicked_on() {
    let env = TestEnvironment::isolated();
    let market = fixtures::scenario("malformed-marketplace");
    let add = env.run(uze_bin(), &["market", "add", market.to_str().unwrap()]);
    assert!(
        !add.status.success(),
        "malformed marketplace.json must be rejected"
    );
    let stderr = String::from_utf8_lossy(&add.stderr);
    assert!(
        stderr.contains("marketplace.json") && stderr.contains("parse"),
        "the error must name the malformed file, got: {stderr}"
    );
    assert!(
        !stderr.contains("panicked"),
        "parsing must never panic, got: {stderr}"
    );

    // Registry still functional afterwards.
    let list = env.run_ok(uze_bin(), &["market", "list"]);
    assert!(String::from_utf8_lossy(&list.stdout).contains("uze-official"));
}

/// `agents.json` is not a compatibility alias: marketplace discovery has one
/// deterministic root-manifest name.
#[test]
fn agents_json_only_root_is_rejected() {
    let env = TestEnvironment::isolated();
    let market = env.root().join("agents-json-only");
    std::fs::create_dir_all(&market).unwrap();
    std::fs::write(
        market.join("agents.json"),
        r#"{"name":"legacy","plugins":[]}"#,
    )
    .unwrap();

    let add = env.run(uze_bin(), &["market", "add", market.to_str().unwrap()]);
    assert!(!add.status.success(), "agents.json must not be accepted");
    let stderr = String::from_utf8_lossy(&add.stderr);
    assert!(
        stderr.contains("marketplace.json"),
        "the missing canonical manifest must be named, got: {stderr}"
    );
}

/// A marketplace with an installed plugin must never be removable — even
/// if `plugin_marketplaces.json` (a cache populated only at install time,
/// never repaired) has lost its entry for that plugin. Production hit
/// exactly this: a plugin (`git@ai`) stayed installed and healthy while its
/// marketplace (`ai`) vanished from `market list`, because the removal
/// guard trusted only that cache. The Store's own package id is already
/// marketplace-qualified (ADR-036) and must be checked directly too.
#[test]
fn removing_a_marketplace_is_blocked_even_if_the_plugin_ledger_is_stale() {
    let env = TestEnvironment::isolated();
    let scenario = Scenario::new()
        .marketplace(
            "stale-ledger-market",
            &marketplace_json("stale-ledger-market", "flow"),
        )
        .marketplace_plugin("flow", fixtures::canonical("flow"))
        .materialize(&env);
    let market = scenario.marketplace.as_ref().unwrap();
    env.run_ok(uze_bin(), &["market", "add", market.to_str().unwrap()]);

    // Seed an empty lock so `resolve_project_root`'s upward walk stops
    // right here. Without a marker of its own, a fresh isolated project
    // dir keeps walking past the env root into the real `/tmp` — and if
    // any earlier test run ever left an `agents.lock` directly there (the
    // walk has no bound), every test with no marker of its own would
    // silently share and mutate that one file instead of its own isolated
    // project.
    std::fs::write(env.project.join("agents.lock"), "version: 1\n").unwrap();

    env.run_ok(uze_bin(), &["flow@stale-ledger-market"]);

    // Simulate the ledger losing its entry for this plugin — the exact gap
    // that let `git@ai` survive in production while its marketplace did not.
    std::fs::write(
        env.uze_home.join("state/plugin_marketplaces.json"),
        r#"{"plugins":{}}"#,
    )
    .unwrap();

    let remove = env.run(uze_bin(), &["market", "remove", "stale-ledger-market"]);
    assert!(
        !remove.status.success(),
        "market remove must still refuse — the Store itself still has `flow@stale-ledger-market` installed"
    );
    let stderr = String::from_utf8_lossy(&remove.stderr);
    assert!(
        stderr.contains("still has installed plugins"),
        "got: {stderr}"
    );

    let list = env.run_ok(uze_bin(), &["market", "list"]);
    assert!(
        String::from_utf8_lossy(&list.stdout).contains("stale-ledger-market"),
        "marketplace must remain registered after a blocked removal"
    );
}
