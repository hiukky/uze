//! Capability compatibility vocabulary without named harness rules.
//!
//! # What "Native" means (ADR-030)
//!
//! A route is **Native** when the harness provides a first-class,
//! officially supported mechanism that preserves the *canonical semantics*
//! of the capability — **not** when the vendor name, file format, or
//! physical primitive happens to match another harness's. Two harnesses
//! may implement one canonical capability under different names; both are
//! Native if each preserves the semantics through a supported primitive.
//! UZE models user-visible semantics, never one-to-one vendor type names:
//! the canonical capability is always the Skill, and its semantics are
//! *who may invoke it* (invocation policy, ADR-030). A user-only Skill
//! reaches Claude as a Skill with `disable-model-invocation: true`,
//! Codex as a Skill with an `agents/openai.yaml` policy sidecar, OpenCode
//! as a Skill with `metadata.opencode/autoinvoke: false`, and Antigravity
//! as an ordinary Skill whose model visibility cannot be disabled — the
//! first three are Native for that policy, the fourth is `Adaptable`
//! because the semantics degrade. (A route that must emulate or degrade
//! through a mechanism the harness does not intend for that capability is
//! `Adaptable`; see `CompatibilityRoute`.)

use std::collections::BTreeSet;

use serde::Serialize;

use crate::capability::CapabilityKind;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CompatibilityRoute {
    /// The harness offers a first-class, officially supported mechanism that
    /// preserves the canonical capability semantics — regardless of whether
    /// the vendor calls it by the same name or uses the same file format as
    /// another harness (ADR-030).
    Native,
    /// UZE must emulate or degrade semantics through a mechanism the
    /// harness does not intend for this capability.
    Adaptable,
    /// Core semantics are preserved only partially.
    Degraded,
    /// No safe route; the harness has nothing equivalent.
    Unsupported,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct HarnessCapabilities {
    pub native: BTreeSet<CapabilityKind>,
    pub adaptable: BTreeSet<CapabilityKind>,
    pub degraded: BTreeSet<CapabilityKind>,
    pub evidence: String,
}
