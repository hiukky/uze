//! Canonical fixture resolution.
//!
//! All persistent test inputs live under `tests/_fixtures/` in four kinds
//! (see `tests/_fixtures/README.md`):
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

/// `tests/_fixtures/`.
///
/// Resolved through `$UZE_TESTKIT_FIXTURES_ROOT` when set (the Lab image
/// copies the canonical tree to a runtime path; compile-time paths do not
/// exist in the container), otherwise by walking up from this crate's
/// manifest directory.
pub fn root() -> PathBuf {
    if let Some(override_dir) = std::env::var_os("UZE_TESTKIT_FIXTURES_ROOT") {
        let root = PathBuf::from(override_dir);
        assert!(
            root.is_dir(),
            "UZE_TESTKIT_FIXTURES_ROOT is set but is not a directory: {}",
            root.display()
        );
        return root;
    }
    workspace_root().join("tests/_fixtures")
}

/// `tests/_fixtures/canonical/<name>`.
pub fn canonical(name: impl AsRef<Path>) -> PathBuf {
    root().join("canonical").join(name)
}

/// `tests/_fixtures/foreign/<vendor>/<name>`.
pub fn foreign(vendor: impl AsRef<Path>, name: impl AsRef<Path>) -> PathBuf {
    root().join("foreign").join(vendor).join(name)
}

/// `tests/_fixtures/scenarios/<name>`.
pub fn scenario(name: impl AsRef<Path>) -> PathBuf {
    root().join("scenarios").join(name)
}

/// `tests/_fixtures/golden/`.
pub fn golden() -> PathBuf {
    root().join("golden")
}

/// The repository's own official plugin (`plugins/uze`) — a canonical
/// package UZE ships and dogfoods, never copied into `tests/_fixtures`.
pub fn official_plugin() -> PathBuf {
    workspace_root().join("plugins/uze")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixtures_root_honors_the_runtime_override() {
        let override_dir = std::env::temp_dir().join(format!(
            "uze-testkit-fixtures-override-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&override_dir).unwrap();
        let mut scope = crate::env::scope();
        scope.set("UZE_TESTKIT_FIXTURES_ROOT", &override_dir);
        assert_eq!(root(), override_dir);
        assert_eq!(canonical("x"), override_dir.join("canonical/x"));
        drop(scope);
        // Without the override the workspace walk is authoritative again.
        assert!(root().ends_with("tests/_fixtures"));
        let _ = std::fs::remove_dir_all(&override_dir);
    }
}
