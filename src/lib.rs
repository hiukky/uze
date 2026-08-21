//! Compatibility facade for the `uze` binary and existing library callers.
//!
//! The harness-agnostic model now lives in `uze-core`; this crate preserves
//! the established public imports while application, integrations, and
//! presentation complete their own extraction.

pub use uze_core::{
    acquisition, bundle, capability, engine, error, exposure, home, importer, importers,
    integration, persistence, project, reconciliation, router, runtime, state, store, trust,
};
pub use uze_core::*;

pub mod application;
pub mod integrations;
pub mod tui;
pub use application::UzeApplication;
