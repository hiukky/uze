use std::collections::BTreeSet;

use serde::Serialize;

use crate::capability::{Capability, CapabilityKind, Representation};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CompatibilityRoute {
    Native,
    Adaptable,
    Degraded,
    Unsupported,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExposureState {
    Available,
    NotExposed,
    Verified,
    #[default]
    Unverified,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HarnessCapabilities {
    pub direct_standard: BTreeSet<CapabilityKind>,
    pub native: BTreeSet<CapabilityKind>,
    pub adaptable: BTreeSet<CapabilityKind>,
    pub degraded: BTreeSet<CapabilityKind>,
    pub exposure: ExposureState,
    pub evidence: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RouteDecision {
    pub route: CompatibilityRoute,
    pub exposure: ExposureState,
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
        exposure: harness.exposure,
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
        assert_eq!(decision.exposure, ExposureState::Unverified);
    }
}
