//! The repository write lock. See the crate doc for why it is here.

use std::{
    cell::RefCell,
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use crate::{SpawnError, read};

const LOCK_FILE_NAME: &str = "uze-write.lock";
const RETRY_INTERVAL: Duration = Duration::from_millis(20);

thread_local! {
    /// Lock files this thread already holds, so a write inside [`super::locked`]
    /// re-enters instead of deadlocking against its own critical section.
    static HELD: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
}

/// Proof the lock is held; releasing it is dropping this.
pub(crate) struct Held {
    /// `None` when this thread already held the lock and this is a re-entry.
    owned: Option<(PathBuf, File)>,
}

impl Drop for Held {
    fn drop(&mut self) {
        if let Some((path, _file)) = self.owned.take() {
            HELD.with(|held| held.borrow_mut().retain(|held| held != &path));
            // Closing the file releases the `flock`.
        }
    }
}

pub(crate) fn acquire(root: &Path, timeout: Duration) -> Result<Held, SpawnError> {
    let Some(path) = lock_path(root) else {
        return Ok(Held { owned: None });
    };
    if HELD.with(|held| held.borrow().contains(&path)) {
        return Ok(Held { owned: None });
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| SpawnError(format!("could not open {}: {error}", path.display())))?;
    let started = Instant::now();
    loop {
        match try_lock(&file) {
            Ok(()) => break,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if started.elapsed() >= timeout {
                    return Err(SpawnError(format!(
                        "the repository write lock at {} was busy for {}s: another uze or git \
                         write is still running",
                        path.display(),
                        timeout.as_secs()
                    )));
                }
                thread::sleep(RETRY_INTERVAL);
            }
            Err(error) => {
                return Err(SpawnError(format!(
                    "could not lock {}: {error}",
                    path.display()
                )));
            }
        }
    }
    HELD.with(|held| held.borrow_mut().push(path.clone()));
    Ok(Held {
        owned: Some((path, file)),
    })
}

/// `<common dir>/uze-write.lock`, or `None` outside a repository.
fn lock_path(root: &Path) -> Option<PathBuf> {
    let common = read(
        root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .ok()?
    .successful()
    .ok()?;
    let common = common.trim();
    if common.is_empty() {
        return None;
    }
    Some(PathBuf::from(common).join(LOCK_FILE_NAME))
}

#[cfg(unix)]
fn try_lock(file: &File) -> std::io::Result<()> {
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

/// Without an OS-level advisory lock the write lock serializes only the
/// writes of this process, which is what the thread-local register above
/// already guarantees for re-entry; cross-process safety is a Unix
/// property here, matching the runtime's supported platforms.
#[cfg(not(unix))]
fn try_lock(_file: &File) -> std::io::Result<()> {
    Ok(())
}
