//! One transport for speaking to the Git binary.
//!
//! Not a domain: nothing here knows what a worktree, a checkout, or a diff
//! view is.
//!
//! # Why a crate rather than a module in `uze-core`
//!
//! Not because Git is peripheral — it is essential — but because of which
//! way the dependencies run. Three crates need it, and two of them cannot
//! depend on the domain: `uze-extensions` is forbidden to by an enforced
//! rule (an extension never names the domain crate), and `uze-testkit`
//! would form a cycle, since `uze-core` dev-depends on it. A leaf with no
//! dependencies of its own is the only position all three can share. What it owns is the part every caller was reinventing — how the
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
//! [`write`] takes the repository write lock; [`read`] never does. The
//! lock lives here and nowhere else because a lock is worthless if a
//! second module can spawn Git around it, and every call site has already
//! declared which side it is on — so a status view never blocks behind a
//! rebase.
//!
//! # The write lock
//!
//! Linked worktrees share one `.git`: `packed-refs`, branch creation,
//! `worktree add`, `prune`, `fetch`, `rebase` and `merge` are not safe
//! against each other, and the operator may have another UZE or a bare
//! `git` running. The lock is therefore inter-process — a `flock` on
//! `<common dir>/uze-write.lock`, keyed on the repository's common
//! directory so a write from inside any worktree of a repository contends
//! with every other — and it is reentrant on the thread that holds it, so
//! [`locked`] can make several writes one critical section. The kernel
//! releases a `flock` when its holder dies, so a lock left by a crashed
//! process costs nothing to reclaim. A directory that is not a repository
//! has no common directory and no lock, which is how `git init` runs.

use std::{
    io,
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};

mod lock;

/// How long a write waits for the lock before giving up. Long enough for a
/// rebase or a fetch in another process, short enough that a hung holder
/// is reported rather than waited on forever.
pub const DEFAULT_WRITE_TIMEOUT: Duration = Duration::from_secs(60);

/// Git could not be run at all. A Git that ran and disagreed with the
/// caller is an [`Output`], not this.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpawnError(pub(crate) String);

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

/// Runs a Git command that changes the repository, under the repository
/// write lock, waiting up to [`DEFAULT_WRITE_TIMEOUT`] for it.
pub fn write(root: &Path, args: &[&str]) -> Result<Output, SpawnError> {
    write_within(root, args, DEFAULT_WRITE_TIMEOUT)
}

/// [`write`] with an explicit bound on how long to wait for the lock. A
/// wait that runs out is a [`SpawnError`] naming the lock, since Git never
/// ran.
pub fn write_within(root: &Path, args: &[&str], timeout: Duration) -> Result<Output, SpawnError> {
    let _held = lock::acquire(root, timeout)?;
    run(base_command(root, args))
}

