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
pub mod importer;
pub mod importers;
pub mod integration;
pub mod managed_file;
pub mod persistence;
pub mod project;
pub mod project_lock;
pub mod project_root;
pub mod provisioning;
pub mod reconciliation;
pub mod router;
pub mod runtime;
pub mod shell_path;
pub mod state;
pub mod store;
pub mod text_region;
pub mod trust;
pub mod workspace;

pub use acquisition::{MaterializedPackage, PackageSource, Provenance, ResolvedSource};
pub use bundle::ImportedBundle;
pub use engine::UzeEngine;
pub use error::{Result, UzeError};
pub use exposure::{ExposureMechanism, ExposurePlan, PackageExposurePlan, PreparedExposure};
pub use home::UzeHome;
pub use project::{
    EffectiveEnvironment, ResolvedProject, Resource, ResourceOrigin, resolve_project,
    resolve_project_resources,
};
pub use store::{PackageId, StoredPackage, UzeStore};
