//! Isolated test environments and real-home safety.
//!
//! [`TestEnvironment`] is the single abstraction for any test that touches
//! the filesystem or process environment. Constructing one guarantees the
//! test runs against disposable directories under the system temp dir, never
//! under the developer's real home, and never under the real `~/.uze`.
//!
//! Process-global mutation is *not* the default: tests that spawn the real
//! `uze` binary should use [`TestEnvironment::command`], which scopes
//! `HOME`/`UZE_HOME`/`PATH`/cwd to the child process only. Only tests that
//! must mutate the *current* process (in-process integration calls that read
//! the ambient `PATH`) use [`TestEnvironment::apply`], which serializes on
//! the crate-wide process-env lock and restores everything on drop.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::env::ProcessEnvGuard;

/// Serializes scratch-dir creation: two tests in the same binary sharing a
/// label must not interleave `create_dir_all` (the nonce is per-call, but
/// the filesystem ordering would still be racy without this).
static CREATE_LOCK: Mutex<()> = Mutex::new(());

fn nonce() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX_EPOCH")
        .as_nanos()
}

/// Subdirectories under the real `$HOME` that a test must never write to.
pub const REAL_HOME_SUBDIRS: &[&str] = &[
    ".uze",
    ".claude",
    ".agents",
    ".codex",
    ".gemini",
    ".config/opencode",
];

/// The real `$HOME` as captured at first use, before any test mutates the
/// process env. Used by [`assert_not_real_home`] even after a
/// [`TestEnvironment::apply`] has overwritten `HOME`.
fn real_home() -> Option<PathBuf> {
    static REAL_HOME: OnceLock<Option<PathBuf>> = OnceLock::new();
    REAL_HOME
        .get_or_init(|| std::env::var_os("HOME").map(PathBuf::from))
        .clone()
}

/// Panics if `path` could reach the developer's real home or a real harness
/// config root — by writing into it, or by being an ancestor of it (an
/// ancestor would let `remove_dir_all` delete real state).
///
/// Every temp root constructed by this module already satisfies this; the
/// guard exists for future builders that take a caller-supplied root, and as
/// an explicit assertion tests can repeat on suspect paths.
pub fn assert_not_real_home(path: &Path) {
    let mut protected: Vec<PathBuf> = Vec::new();
    if let Some(home) = real_home() {
        for sub in REAL_HOME_SUBDIRS {
            protected.push(home.join(sub));
        }
        if let Some(uze_home) = std::env::var_os("UZE_HOME").map(PathBuf::from) {
            protected.push(uze_home);
        }
        if let Some(config) = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from) {
            protected.push(config.join("opencode"));
        }
    }
    if let Some(project_dir) = std::env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from) {
        protected.push(project_dir);
    }

    for protected in protected {
        if path.starts_with(&protected) || protected.starts_with(path) {
            panic!(
                "TestEnvironment safety guard: {} must never overlap the real home/config \
                 location {}",
                path.display(),
                protected.display()
            );
        }
    }
}

/// A directory that exists only for the lifetime of the test and is removed
/// on drop. Use [`TempDir::keep`] to retain it across a panic for manual
/// inspection (set `UZE_TEST_KEEP=1` to keep everything when chasing a
/// failure).
pub struct TempDir {
    path: PathBuf,
    keep: bool,
}

impl TempDir {
    /// Creates a fresh, empty directory under the system temp dir.
    pub fn new(label: &str) -> Self {
        let _guard = CREATE_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let keep = std::env::var_os("UZE_TEST_KEEP").is_some();
        let path = std::env::temp_dir().join(format!(
            "uze-tests-{label}-{}-{}",
            std::process::id(),
            nonce()
        ));
        std::fs::create_dir_all(&path).unwrap_or_else(|error| {
            panic!(
                "TestEnvironment: failed to create scratch dir {}: {error}",
                path.display()
            )
        });
        assert_not_real_home(&path);
        TempDir { path, keep }
    }

