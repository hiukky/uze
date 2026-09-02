//! Genuinely vendor-neutral helpers shared between peer integrations.
//!
//! **Scope discipline**: this module holds only logic proven, by direct
//! comparison, to be byte-for-byte identical (modulo an injected label)
//! between two or more integrations — never a home for "looks similar."
//! A candidate that turns out to diverge in real behavior stays duplicated,
//! one copy per vendor, rather than being forced together (see the
//! Integration Capability Contracts Audit this module's one member came
//! from: `provision_cli` was byte-identical between Claude and Codex; the
//! `..`/absolute-path normalization each integration's own coverage
//! function does was NOT — Codex's version correctly rejects a
//! leading-`/` path as absolute, Claude's strips the leading `/` first and
//! lets it through as a relative path — so that one stays vendor-specific
//! by design, not merged here.
//!
//! Not part of this crate's public API: every integration composition root
//! (`claude.rs`, `codex.rs`, `opencode.rs`, `antigravity.rs`) is still the
//! only thing `uze-application` or any downstream crate ever names.

pub(crate) mod json_config;
pub(crate) mod path;
pub(crate) mod preference;
pub(crate) mod process;
pub(crate) mod provision;
pub(crate) mod skill;
pub(crate) mod toml_config;
