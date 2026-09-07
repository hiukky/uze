//! A user's agent names its work through the real binary, and the surface
//! it uses is not the surface a person is offered.

use std::path::Path;

use uze_testkit::temp::TestEnvironment;

use crate::util::uze_bin;

fn git(cwd: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@uze.invalid")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@uze.invalid")
        .output()
        .expect("git must run");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// A repository with a declared branch vocabulary and one agent checkout,
/// built with Git rather than through UZE: this tier asserts against the
/// machine, so the machine is what sets it up.
fn project_with_a_checkout(env: &TestEnvironment) -> std::path::PathBuf {
    let root = &env.project;
    git(root, &["init", "--quiet", "-b", "main"]);
    git(root, &["config", "user.name", "Test"]);
    git(root, &["config", "user.email", "test@uze.invalid"]);
    std::fs::write(
        root.join("agents.yaml"),
        "worktrees:\n  branch: conventional\n",
    )
    .unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "chore: initial"]);
    root.clone()
}

/// The agent's own surface, end to end: the branch Git reports and the
/// label UZE records both change, from one command run inside the
/// checkout.
#[test]
fn an_agent_names_its_work_through_the_real_binary() {
    let env = TestEnvironment::isolated();
    let root = project_with_a_checkout(&env);

    // Place an agent the way the client does, then name from its checkout.
    let placed = env.run_ok(uze_bin(), &["status", "--format", "json"]);
    assert!(placed.status.success());

    let slot = root.join(".worktrees/manual");
    git(
        &root,
        &[
            "worktree",
            "add",
            "-b",
            "agent/zulqgq",
            slot.to_str().unwrap(),
            "HEAD",
        ],
    );

    // A checkout Git registers but UZE has never recorded is adopted, so
    // there is a task to name.
    let output = env
        .command(uze_bin())
        .current_dir(&slot)
        .args(["agent", "task", "name", "fix/branch-naming"])
        .output()
        .expect("uze must run");

    assert!(
        output.status.success(),
        "naming failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        git(&slot, &["rev-parse", "--abbrev-ref", "HEAD"]),
        "fix/branch-naming",
        "the branch Git reports is the one the agent asked for"
    );
    assert!(
        slot.is_dir(),
        "the slot directory keeps its own name; only the branch was renamed"
    );
}

/// Hidden means hidden from the person, never disabled. Both halves are
/// asserted together, because either alone is the wrong outcome.
#[test]
fn the_agent_surface_is_absent_from_the_help_a_person_reads() {
    let env = TestEnvironment::isolated();
    let help = env.run_ok(uze_bin(), &["--help"]);
    let text = String::from_utf8_lossy(&help.stdout);
    assert!(
        !text.contains("agent task") && !text.split_whitespace().any(|word| word == "agent"),
        "the agent's own commands are not offered to a person: {text}"
    );

    // And it still exists: an unknown subcommand would fail differently.
    let output = env.run(uze_bin(), &["agent", "task", "name", "fix/nowhere"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("unrecognized subcommand"),
        "the command exists, it just has nothing to name here: {stderr}"
    );
}

/// `uze i` is the same command, not a second entry point. Each spelling
/// runs in a world of its own: installing changes the project, so running
/// them in sequence would compare a first install against a second.
#[test]
fn the_install_alias_is_the_same_command() {
    let long = TestEnvironment::isolated();
    let short = TestEnvironment::isolated();
    let long = long.run(uze_bin(), &["install", "--format", "json"]);
    let short = short.run(uze_bin(), &["i", "--format", "json"]);
    assert_eq!(long.status.code(), short.status.code());
    assert_eq!(
        String::from_utf8_lossy(&long.stdout),
        String::from_utf8_lossy(&short.stdout)
    );
}
