//! Cache for per-receipt attachment inspection on READ paths — the
//! mechanism behind ADR 024.
//!
//! The expensive half of `doctor()` is per-receipt
//! `IntegrationPort::inspect_receipt`: several integrations verify their
//! attachments by running vendor CLIs (`codex plugin list`, `claude
//! plugin list`, …), each a subprocess of a slow install-managed binary.
//! With N packages and 4 integrations that is seconds per run, paid by
//! every screen that renders attachment health.
//!
//! This cache holds the *verdicts* of those inspections so read paths are
//! milliseconds instead. It deliberately mirrors `detection_cache`'s
//! contract (ADR 018):
//!
//! - two tiers — in-process memoization (one invocation collapses repeated
//!   reads) and an on-disk JSON file under `UzeHome::cache_dir()`;
//! - a bounded TTL backstop for state a fingerprint cannot see;
//! - **fingerprint + TTL invalidation**: artifacts with a directly
//!   stat-able presence (`SymlinkReference`) are re-checked against the
//!   filesystem on every read, so a hand-removed link is detected without
//!   paying a vendor CLI; vendor-native catalogue verdicts (no cheap
//!   fingerprint) are bounded by TTL + mutation invalidation alone;
//! - **only `Matched` verdicts are ever stored**: an anomaly
//!   (Missing/Drifted/Conflict/Blocked) is exactly the case a user needs
//!   to see fresh, so it is always re-inspected live;
//! - **mutations invalidate**: every Store/vendor-state mutation clears
//!   the cache, so a verdict can never outlive the change that produced
//!   the inspected state.
//!
//! Deliberately scoped to read paths. Removal planning and detach keep
//! calling the live `reconcile_package` — a stale `Matched` must never
//! authorize destroying a vendor artifact that drifted.

#![allow(clippy::empty_line_after_doc_comments)]

use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use serde_json;

use uze_core::{
    UzeHome,
    integration::{AttachmentInspection, AttachmentState},
};

/// Backstop for the verdicts' staleness window — matches the detection
/// cache's own `MAX_AGE` (ADR 018): fingerprint-free v1, bounded by
/// mutation invalidation on UZE's side and by TTL for changes made
/// outside UZE.
const MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone, Debug, Deserialize, Serialize)]
struct CachedInspection {
    state: AttachmentState,
    reason: String,
    cached_at_unix_nanos: u128,
    /// `managed_artifact_fingerprint` at the time the verdict was
    /// obtained. `None` for artifacts whose state is not cheaply
    /// stat-able (vendor-native catalogues) — those rely on TTL +
    /// mutation invalidation; `Some` ones are re-checked on every read.
    fingerprint: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct OnDiskCache {
    entries: HashMap<String, CachedInspection>,
}

impl OnDiskCache {
    /// Fail-open: any read or parse problem is an empty cache, never an
    /// error (same rule as the detection cache, ADR 018).
    fn load(path: &std::path::Path) -> Self {
        fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Best-effort: a write failure (e.g. a read-only `UZE_HOME`) must not
    /// fail the command that triggered it.
    fn save(&self, path: &std::path::Path) {
        if let Ok(payload) = serde_json::to_vec_pretty(self) {
            let _ = uze_core::persistence::write_atomic(path, &payload);
        }
    }
}

/// Cross-invocation cache of `Matched` attachment inspections, keyed by
/// ledger key. One instance lives per `UzeApplication`; its in-process
/// tier naturally scopes to one logical command — but fingerprint checks
/// still run on memo hits, because they are two `stat` calls, not a
/// subprocess (the "one probe per command" rule only binds live vendor
/// CLIs, ADR 018 decision 1).
pub struct InspectionCache {
    path: PathBuf,
    memo: RefCell<HashMap<String, CachedInspection>>,
}

impl InspectionCache {
    pub fn new(home: &UzeHome) -> Self {
        Self {
            path: home.inspection_cache_path(),
            memo: RefCell::new(HashMap::new()),
        }
    }

