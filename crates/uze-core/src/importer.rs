//! Core compatibility importer facade. Concrete foreign formats live in
//! `importers/`; runtime integrations never depend on this module.

pub use crate::importers::{
    AgentPluginImporter, ClaudePluginImporter, ForeignImporter, import_bundle,
};
