//! One transport for speaking to the Git binary.
//!
//! Not a domain: nothing here knows what a worktree, a checkout, or a diff
//! view is. What it owns is the part every caller was reinventing — how the
//! process is spawned, what environment it inherits, and what a non-zero
//! exit means.
//!
//! # Why a non-zero exit is not an error
//!
//! Two callers grew two incompatible conventions. `worktree` treated any
//! non-zero exit as failure; the diff view treated `1` as success, because
//! `git diff` uses it for "there are differences". Both were right for
//! their own command and wrong for the other's, and a third caller would
//! have had to guess again — `git rebase` exits 1 on a conflict, which is a
//! state, and `git rev-parse --verify --quiet` exits 1 for "no such ref",
//! which is an answer.
//!
//! So this layer does not decide. It reports what Git said and lets the
//! caller — which knows the subcommand it asked for — classify it.
//! [`Output::successful`] is there for the common case where non-zero
//! really is a failure.
//!
//! # Reads and writes are separate on purpose
//!
//! [`read`] and [`write`] do the same thing today. They are two entry
//! points because a repository-wide write lock is coming, and a lock is
//! worthless if a second module can spawn Git around it. Putting the
//! asymmetry in the signature now means the day the lock arrives, every
//! call site has already declared which side it is on — and a status view
//! never ends up blocking behind a rebase.

use std::{
    io,
    path::Path,
    process::{Command, Stdio},
};

/// Git could not be run at all. A Git that ran and disagreed with the
/// caller is an [`Output`], not this.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpawnError(String);

impl std::fmt::Display for SpawnError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for SpawnError {}

/// What Git said.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Output {
    /// `None` when the process was terminated by a signal, which is never
    /// an answer to anything.
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn is_success(&self) -> bool {
        self.code == Some(0)
    }

    /// Stdout when Git exited zero, trimmed stderr otherwise — the shape a
    /// caller wants when non-zero genuinely means failure for the
    /// subcommand it ran.
    pub fn successful(self) -> Result<String, String> {
        if self.is_success() {
            Ok(self.stdout)
        } else {
            Err(self.stderr.trim().to_owned())
        }
    }

    /// Stdout when Git exited with `code` or zero. For a subcommand whose
    /// non-zero exit is an answer rather than a failure — `diff` reporting
    /// differences, `rev-parse --verify --quiet` reporting a missing ref.
    pub fn or_exit(self, code: i32) -> Result<String, String> {
        if self.is_success() || self.code == Some(code) {
            Ok(self.stdout)
        } else {
            Err(self.stderr.trim().to_owned())
        }
    }
}

/// Runs a Git command that only observes. Never takes the repository write
/// lock, and asks Git not to take its own optional index lock either, so a
/// status view cannot block behind — or interfere with — a write in
/// another checkout of the same repository.
pub fn read(root: &Path, args: &[&str]) -> Result<Output, SpawnError> {
    let mut command = base_command(root, args);
    command.env("GIT_OPTIONAL_LOCKS", "0");
    run(command)
}

/// Runs a Git command that changes the repository. Separate from [`read`]
/// so the write lock has one place to live when it arrives.
pub fn write(root: &Path, args: &[&str]) -> Result<Output, SpawnError> {
    run(base_command(root, args))
}

fn base_command(root: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command.arg("-C").arg(root).args(args);
    // A subprocess that stops to ask for a credential never gets an
    // answer: nothing here is attached to a terminal the operator can see.
    command.env("GIT_TERMINAL_PROMPT", "0");
    command.stdin(Stdio::null());
    command
}

fn run(mut command: Command) -> Result<Output, SpawnError> {
    let output = command.output().map_err(describe_spawn_failure)?;
    Ok(Output {
        code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn describe_spawn_failure(error: io::Error) -> SpawnError {
    SpawnError(if error.kind() == io::ErrorKind::NotFound {
        "git is not installed or not on PATH".to_owned()
    } else {
        format!("could not run git: {error}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Every test here spawns Git, which resolves through the process-global
    /// `PATH` — including the one test that empties it. Reading ambient env
    /// is exactly what `env::scope` serializes, so a test that only reads it
    /// must still take the lock or it races the one that writes.
    fn repository(label: &str) -> PathBuf {
        let root = uze_testkit::temp::scratch(label);
        for args in [
            vec!["init", "-q", "-b", "main", "."],
            vec!["config", "user.email", "t@example.invalid"],
            vec!["config", "user.name", "t"],
        ] {
            write(&root, &args).unwrap().successful().unwrap();
        }
        std::fs::write(root.join("file"), b"seed").unwrap();
        write(&root, &["add", "."]).unwrap().successful().unwrap();
        write(&root, &["commit", "-qm", "seed"])
            .unwrap()
            .successful()
            .unwrap();
        root
    }

    /// The whole reason this layer exists: the same exit code means
    /// failure for one subcommand and an answer for another, so the
    /// transport reports it instead of deciding.
    #[test]
    fn a_non_zero_exit_is_reported_not_flattened_into_an_error() {
        let _environment = uze_testkit::env::scope();
        let root = repository("git-exit-codes");
        std::fs::write(root.join("file"), b"changed").unwrap();

        let diff = read(&root, &["diff", "--quiet"]).unwrap();
        assert_eq!(diff.code, Some(1), "differences, not a failure");
        assert!(diff.clone().successful().is_err());
        assert!(diff.or_exit(1).is_ok());

        let missing = read(
            &root,
            &["rev-parse", "--verify", "--quiet", "refs/heads/nope"],
        )
        .unwrap();
        assert_eq!(missing.code, Some(1));
        assert!(
            !missing.is_success(),
            "an absent ref must stay distinguishable from a present one"
        );
        assert!(
            read(
                &root,
                &["rev-parse", "--verify", "--quiet", "refs/heads/main"]
            )
            .unwrap()
            .is_success()
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stdout_and_stderr_are_both_kept() {
        let _environment = uze_testkit::env::scope();
        let root = repository("git-streams");
        let output = read(&root, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap();
        assert_eq!(output.stdout.trim(), "main");
        assert!(output.stderr.is_empty());

        let failed = read(&root, &["cat-file", "-p", "definitely-not-an-object"]).unwrap();
        assert!(!failed.is_success());
        assert!(
            !failed.clone().successful().unwrap_err().is_empty(),
            "a failure keeps Git's own words: {failed:?}"
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_missing_git_is_named_rather_than_reported_as_an_io_error() {
        let mut environment = uze_testkit::env::scope();
        environment.set("PATH", uze_testkit::temp::scratch("git-absent"));
        let error = read(Path::new("."), &["status"]).unwrap_err();
        assert!(error.to_string().contains("not installed"), "{error}");
    }

    #[test]
    fn a_directory_outside_a_repository_answers_rather_than_failing_to_spawn() {
        let _environment = uze_testkit::env::scope();
        let root = uze_testkit::temp::scratch("git-norepo");
        let output = read(&root, &["rev-parse", "--show-toplevel"]).unwrap();
        assert!(!output.is_success());
        std::fs::remove_dir_all(root).unwrap();
    }
}
