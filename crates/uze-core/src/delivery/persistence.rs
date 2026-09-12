//! Core small, local-only durability primitives for UZE-owned state.
//!
//! They intentionally do not attempt distributed transactions with vendor
//! CLIs. A confirmed external side effect is recorded immediately by the
//! caller, while these helpers keep registry and ledger replacement atomic.

use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{Result, UzeError, home::UzeHome};

pub fn write_atomic(path: &Path, payload: &[u8]) -> Result<()> {
    let _span =
        tracing::debug_span!("persistence.write", path = %path.display(), bytes = payload.len())
            .entered();
    let parent = path.parent().expect("UZE state paths have a parent");
    fs::create_dir_all(parent).map_err(|source| UzeError::Write {
        path: parent.to_path_buf(),
        source,
    })?;
    let temporary = temporary_path(path, parent);
    // Opened outside the fallible block on purpose: the cleanup below
    // removes `temporary`, and this call may only remove a file it created
    // itself.
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|source| UzeError::Write {
            path: temporary.clone(),
            source,
        })?;
    let result = (|| {
        file.write_all(payload).map_err(|source| UzeError::Write {
            path: temporary.clone(),
            source,
        })?;
        file.sync_all().map_err(|source| UzeError::Write {
            path: temporary.clone(),
            source,
        })?;
        fs::rename(&temporary, path).map_err(|source| UzeError::Write {
            path: path.to_path_buf(),
            source,
        })?;
        sync_directory(parent);
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// A name no other `write_atomic` can be using, for the file `path` is
/// published from by rename.
///
/// The clock alone does not separate two writes of one process — a TUI
/// refresh, a shim launch and a second attached session project the same
/// file concurrently, and the nanosecond they read can be the same one — so
/// a process-wide sequence separates them. Two callers on one temporary name
/// is not a near-miss: the loser of `create_new` deletes the file the winner
/// is about to rename, and the winner fails with a bare `No such file or
/// directory`.
fn temporary_path(path: &Path, parent: &Path) -> PathBuf {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_nanos();
    parent.join(format!(
        ".{}.{}.{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("state"),
        std::process::id(),
        nonce,
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ))
}

#[cfg(unix)]
fn sync_directory(path: &Path) {
    if let Ok(directory) = File::open(path) {
        let _ = directory.sync_all();
    }
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) {}

/// Process-wide mutation guard for one UZE home.
///
/// The lock is a file's existence, taken atomically by `create_new`, and the
/// pid written inside it is what makes the lock recoverable: `Drop` does not
/// run on `^C`, on `SIGKILL`, or on an abort, and nothing in UZE installs a
/// signal handler to make it. A lock file outliving its process is therefore
/// the ordinary outcome of an interrupted `uze install`, not an exotic one —
/// and a guard that then blocks every mutating command forever, with no
/// stated way out, is a dead end rather than caution. So a *live* holder
/// still blocks, named by pid; a pid nobody owns is debris and is reclaimed.
pub struct MutationLock {
    path: PathBuf,
}

impl MutationLock {
    pub fn acquire(home: &UzeHome) -> Result<Self> {
        home.ensure_layout()?;
        let path = home.state_dir().join("mutation.lock");
        if let Some(lock) = Self::claim(&path)? {
            return Ok(lock);
        }
        if let Some(pid) = settled_pid(&path)
            && process_is_alive(pid)
        {
            return Err(UzeError::MutationInProgress {
                path,
                pid: Some(pid),
            });
        }
        fs::remove_file(&path).map_err(|source| UzeError::Write {
            path: path.clone(),
            source,
        })?;
        // Losing this second claim means a live process took the lock in
        // between, which is the answer the caller was asking for anyway.
        Self::claim(&path)?.ok_or_else(|| UzeError::MutationInProgress {
            pid: recorded_pid(&path),
            path,
        })
    }

    fn claim(path: &Path) -> Result<Option<Self>> {
        match OpenOptions::new().create_new(true).write(true).open(path) {
            Ok(mut file) => {
                // A lock whose pid never landed would be read as debris by
                // the next `acquire` while this process still holds it, so
                // a failed write gives the lock back rather than keeping a
                // claim nobody else can see the owner of. No fsync: the
                // question the pid answers is whether a *process* is alive,
                // and no process outlives the page cache.
                if let Err(source) = writeln!(file, "pid={}", std::process::id()) {
                    let _ = fs::remove_file(path);
                    return Err(UzeError::Write {
                        path: path.to_path_buf(),
                        source,
                    });
                }
                Ok(Some(Self {
                    path: path.to_path_buf(),
                }))
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(None),
            Err(source) => Err(UzeError::Write {
                path: path.to_path_buf(),
                source,
            }),
        }
    }
}

fn recorded_pid(path: &Path) -> Option<u32> {
    fs::read_to_string(path)
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("pid=")?.trim().parse().ok())
}

/// The holder's pid, waiting out the gap between `create_new` and the write
/// that fills the file.
///
/// That gap is microseconds wide, but reading inside it would show an empty
/// file and a live holder would be reclaimed out from under itself. The
/// window below is the only thing standing between the two, and it is only
/// ever paid when a lock is already contended.
fn settled_pid(path: &Path) -> Option<u32> {
    const SETTLE: Duration = Duration::from_millis(100);
    const POLL: Duration = Duration::from_millis(5);

    let deadline = std::time::Instant::now() + SETTLE;
    loop {
        if let Some(pid) = recorded_pid(path) {
            return Some(pid);
        }
        if std::time::Instant::now() >= deadline {
            return None;
        }
        thread::sleep(POLL);
    }
}

/// Whether a process with this pid exists — signal `0` performs the
/// existence and permission checks and delivers nothing.
///
/// `EPERM` counts as alive: the process is there, it just is not ours. A pid
/// can be recycled, and a recycled one reads as alive, which keeps the guard
/// on the conservative side of its own rule.
#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    // `kill(0, …)` signals our own group and `kill(-1, …)` everything we
    // own; no lock holder has such a pid, and signal `0` would answer for
    // the wrong subject.
    if pid <= 1 {
        return true;
    }
    // SAFETY: `kill` with signal 0 only queries; no memory is involved.
    let outcome = unsafe { libc::kill(pid, 0) };
    outcome == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// Without a way to ask, a lock is never judged stale — the old behaviour.
#[cfg(not(unix))]
fn process_is_alive(_pid: u32) -> bool {
    true
}

impl Drop for MutationLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_blocks_a_concurrent_mutation_attempt() {
        let root = uze_testkit::temp::scratch("lock");
        let home = UzeHome::at(&root);
        let first = MutationLock::acquire(&home).unwrap();
        // This process is the holder and is very much alive, so the lock
        // holds and names who holds it.
        assert!(matches!(
            MutationLock::acquire(&home),
            Err(UzeError::MutationInProgress { pid: Some(pid), .. }) if pid == std::process::id()
        ));
        drop(first);
        MutationLock::acquire(&home).unwrap();
        let _ = fs::remove_dir_all(root);
    }

    /// `^C` and `SIGKILL` both skip `Drop`, and nothing installs a signal
    /// handler to change that. A lock left behind by a process that no
    /// longer exists must not be the end of mutating this home forever.
    #[cfg(unix)]
    #[test]
    fn a_lock_left_by_a_killed_process_is_reclaimed() {
        let root = uze_testkit::temp::scratch("lock-stale");
        let home = UzeHome::at(&root);
        home.ensure_layout().unwrap();
        let path = home.state_dir().join("mutation.lock");

        // A real holder in a real process: it takes the lock the same way
        // `claim` does, then is killed without ever running a destructor.
        let mut child = std::process::Command::new("/bin/sh")
            .args([
                "-c",
                "echo pid=$$ > \"$1\"; sleep 30",
                "sh",
                &path.to_string_lossy(),
            ])
            .spawn()
            .unwrap();
        while recorded_pid(&path).is_none() {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(matches!(
            MutationLock::acquire(&home),
            Err(UzeError::MutationInProgress { .. })
        ));
        child.kill().unwrap();
        child.wait().unwrap();

        let reclaimed = MutationLock::acquire(&home).expect("a dead holder's lock is reclaimed");
        assert_eq!(
            recorded_pid(&path),
            Some(std::process::id()),
            "the reclaimed lock still names the process that died"
        );
        drop(reclaimed);
        assert!(!path.exists());
        let _ = fs::remove_dir_all(root);
    }

    /// A lock file that never received its pid is debris too — the writer
    /// died between creating it and filling it — but only after the window
    /// a live holder needs to write one.
    #[cfg(unix)]
    #[test]
    fn a_lock_with_no_recorded_holder_is_reclaimed() {
        let root = uze_testkit::temp::scratch("lock-headless");
        let home = UzeHome::at(&root);
        home.ensure_layout().unwrap();
        fs::write(home.state_dir().join("mutation.lock"), b"").unwrap();

        MutationLock::acquire(&home).expect("a lock naming nobody is reclaimed");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn write_atomic_creates_parent_and_is_idempotent() {
        let root = uze_testkit::temp::scratch("write-atomic");
        let path = root.join("a/b/state.json");
        write_atomic(&path, b"first").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"first");
        // No temp files left behind.
        assert!(
            !fs::read_dir(root.join("a/b")).unwrap().any(|e| e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp"))
        );

        write_atomic(&path, b"second").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"second");
        let _ = fs::remove_dir_all(root);
    }

    /// A temporary name must carry a discriminator the clock does not
    /// supply. Two callers reading the same nanosecond is the whole failure
    /// — one deletes the file the other is about to rename — and it is a
    /// race no test can schedule on demand, so what is asserted here is the
    /// part that makes it impossible: consecutive names differ in a field
    /// that is not the timestamp.
    #[test]
    fn every_temporary_is_a_name_of_its_own() {
        let root = uze_testkit::temp::scratch("write-atomic-naming");
        let path = root.join("state.json");

        // `.state.json.<pid>.<nanos>.<sequence>.tmp`
        let sequence_of = |name: &Path| {
            name.file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_suffix(".tmp"))
                .and_then(|name| name.rsplit('.').next())
                .expect("a temporary name ends in `.<sequence>.tmp`")
                .to_owned()
        };
        let first = temporary_path(&path, &root);
        let second = temporary_path(&path, &root);

        assert_ne!(
            sequence_of(&first),
            sequence_of(&second),
            "two temporaries told apart by the clock alone: {first:?} vs {second:?}"
        );
        let _ = fs::remove_dir_all(root);
    }

    /// Two writers of one file are a normal moment, not an edge case: a
    /// TUI refresh, a shim launch and a second attached session all project
    /// the same `CLAUDE.md`. Each must end with the whole payload of *some*
    /// writer and an error from none — the failure this guards is a writer
    /// deleting the temporary of another and losing the rename to a bare
    /// `No such file or directory`.
    #[test]
    fn concurrent_writers_of_one_path_all_succeed() {
        let root = uze_testkit::temp::scratch("write-atomic-concurrent");
        let path = root.join("state.json");
        let payloads: Vec<Vec<u8>> = (0..8).map(|writer| vec![b'a' + writer; 4096]).collect();

        std::thread::scope(|scope| {
            for payload in &payloads {
                let path = &path;
                scope.spawn(move || {
                    write_atomic(path, payload).expect("a racing write must not fail")
                });
            }
        });

        // Whole, never a mix of two writers: `rename` is what publishes.
        let written = fs::read(&path).unwrap();
        assert!(
            payloads.contains(&written),
            "the published file must be one writer's payload in full"
        );
        assert!(
            !fs::read_dir(&root).unwrap().any(|entry| entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".tmp")),
            "no temporary may outlive its writer"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn mutation_lock_is_released_on_drop_and_allows_reacquire() {
        let root = uze_testkit::temp::scratch("lock-drop");
        let home = UzeHome::at(&root);
        {
            let _guard = MutationLock::acquire(&home).unwrap();
            assert!(home.state_dir().join("mutation.lock").exists());
        }
        // Dropped — file must be gone and a new acquire must succeed.
        assert!(!home.state_dir().join("mutation.lock").exists());
        MutationLock::acquire(&home).unwrap();
        let _ = fs::remove_dir_all(root);
    }
}