/// Runs `body` with the repository write lock held throughout, so the
/// writes it makes — through [`write`], which re-enters the lock on this
/// thread — form one critical section: a prune, a name check and a
/// `worktree add` that no other process can interleave with.
pub fn locked<R>(
    root: &Path,
    timeout: Duration,
    body: impl FnOnce() -> R,
) -> Result<R, SpawnError> {
    let _held = lock::acquire(root, timeout)?;
    Ok(body())
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
    use std::{path::PathBuf, time::Instant};

    /// Every test here spawns Git, which resolves through the process-global
    /// `PATH` — including the one test that empties it. Reading ambient env
    /// is exactly what `env::scope` serializes, so a test that only reads it
    /// must still take the lock or it races the one that writes.
    /// Hand-rolled rather than `uze_testkit::git::Repository`: that fixture
    /// is built on this crate, so using it here would test the transport
    /// through itself.
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

    /// Lost updates are the observable failure a missing lock produces:
    /// every thread reads a counter, pauses, and writes it back, and only
    /// mutual exclusion makes the total come out right.
    #[test]
    fn concurrent_critical_sections_never_interleave() {
        let _environment = uze_testkit::env::scope();
        let root = repository("git-lock-sections");
        let counter = root.join("counter");
        std::fs::write(&counter, b"0").unwrap();
        std::thread::scope(|scope| {
            for _ in 0..6 {
                scope.spawn(|| {
                    for _ in 0..5 {
                        locked(&root, DEFAULT_WRITE_TIMEOUT, || {
                            let value: u32 = std::fs::read_to_string(&counter)
                                .unwrap()
                                .trim()
                                .parse()
                                .unwrap();
                            std::thread::sleep(Duration::from_millis(1));
                            std::fs::write(&counter, (value + 1).to_string()).unwrap();
                        })
                        .unwrap();
                    }
                });
            }
        });
        assert_eq!(std::fs::read_to_string(&counter).unwrap().trim(), "30");
        std::fs::remove_dir_all(root).unwrap();
    }

    /// The write that motivated the lock: several worktrees created at
    /// once contend on the shared refs, and each must still succeed and be
    /// registered.
    #[test]
    fn concurrent_worktree_adds_do_not_collide() {
        let _environment = uze_testkit::env::scope();
        let root = repository("git-lock-worktrees");
        let names: Vec<String> = (0..6).map(|index| format!("slot-{index}")).collect();
        std::thread::scope(|scope| {
            for name in &names {
                let root = &root;
                scope.spawn(move || {
                    let path = root.join(".worktrees").join(name);
                    write(
                        root,
                        &[
                            "worktree",
                            "add",
                            "-b",
                            name,
                            path.to_str().unwrap(),
                            "HEAD",
                        ],
                    )
                    .unwrap()
                    .successful()
                    .unwrap_or_else(|error| panic!("{name}: {error}"));
                });
            }
        });
        let listed = read(&root, &["worktree", "list", "--porcelain"])
            .unwrap()
            .successful()
            .unwrap();
        for name in &names {
            assert!(
                listed.contains(&format!("branch refs/heads/{name}")),
                "{listed}"
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    /// A write inside a critical section must not deadlock against the
    /// section that contains it.
    #[test]
    fn a_write_inside_a_critical_section_re_enters_the_lock() {
        let _environment = uze_testkit::env::scope();
        let root = repository("git-lock-reentry");
        locked(&root, Duration::from_millis(500), || {
            write(&root, &["branch", "inner"])
                .unwrap()
                .successful()
                .unwrap();
            locked(&root, Duration::from_millis(500), || {
                write(&root, &["branch", "innermost"])
                    .unwrap()
                    .successful()
                    .unwrap();
            })
            .unwrap();
        })
        .unwrap();
        let branches = read(&root, &["branch", "--list"])
            .unwrap()
            .successful()
            .unwrap();
        assert!(
            branches.contains("inner") && branches.contains("innermost"),
            "{branches}"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_panic_inside_the_critical_section_releases_the_lock() {
        let _environment = uze_testkit::env::scope();
        let root = repository("git-lock-panic");
        let outcome = std::panic::catch_unwind(|| {
            let _ = locked(&root, DEFAULT_WRITE_TIMEOUT, || panic!("inside"));
        });
        assert!(outcome.is_err());
        locked(&root, Duration::from_millis(500), || ()).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_busy_lock_is_reported_by_name_after_the_timeout_and_reads_never_wait() {
        let _environment = uze_testkit::env::scope();
        let root = repository("git-lock-busy");
        let root_for_holder = root.clone();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let (held_tx, held_rx) = std::sync::mpsc::channel::<()>();
        std::thread::scope(|scope| {
            scope.spawn(move || {
                locked(&root_for_holder, DEFAULT_WRITE_TIMEOUT, || {
                    held_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                })
                .unwrap();
            });
            held_rx.recv().unwrap();

            let started = Instant::now();
            assert!(
                read(&root, &["status", "--porcelain"])
                    .unwrap()
                    .is_success(),
                "a read runs while the lock is held"
            );
            assert!(started.elapsed() < Duration::from_millis(500));

            let error = write_within(&root, &["branch", "blocked"], Duration::from_millis(150))
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("write lock") && error.contains("busy"),
                "{error}"
            );
            release_tx.send(()).unwrap();
        });
        write(&root, &["branch", "after"])
            .unwrap()
            .successful()
            .unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    /// Holds the lock until killed; the process side of the test below.
    /// Ignored so it never runs on its own.
    #[test]
    #[ignore]
    fn hold_the_lock_until_killed() {
        let Some(root) = std::env::var_os("UZE_GIT_LOCK_HOLD") else {
            return;
        };
        let root = PathBuf::from(root);
        locked(&root, DEFAULT_WRITE_TIMEOUT, || {
            std::fs::write(root.join("held"), b"").unwrap();
            std::thread::sleep(Duration::from_secs(120));
        })
        .unwrap();
    }

    #[test]
    fn a_lock_held_by_a_dead_process_is_reclaimed() {
        let _environment = uze_testkit::env::scope();
        let root = repository("git-lock-dead-holder");
        let mut holder = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "tests::hold_the_lock_until_killed", "--ignored"])
            .env("UZE_GIT_LOCK_HOLD", &root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let started = Instant::now();
        while !root.join("held").exists() {
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "holder never took the lock"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            write_within(&root, &["branch", "blocked"], Duration::from_millis(150)).is_err(),
            "the lock is held across processes"
        );
        holder.kill().unwrap();
        holder.wait().unwrap();
        write_within(&root, &["branch", "reclaimed"], Duration::from_secs(5))
            .unwrap()
            .successful()
            .unwrap();
        std::fs::remove_dir_all(root).unwrap();
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
