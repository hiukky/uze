//! Core compatibility importer facade. The concrete canonical importer
//! lives in `importers/`; runtime integrations never depend on this
//! module. See `importers.rs`'s own doc comment for why foreign
//! (vendor-authored) format import is not currently implemented.

pub use crate::importers::{AgentPluginImporter, ForeignImporter};
