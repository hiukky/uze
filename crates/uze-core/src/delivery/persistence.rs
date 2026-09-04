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
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{Result, UzeError, home::UzeHome};

pub fn write_atomic(path: &Path, payload: &[u8]) -> Result<()> {
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

/// Process-wide mutation guard for one UZE home. Stale locks deliberately
/// block mutation rather than guessing that an interrupted process is safe
/// to race; `doctor` remains read-only and can report the state.
pub struct MutationLock {
    path: PathBuf,
}

impl MutationLock {
    pub fn acquire(home: &UzeHome) -> Result<Self> {
        home.ensure_layout()?;
        let path = home.state_dir().join("mutation.lock");
        match OpenOptions::new().create_new(true).write(true).open(&path) {
            Ok(mut file) => {
                let _ = writeln!(file, "pid={}", std::process::id());
                let _ = file.sync_all();
                Ok(Self { path })
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                Err(UzeError::MutationInProgress(path))
            }
            Err(source) => Err(UzeError::Write { path, source }),
        }
    }
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
        assert!(matches!(
            MutationLock::acquire(&home),
            Err(UzeError::MutationInProgress(_))
        ));
        drop(first);
        MutationLock::acquire(&home).unwrap();
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
