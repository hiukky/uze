//! Capability compatibility routing without named harness rules.
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

use crate::capability::{Capability, CapabilityKind, Representation};

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

/// Evidence from an actual exposure attempt. This is intentionally distinct
/// from representation and compatibility: an external quota or an absent
/// executable cannot demonstrate that a capability is unsupported.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum VerificationStatus {
    #[default]
    Unverified,
    NotExposed,
    Verified,
    Failed {
        reason: String,
    },
    BlockedByEnvironment {
        reason: String,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct HarnessCapabilities {
    pub direct_standard: BTreeSet<CapabilityKind>,
    pub native: BTreeSet<CapabilityKind>,
    pub adaptable: BTreeSet<CapabilityKind>,
    pub degraded: BTreeSet<CapabilityKind>,
    pub verification: VerificationStatus,
    pub evidence: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RouteDecision {
    pub route: CompatibilityRoute,
    pub verification: VerificationStatus,
    pub rationale: String,
    pub evidence: String,
}

pub fn route(capability: &Capability, harness: &HarnessCapabilities) -> RouteDecision {
    let (route, rationale) = if capability.representation == Representation::Standard
        && harness.direct_standard.contains(&capability.kind)
    {
        (
            CompatibilityRoute::Native,
            "The integration declares direct consumption of this standard representation."
                .to_owned(),
        )
    } else if harness.native.contains(&capability.kind) {
        (
            CompatibilityRoute::Native,
            "The integration declares native support for this capability kind.".to_owned(),
        )
    } else if harness.adaptable.contains(&capability.kind) {
        (
            CompatibilityRoute::Adaptable,
            "The integration declares an explicit adapter for this capability kind.".to_owned(),
        )
    } else if harness.degraded.contains(&capability.kind) {
        (
            CompatibilityRoute::Degraded,
            "The integration declares partial semantics for this capability kind.".to_owned(),
        )
    } else {
        (
            CompatibilityRoute::Unsupported,
            "No verified route was supplied by the integration.".to_owned(),
        )
    };

    RouteDecision {
        route,
        verification: harness.verification.clone(),
        rationale,
        evidence: harness.evidence.clone(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn routes_a_standard_skill_without_knowing_a_harness_name() {
        let capability = Capability {
            kind: CapabilityKind::AgentSkill,
            representation: Representation::Standard,
            path: PathBuf::from("SKILL.md"),
            payload: Vec::new(),
        };
        let capabilities = HarnessCapabilities {
            direct_standard: [CapabilityKind::AgentSkill].into_iter().collect(),
            evidence: "fake integration evidence".to_owned(),
            ..HarnessCapabilities::default()
        };

        let decision = route(&capability, &capabilities);
        assert_eq!(decision.route, CompatibilityRoute::Native);
        assert_eq!(decision.verification, VerificationStatus::Unverified);
    }
}
