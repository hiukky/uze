//! A real Git repository a test owns entirely.
//!
//! Tests that drive real `git` used to inherit the developer's and CI's
//! *global* configuration: `commit.gpgsign` blocking on a GPG agent that has
//! no key in a sandbox, `init.defaultBranch` renaming the branch an assertion
//! names, plus whatever hooks and aliases happen to be installed. Every one of
//! those is a failure whose cause is outside the repository under test.
//!
//! [`Repository`] points `GIT_CONFIG_GLOBAL` and `GIT_CONFIG_SYSTEM` at an
//! empty file for its lifetime, so the only configuration that applies is the
//! one the fixture writes itself.
//!
//! That pointing is process-global on purpose: the code under test spawns its
//! own `git`, and a child process only inherits what *this* process exports.
//! A `Repository` therefore holds the crate-wide process-env lock
//! ([`crate::env::scope`]) until it is dropped — a test must not take that
//! lock separately, and must not hold two repositories at once.

use std::path::{Path, PathBuf};

use crate::env::ProcessEnvGuard;

/// The branch every fixture repository starts on. Named explicitly at `init`
/// rather than left to `init.defaultBranch`, which the empty configuration no
/// longer supplies.
pub const INITIAL_BRANCH: &str = "main";

const AUTHOR_NAME: &str = "UZE Test";
const AUTHOR_EMAIL: &str = "test@uze.invalid";

/// A scratch Git repository, isolated from all ambient Git configuration.
pub struct Repository {
    root: PathBuf,
    /// Held, never read: dropping it restores the ambient Git configuration
    /// and releases the process-env lock.
    _environment: ProcessEnvGuard<'static>,
}

impl Repository {
    /// An initialized repository with no commits — the unborn-`HEAD` state.
    pub fn empty(label: &str) -> Self {
        let base = crate::temp::scratch(label);
        // Outside the repository, or the fixture's own configuration would
        // show up in every `git status` the test asserts on.
        let configuration = base.join("gitconfig");
        std::fs::write(&configuration, b"").expect("could not write the isolated git config");

        let mut environment = crate::env::scope();
        environment.set("GIT_CONFIG_GLOBAL", &configuration);
        environment.set("GIT_CONFIG_SYSTEM", &configuration);

        let root = base.join("repository");
        std::fs::create_dir_all(&root).expect("could not create the fixture repository");
        let repository = Self {
            root,
            _environment: environment,
        };
        repository.git(&["init", "--quiet", "-b", INITIAL_BRANCH, "."]);
        repository.git(&["config", "user.name", AUTHOR_NAME]);
        repository.git(&["config", "user.email", AUTHOR_EMAIL]);
        repository
    }

    /// A repository with one initial commit — the ordinary starting point,
    /// since almost everything Git does needs a `HEAD` to resolve.
    pub fn new(label: &str) -> Self {
        let repository = Self::empty(label);
        repository.commit_file("README.md", &format!("# {label}\n"));
        repository
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Writes `relative`, stages it, and commits it. Returns the new `HEAD`.
    pub fn commit_file(&self, relative: &str, contents: &str) -> String {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("could not create the file's parent");
        }
        std::fs::write(&path, contents).expect("could not write the file to commit");
        self.git(&["add", "--", relative]);
        self.git(&["commit", "--quiet", "-m", relative]);
        self.head()
    }

    /// The commit `HEAD` resolves to.
    pub fn head(&self) -> String {
        self.git(&["rev-parse", "HEAD"])
    }

    /// The branch checked out in the repository itself.
    pub fn branch(&self) -> String {
        self.branch_of(&self.root)
    }

    /// The branch checked out in `checkout` — a linked worktree of this
    /// repository, usually.
    pub fn branch_of(&self, checkout: &Path) -> String {
        self.git_in(checkout, &["rev-parse", "--abbrev-ref", "HEAD"])
    }

    /// Runs `git` in the repository, returning its trimmed stdout and
    /// panicking on failure.
    pub fn git(&self, args: &[&str]) -> String {
        self.git_in(&self.root, args)
    }

    /// Runs `git` in `checkout` under the same isolated configuration.
    pub fn git_in(&self, checkout: &Path, args: &[&str]) -> String {
        self.try_git_in(checkout, args)
            .unwrap_or_else(|error| panic!("git {args:?} failed: {error}"))
    }

    /// The fallible form, for tests that assert on what `git` refuses.
    ///
    /// Through `uze-git` like everything else: a fixture that spawned Git
    /// its own way would be a second convention over the same binary,
    /// which is exactly what that crate exists to prevent.
    pub fn try_git_in(&self, checkout: &Path, args: &[&str]) -> Result<String, String> {
        uze_git::write(checkout, args)
            .map_err(|error| format!("git must be on PATH for this test: {error}"))?
            .successful()
            .map(|stdout| stdout.trim().to_owned())
    }
}

