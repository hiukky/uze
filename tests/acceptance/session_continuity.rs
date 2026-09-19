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
//!
//! A launch UZE composes carries the agent's identity in the environment
//! (see `uze_terminal::launch`), and the shim resumes by that claim, never
//! by the directory: a launch with no identity, or one whose identity is
//! owned by an enclosing launch, is an ordinary invocation.

use std::path::{Path, PathBuf};

use uze_core::{
    UzeHome,
    checkout::CheckoutId,
    task::{self, Agent, Base, TaskStore},
};
use uze_testkit::fake_harness::{Action, FakeHarness};
use uze_testkit::temp::TestEnvironment;

use crate::util::uze_bin;

const HARNESS: &str = "claude";
/// The integration the harness belongs to, which is what a conversation is
/// recorded under.
const INTEGRATION: &str = "claude-code";

/// A repository with one managed task in one slot: the primary, the slot
/// itself, and the task's identifier — what a launch there claims.
fn managed_slot(env: &TestEnvironment) -> (PathBuf, PathBuf, String) {
    let primary = env.root().join("repo");
    let slot = primary.join(".worktrees").join("slot-1");
    std::fs::create_dir_all(&slot).unwrap();

    let mut recorded = Agent::isolated(
        "claude",
        None,
        Base::Ref("main".into()),
        String::new(),
        "main".into(),
    );
    recorded
        .isolation_mut()
        .expect("an isolated agent carries its isolation")
        .checkout = Some(CheckoutId::adopted("slot-1"));
    let id = recorded.id.as_str().to_owned();
    let mut store = TaskStore::default();
    store.upsert(recorded);
    task::save(&UzeHome::at(&env.uze_home), &primary, &store).unwrap();
    (primary, slot, id)
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

/// How a launch reaches the shim: composed by UZE with the agent's
/// identity stamped, typed by a person with nothing stamped, or started
/// from inside another agent's launch, whose identity it inherits but
/// does not own.
enum Launch<'a> {
    Composed { identity: &'a str },
    ByHand,
    Nested { identity: &'a str },
}

/// Launches through the shim, exactly as a pane does.
fn launch(env: &TestEnvironment, shim: &Path, cwd: &Path, args: &[&str], launch: Launch<'_>) {
    let shims = UzeHome::at(&env.uze_home).shims_dir();
    let mut command = std::process::Command::new(shim);
    command
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
        );
    // What the server does for every pane it spawns: nothing of the launch
    // this test suite itself runs under reaches the shim — on a dogfooding
    // machine the suite is routinely run from inside a shimmed agent.
    for stamped in uze_terminal::launch::STAMPED_VARIABLES {
        command.env_remove(stamped);
    }
    match launch {
        Launch::Composed { identity } => {
            command.env(uze_terminal::launch::AGENT_IDENTITY_VARIABLE, identity);
        }
        Launch::ByHand => {}
        // An identity already owned by another pid — the enclosing launch's
        // shim stamped that pid at its own `exec`.
        Launch::Nested { identity } => {
            command
                .env(uze_terminal::launch::AGENT_IDENTITY_VARIABLE, identity)
                .env(uze_terminal::launch::SHIM_PID_VARIABLE, "1");
        }
    }
    let status = command.status().expect("the shim runs");
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
    let (_primary, slot, id) = managed_slot(&env);

    launch(&env, &shim, &slot, &[], Launch::Composed { identity: &id });
    launch(&env, &shim, &slot, &[], Launch::Composed { identity: &id });

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
    let (_primary, slot, id) = managed_slot(&env);

    launch(
        &env,
        &shim,
        &slot,
        &["--resume", "one-the-operator-picked"],
        Launch::Composed { identity: &id },
    );

    assert_eq!(
        launches(&harness),
        vec![vec![
            "--resume".to_owned(),
            "one-the-operator-picked".to_owned()
        ]]
    );
}

/// A launch that carries no identity is the operator's own, wherever it
/// is made — a managed task's checkout included. Continuity is for the
/// launches UZE composes, and the directory was never what said so.
#[cfg(unix)]
#[test]
fn a_launch_carrying_no_identity_is_launched_untouched_even_inside_a_slot() {
    let env = TestEnvironment::isolated();
    let harness = harness_recording_its_conversations(&env);
    let shim = shim(&env);
    let (_primary, slot, _id) = managed_slot(&env);

    launch(&env, &shim, &slot, &[], Launch::ByHand);
    launch(&env, &shim, &env.project, &[], Launch::ByHand);

    assert_eq!(
        launches(&harness),
        vec![Vec::<String>::new(), Vec::<String>::new()]
    );
}

/// An identity the record contradicts is no identity: claimed from the
/// operator's checkout rather than the task's own slot, it carries nothing
/// over, so a process cannot reach a task by naming it from elsewhere.
#[cfg(unix)]
#[test]
fn an_identity_claimed_from_the_wrong_directory_is_launched_untouched() {
    let env = TestEnvironment::isolated();
    let harness = harness_recording_its_conversations(&env);
    let shim = shim(&env);
    let (primary, _slot, id) = managed_slot(&env);

    launch(
        &env,
        &shim,
        &primary,
        &[],
        Launch::Composed { identity: &id },
    );
    launch(
        &env,
        &shim,
        &env.project,
        &[],
        Launch::Composed {
            identity: "nobody-recorded-this",
        },
    );

    assert_eq!(
        launches(&harness),
        vec![Vec::<String>::new(), Vec::<String>::new()]
    );
}

/// A harness started from inside an agent's launch inherits the identity
/// but does not own it: it is an ordinary invocation, and the enclosing
/// agent's conversation is untouched — two processes never share one
/// resume.
#[cfg(unix)]
#[test]
fn a_launch_nested_inside_an_agents_launch_is_ordinary() {
    let env = TestEnvironment::isolated();
    let harness = harness_recording_its_conversations(&env);
    let shim = shim(&env);
    let (primary, slot, id) = managed_slot(&env);

    launch(&env, &shim, &slot, &[], Launch::Composed { identity: &id });
    let started = launches(&harness)[0][1].clone();
    launch(&env, &shim, &slot, &[], Launch::Nested { identity: &id });

    let launches = launches(&harness);
    assert_eq!(launches.len(), 2, "{launches:?}");
    assert_eq!(
        launches[1],
        Vec::<String>::new(),
        "the nested launch is ordinary"
    );
    let store = task::load(&UzeHome::at(&env.uze_home), &primary).unwrap();
    let record =
        uze_core::conversation::load(&UzeHome::at(&env.uze_home), &primary, &store.agents[0].id);
    assert_eq!(
        record
            .get(INTEGRATION)
            .and_then(|entry| entry.conversation.as_ref())
            .map(|session| session.as_str().to_owned()),
        Some(started),
        "the enclosing agent's conversation is exactly what its own launch recorded"
    );
}

#[cfg(unix)]
#[test]
fn the_bypass_escape_hatch_still_carries_nothing() {
    let env = TestEnvironment::isolated();
    let harness = harness_recording_its_conversations(&env);
    let shim = shim(&env);
    let (_primary, slot, id) = managed_slot(&env);
    let shims = UzeHome::at(&env.uze_home).shims_dir();

    let status = std::process::Command::new(&shim)
        .current_dir(&slot)
        .env("HOME", &env.home)
        .env("UZE_HOME", &env.uze_home)
        .env_remove(uze_terminal::launch::SHIM_PID_VARIABLE)
        .env_remove(uze_terminal::launch::SHIM_NAME_VARIABLE)
        .env(uze_terminal::launch::AGENT_IDENTITY_VARIABLE, &id)
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
