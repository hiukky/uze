//! Shared acceptance-suite helpers.

use std::path::{Path, PathBuf};

use uze_test_support::temp::TestEnvironment;

/// The real UZE binary built for this test run (public path only).
pub(crate) fn uze_bin() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_uze"))
}

/// Writes a small canonical plugin directory with `name` and one Skill per
/// (`skill`, `body`) pair under `<root>/<package_id>`.
pub(crate) fn make_skill_package(
    root: &Path,
    package_id: &str,
    skills: &[(&str, &str)],
) -> PathBuf {
    let dir = root.join(package_id);
    std::fs::create_dir_all(&dir).expect("acceptance: package dir must be creatable");
    std::fs::write(
        dir.join("plugin.json"),
        format!(
            r#"{{"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"{package_id}"}}"#
        ),
    )
    .expect("acceptance: plugin.json must be writable");
    for (name, body) in skills {
        let skill_dir = dir.join("skills").join(name);
        std::fs::create_dir_all(&skill_dir).expect("acceptance: skill dir must be creatable");
        std::fs::write(skill_dir.join("SKILL.md"), body)
            .expect("acceptance: SKILL.md must be writable");
    }
    dir
}

/// Default SKILL.md body (model+user).
pub(crate) fn default_body(name: &str) -> String {
    format!("---\nname: {name}\ndescription: acceptance fixture.\n---\n\nBody.\n")
}

/// User-only SKILL.md body.
pub(crate) fn user_only_body(name: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: acceptance fixture.\ninvoke:\n  model: false\n  user: true\n---\n\nBody.\n"
    )
}

/// Model-only SKILL.md body.
pub(crate) fn model_only_body(name: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: acceptance fixture.\ninvoke:\n  model: true\n  user: false\n---\n\nBody.\n"
    )
}

/// A canonical marketplace `agents.json` with one local `plugins/<name>`
/// entry pointing at `./plugins/<name>`.
pub(crate) fn agents_json(name: &str, plugin: &str) -> String {
    format!(
        r#"{{"name":"{name}","description":"acceptance marketplace","plugins":[{{"name":"{plugin}","source":"./plugins/{plugin}"}}]}}"#
    )
}

/// Deterministic harness CLIs for the acceptance suite: Claude and Codex
/// run the full marketplace state machine (so attach and inspection agree),
/// Antigravity stages byte copies, OpenCode only needs detection answers.
/// `recorded` receives the fake harnesses for invocation-log assertions.
pub(crate) fn install_fake_harnesses(
    env: &TestEnvironment,
) -> Vec<uze_test_support::fake_harness::FakeHarness> {
    use uze_test_support::fake_harness::{Action, FakeHarness, MarketplaceVendor};

    let claude_state = env.root().join("fake-state/claude");
    let codex_state = env.root().join("fake-state/codex");
    let agy_state = env.root().join("fake-state/agy");

    vec![
        FakeHarness::new(&env.fake_bin, "claude")
            .version_line("9.9.9 (Fake Claude)")
            .on_prefix(
                ["plugin"],
                Action::VendorMarketplace {
                    state_dir: claude_state,
                    vendor: MarketplaceVendor::Claude,
                },
            )
            .build(),
        FakeHarness::new(&env.fake_bin, "codex")
            .version_line("codex-cli 9.9.9")
            .on_prefix(
                ["plugin"],
                Action::VendorMarketplace {
                    state_dir: codex_state,
                    vendor: MarketplaceVendor::Codex,
                },
            )
            .build(),
        FakeHarness::new(&env.fake_bin, "opencode")
            .version_line("opencode 9.9.9")
            .build(),
        FakeHarness::new(&env.fake_bin, "agy")
            .version_line("agy 9.9.9")
            .on_prefix(
                ["plugin"],
                Action::VendorAgy {
                    state_dir: agy_state,
                    dest: env.home.join(".gemini/config/plugins"),
                },
            )
            .build(),
    ]
}
