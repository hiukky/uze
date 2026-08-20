use uze::{
    capability::CapabilityKind,
    exposure::{ExposureMechanism, ExposurePlan},
    integration::IntegrationPort,
    project::Resource,
    router::{CompatibilityRoute, HarnessCapabilities, VerificationStatus},
};

pub struct ClaudeIntegration;

impl IntegrationPort for ClaudeIntegration {
    fn id(&self) -> &'static str {
        "claude-code"
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            adaptable: [CapabilityKind::AgentSkill].into_iter().collect(),
            verification: VerificationStatus::Unverified,
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
            verification: VerificationStatus::Unverified,
            mechanism: ExposureMechanism::RuntimeBridge {
                bridge: "Claude Code --plugin-dir".to_owned(),
                arguments: vec!["--plugin-dir".to_owned(), package_root.display().to_string()],
            },
            evidence: "Per-session --plugin-dir is a conformance exposure probe, not transparent normal Claude invocation. A supported globally installed UZE connector has not yet been proven."
                .to_owned(),
        }
    }
}

fn unsupported(resource: &Resource, rationale: &str) -> ExposurePlan {
    ExposurePlan {
        representation: resource.capability.representation,
        route: CompatibilityRoute::Unsupported,
        verification: VerificationStatus::NotExposed,
        mechanism: ExposureMechanism::Unsupported {
            rationale: rationale.to_owned(),
        },
        evidence: rationale.to_owned(),
    }
}
