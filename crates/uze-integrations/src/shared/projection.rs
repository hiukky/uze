//! The one report of a shared-root projection conflict.
//!
//! The `~/.agents/skills` directory belongs to no single harness, so the
//! conflict it raises belongs to none either: an entry already owned by a
//! peer's artifact that cannot carry the asking integration's invocation
//! encoding is the same fact whichever integration asks.

use std::path::Path;

use uze_core::{UzeError, project::Resource};

/// Deterministic, pre-attach projection conflict: the shared
/// `~/.agents/skills` entry this resource would reuse is already owned by
/// another integration's artifact that cannot preserve `integration`'s
/// invocation encoding (ADR-030 §25 — never degrade silently).
pub(crate) fn conflict(
    resource: &Resource,
    entry: &Path,
    reused_target: &Path,
    requirement: &str,
    integration: &str,
) -> UzeError {
    let requested_target = resource
        .capability
        .path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| resource.capability.path.clone());
    UzeError::ProjectionConflict(Box::new(uze_core::error::ProjectionConflictDetails {
        entry: entry.to_path_buf(),
        requested: format!("{} ({requirement})", resource.identity()),
        requested_integration: integration.to_owned(),
        requested_target,
        existing: format!("{} ({requirement})", resource.identity()),
        existing_integration: "shared-root owner".to_owned(),
        existing_target: reused_target.to_path_buf(),
    }))
}