    /// A still-fresh cached verdict for `ledger_key`, given the artifact's
    /// current fingerprint. Never re-inspects — that is the caller's job on
    /// a miss. TTL-expired, fingerprint-mismatched, or absent → `None`.
    pub fn get(&self, ledger_key: &str, fingerprint: Option<&str>) -> Option<AttachmentInspection> {
        if let Some(entry) = self.memo.borrow().get(ledger_key)
            && fingerprint_matches(entry, fingerprint)
        {
            return Some(AttachmentInspection {
                state: entry.state,
                reason: entry.reason.clone(),
            });
        }
        let on_disk = OnDiskCache::load(&self.path);
        let entry = on_disk.entries.get(ledger_key)?;
        let age_nanos = now_unix_nanos().saturating_sub(entry.cached_at_unix_nanos);
        if age_nanos >= MAX_AGE.as_nanos() {
            return None;
        }
        // A stat-able artifact that changed since the verdict (or vanished:
        // a hand-removed link) makes the verdict stale — detected with two
        // stat calls, no vendor CLI.
        if !fingerprint_matches(entry, fingerprint) {
            return None;
        }
        let inspection = AttachmentInspection {
            state: entry.state,
            reason: entry.reason.clone(),
        };
        self.memo
            .borrow_mut()
            .insert(ledger_key.to_owned(), entry.clone());
        Some(inspection)
    }

    /// Records a freshly-obtained verdict — **Matched only**: anomalies
    /// are always re-inspected live so the first glance at a problem is
    /// never stale.
    pub fn put(
        &self,
        ledger_key: &str,
        inspection: &AttachmentInspection,
        fingerprint: Option<String>,
    ) {
        if inspection.state != AttachmentState::Matched {
            return;
        }
        let entry = CachedInspection {
            state: inspection.state,
            reason: inspection.reason.clone(),
            cached_at_unix_nanos: now_unix_nanos(),
            fingerprint,
        };
        self.memo
            .borrow_mut()
            .insert(ledger_key.to_owned(), entry.clone());
        let mut on_disk = OnDiskCache::load(&self.path);
        on_disk.entries.insert(ledger_key.to_owned(), entry);
        on_disk.save(&self.path);
    }

    /// Clears both tiers. Called by every mutating entry point — an
    /// inspection must never outlive the mutation that changed the state
    /// it describes.
    pub fn invalidate(&self) {
        self.memo.borrow_mut().clear();
        let _ = fs::remove_file(&self.path);
    }
}

fn fingerprint_matches(entry: &CachedInspection, current: Option<&str>) -> bool {
    entry.fingerprint.is_none() || entry.fingerprint.as_deref() == current
}

fn now_unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_nanos()
}

#[cfg(test)]
mod tests {
    use super::*;
    use uze_core::UzeHome;

