//! Cross-invocation, cross-call-site cache for `IntegrationPort::detect()`
//! results. See ADR 018 (`docs/adr/018-cache-harness-detection-with-
//! fingerprint-ttl-invalidation.md`) and `specs/cli-performance/spec.md`
//! for the behavior contract this module implements the mechanism for.
//!
//! Two tiers, both fully automatic — there is no manual refresh:
//! - **in-process memoization**, scoped to the lifetime of one
//!   `DetectionCache` (one command invocation, or one TUI session):
//!   collapses every call site's repeated `.detect()` for the same
//!   integration down to at most one live probe;
//! - **on-disk**, a JSON file under `UzeHome::cache_dir()`: read-through
//!   and write-through on top of the in-process tier, so a fresh CLI
//!   invocation can reuse the previous one's result.
//!
//! Invalidation is a fingerprint (resolved executable path + mtime),
//! backstopped by a bounded TTL for the cases a fingerprint cannot see
//! (e.g. an installer that preserves a packaged mtime instead of stamping
//! install time). `IntegrationPort::provision()` succeeding writes its
//! already-obtained result straight through, so a UZE-driven install or
//! update never has a stale window.

use std::{
    cell::RefCell,
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{home::UzeHome, integration::HarnessDetection};

/// Backstop for fingerprint blind spots — see the module doc and ADR 018.
const MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// Identity of the resolved executable an entry was probed against. A read
/// that finds either half changed treats the entry as stale. Absent for a
/// harness that resolved to nothing, which is itself a meaningful,
/// checkable fingerprint state (distinct from "never probed").
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct Fingerprint {
    resolved_path: Option<PathBuf>,
    modified_unix_nanos: Option<u128>,
}

impl Fingerprint {
    /// Resolves the first of `candidates` found on `PATH` (or, for a
    /// candidate that is itself already a path, checked directly — the
    /// route every test in this module uses to avoid mutating the
    /// process-wide `PATH` environment variable) and stats its mtime. No
    /// subprocess spawned; this is the entire reason a fingerprint check
    /// is cheap enough to run on every cache read.
    fn resolve(candidates: &[&str]) -> Self {
        let resolved_path = candidates.iter().find_map(|name| resolve_on_path(name));
        let modified_unix_nanos = resolved_path.as_deref().and_then(mtime_unix_nanos);
        Self {
            resolved_path,
            modified_unix_nanos,
        }
    }
}

fn resolve_on_path(program: &str) -> Option<PathBuf> {
    if program.contains(std::path::MAIN_SEPARATOR) {
        let path = PathBuf::from(program);
        return is_executable_file(&path).then_some(path);
    }
    let path_var = env::var_os("PATH")?;
    env::split_paths(&path_var)
        .map(|dir| dir.join(program))
        .find(|candidate| is_executable_file(candidate))
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path)
        .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

fn mtime_unix_nanos(path: &Path) -> Option<u128> {
    fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_nanos())
}

