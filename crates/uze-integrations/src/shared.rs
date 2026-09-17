//! Vendor-neutral helpers the peer integrations share.
//!
//! **Scope discipline**: a helper lives here once two or more integrations
//! state the same fact the same way — what differs between vendors is a
//! parameter or a dialect (`marketplace::MarketplaceDialect`), never a
//! branch on a vendor's name. A candidate that turns out to diverge in real
//! behavior stays in its vendor's module rather than being forced together:
//! explicit-envelope coverage, for one, parses two different manifest
//! schemas and stays per vendor.
//!
//! Not part of this crate's public API: every integration composition root
//! (`claude.rs`, `codex.rs`, `opencode.rs`, `antigravity.rs`) is still the
//! only thing `uze-application` or any downstream crate ever names.

pub(crate) mod agent;
pub(crate) mod json_config;
pub(crate) mod marketplace;
pub(crate) mod mcp;
pub(crate) mod path;
pub(crate) mod plan;
pub(crate) mod preference;
pub(crate) mod process;
pub(crate) mod projection;
pub(crate) mod provision;
pub(crate) mod skill;
pub(crate) mod toml_config;
