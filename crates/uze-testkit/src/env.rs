//! Serialized, restoring mutation of process-global environment state.
//!
//! Rust integration tests run in parallel threads inside one test binary.
//! `HOME`, `PATH`, the cwd and every other process env var are shared, so
//! any test that mutates them must (a) serialize against every other such
//! test in the binary and (b) restore the previous values even on panic.
//!
//! [`scope`] provides both: it takes the crate-wide lock for the duration of
//! the guard and restores all changed variables (in reverse order) on drop.
//! Prefer child-process scoping ([`crate::temp::TestEnvironment::command`])
//! whenever the code under test can run in a subprocess; this module is the
//! last resort for in-process code that reads the ambient env.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

/// Serializes every process-env mutation in the current test binary.
pub static PROCESS_ENV_LOCK: Mutex<()> = Mutex::new(());

type PreviousCwd = Option<PathBuf>;

struct Mutated<'a> {
    key: &'a str,
    previous: Option<OsString>,
}

/// A scoped, RAII mutation of process-global environment state.
pub struct ProcessEnvGuard<'a> {
    _lock: MutexGuard<'static, ()>,
    mutated: Vec<Mutated<'a>>,
    previous_cwd: PreviousCwd,
    cwd_mutated: bool,
}

impl<'a> ProcessEnvGuard<'a> {
    /// Sets `key` to `value`, remembering the previous value (or absence).
    pub fn set(&mut self, key: &'a str, value: impl AsRef<std::ffi::OsStr>) -> &mut Self {
        let previous = std::env::var_os(key);
        // SAFETY: serialized by PROCESS_ENV_LOCK against every other env
        // mutation in this test binary; restored by this guard's Drop.
        unsafe { std::env::set_var(key, value.as_ref()) };
        self.mutated.push(Mutated { key, previous });
        self
    }

    /// Removes `key`, remembering the previous value so it can be restored.
    pub fn remove(&mut self, key: &'a str) -> &mut Self {
        let previous = std::env::var_os(key);
        // SAFETY: same reasoning as `set`.
        unsafe { std::env::remove_var(key) };
        self.mutated.push(Mutated { key, previous });
        self
    }

    /// Changes the process cwd, restoring the previous one on drop.
    pub fn set_cwd(&mut self, dir: impl AsRef<std::path::Path>) -> &mut Self {
        let previous = std::env::current_dir().ok();
        // SAFETY: no other thread may change the cwd while the lock is held;
        // `current_dir` is process-global like the env, so the same lock
        // applies.
        std::env::set_current_dir(dir)
            .unwrap_or_else(|error| panic!("env::scope: failed to set cwd: {error}"));
        self.previous_cwd = previous;
        self.cwd_mutated = true;
        self
    }
}

impl Drop for ProcessEnvGuard<'_> {
    fn drop(&mut self) {
        for mutated in self.mutated.drain(..).rev() {
            // SAFETY: the lock is still held (dropped after this block), so
            // no concurrent mutation can be racing this restore.
            match mutated.previous {
                Some(value) => unsafe { std::env::set_var(mutated.key, value) },
                None => unsafe { std::env::remove_var(mutated.key) },
            }
        }
        if self.cwd_mutated
            && let Some(previous) = self.previous_cwd.take()
        {
            // SAFETY: same lock reasoning as above.
            std::env::set_current_dir(previous).ok();
        }
    }
}

/// Acquires the process-env lock and returns a scoped mutation guard.
pub fn scope() -> ProcessEnvGuard<'static> {
    let lock = PROCESS_ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    ProcessEnvGuard {
        _lock: lock,
        mutated: Vec::new(),
        previous_cwd: None,
        cwd_mutated: false,
    }
}

/// Runs `f` with `key` temporarily set to `value`, serialized against every
/// other env mutation in the binary and restored afterwards.
pub fn with_env_var<R>(
    key: &'static str,
    value: impl AsRef<std::ffi::OsStr>,
    f: impl FnOnce() -> R,
) -> R {
    let mut scope = scope();
    scope.set(key, value);
    f()
}

/// Runs `f` with the process cwd temporarily set to `dir`, serialized and
/// restored.
pub fn with_cwd<R>(dir: impl AsRef<std::path::Path>, f: impl FnOnce() -> R) -> R {
    let mut scope = scope();
    scope.set_cwd(dir);
    f()
}
