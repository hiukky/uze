//! Shared test infrastructure for the UZE workspace.
//!
//! This crate exists so that every test binary in the workspace shares one
//! notion of:
//!
//! - **Isolation**: [`temp::TestEnvironment`] owns the whole environment a
//!   test runs in (HOME, UZE_HOME, PATH head, cwd, harness config roots) and
//!   guards against ever touching the developer's real home.
//! - **Process-global safety**: [`env::scope`] serializes and restores any
//!   `PATH`/`HOME`/cwd mutation (Rust tests run in parallel; process env is
//!   shared).
//! - **Process simulation**: [`fake_harness::FakeHarness`] writes executable
//!   scripts with a rule table and an invocation log, so integration tests
//!   describe vendor behavior instead of inventing shell scripts inline.
//! - **Fixtures**: [`fixtures`] resolves `tests/_fixtures/{canonical,foreign,
//!   scenarios,golden}` at the canonical location no matter which crate's
//!   test is compiling.
//! - **Intent**: [`scenario::Scenario`] assembles a deliberate system state
//!   from a few declarative steps.
//!
//! It depends on nothing product-specific; production crates never depend on
//! it (it is a dev-dependency of the root crate only, and unit tests inside
//! product crates keep their own guards).

pub mod assertions;
pub mod env;
pub mod fake_harness;
pub mod fixtures;
pub mod marketplace;
pub mod scenario;
pub mod temp;

use std::path::{Path, PathBuf};

/// Absolute path of the workspace root: the directory whose `Cargo.toml`
/// declares `[workspace]`, found by walking up from this crate's own
/// manifest directory. All workspace-local test assets (`tests/_fixtures`,
/// `plugins/uze`) are addressed from here, so tests never depend on which
/// crate they compile in.
pub fn workspace_root() -> PathBuf {
    let mut candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        let manifest = candidate.join("Cargo.toml");
        if manifest.is_file()
            && let Ok(contents) = std::fs::read_to_string(&manifest)
            && contents.contains("[workspace]")
        {
            return candidate;
        }
        if !candidate.pop() {
            panic!(
                "uze-testkit: could not locate the workspace root above {}",
                env!("CARGO_MANIFEST_DIR")
            );
        }
    }
}

/// Convenience for tests that only need a path inside the workspace.
pub fn workspace_path(rel: impl AsRef<Path>) -> PathBuf {
    workspace_root().join(rel)
}
