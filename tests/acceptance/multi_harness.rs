//! Acceptance A4/A5: multi-harness projection and invocation policy,
//! driven through the public CLI with deterministic fake harness CLI
//! binaries (no real vendor binary, no model call).

use uze_testkit::assertions;
use uze_testkit::fixtures;
use uze_testkit::temp::TestEnvironment;

use crate::util::{
    default_body, install_fake_harnesses, make_skill_package, model_only_body, user_only_body,
    uze_bin,
};

/// A4 — one canonical plugin reaches every harness through its most native
/// safe representation, with no duplicate delivery.
#[test]
fn one_plugin_reaches_every_harness_with_no_duplicate_delivery() {
    let env = TestEnvironment::isolated();
    let harnesses = install_fake_harnesses(&env);

    // One `setup` provisions every detected harness (and records setup
    // state so codex/opencode prefer UZE-managed attachment); running it
    // twice must stay idempotent (regression for the double-attach found
    // by this suite).
    env.run_ok(uze_bin(), &["setup"]);
    let fixture = fixtures::canonical("skill-plugin");
    let (market_args, install_args) =
        uze_testkit::marketplace::marketplace_install_args(&env.home, &fixture);
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
    let delivery = report["package_plans"]
        .as_array()
        .expect("package_plans array");
    assert!(
        !delivery.is_empty(),
        "install must report package plans, got: {report}"
    );

    // Claude: generated native package envelope under UZE_HOME state.
    let claude_envelope = env.uze_home.join(
        "state/attachments/claude/generated/uze-agent-skill-conformance/.claude-plugin/plugin.json",
    );
    assertions::assert_file(&claude_envelope, "claude generated envelope");

    // Codex/OpenCode: one shared `.agents/skills` wrapper preserving the
    // canonical body while publishing the stable qualified label.
    let codex_entry = env
        .home
        .join(".agents/skills/uze-agent-skill-conformance:uze-e2e");
    assert!(
        codex_entry.is_symlink(),
        "codex/opencode shared skill entry must be a symlink"
    );
    let shared_target = std::fs::read_link(&codex_entry).expect("read shared skill target");
    let wrapper =
        std::fs::read_to_string(shared_target.join("SKILL.md")).expect("read shared skill wrapper");
    assert!(
        wrapper.starts_with("---\nname: uze-agent-skill-conformance:uze-e2e\n"),
        "the shared wrapper preserves the qualified skill label: {wrapper}"
    );

    let ledger = std::fs::read(env.uze_home.join("state/attachments.json")).unwrap();
    let ledger: serde_json::Value = serde_json::from_slice(&ledger).unwrap();
    let receipts = ledger["receipts"].as_object().unwrap();
    let for_package: Vec<_> = receipts
        .values()
        .filter(|receipt| receipt["package_id"] == "uze-agent-skill-conformance")
        .collect();
    // One package-level (or one capability-level) receipt per integration —
    // never both, never two of the same kind. The exact count differs by
    // harness; the invariant is: no integration appears more than once.
    let mut integrations: Vec<&str> = for_package
        .iter()
        .map(|receipt| receipt["integration"].as_str().unwrap_or("?"))
        .collect();
    integrations.sort();
    integrations.dedup();
    assert!(
        for_package.len() == integrations.len(),
        "no duplicate capability receipt may exist for a package-covered resource, got \
         {for_package:?}"
    );
    let _ = harnesses;
}

