use uze::{
    capability::CapabilityKind,
    exposure::{ExposureMechanism, ExposurePlan},
    integration::IntegrationPort,
    project::Resource,
    router::{CompatibilityRoute, ExposureState, HarnessCapabilities},
};

pub struct ClaudeIntegration;

impl IntegrationPort for ClaudeIntegration {
    fn id(&self) -> &'static str {
        "claude-code"
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            adaptable: [CapabilityKind::AgentSkill].into_iter().collect(),
            exposure: ExposureState::Unverified,
            evidence: "Claude Code can receive a stored Agent Plugin through its explicit per-session --plugin-dir bridge; real UZE integration conformance remains unverified."
                .to_owned(),
            ..HarnessCapabilities::default()
        }
    }

    fn exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        let Some(package_root) = resource.package_root() else {
            return unsupported(
                resource,
                "Claude Code needs a UZE-stored Agent Plugin package for this bridge.",
            );
        };
        if resource.capability.kind != CapabilityKind::AgentSkill {
            return unsupported(
                resource,
                "Claude Code bridge is only modeled for Agent Skills in this PoC.",
            );
        }
        ExposurePlan {
            representation: resource.capability.representation,
            route: CompatibilityRoute::Adaptable,
            exposure: ExposureState::Unverified,
            mechanism: ExposureMechanism::RuntimeBridge {
                bridge: "Claude Code --plugin-dir".to_owned(),
                arguments: vec!["--plugin-dir".to_owned(), package_root.display().to_string()],
            },
            evidence: "Per-session plugin loading is explicit; the STANDARD skill is not assumed to be directly discoverable from UZE_HOME."
                .to_owned(),
        }
    }
}

fn unsupported(resource: &Resource, rationale: &str) -> ExposurePlan {
    ExposurePlan {
        representation: resource.capability.representation,
        route: CompatibilityRoute::Unsupported,
        exposure: ExposureState::NotExposed,
        mechanism: ExposureMechanism::Unsupported {
            rationale: rationale.to_owned(),
        },
        evidence: rationale.to_owned(),
    }
}
