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
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
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
/// The lock is an `flock` on a permanent file, exactly as
/// [`crate::project::task::locked`] holds its own document: the file always
/// exists and is never unlinked, and what is held is the kernel's advisory
/// lock on this process's open descriptor. That is what makes it both
/// atomic and self-releasing — `Drop` does not run on `^C`, on `SIGKILL` or
/// on an abort, and nothing in UZE installs a signal handler to make it, but
/// the kernel closes every descriptor of a process that dies however it
/// dies. An interrupted `uze install` therefore leaves no debris to reclaim,
/// and there is no window in which two acquirers can both judge a holder
/// dead and both take the lock.
///
/// The holder's pid is still written into the file, for one reason only: to
/// name who is blocking in the error message. Nothing decides anything by
/// it, which also keeps the guard out of the PID-namespace hole a
/// bind-mounted `$UZE_HOME` in a container opens, and out of PID reuse.
pub struct MutationLock {
    /// Closing this is what releases the `flock`.
    _file: File,
}

impl MutationLock {
    pub fn acquire(home: &UzeHome) -> Result<Self> {
        home.ensure_layout()?;
        let path = home.state_dir().join("mutation.lock");
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|source| UzeError::Write {
                path: path.clone(),
                source,
            })?;
        match try_lock_exclusive_briefly(&file) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                // Read rather than trusted: the pid is display only, and a
                // holder that has not written its own yet simply goes
                // unnamed.
                return Err(UzeError::MutationInProgress {
                    pid: recorded_pid(&path),
                    path,
                });
            }
            Err(source) => return Err(UzeError::Write { path, source }),
        }
        // Only now that the lock is held, so no reader can see a half-written
        // pid belonging to nobody. A failed write costs the next contender a
        // name, never the lock itself. No fsync: the question the pid answers
        // is who to go and look at, and no process outlives the page cache.
        let _ = file.set_len(0);
        let _ = writeln!(file, "pid={}", std::process::id());
        Ok(Self { _file: file })
    }
}

fn recorded_pid(path: &Path) -> Option<u32> {
    fs::read_to_string(path)
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("pid=")?.trim().parse().ok())
}

/// How long a contended lock is retried before it is reported as held.
///
/// A `flock` belongs to the open file description, and a `fork` on any
/// thread of this process copies every descriptor into the child until its
/// `exec` closes them — so a guard this thread just dropped can still be
/// "held", by nobody, for the microseconds another thread's spawn is
/// between the two calls. Detection spawns harness binaries constantly. A
/// genuine holder is another UZE process that keeps the lock for the whole
/// command, which this budget never masks.
const CONTENTION_GRACE: Duration = Duration::from_millis(25);
const CONTENTION_POLL: Duration = Duration::from_millis(2);

fn try_lock_exclusive_briefly(file: &File) -> std::io::Result<()> {
    let deadline = Instant::now() + CONTENTION_GRACE;
    loop {
        match try_lock_exclusive(file) {
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
            {
                std::thread::sleep(CONTENTION_POLL);
            }
            outcome => return outcome,
        }
    }
}

#[cfg(unix)]
fn try_lock_exclusive(file: &File) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    // SAFETY: `flock` is called on a file descriptor this process owns and
    // keeps open for as long as the lock is held.
    let outcome = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if outcome == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Without an OS-level advisory lock there is nothing to serialize two
