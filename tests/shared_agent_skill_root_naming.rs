//! Regression coverage for the cross-harness naming collision fixed
//! alongside `IntegrationPort::shared_agent_skill_root`: OpenCode, Codex,
//! and Gemini CLI all discover Agent Skills from the same physical
//! `~/.agents/skills` directory. Before the fix, OpenCode's own
//! `short_then_qualified` naming policy (bare name first) and Codex/Gemini's
//! always-qualified default policy each computed a name independently, so
//! installing a package attached to all three left *two* symlinks for the
//! identical skill sitting in that one shared folder — visible to OpenCode,
//! which scans the whole directory, as a duplicate `/uze` and `/uze-uze`
//! slash command.
//!
//! Deterministic by construction: a `NoopProcessRunner` means no real
//! `opencode`/`codex`/`gemini` binary is ever spawned.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use uze::integrations::{codex::CodexIntegration, gemini::GeminiIntegration, opencode::OpenCodeIntegration};
use uze::{
    PackageSource, UzeApplication, UzeHome,
    provisioning::{ProcessResult, ProcessRunner, ProcessSpec},
};

fn temp(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "uze-shared-skill-root-{label}-{}-{nonce}",
        std::process::id()
    ))
}

struct NoopProcessRunner;

impl ProcessRunner for NoopProcessRunner {
    fn run(&self, _spec: &ProcessSpec) -> uze::Result<ProcessResult> {
        Ok(ProcessResult {
            success: true,
            timed_out: false,
        })
    }
}

fn skill_fixture(root: &Path, package_id: &str, skill_name: &str) -> PathBuf {
    let dir = root.join(package_id);
    fs::create_dir_all(dir.join("skills").join(skill_name)).unwrap();
    fs::write(
        dir.join("plugin.json"),
        format!(
            r#"{{"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"{package_id}"}}"#
        ),
    )
    .unwrap();
    fs::write(
        dir.join("skills").join(skill_name).join("SKILL.md"),
        format!("---\nname: {skill_name}\ndescription: test fixture.\n---\n\nBody.\n"),
    )
    .unwrap();
    dir
}

#[test]
fn opencode_codex_and_gemini_share_exactly_one_symlink_for_the_same_skill() {
    let root = temp("three-harness");
    let agents_home = root.join("agents-home");
    let uze_home = UzeHome::at(root.join("uze-home"));

    // Codex and Gemini registered *before* OpenCode, deliberately: both use
    // the always-qualified default policy in isolation, so if attach order
    // alone decided the group's name, whichever of them resolves first
    // would lock the shared folder onto "acme-review" before OpenCode ever
    // got a chance to try its own preferred bare name. The fix must
    // converge on OpenCode's preference regardless of this order.
    let application = UzeApplication::new_with_runner(
        uze_home.clone(),
        vec![
            Box::new(CodexIntegration::new(agents_home.clone(), uze_home.clone())),
            Box::new(GeminiIntegration::new(agents_home.clone(), uze_home.clone())),
            Box::new(OpenCodeIntegration::new(
                agents_home.clone(),
                root.join("opencode-config.json"),
                uze_home.clone(),
            )),
        ],
        Box::new(NoopProcessRunner),
    );

    let package_dir = skill_fixture(&root.join("fixtures"), "acme", "review");
    application
        .add_plugin(PackageSource::local(package_dir), &uze::trust::AlwaysTrust)
        .unwrap();

    let skills_dir = agents_home.join("skills");
    let entries: Vec<String> = fs::read_dir(&skills_dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        entries,
        vec!["review".to_owned()],
        "exactly one physical entry must exist for the one skill shared by \
         opencode/codex/gemini in ~/.agents/skills, not one per harness, and \
         it must be OpenCode's preferred bare name even though Codex and \
         Gemini attach first: {entries:?}"
    );

    let inspection = application.inspect_plugin("acme").unwrap();
    assert_eq!(
        inspection.managed_state.matched, 3,
        "all three harnesses must still each have a matched receipt, even \
         though they share one physical artifact"
    );

    fs::remove_dir_all(root).unwrap();
}