/// A5 — invocation policy: normal/user-only/model-only Skills project with
/// the correct per-harness classification through the public CLI.
#[test]
fn invocation_policy_projects_per_harness_classification() {
    let env = TestEnvironment::isolated();
    install_fake_harnesses(&env);
    env.run_ok(uze_bin(), &["setup"]);

    let policy_package = make_skill_package(
        env.root(),
        "policy-fixture",
        &[
            ("commit", &default_body("commit")),
            ("review", &user_only_body("review")),
        ],
    );
    let (market_args, install_args) =
        uze_testkit::marketplace::marketplace_install_args(&env.home, &policy_package);
    env.run_ok(
        uze_bin(),
        &market_args.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    env.run_ok(
        uze_bin(),
        &install_args.iter().map(String::as_str).collect::<Vec<_>>(),
    );

    // Inspect reports all three skills with their policy classification.
    let inspect = env.run_ok(
        uze_bin(),
        &["plugin", "inspect", "policy-fixture", "--format", "json"],
    );
    let report: serde_json::Value = serde_json::from_slice(&inspect.stdout).expect("json report");
    let names: Vec<&str> = report["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|cap| cap["name"].as_str())
        .collect();
    for expected in ["commit", "review"] {
        assert!(
            names.contains(&expected),
            "inspect must list {expected}, got {names:?}"
        );
    }

    // Physical projection: the default skill is exposed through the shared
    // `.agents/skills` symlink (codex/opencode), the user-only skill is
    // carried by its policy sidecar + slash surface, never a bare symlink
    // that would make it model-visible.
    let shared_root = env.home.join(".agents/skills");
    assert!(
        shared_root.join("policy-fixture:commit").is_symlink(),
        "default skill must be projected for codex/opencode"
    );
    // The user-only skill goes through a policy wrapper: its shared-root
    // entry is a symlink into UZE-owned attachments (never the Store
    // bytes), and Codex's generated envelope carries its own
    // `agents/openai.yaml` with implicit invocation disabled — the model
    // never sees it as auto-discoverable.
    let review = shared_root.join("policy-fixture:review");
    assert!(
        review.is_symlink(),
        "user-only skill must be projected via its policy wrapper"
    );
    let review_target = std::fs::read_link(&review).expect("readlink");
    assert!(
        review_target.join("SKILL.md").is_file(),
        "the wrapper must carry the skill bytes, got {review_target:?}"
    );
    let codex_policy = env
        .uze_home
        .join("state/attachments/codex/generated/policy-fixture/skills/review/agents/openai.yaml");
    assert!(
        codex_policy.is_file(),
        "codex user-only delivery must carry the policy sidecar"
    );
    let sidecar = std::fs::read_to_string(&codex_policy).unwrap();
    assert!(
        sidecar.contains("allow_implicit_invocation: false"),
        "the sidecar must disable implicit (model) invocation, got: {sidecar}"
    );
}

/// A11 — shared-root superset: a model-only Skill on the Codex+OpenCode
/// pair (Codex claims nothing in its envelope, OpenCode needs `slash:
/// false` on the same entry) must install cleanly through the CLI, with the
/// single shared entry carrying OpenCode's encoding — reusing the entry is
/// only safe because the superset wrapper never silently drops a policy.
/// Codex still reports its own user=false limitation honestly (Degraded);
/// this is a physical representation fix, not a policy rewrite.
#[test]
fn superset_shared_entry_keeps_both_integrations_preserved() {
    let env = TestEnvironment::isolated();
    install_fake_harnesses(&env);
    env.run_ok(uze_bin(), &["setup"]);

    let conflict_package = make_skill_package(
        env.root(),
        "conflict-fixture",
        &[("audit", &model_only_body("audit"))],
    );
    let (market_args, install_args) =
        uze_testkit::marketplace::marketplace_install_args(&env.home, &conflict_package);
    env.run_ok(
        uze_bin(),
        &market_args.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    env.run_ok(
        uze_bin(),
        &install_args.iter().map(String::as_str).collect::<Vec<_>>(),
    );

    let shared_entry = env.home.join(".agents/skills/conflict-fixture:audit");
    assert!(
        shared_entry.is_symlink(),
        "one shared physical entry for the model-only Skill"
    );
    let target = std::fs::read_link(&shared_entry).expect("readlink");
    let wrapper = std::fs::read_to_string(target.join("SKILL.md")).unwrap();
    assert!(
        wrapper.contains("slash: false"),
        "the shared entry carries OpenCode's user-invocation suppression: {wrapper}"
    );
    assert!(
        !wrapper.contains("opencode/autoinvoke"),
        "model discovery stays enabled for a model-only Skill: {wrapper}"
    );
    assert!(
        !target.join("agents/openai.yaml").exists(),
        "no Codex policy sidecar for a model=true Skill"
    );
}