/// Runs `git` in `checkout` with every ambient configuration neutralized,
/// without touching the process environment.
///
/// The per-invocation form of what [`Repository`] does. `Repository` has to
/// export `GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM` *for the process*, because
/// the code under test spawns its own `git` and a child inherits only what
/// this process exports — which costs a crate-wide lock and allows one
/// fixture at a time. A fixture that only drives `git` itself needs neither:
/// the same variables are passed per command, so a test may build as many
/// repositories as it likes and each is still isolated from the operator's
/// `commit.gpgsign` (which blocks on a GPG agent holding no key), their
/// hooks, their aliases and their `init.defaultBranch`.
pub fn isolated_git_in(checkout: &Path, args: &[&str]) -> String {
    uze_git::write_with_env(
        checkout,
        args,
        &[
            // An empty configuration file that exists on every platform this
            // suite runs on, so neither the operator's nor the runner's
            // settings reach the fixture.
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_SYSTEM", "/dev/null"),
            ("GIT_AUTHOR_NAME", AUTHOR_NAME),
            ("GIT_AUTHOR_EMAIL", AUTHOR_EMAIL),
            ("GIT_COMMITTER_NAME", AUTHOR_NAME),
            ("GIT_COMMITTER_EMAIL", AUTHOR_EMAIL),
        ],
    )
    .unwrap_or_else(|error| panic!("git must be on PATH for this fixture: {error}"))
    .successful()
    .unwrap_or_else(|error| panic!("git {args:?} failed: {error}"))
    .trim()
    .to_owned()
}

/// Turns an existing directory into a repository with one commit holding
/// everything in it, and answers with that commit.
///
/// Deliberately not a [`Repository`], for the reason
/// [`isolated_git_in`] gives: a test that needs a project repository *and* a
/// marketplace repository needs two at once, which the process-global form
/// cannot give it.
pub fn commit_everything_in(root: &Path) -> String {
    let isolated = |args: &[&str]| -> String { isolated_git_in(root, args) };
    let already = uze_git::read(root, &["rev-parse", "--git-dir"])
        .map(|output| output.is_success())
        .unwrap_or(false);
    if !already {
        isolated(&["init", "--quiet", "-b", INITIAL_BRANCH, "."]);
        isolated(&["config", "user.name", AUTHOR_NAME]);
        isolated(&["config", "user.email", AUTHOR_EMAIL]);
    }
    isolated(&["add", "-A"]);
    // Idempotent: a fixture may commit, write more, and commit again, and
    // a second call with nothing new to record is not a failure.
    let unchanged = uze_git::read(root, &["diff", "--cached", "--quiet"])
        .map(|output| output.is_success())
        .unwrap_or(false);
    if !unchanged {
        isolated(&["commit", "--quiet", "-m", "fixture"]);
    }
    isolated(&["rev-parse", "HEAD"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_repository_starts_on_the_named_branch_with_one_commit() {
        let repository = Repository::new("testkit-git");
        assert_eq!(repository.branch(), INITIAL_BRANCH);
        assert_eq!(repository.git(&["rev-list", "--count", "HEAD"]), "1");
        assert!(repository.git(&["status", "--short"]).is_empty());
    }

    /// The reason this fixture exists: whatever the developer or CI has in a
    /// global config must not reach the repository under test.
    #[test]
    fn no_ambient_configuration_reaches_the_repository() {
        let repository = Repository::new("testkit-git-isolated");
        assert!(
            repository
                .try_git_in(repository.root(), &["config", "--global", "--list"])
                .unwrap()
                .is_empty()
        );
        assert_eq!(repository.git(&["config", "user.email"]), AUTHOR_EMAIL);
        assert_eq!(
            repository.git(&["config", "--default", "false", "commit.gpgsign"]),
            "false",
            "an inherited signing setting is what hangs on an agent with no key"
        );
    }

    #[test]
    fn an_empty_repository_has_no_head_to_resolve() {
        let repository = Repository::empty("testkit-git-unborn");
        assert!(
            repository
                .try_git_in(repository.root(), &["rev-parse", "HEAD"])
                .is_err()
        );
    }

    #[test]
    fn committing_a_file_moves_head() {
        let repository = Repository::new("testkit-git-commit");
        let before = repository.head();
        let after = repository.commit_file("nested/file.txt", "contents\n");
        assert_ne!(before, after);
        assert_eq!(
            std::fs::read_to_string(repository.root().join("nested/file.txt")).unwrap(),
            "contents\n"
        );
    }
}
