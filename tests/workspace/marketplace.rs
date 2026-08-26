//! Workspace negatives: malformed external inputs are reported cleanly,
//! never panicked on (L2 CLI evidence, isolated env).

use uze_testkit::fixtures;
use uze_testkit::temp::TestEnvironment;

use crate::util::uze_bin;

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
