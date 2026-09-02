//! The `uze` binary's library half: the terminal UI.
//!
//! It carried a compatibility facade re-exporting all of `uze-core` and
//! `uze-integrations`, which let the CLI, the TUI and the test suite reach
//! the domain without naming it — the one hole the layering rule in
//! `tests/architecture/layering.rs` could not see, since a reach written as
//! `uze_application::UzeHome` names no forbidden path. Callers now name the crate
//! they mean.

pub mod ui;