    fn temp_cache(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "uze-inspection-cache-{label}-{}-{nonce}.json",
            std::process::id()
        ))
    }

    fn home_at(label: &str) -> UzeHome {
        let nonce = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        UzeHome::at(std::env::temp_dir().join(format!(
            "uze-inspection-home-{label}-{}-{nonce}",
            std::process::id()
        )))
    }

    fn matched(reason: &str) -> AttachmentInspection {
        AttachmentInspection {
            state: AttachmentState::Matched,
            reason: reason.to_owned(),
        }
    }

    #[test]
    fn matched_round_trips_through_both_tiers() {
        let home = home_at("roundtrip");
        let cache = InspectionCache::new(&home);
        assert!(cache.get("pkg:test:skill", None).is_none());
        cache.put("pkg:test:skill", &matched("ok"), None);
        assert_eq!(cache.get("pkg:test:skill", None), Some(matched("ok")));
        // A fresh instance must reuse the on-disk tier.
        let fresh = InspectionCache::new(&home);
        assert_eq!(fresh.get("pkg:test:skill", None), Some(matched("ok")));
    }

    #[test]
    fn fingerprint_change_invalidates_the_verdict() {
        let home = home_at("fingerprint");
        let cache = InspectionCache::new(&home);
        cache.put(
            "pkg:test:symlink",
            &matched("ok"),
            Some("111:link-target:orig".to_owned()),
        );
        // Same fingerprint → hit, in both tiers.
        assert!(
            cache
                .get("pkg:test:symlink", Some("111:link-target:orig"))
                .is_some()
        );
        let fresh = InspectionCache::new(&home);
        assert!(
            fresh
                .get("pkg:test:symlink", Some("111:link-target:orig"))
                .is_some()
        );
        // Changed fingerprint (a hand-removed/re-pointed artifact) → miss,
        // even on the same instance (fingerprints are stat-checks, not
        // probes).
        assert!(
            cache
                .get("pkg:test:symlink", Some("999:none:orig"))
                .is_none()
        );
        assert!(
            fresh
                .get("pkg:test:symlink", Some("999:none:orig"))
                .is_none()
        );
        // And it never silently returns the stale verdict for the old
        // fingerprint either (the memo tier re-checks).
        assert!(
            cache
                .get("pkg:test:symlink", Some("111:none:orig"))
                .is_none()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_removed_symlink_is_detected_without_a_probe() {
        use std::os::unix::fs::symlink;
        use uze_core::integration::{ManagedArtifact, managed_artifact_fingerprint};

        let base = home_at("symlink");
        let home_dir = base.root().to_path_buf();
        let target = home_dir.join("target-dir");
        std::fs::create_dir_all(&target).unwrap();
        let link = home_dir.join("managed-link");
        symlink(&target, &link).unwrap();

        let artifact = ManagedArtifact::SymlinkReference {
            path: link.clone(),
            target: target.clone(),
        };
        let fingerprint = managed_artifact_fingerprint(&artifact).expect("symlink carries one");
        let cache = InspectionCache::new(&base);
        cache.put(
            "pkg:test:symlink",
            &matched("ok"),
            Some(fingerprint.clone()),
        );
        assert!(cache.get("pkg:test:symlink", Some(&fingerprint)).is_some());

        // Hand-removal: recomputed fingerprint flips to "absent" → miss —
        // the lifecycle that a Matched verdict must never mask.
        std::fs::remove_file(&link).unwrap();
        let now = managed_artifact_fingerprint(&artifact).expect("absent is still a state");
        assert_ne!(now, fingerprint);
        assert!(cache.get("pkg:test:symlink", Some(&now)).is_none());
    }

    #[test]
    fn anomalies_are_never_stored() {
        let home = home_at("anomaly");
        let cache = InspectionCache::new(&home);
        cache.put(
            "pkg:test:drifted",
            &AttachmentInspection {
                state: AttachmentState::Drifted,
                reason: "changed".to_owned(),
            },
            None,
        );
        assert!(cache.get("pkg:test:drifted", None).is_none());
        // And a Matched key already cached is untouched by an anomaly put.
        cache.put("pkg:test:ok", &matched("ok"), None);
        cache.put(
            "pkg:test:ok",
            &AttachmentInspection {
                state: AttachmentState::Missing,
                reason: "gone".to_owned(),
            },
            None,
        );
        assert_eq!(cache.get("pkg:test:ok", None), Some(matched("ok")));
    }

    #[test]
    fn invalidate_clears_both_tiers() {
        let home = home_at("invalidate");
        let cache = InspectionCache::new(&home);
        cache.put("pkg:test:skill", &matched("ok"), None);
        assert!(cache.get("pkg:test:skill", None).is_some());
        cache.invalidate();
        assert!(cache.get("pkg:test:skill", None).is_none());
        let fresh = InspectionCache::new(&home);
        assert!(fresh.get("pkg:test:skill", None).is_none());
    }

    #[test]
    fn expired_ttl_is_a_miss() {
        let home = home_at("ttl");
        let path = home.inspection_cache_path();
        let persisted = OnDiskCache {
            entries: [(
                "pkg:test:old".to_owned(),
                CachedInspection {
                    state: AttachmentState::Matched,
                    reason: "ok".to_owned(),
                    cached_at_unix_nanos: now_unix_nanos() - (MAX_AGE.as_nanos() + 1),
                    fingerprint: None,
                },
            )]
            .into_iter()
            .collect(),
        };
        persisted.save(&path);
        let cache = InspectionCache::new(&home);
        assert!(cache.get("pkg:test:old", None).is_none());
    }

    #[test]
    fn missing_or_corrupt_cache_file_is_fail_open() {
        let cache = InspectionCache {
            path: PathBuf::from("/nonexistent/inspection.json"),
            memo: RefCell::new(HashMap::new()),
        };
        assert!(cache.get("k", None).is_none());

        let path = temp_cache("corrupt");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"not json").unwrap();
        let cache = InspectionCache {
            path,
            memo: RefCell::new(HashMap::new()),
        };
        assert!(cache.get("k", None).is_none());
    }
}
