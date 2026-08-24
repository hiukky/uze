//! Canonical fixture resolution.
//!
//! All persistent test inputs live under `tests/fixtures/` in four kinds
//! (see `tests/fixtures/README.md`):
//!
//! - `canonical/` — valid UZE-authored package/project inputs, small and
//!   stable; the same directory the product itself would accept.
//! - `foreign/` — vendor-native formats (`.claude-plugin/`, `.codex-plugin/`,
//!   …) that integrations must translate, never treat as canonical.
//! - `scenarios/` — deliberate broken/edge states (malformed lock,
//!   malformed marketplace, nested workspace, …).
//! - `golden/` — the single evolving "North Star" environment used only by
//!   the acceptance suite's [`crate::scenario`]-free golden health test.
//!
//! Tests should never build paths with `env!("CARGO_MANIFEST_DIR")`
//! directly: that resolves to whichever crate compiles the test, and the
//! same fixture must be reachable from every crate.

use std::path::{Path, PathBuf};

use crate::workspace_root;

/// `tests/fixtures/`.
pub fn root() -> PathBuf {
    workspace_root().join("tests/fixtures")
}

/// `tests/fixtures/canonical/<name>`.
pub fn canonical(name: impl AsRef<Path>) -> PathBuf {
    root().join("canonical").join(name)
}

/// `tests/fixtures/foreign/<vendor>/<name>`.
pub fn foreign(vendor: impl AsRef<Path>, name: impl AsRef<Path>) -> PathBuf {
    root().join("foreign").join(vendor).join(name)
}

/// `tests/fixtures/scenarios/<name>`.
pub fn scenario(name: impl AsRef<Path>) -> PathBuf {
    root().join("scenarios").join(name)
}

/// `tests/fixtures/golden/`.
pub fn golden() -> PathBuf {
    root().join("golden")
}

/// The official marketplace fixture (`tests/fixtures/golden/marketplace` if
/// present, else the repository's own `agents.json` directory root).
pub fn marketplace() -> PathBuf {
    root().join("golden").join("marketplace")
}

/// The repository's own official plugin (`plugins/uze`) — a canonical
/// package UZE ships and dogfoods, never copied into `tests/fixtures`.
pub fn official_plugin() -> PathBuf {
    workspace_root().join("plugins/uze")
}

/// Asserts `path` (or one of its ancestors) lives under `tests/fixtures`,
/// for tests that copy fixture content around and want to fail loudly on a
/// stale hard-coded path.
pub fn assert_in_fixtures(path: &Path) {
    let fixtures_root = root();
    let checked = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    assert!(
        checked.starts_with(&fixtures_root),
        "expected {} to live under tests/fixtures ({}); use uze_test_support::fixtures instead \
         of hard-coded paths",
        checked.display(),
        fixtures_root.display()
    );
}
