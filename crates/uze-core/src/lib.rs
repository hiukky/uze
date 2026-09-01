//! Harness-agnostic UZE domain, persistence, planning, and integration
//! contracts. Concrete vendor integrations and presentation layers depend on
//! this crate; it never depends on them.

pub mod acquisition;
pub mod bundle;
pub mod capability;
pub mod context;
pub mod detection_cache;
pub mod engine;
pub mod error;
pub mod exposure;
pub mod harness_runtime;
pub mod home;
pub mod hook;
pub mod importer;
pub mod importers;
pub mod integration;
pub mod naming;
pub mod persistence;
pub mod preference;
pub mod profile_state;
pub mod project;
pub mod project_lock;
pub mod project_root;
pub mod prompt_history;
pub mod provisioning;
pub mod reconciliation;
pub mod router;
pub mod shell_path;
pub mod skill;
pub mod state;
pub mod store;
pub mod subprocess;
pub mod text_region;
pub mod trust;
pub mod workspace;

pub use acquisition::{MaterializedPackage, PackageSource, Provenance, ResolvedSource};
pub use engine::UzeEngine;
pub use error::{ProjectionConflictDetails, Result, UzeError};
pub use exposure::{ExposureMechanism, ExposurePlan, PackageExposurePlan, PreparedExposure};
pub use home::UzeHome;
pub use project::{
    EffectiveEnvironment, ResolvedProject, Resource, ResourceOrigin, resolve_project,
    resolve_project_resources,
};
pub use skill::SkillInvocationPolicy;
pub use store::{PackageId, StoredPackage, UzeStore};

/// Test-only discipline for anything in this crate's own test suite that
/// touches process-global `PATH` — either by mutating it directly, or by
/// spawning a subprocess resolved *by name* (so its success depends on
/// `PATH`'s current content, unlike the sibling tests across this crate
/// that spawn by absolute path and are immune to this entirely).
///
/// `cargo test` runs every test in one binary across parallel threads, so
/// without a shared lock a test that temporarily narrows `PATH` to a
/// scratch directory (`harness_runtime`'s resolver tests) can race a wholly
/// unrelated test spawning a real, PATH-resolved binary (`git`) mid-window
/// and see that narrowed `PATH` — an intermittent `NotFound` with no
/// connection visible from either test's own code. A lock local to one
/// module only protects that module's own tests against each other, not
/// against the rest of the crate, so this lives here instead.
#[cfg(test)]
pub(crate) mod test_support {
    pub static PROCESS_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
}
