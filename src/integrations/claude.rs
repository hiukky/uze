use uze::{
    integration::IntegrationPort,
    router::{ExposureState, HarnessCapabilities},
};

pub struct ClaudeIntegration;

impl IntegrationPort for ClaudeIntegration {
    fn id(&self) -> &'static str {
        "claude-code"
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            exposure: ExposureState::Unverified,
            evidence: "Claude Code Agent Skill exposure from .agents/skills remains unverified: the latest opt-in probe was blocked by an API session-limit response and therefore produced no conformance evidence."
                .to_owned(),
            ..HarnessCapabilities::default()
        }
    }
}