fn now_unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_nanos()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CachedEntry {
    detection: HarnessDetection,
    fingerprint: Fingerprint,
    cached_at_unix_nanos: u128,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct OnDiskCache {
    entries: HashMap<String, CachedEntry>,
}

impl OnDiskCache {
    /// Fail-open: any read or parse problem is an empty cache, never an
    /// error — a cache is a reconstructable optimization, not a new
    /// failure mode for commands that worked before it existed (ADR 018).
    fn load(path: &Path) -> Self {
        fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Best-effort: a write failure (e.g. a read-only `UZE_HOME`) must not
    /// fail the command that triggered it — the cache is an optimization
    /// layered on top of a correct uncached path, not a new dependency.
    fn save(&self, path: &Path) {
        if let Ok(payload) = serde_json::to_vec_pretty(self) {
            let _ = crate::persistence::write_atomic(path, &payload);
        }
    }
}

/// Cross-invocation cache for `IntegrationPort::detect()`. One instance is
/// meant to live for one command invocation (or one TUI session): its
/// in-process tier naturally scopes to "one logical command" that way.
pub struct DetectionCache {
    path: PathBuf,
    memo: RefCell<HashMap<&'static str, HarnessDetection>>,
}

impl DetectionCache {
    pub fn new(home: &UzeHome) -> Self {
        Self {
            path: home.harness_detection_cache_path(),
            memo: RefCell::new(HashMap::new()),
        }
    }

    #[cfg(test)]
    fn at(path: PathBuf) -> Self {
        Self {
            path,
            memo: RefCell::new(HashMap::new()),
        }
    }

    /// A still-valid cached result for `integration_id`, checking the
    /// in-process tier first and falling back to the on-disk tier. Never
    /// spawns a subprocess. `program_candidates` (see
    /// `IntegrationPort::detection_program_candidates`) are used only to
    /// compute the freshness fingerprint, never to decide presence itself.
    pub fn get(
        &self,
        integration_id: &'static str,
        program_candidates: &[&str],
    ) -> Option<HarnessDetection> {
        if let Some(detection) = self.memo.borrow().get(integration_id) {
            return Some(detection.clone());
        }
        let on_disk = OnDiskCache::load(&self.path);
        let entry = on_disk.entries.get(integration_id)?;
        let age_nanos = now_unix_nanos().saturating_sub(entry.cached_at_unix_nanos);
        if age_nanos >= MAX_AGE.as_nanos() {
            return None;
        }
        if entry.fingerprint != Fingerprint::resolve(program_candidates) {
            return None;
        }
        self.memo
            .borrow_mut()
            .insert(integration_id, entry.detection.clone());
        Some(entry.detection.clone())
    }

    /// Records a freshly-obtained detection result — from a live probe, or
    /// from `IntegrationPort::provision()` succeeding — into both tiers.
    pub fn put(
        &self,
        integration_id: &'static str,
        program_candidates: &[&str],
        detection: HarnessDetection,
    ) {
        self.memo
            .borrow_mut()
            .insert(integration_id, detection.clone());
        let mut on_disk = OnDiskCache::load(&self.path);
        on_disk.entries.insert(
            integration_id.to_owned(),
            CachedEntry {
                detection,
                fingerprint: Fingerprint::resolve(program_candidates),
                cached_at_unix_nanos: now_unix_nanos(),
            },
        );
        on_disk.save(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_cache_path(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "uze-detection-cache-{label}-{}-{nonce}.json",
            std::process::id()
        ))
    }

    /// A directory unique to this call, not just this process — tests run
    /// concurrently in threads that share one process id, so a directory
    /// name derived only from `std::process::id()` collides across tests.
    fn unique_temp_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("uze-dc-bin-{label}-{}-{nonce}", std::process::id()))
    }

    fn fake_executable(dir: &Path, name: &str) -> PathBuf {
        fs::create_dir_all(dir).unwrap();
        let path = dir.join(name);
        fs::write(&path, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    #[test]
    fn empty_cache_returns_none() {
        let cache = DetectionCache::at(temp_cache_path("empty"));
        assert!(
            cache
                .get("antigravity", &["/nonexistent/antigravity"])
                .is_none()
        );
    }

    #[test]
    fn put_then_get_within_one_instance_hits_in_process_tier() {
        let dir = unique_temp_dir("memo");
        let bin = fake_executable(&dir, "fake-harness-a");
        let path_str = bin.to_str().unwrap();
        let cache = DetectionCache::at(temp_cache_path("memo"));
        let detection = HarnessDetection {
            present: true,
            version: Some("1.2.3".to_owned()),
        };
        cache.put("claude-code", &[path_str], detection.clone());
        assert_eq!(cache.get("claude-code", &[path_str]), Some(detection));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_fresh_instance_reuses_the_on_disk_entry() {
        let dir = unique_temp_dir("persist");
        let bin = fake_executable(&dir, "fake-harness-b");
        let path_str = bin.to_str().unwrap();
        let cache_path = temp_cache_path("persist");
        let detection = HarnessDetection {
            present: true,
            version: Some("9.9.9".to_owned()),
        };
        DetectionCache::at(cache_path.clone()).put("antigravity", &[path_str], detection.clone());
        let fresh = DetectionCache::at(cache_path);
        assert_eq!(fresh.get("antigravity", &[path_str]), Some(detection));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn fingerprint_change_invalidates_the_entry() {
        // In-process memoization deliberately does not re-check the
        // fingerprint on every call within one instance (that is the
        // point of decision 1 — one live-vs-cache decision per command).
        // Freshness is enforced on the *next* invocation, i.e. a new
        // `DetectionCache` instance reading the same on-disk file — so
        // this test simulates two separate invocations, matching
        // `a_fresh_instance_reuses_the_on_disk_entry`.
        let dir = unique_temp_dir("fingerprint");
        let bin = fake_executable(&dir, "fake-harness-c");
        let path_str = bin.to_str().unwrap();
        let cache_path = temp_cache_path("fingerprint");
        DetectionCache::at(cache_path.clone()).put(
            "codex",
            &[path_str],
            HarnessDetection {
                present: true,
                version: Some("1.0.0".to_owned()),
            },
        );
        // Simulate an update: replace the file with a distinct mtime.
        std::thread::sleep(Duration::from_millis(10));
        fs::write(&bin, "#!/bin/sh\necho updated\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let next_invocation = DetectionCache::at(cache_path);
        assert!(next_invocation.get("codex", &[path_str]).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn expired_ttl_invalidates_the_entry_even_with_a_matching_fingerprint() {
        let dir = unique_temp_dir("ttl");
        let bin = fake_executable(&dir, "fake-harness-d");
        let path_str = bin.to_str().unwrap();
        let cache_path = temp_cache_path("ttl");
        let fingerprint = Fingerprint::resolve(&[path_str]);
        let mut on_disk = OnDiskCache::default();
        on_disk.entries.insert(
            "opencode".to_owned(),
            CachedEntry {
                detection: HarnessDetection {
                    present: true,
                    version: Some("0.0.1".to_owned()),
                },
                fingerprint,
                cached_at_unix_nanos: now_unix_nanos() - (MAX_AGE.as_nanos() + 1),
            },
        );
        on_disk.save(&cache_path);
        let cache = DetectionCache::at(cache_path);
        assert!(cache.get("opencode", &[path_str]).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_cache_file_is_fail_open() {
        let cache = DetectionCache::at(PathBuf::from("/nonexistent/dir/harness_detection.json"));
        assert!(
            cache
                .get("antigravity", &["/nonexistent/antigravity"])
                .is_none()
        );
    }

    #[test]
    fn corrupted_cache_file_is_fail_open() {
        let path = temp_cache_path("corrupt");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"not json").unwrap();
        let cache = DetectionCache::at(path.clone());
        assert!(
            cache
                .get("antigravity", &["/nonexistent/antigravity"])
                .is_none()
        );
        let _ = fs::remove_file(&path);
    }
}
