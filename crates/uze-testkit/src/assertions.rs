//! Reusable semantic assertions.
//!
//! These are thin, context-carrying helpers: they make failures name the
//! fixture/harness/path involved instead of dumping a bare `false`. They
//! never hide domain state behind opaque wrappers — when a domain object has
//! a meaningful shape (receipts, coverage, environment readiness) the
//! domain tests assert on that shape directly.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Output;

/// Asserts `path` exists and is a file; the failure message names `label`.
pub fn assert_file(path: &Path, label: &str) {
    assert!(
        path.is_file(),
        "{label}: expected a file at {}",
        path.display()
    );
}

/// Asserts `path` exists and is a directory.
pub fn assert_dir(path: &Path, label: &str) {
    assert!(
        path.is_dir(),
        "{label}: expected a directory at {}",
        path.display()
    );
}

/// Asserts `path` does not exist (file or dir).
pub fn assert_missing(path: &Path, label: &str) {
    assert!(
        !path.exists(),
        "{label}: expected nothing at {}",
        path.display()
    );
}

/// Asserts `path` is a symlink pointing exactly at `target`.
pub fn assert_symlink(path: &Path, target: &Path, label: &str) {
    let actual = std::fs::read_link(path).unwrap_or_else(|error| {
        panic!(
            "{label}: expected {} to be a symlink resolving to {}, got: {error}",
            path.display(),
            target.display()
        )
    });
    assert_eq!(
        actual,
        target,
        "{label}: symlink at {} points at {}, expected {}",
        path.display(),
        actual.display(),
        target.display()
    );
}

/// Asserts stdout of `output` contains `needle` (for exit-success-shaped
/// CLI evidence), naming `label` and the stderr on mismatch.
pub fn assert_stdout_contains(output: &Output, needle: &str, label: &str) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(needle),
        "{label}: expected stdout to contain {needle:?}; stdout: {stdout:?}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Asserts `output` succeeded, showing stdout/stderr and `label`.
pub fn assert_success(output: &Output, label: &str) {
    assert!(
        output.status.success(),
        "{label}: expected success, got {:?}\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Recursively snapshots a directory into relative-path → contents, so a
/// test can prove that a projection/reconcile/plan left bytes untouched.
pub fn snapshot_dir(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .unwrap_or_else(|error| panic!("snapshot_dir: read_dir {}: {error}", dir.display()))
            .map(|entry| entry.unwrap().path())
            .collect();
        entries.sort();
        for entry in entries {
            let rel = entry
                .strip_prefix(root)
                .expect("snapshot_dir: entry must be under root")
                .to_path_buf();
            if std::fs::symlink_metadata(&entry)
                .map(|meta| meta.file_type().is_symlink())
                .unwrap_or(false)
            {
                let target = std::fs::read_link(&entry).unwrap();
                out.insert(rel, format!("->{}", target.display()).into_bytes());
            } else if entry.is_dir() {
                walk(root, &entry, out);
            } else {
                out.insert(
                    rel,
                    std::fs::read(&entry).unwrap_or_else(|error| {
                        panic!("snapshot_dir: read {}: {error}", entry.display())
                    }),
                );
            }
        }
    }

    let mut out = BTreeMap::new();
    if root.is_dir() {
        walk(root, root, &mut out);
    }
    out
}

/// Asserts two snapshots (see [`snapshot_dir`]) are byte-identical, with the
/// first differing path named.
pub fn assert_snapshot_unchanged(
    before: &BTreeMap<PathBuf, Vec<u8>>,
    after: &BTreeMap<PathBuf, Vec<u8>>,
    label: &str,
) {
    let mut only_before: Vec<_> = before
        .keys()
        .filter(|key| !after.contains_key(*key))
        .collect();
    let mut only_after: Vec<_> = after
        .keys()
        .filter(|key| !before.contains_key(*key))
        .collect();
    let mut changed: Vec<_> = before
        .iter()
        .filter(|(key, value)| after.get(*key) != Some(*value))
        .map(|(key, _)| key)
        .collect();
    only_before.sort();
    only_after.sort();
    changed.sort();
    assert!(
        only_before.is_empty() && only_after.is_empty() && changed.is_empty(),
        "{label}: filesystem changed:\n  only before: {:?}\n  only after: {:?}\n  changed: {:?}",
        only_before,
        only_after,
        changed
    );
}

/// Asserts every entry in `paths` is unique (no duplicate delivery entries).
pub fn assert_all_unique(paths: &[PathBuf], label: &str) {
    let mut sorted = paths.to_vec();
    sorted.sort();
    let mut duplicates = sorted.windows(2).filter(|pair| pair[0] == pair[1]);
    if let Some(dup) = duplicates.next() {
        panic!("{label}: duplicate entry {} ({paths:?})", dup[0].display());
    }
}
