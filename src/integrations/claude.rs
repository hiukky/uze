use uze::{capability::CapabilityKind, integration::IntegrationPort, router::HarnessCapabilities};

pub struct ClaudeIntegration;

impl IntegrationPort for ClaudeIntegration {
    fn id(&self) -> &'static str {
        "claude-code"
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            direct_standard: [CapabilityKind::AgentSkill].into_iter().collect(),
            evidence: "Claude integration declaration; real-harness conformance is pending."
                .to_owned(),
            ..HarnessCapabilities::default()
        }
    }
}
