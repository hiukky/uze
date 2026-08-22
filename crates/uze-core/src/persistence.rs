//! Core small, local-only durability primitives for UZE-owned state.
//!
//! They intentionally do not attempt distributed transactions with vendor
//! CLIs. A confirmed external side effect is recorded immediately by the
//! caller, while these helpers keep registry and ledger replacement atomic.

use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{Result, UzeError, home::UzeHome};

pub fn write_atomic(path: &Path, payload: &[u8]) -> Result<()> {
    let parent = path.parent().expect("UZE state paths have a parent");
    fs::create_dir_all(parent).map_err(|source| UzeError::Write {
        path: parent.to_path_buf(),
        source,
    })?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_nanos();
    let temporary = parent.join(format!(
        ".{}.{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("state"),
        std::process::id(),
        nonce
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|source| UzeError::Write {
                path: temporary.clone(),
                source,
            })?;
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
        let root = std::env::temp_dir().join(format!("uze-lock-{}", std::process::id()));
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
        let root = std::env::temp_dir().join(format!(
            "uze-write-atomic-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
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

    #[test]
    fn mutation_lock_is_released_on_drop_and_allows_reacquire() {
        let root = std::env::temp_dir().join(format!(
            "uze-lock-drop-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
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