    /// Builds a `TempDir` for a caller-supplied path (still under the system
    /// temp dir) after running the real-home safety guard.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        assert_not_real_home(&path);
        std::fs::create_dir_all(&path).unwrap_or_else(|error| {
            panic!(
                "TestEnvironment: failed to create scratch dir {}: {error}",
                path.display()
            )
        });
        TempDir {
            path,
            keep: std::env::var_os("UZE_TEST_KEEP").is_some(),
        }
    }

    /// The root path of this temp directory.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Joins `rel` onto the root path.
    pub fn join(&self, rel: impl AsRef<Path>) -> PathBuf {
        self.path.join(rel)
    }

    /// Keeps the directory (and its contents) after the guard drops.
    pub fn keep(mut self) -> TempDir {
        self.keep = true;
        self
    }
}

impl AsRef<Path> for TempDir {
    fn as_ref(&self) -> &Path {
        self.path()
    }
}

impl std::fmt::Debug for TempDir {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TempDir")
            .field("path", &self.path)
            .field("keep", &self.keep)
            .finish()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

/// Per-harness config homes kept inside the environment root, so
/// `HOME`-scoped vendor state never leaks between tests.
#[derive(Debug, Clone)]
pub struct HarnessHomes {
    pub claude: PathBuf,
    pub codex: PathBuf,
    pub opencode: PathBuf,
    pub antigravity: PathBuf,
}

impl HarnessHomes {
    fn new(root: &Path) -> Self {
        HarnessHomes {
            claude: root.join("home-claude"),
            codex: root.join("home-codex"),
            opencode: root.join("home-opencode"),
            antigravity: root.join("home-antigravity"),
        }
    }
}

/// Creates a fresh scratch directory and returns its path (the caller keeps
/// ownership of cleanup, as migrated tests have always done). Centralizes
/// nonce generation, creation locking and the real-home guard; prefer
/// [`TempDir`] (RAII) for new tests.
pub fn scratch(label: &str) -> PathBuf {
    TempDir::new(label).path().to_path_buf()
}

/// `$PATH` with `dir` first and the ambient `PATH` kept as fallback — the
/// shape tests used to hand-roll (`format!("{}:{}", fake_bin.display(),
/// env::var("PATH")...)`), without the panic on a missing ambient PATH and
/// without relying on `:` being the platform separator.
pub fn path_prefixed(dir: impl AsRef<Path>) -> OsString {
    let dir = dir.as_ref();
    let mut parts = vec![dir.to_path_buf()];
    parts.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    std::env::join_paths(parts)
        .unwrap_or_else(|error| panic!("TestEnvironment: could not join PATH: {error}"))
}

/// Joins `dirs` in order (with no ambient fallback) into a `$PATH` value —
/// for tests that need exact control over what a child/in-process resolver
/// can see.
pub fn join_paths(dirs: &[&Path]) -> OsString {
    let parts: Vec<_> = dirs
        .iter()
        .map(|dir| dir.as_os_str().to_os_string())
        .collect();
    std::env::join_paths(parts)
        .unwrap_or_else(|error| panic!("TestEnvironment: could not join PATH: {error}"))
}

/// A fully isolated test environment:
///
/// ```text
/// <root>/
/// ├── home/          <- $HOME
/// ├── uze/           <- $UZE_HOME
/// ├── bin/           <- fake binaries live here (PATH head)
/// ├── project/       <- default cwd / project root
/// └── home-<harness> <- per-harness config homes
/// ```
pub struct TestEnvironment {
    root: TempDir,
    /// `$HOME` for the test (never the developer's).
    pub home: PathBuf,
    /// `$UZE_HOME` for the test (never the real `~/.uze`).
    pub uze_home: PathBuf,
    /// Directory holding generated fake executables; prepended to `PATH`.
    pub fake_bin: PathBuf,
    /// The default project root (also the default cwd).
    pub project: PathBuf,
    /// Per-harness config roots, all inside the environment root.
    pub harness_homes: HarnessHomes,
}

impl TestEnvironment {
    /// Creates a clean, empty environment rooted at a fresh temp dir.
    pub fn isolated() -> Self {
        let root = TempDir::new("env");
        let home = root.join("home");
        let uze_home = root.join("uze");
        let fake_bin = root.join("bin");
        let project = root.join("project");
        for dir in [&home, &uze_home, &fake_bin, &project] {
            std::fs::create_dir_all(dir).unwrap_or_else(|error| {
                panic!(
                    "TestEnvironment: failed to create {}: {error}",
                    dir.display()
                )
            });
        }
        let harness_homes = HarnessHomes::new(root.path());
        TestEnvironment {
            root,
            home,
            uze_home,
            fake_bin,
            project,
            harness_homes,
        }
    }

