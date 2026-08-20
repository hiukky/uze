//! UZE resolves a standards-first agent project into an explainable effective
//! agent environment. See docs/adr/003 and docs/adr/004.

pub mod bundle;
pub mod capability;
pub mod error;
pub mod project;
pub mod report;
pub mod runtime;

pub use bundle::{ImportedBundle, import_bundle};
pub use capability::harness_evidence;
pub use error::{Result, UzeError};
pub use project::{ResolvedProject, resolve_project};
pub use report::{CompatibilityReport, build_report};
