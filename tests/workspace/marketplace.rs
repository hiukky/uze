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

/// A marketplace with an installed plugin must never be removable. Production
/// once lost a marketplace (`ai`) from `market list` while its plugin
/// (`git@ai`) stayed installed, because the guard trusted a side ledger; the
/// Store's marketplace-qualified package ids (ADR-036) are the answer.
#[test]
fn removing_a_marketplace_takes_its_packages_with_it() {
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

    // The project declares itself, so resolving its root never walks past
    // the isolated environment into whatever an ancestor directory holds.
    std::fs::write(env.project.join("agents.yaml"), "worktrees: {}\n").unwrap();

    env.run_ok(uze_bin(), &["flow@stale-ledger-market"]);

    // The teardown: the package comes off — detach, Store bytes — and the
    // registry entry goes last.
    let remove = env.run_ok(uze_bin(), &["market", "remove", "stale-ledger-market"]);
    let stdout = String::from_utf8_lossy(&remove.stdout);
    assert!(
        stdout.contains("Removed"),
        "the purge names what it took off: {stdout}"
    );

    let list = env.run_ok(uze_bin(), &["market", "list"]);
    assert!(
        !String::from_utf8_lossy(&list.stdout).contains("stale-ledger-market"),
        "the registry entry went with the packages"
    );
    let store = env.run_ok(uze_bin(), &["status", "-m", "--format", "json"]);
    let machine: serde_json::Value =
        serde_json::from_slice(&store.stdout).expect("machine read model");
    assert!(
        !machine["packages"]
            .as_array()
            .unwrap()
            .iter()
            .any(|package| package["id"] == "flow@stale-ledger-market"),
        "the Store holds no package from the removed marketplace: {machine}"
    );
}

fn marketplace_with_two_plugins() -> (TestEnvironment, std::path::PathBuf) {
    let env = TestEnvironment::isolated();
    // The stand-ins shadow whatever the host has on PATH, so the packages
    // are delivered — and hold receipts — on a CI runner with no harness
    // exactly as on a machine with four.
    uze_testkit::fake_harness::Standard {
        bin_dir: &env.fake_bin,
        home: &env.home,
        state_root: &env.root().join("fake-state"),
        interactive: false,
        opencode_binary: "opencode",
    }
    .install();
    let scenario = Scenario::new()
        .marketplace(
            "purge-market",
            r#"{"name":"purge-market","plugins":[
                {"name":"flow","source":"./plugins/flow"},
                {"name":"uze-mcp-conformance","source":"./plugins/uze-mcp-conformance"}
            ]}"#,
        )
        .marketplace_plugin("flow", fixtures::canonical("flow"))
        .marketplace_plugin("uze-mcp-conformance", fixtures::canonical("mcp-plugin"))
        .materialize(&env);
    let market = scenario.marketplace.as_ref().unwrap();
    env.run_ok(uze_bin(), &["market", "add", market.to_str().unwrap()]);
    std::fs::write(env.project.join("agents.yaml"), "worktrees: {}\n").unwrap();
    env.run_ok(uze_bin(), &["flow@purge-market"]);
    env.run_ok(uze_bin(), &["uze-mcp-conformance@purge-market"]);
    (env, scenario.marketplace.unwrap())
}

/// A package whose teardown is blocked keeps the marketplace registered —
/// the leftovers it still holds stay reachable — and names the block; the
/// packages that came off stay off.
#[test]
fn a_blocked_package_keeps_the_marketplace_registered() {
    let (env, _market) = marketplace_with_two_plugins();
    // The drift target comes from the receipts ledger, not from a guessed
    // filesystem layout: what was delivered (and where) is what the ledger
    // owns, on any machine, detected harness or not.
    // The drift is injected through the ledger itself: a receipt that no
    // longer matches what was delivered is exactly "managed state has
    // drifted" — the one thing the removal machinery refuses to destroy.
    let receipts = uze_core::state::receipts(
        &uze_core::UzeHome::at(&env.uze_home),
        Some("flow@purge-market"),
    )
    .unwrap();
    let mut drifted = receipts
        .first()
        .cloned()
        .expect("flow has receipts after install");
    let foreign = env.home.join("foreign");
    std::fs::create_dir_all(&foreign).unwrap();
    drifted.artifact = uze_core::exposure::ManagedArtifact::SymlinkReference {
        path: foreign.clone(),
        target: foreign.clone(),
    };
    uze_core::state::record_receipt(&uze_core::UzeHome::at(&env.uze_home), drifted).unwrap();

    let remove = env.run(uze_bin(), &["market", "remove", "purge-market"]);
    assert!(
        !remove.status.success(),
        "a blocked teardown is a non-zero answer: {}",
        String::from_utf8_lossy(&remove.stdout)
    );
    let stdout = String::from_utf8_lossy(&remove.stdout);
    assert!(
        stdout.contains("Blocked") && stdout.contains("flow@purge-market"),
        "the block is named with its package: {stdout}"
    );

    let list = env.run_ok(uze_bin(), &["market", "list"]);
    assert!(
        String::from_utf8_lossy(&list.stdout).contains("purge-market"),
        "the marketplace stays registered while a leftover remains"
    );

    let store = uze_core::UzeHome::at(&env.uze_home)
        .plugins_dir()
        .join("purge-market");
    assert!(
        store.join("flow").is_dir(),
        "the blocked package keeps its Store bytes"
    );
    assert!(
        !store.join("uze-mcp-conformance").exists(),
        "the package whose teardown was not blocked came off the Store"
    );
}

#[test]
fn removing_a_marketplace_that_installs_nothing_removes_in_one_step() {
    let env = TestEnvironment::isolated();
    let scenario = Scenario::new()
        .marketplace("empty-market", &marketplace_json("empty-market", "flow"))
        .marketplace_plugin("flow", fixtures::canonical("flow"))
        .materialize(&env);
    let market = scenario.marketplace.as_ref().unwrap();
    env.run_ok(uze_bin(), &["market", "add", market.to_str().unwrap()]);

    let remove = env.run_ok(uze_bin(), &["market", "remove", "empty-market"]);
    let stdout = String::from_utf8_lossy(&remove.stdout);
    assert!(
        stdout.contains("Marketplace removed"),
        "the record removal alone: {stdout}"
    );
    let list = env.run_ok(uze_bin(), &["market", "list"]);
    assert!(
        !String::from_utf8_lossy(&list.stdout).contains("empty-market"),
        "gone from the registry"
    );
}