/// processes with, which matches the runtime's supported platforms — the
/// same position `project::task` takes.
#[cfg(not(unix))]
fn try_lock_exclusive(_file: &File) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_blocks_a_concurrent_mutation_attempt() {
        let root = uze_testkit::temp::scratch("lock");
        let home = UzeHome::at(&root);
        let first = MutationLock::acquire(&home).unwrap();
        // `flock` is held by the open descriptor, not by the process, so a
        // second acquirer in this very process is refused exactly as another
        // process would be — and is told whose lock it is.
        assert!(matches!(
            MutationLock::acquire(&home),
            Err(UzeError::MutationInProgress { pid: Some(pid), .. }) if pid == std::process::id()
        ));
        drop(first);
        MutationLock::acquire(&home).unwrap();
        let _ = fs::remove_dir_all(root);
    }

    /// A lock file left on disk by an interrupted run is not a lock: the
    /// file is permanent and never unlinked, so the only thing that blocks
    /// is a descriptor somebody still holds. A stale file must therefore
    /// never be the end of mutating this home.
    #[test]
    fn a_lock_file_nobody_holds_is_not_a_lock() {
        let root = uze_testkit::temp::scratch("lock-stale");
        let home = UzeHome::at(&root);
        home.ensure_layout().unwrap();
        let path = home.state_dir().join("mutation.lock");
        fs::write(&path, b"pid=999999\n").unwrap();

        let taken = MutationLock::acquire(&home).expect("a lock file nobody holds is free");
        assert_eq!(
            recorded_pid(&path),
            Some(std::process::id()),
            "the lock names whoever actually holds it now"
        );
        drop(taken);
        assert!(path.exists(), "the lock file is permanent");
        let _ = fs::remove_dir_all(root);
    }

    /// A lock file that never received its pid names nobody, and a lock
    /// naming nobody still has to be acquirable — and refusable.
    #[test]
    fn a_lock_with_no_recorded_holder_still_answers() {
        let root = uze_testkit::temp::scratch("lock-headless");
        let home = UzeHome::at(&root);
        home.ensure_layout().unwrap();
        fs::write(home.state_dir().join("mutation.lock"), b"").unwrap();

        let held = MutationLock::acquire(&home).expect("a lock naming nobody is free");
        drop(held);
        let _ = fs::remove_dir_all(root);
    }

    /// The race the reclaim-and-recreate dance lost: two acquirers both
    /// judging a stale file's holder dead both unlinked it and both
    /// re-created it, so both believed they held the lock — two concurrent
    /// `uze install` runs rewriting the Store, the ledger and every harness
    /// config at once (reproduced 5/200), while 190/200 of the ordinary
    /// case was misreported as `No such file or directory`.
    ///
    /// `flock` is held per open file description, so two threads opening the
    /// file separately race exactly as two processes do.
    #[test]
    fn two_acquirers_racing_on_a_stale_lock_never_both_win() {
        const TRIALS: usize = 200;

        let root = uze_testkit::temp::scratch("lock-race");
        let home = UzeHome::at(&root);
        home.ensure_layout().unwrap();
        let path = home.state_dir().join("mutation.lock");

        for trial in 0..TRIALS {
            fs::write(&path, b"pid=999999\n").unwrap();
            let start = std::sync::Barrier::new(2);
            let (won, refused) = std::thread::scope(|scope| {
                let one = scope.spawn(|| {
                    start.wait();
                    MutationLock::acquire(&home)
                });
                let two = scope.spawn(|| {
                    start.wait();
                    MutationLock::acquire(&home)
                });
                // Both held until both have answered: a winner that released
                // before its rival asked would make the rival's success mean
                // nothing.
                let outcomes = [one.join().unwrap(), two.join().unwrap()];
                (
                    outcomes.iter().filter(|outcome| outcome.is_ok()).count(),
                    outcomes
                        .iter()
                        .filter(|outcome| {
                            matches!(outcome, Err(UzeError::MutationInProgress { .. }))
                        })
                        .count(),
                )
            });
            assert_eq!(
                won, 1,
                "trial {trial}: {won} acquirers believed they held the lock"
            );
            assert_eq!(
                refused, 1,
                "trial {trial}: the loser was told something other than \
                 \"another mutation is in progress\""
            );
        }
        let _ = fs::remove_dir_all(root);
    }

    /// The case `Drop` cannot cover: a holder killed outright. The kernel
    /// closes its descriptors, so the lock is free for the next process with
    /// nothing to reclaim and nobody to ask about a pid.
    #[cfg(unix)]
    #[test]
    fn a_holder_killed_outright_releases_the_lock() {
        use std::os::fd::AsRawFd;

        let root = uze_testkit::temp::scratch("lock-killed");
        let home = UzeHome::at(&root);
        home.ensure_layout().unwrap();
        let path = home.state_dir().join("mutation.lock");

        // A real second process holding the real lock. It inherits an open
        // descriptor and takes the `flock` itself; only raw syscalls run in
        // the child, since after `fork` in a threaded process nothing else
        // is safe to call.
        let held = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .unwrap();
        let mut took = [0 as libc::c_int; 2];
        assert_eq!(unsafe { libc::pipe(took.as_mut_ptr()) }, 0);
        let child = unsafe { libc::fork() };
        assert!(child >= 0, "fork failed");
        if child == 0 {
            unsafe {
                libc::close(took[0]);
                let answer = [u8::from(libc::flock(held.as_raw_fd(), libc::LOCK_EX) == 0)];
                libc::write(took[1], answer.as_ptr().cast(), 1);
                loop {
                    libc::pause();
                }
            }
        }
        unsafe { libc::close(took[1]) };
        let mut answer = [0u8; 1];
        assert_eq!(
            unsafe { libc::read(took[0], answer.as_mut_ptr().cast(), 1) },
            1
        );
        assert_eq!(answer[0], 1, "the child never took the lock");
        // The child's descriptor is a dup of this one; the lock outlives our
        // close and dies with the child.
        drop(held);
        unsafe { libc::close(took[0]) };

        assert!(matches!(
            MutationLock::acquire(&home),
            Err(UzeError::MutationInProgress { .. })
        ));

        unsafe {
            libc::kill(child, libc::SIGKILL);
            libc::waitpid(child, std::ptr::null_mut(), 0);
        }
        MutationLock::acquire(&home).expect("a killed holder's lock is free");
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
        // Dropped — the file stays (it is the lock's permanent home) and a
        // new acquire must succeed on it.
        assert!(home.state_dir().join("mutation.lock").exists());
        MutationLock::acquire(&home).unwrap();
        let _ = fs::remove_dir_all(root);
    }
}
