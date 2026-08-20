use uze::{
    capability::CapabilityKind,
    integration::IntegrationPort,
    router::{ExposureState, HarnessCapabilities},
};

/// OpenCode's documentation declares `.agents/skills/*/SKILL.md` as a native
/// project skill discovery path. Exposure is kept separate from that declared
/// route until an opt-in probe succeeds against a configured local provider.
pub struct OpenCodeIntegration;

impl IntegrationPort for OpenCodeIntegration {
    fn id(&self) -> &'static str {
        "opencode"
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            direct_standard: [CapabilityKind::AgentSkill].into_iter().collect(),
            exposure: ExposureState::Unverified,
            evidence: "OpenCode documentation declares .agents/skills/*/SKILL.md as a native project skill discovery path; local real-harness conformance is pending."
                .to_owned(),
            ..HarnessCapabilities::default()
        }
    }
}
