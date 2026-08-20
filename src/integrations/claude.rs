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
            exposure: ExposureState::NotExposed,
            evidence: "Claude Code 2.1.237 conformance on 2026-08-20 did not expose the Agent Skill from .agents/skills without a vendor-specific path."
                .to_owned(),
            ..HarnessCapabilities::default()
        }
    }
}
