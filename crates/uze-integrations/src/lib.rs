//! Vendor-specific peer integrations. This crate depends on `uze-core`; the
//! Core never depends on a named harness.

pub mod claude;
pub mod codex;
pub mod gemini;
pub mod opencode;

mod shared;
