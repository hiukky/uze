use uze::{
    capability::CapabilityKind,
    integration::IntegrationPort,
    router::{ExposureState, HarnessCapabilities},
};

pub struct CodexIntegration;

impl IntegrationPort for CodexIntegration {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            direct_standard: [CapabilityKind::AgentSkill].into_iter().collect(),
            exposure: ExposureState::Verified,
            evidence: "Codex CLI 0.148.0 conformance on 2026-08-20 activated the Agent Skill from .agents/skills."
                .to_owned(),
            ..HarnessCapabilities::default()
        }
    }
}
