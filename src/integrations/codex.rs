use uze::{capability::CapabilityKind, integration::IntegrationPort, router::HarnessCapabilities};

pub struct CodexIntegration;

impl IntegrationPort for CodexIntegration {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            direct_standard: [CapabilityKind::AgentSkill].into_iter().collect(),
            evidence: "Codex integration declaration; real-harness conformance is pending."
                .to_owned(),
            ..HarnessCapabilities::default()
        }
    }
}
