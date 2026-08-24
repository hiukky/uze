//! Acceptance suite (L3): the real `uze` binary, exercised through its
//! public CLI, inside a fully isolated `TestEnvironment`.
//!
//! Rule (tests/README.md): an acceptance test walks the public path —
//! `uze` binary → Application → Store/Engine → Integration — never internal
//! methods, and never against the developer's real HOME/UZE_HOME/PATH.

mod fresh_project;
mod lifecycle;
mod multi_harness;
mod runtime_shim;
mod util;
mod workspace_health;
