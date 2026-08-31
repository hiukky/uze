//! Reusable semantic assertions.
//!
//! These are thin, context-carrying helpers: they make failures name the
//! fixture/harness/path involved instead of dumping a bare `false`. They
//! never hide domain state behind opaque wrappers — when a domain object has
//! a meaningful shape (receipts, coverage, environment readiness) the
//! domain tests assert on that shape directly.

use std::path::Path;

/// Asserts `path` exists and is a file; the failure message names `label`.
pub fn assert_file(path: &Path, label: &str) {
    assert!(
        path.is_file(),
        "{label}: expected a file at {}",
        path.display()
    );
}
