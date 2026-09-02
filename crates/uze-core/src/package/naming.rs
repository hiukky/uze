//! Plugin name collision boundary (ADR-038): the question `ingest_with_active_name`
//! made necessary once a bare plugin name became something only one
//! marketplace-qualified identity may actively claim at a time — *whose*
//! decision is it when a second one wants the same name?
//!
//! Mirrors `trust`'s shape exactly, for the same reason: the Application
//! asks, it never decides, and every front end (CLI now, TUI later) asks the
//! same question from the same facts instead of reimplementing the
//! judgement.

use serde::Serialize;

/// What the operator is being asked to resolve.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct NameCollisionRequest {
    /// The bare local name both plugins want (`git`).
    pub name: String,
    /// The marketplace-qualified identity already active under `name`.
    pub existing: String,
    /// The marketplace-qualified identity being installed.
    pub requested: String,
}

/// The decision. `Abort` is not a failure to answer — see
/// [`NoNameCollisionAuthority`] for that — it is a considered "keep what's
/// already there."
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NameCollisionResolution {
    /// Keep the existing active plugin; refuse this install.
    Abort,
    /// Detach and remove the existing active plugin first (once that is
    /// proven safe — the same reconciliation rule `remove_plugin` enforces),
    /// then let the new install claim the bare name.
    Replace,
    /// Give the new install this explicit local name instead of its bare
    /// plugin name, so both stay active side by side.
    Alias(String),
}

/// Whoever can answer the question.
pub trait NameCollisionAuthority {
    fn resolve(&self, request: &NameCollisionRequest) -> NameCollisionResolution;
}

/// Refuses without asking — the correct authority for a non-interactive
/// process, and the default every existing install call site keeps using
/// unless it opts into resolution. A collision then surfaces as the same
/// structured `PluginNameCollision` error it always would with nobody to
/// ask, never a silent shadowing.
pub struct NoNameCollisionAuthority;

impl NameCollisionAuthority for NoNameCollisionAuthority {
    fn resolve(&self, _request: &NameCollisionRequest) -> NameCollisionResolution {
        NameCollisionResolution::Abort
    }
}

/// Answers with a resolution already decided out of band — an explicit
/// `--replace`/`--alias` flag, or a resolution already picked by a prior
/// interactive answer.
pub struct FixedResolution(pub NameCollisionResolution);

impl NameCollisionAuthority for FixedResolution {
    fn resolve(&self, _request: &NameCollisionRequest) -> NameCollisionResolution {
        self.0.clone()
    }
}