    /// Root that owns every path of this environment.
    pub fn root(&self) -> &Path {
        self.root.path()
    }

    /// Creates (`project/<rel>/...`) and returns a nested project directory,
    /// for cwd-depth resolution scenarios.
    pub fn nested_project(&self, rel: impl AsRef<Path>) -> PathBuf {
        let path = self.project.join(rel);
        std::fs::create_dir_all(&path).unwrap_or_else(|error| {
            panic!(
                "TestEnvironment: failed to create nested project {}: {error}",
                path.display()
            )
        });
        path
    }

    /// A `Command` for `program` with HOME/UZE_HOME/PATH/cwd scoped to this
    /// environment only — the process env of the test binary is untouched.
    pub fn command(&self, program: impl AsRef<std::ffi::OsStr>) -> Command {
        let mut command = Command::new(program);
        command
            .env("HOME", &self.home)
            .env("UZE_HOME", &self.uze_home)
            .env("PATH", self.scoped_path())
            .current_dir(&self.project);
        command
    }

    /// `$PATH` for child processes: the fake bin first, then the ambient
    /// `PATH`. Never drops the ambient path entirely, so the harness's own
    /// helper tools remain resolvable; tests that must see *no* real binary
    /// use [`TestEnvironment::command_no_real_path`] instead.
    pub fn scoped_path(&self) -> OsString {
        let mut parts = vec![self.fake_bin.clone()];
        parts.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        std::env::join_paths(parts)
            .unwrap_or_else(|error| panic!("TestEnvironment: could not join PATH: {error}"))
    }

    /// Like [`TestEnvironment::command`] but with `PATH` reduced to the fake
    /// bin: no real harness binary can be resolved.
    pub fn command_no_real_path(&self, program: impl AsRef<std::ffi::OsStr>) -> Command {
        let mut command = Command::new(program);
        command
            .env("HOME", &self.home)
            .env("UZE_HOME", &self.uze_home)
            .env("PATH", &self.fake_bin)
            .current_dir(&self.project);
        command
    }

    /// Runs `program` with this environment applied and returns the raw
    /// output (no assertions).
    pub fn run(&self, program: &Path, args: &[&str]) -> std::process::Output {
        self.command(program)
            .args(args)
            .output()
            .unwrap_or_else(|error| {
                panic!(
                    "TestEnvironment: failed to run {} {args:?}: {error}",
                    program.display()
                )
            })
    }

    /// Like [`TestEnvironment::run`] but panics with captured stderr when the
    /// command exits non-zero — for the common "must succeed" shape.
    pub fn run_ok(&self, program: &Path, args: &[&str]) -> std::process::Output {
        let output = self.run(program, args);
        if !output.status.success() {
            panic!(
                "TestEnvironment: expected {} {args:?} to succeed, got {:?}\nstdout: {}\nstderr: {}",
                program.display(),
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        output
    }

    /// Applies HOME/UZE_HOME/PATH/cwd to *this* process, serialized on the
    /// crate-wide process-env lock and fully restored on drop. Only for
    /// tests that exercise code reading the ambient process env; everything
    /// that can go through a child process should use [`TestEnvironment::command`].
    pub fn apply(&self) -> ProcessEnvGuard<'_> {
        let mut scope = crate::env::scope();
        scope
            .set("HOME", &self.home)
            .set("UZE_HOME", &self.uze_home)
            .set("PATH", self.scoped_path())
            .set_cwd(&self.project);
        scope
    }

    /// Asserts this environment's roots never overlap protected locations;
    /// also a convenient re-check point after a test mutated paths.
    pub fn assert_safe(&self) {
        assert_not_real_home(self.root());
        assert_not_real_home(&self.home);
        assert_not_real_home(&self.uze_home);
        assert_not_real_home(&self.project);
        assert_not_real_home(&self.fake_bin);
    }
}
