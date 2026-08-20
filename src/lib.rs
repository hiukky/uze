//! UZE resolves a standards-first agent project into an explainable effective
//! agent environment. See docs/adr/003 and docs/adr/004.

pub mod application;
pub mod bundle;
pub mod capability;
pub mod conformance;
pub mod engine;
pub mod error;
pub mod exposure;
pub mod home;
pub mod importer;
pub mod importers;
pub mod integration;
pub mod integrations;
pub mod persistence;
pub mod project;
pub mod reconciliation;
pub mod router;
pub mod runtime;
pub mod state;
pub mod store;

pub use application::UzeApplication;
pub use bundle::ImportedBundle;
pub use engine::UzeEngine;
pub use error::{Result, UzeError};
pub use exposure::{
    ExposureMechanism, ExposurePlan, PackageExposureMechanism, PackageExposurePlan,
    PreparedExposure,
};
pub use home::UzeHome;
pub use project::{
    EffectiveEnvironment, ResolvedProject, Resource, ResourceOrigin, resolve_project,
    resolve_project_resources,
};
pub use store::{PackageId, StoredPackage, UzeStore};
