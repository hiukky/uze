//! Acceptance: an agent relaunched into its task continues its conversation.
//!
//! Walks the public path a relaunch actually takes — the shim symlink, the
//! real `uze` binary under it, the integration's own continuity contract —
//! and reads the answer off the argv the harness was launched with, never
//! off anything UZE says about itself.
//!
//! The stand-in harness records its conversation where its vendor does, so
//! the existence check a resume makes has something real to find; a fake
//! that only echoed its argv would prove the launch and not the continuity.

use std::path::{Path, PathBuf};

use uze_core::{
    UzeHome,
    checkout::CheckoutId,
    task::{self, Base, Task, TaskStore},
};
use uze_testkit::fake_harness::{Action, FakeHarness};
use uze_testkit::temp::TestEnvironment;

use crate::util::uze_bin;

const HARNESS: &str = "claude";

/// A repository with one managed task in one slot, and the slot itself.
fn managed_slot(env: &TestEnvironment) -> (PathBuf, PathBuf) {
    let primary = env.root().join("repo");
    let slot = primary.join(".worktrees").join("slot-1");
    std::fs::create_dir_all(&slot).unwrap();

    let mut recorded = Task::new(None, Base::Ref("main".into()), String::new(), "main".into());
    recorded.checkout = Some(CheckoutId::adopted("slot-1"));
    let mut store = TaskStore::default();
    store.upsert(recorded);
    task::save(&UzeHome::at(&env.uze_home), &primary, &store).unwrap();
    (primary, slot)
}

/// A stand-in that writes a transcript named by the conversation it was
/// told to start, under the directory its vendor keeps them in — the same
/// file the resume path proves the conversation by.
fn harness_recording_its_conversations(env: &TestEnvironment) -> FakeHarness {
    let transcripts = env.home.join(".claude").join("projects");
    FakeHarness::new(&env.fake_bin, HARNESS)
        .on_prefix(
            ["--session-id"],
            Action::Script(format!(
                "slug=$(printf '%s' \"$PWD\" | sed 's/[^a-zA-Z0-9]/-/g')\n\
                 mkdir -p '{root}'/\"$slug\"\n\
                 printf '{{}}\\n' > '{root}'/\"$slug\"/\"$2\".jsonl\n\
                 exit 0",
                root = transcripts.display()
            )),
        )
        .build()
}

/// The shim symlink UZE creates at setup, planted directly so the test
/// exercises the launch boundary without depending on provisioning.
fn shim(env: &TestEnvironment) -> PathBuf {
    let shims = UzeHome::at(&env.uze_home).shims_dir();
    std::fs::create_dir_all(&shims).unwrap();
    let path = shims.join(HARNESS);
    std::os::unix::fs::symlink(uze_bin(), &path).unwrap();
    path
}

/// Launches through the shim, exactly as a pane does.
fn launch(env: &TestEnvironment, shim: &Path, cwd: &Path, args: &[&str]) {
    let shims = UzeHome::at(&env.uze_home).shims_dir();
    let status = std::process::Command::new(shim)
        .args(args)
        .current_dir(cwd)
        .env("HOME", &env.home)
        .env("UZE_HOME", &env.uze_home)
        .env(
            "PATH",
            format!(
                "{}:{}:/usr/bin:/bin",
                shims.display(),
                env.fake_bin.display()
            ),
        )
        .status()
        .expect("the shim runs");
    assert!(status.success(), "the agent must start: {args:?}");
}

/// What the harness was actually launched with, one entry per launch.
fn launches(harness: &FakeHarness) -> Vec<Vec<String>> {
    harness.invocations()
}

#[cfg(unix)]
#[test]
fn a_relaunched_agent_resumes_the_conversation_its_task_was_left_in() {
    let env = TestEnvironment::isolated();
    let harness = harness_recording_its_conversations(&env);
    let shim = shim(&env);
    let (_primary, slot) = managed_slot(&env);

    launch(&env, &shim, &slot, &[]);
    launch(&env, &shim, &slot, &[]);

    let launches = launches(&harness);
    assert_eq!(launches.len(), 2, "{launches:?}");
    let [flag, started] = launches[0].as_slice() else {
        panic!("the first launch names a conversation: {launches:?}");
    };
    assert_eq!(flag, "--session-id", "{launches:?}");
    assert_eq!(
        launches[1],
        vec!["--resume".to_owned(), started.clone()],
        "the second launch continues the first: {launches:?}"
    );
}

#[cfg(unix)]
#[test]
fn an_invocation_the_operator_composed_is_launched_exactly_as_typed() {
    let env = TestEnvironment::isolated();
    let harness = harness_recording_its_conversations(&env);
    let shim = shim(&env);
    let (_primary, slot) = managed_slot(&env);

    launch(&env, &shim, &slot, &["--resume", "one-the-operator-picked"]);

    assert_eq!(
        launches(&harness),
        vec![vec![
            "--resume".to_owned(),
            "one-the-operator-picked".to_owned()
        ]]
    );
}

#[cfg(unix)]
#[test]
fn a_directory_no_task_owns_launches_the_harness_untouched() {
    let env = TestEnvironment::isolated();
    let harness = harness_recording_its_conversations(&env);
    let shim = shim(&env);
    managed_slot(&env);

    launch(&env, &shim, &env.project, &[]);

    assert_eq!(launches(&harness), vec![Vec::<String>::new()]);
}

#[cfg(unix)]
#[test]
fn the_bypass_escape_hatch_still_carries_nothing() {
    let env = TestEnvironment::isolated();
    let harness = harness_recording_its_conversations(&env);
    let shim = shim(&env);
    let (_primary, slot) = managed_slot(&env);
    let shims = UzeHome::at(&env.uze_home).shims_dir();

    let status = std::process::Command::new(&shim)
        .current_dir(&slot)
        .env("HOME", &env.home)
        .env("UZE_HOME", &env.uze_home)
        .env("UZE_BYPASS", "1")
        .env(
            "PATH",
            format!(
                "{}:{}:/usr/bin:/bin",
                shims.display(),
                env.fake_bin.display()
            ),
        )
        .status()
        .expect("the shim runs");
    assert!(status.success());

    assert_eq!(launches(&harness), vec![Vec::<String>::new()]);
}
