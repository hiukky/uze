//! Compatibility facade for the `uze` binary and existing library callers.
//!
//! The harness-agnostic model now lives in `uze-core`; this crate preserves
//! the established public imports while application, integrations, and
//! presentation complete their own extraction.

pub use uze_core::*;
pub use uze_core::{
    acquisition, bundle, capability, context, engine, error, exposure, harness_runtime, home,
    importer, importers, integration, persistence, project, reconciliation, router, state, store,
    text_region, trust,
};

pub use uze_application::{UzeApplication, application};
pub use uze_integrations as integrations;
pub mod ui;
