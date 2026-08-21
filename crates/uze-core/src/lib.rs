//! Harness-agnostic UZE domain, persistence, planning, and integration
//! contracts. Concrete vendor integrations and presentation layers depend on
//! this crate; it never depends on them.

pub mod acquisition;
pub mod bundle;
pub mod capability;
pub mod engine;
pub mod error;
pub mod exposure;
pub mod home;
pub mod importer;
pub mod importers;
pub mod integration;
pub mod persistence;
pub mod project;
pub mod reconciliation;
pub mod router;
pub mod runtime;
pub mod state;
pub mod store;
pub mod trust;

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
