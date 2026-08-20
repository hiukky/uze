use uze::{
    capability::CapabilityKind,
    exposure::{ExposureMechanism, ExposurePlan},
    integration::IntegrationPort,
    project::Resource,
    router::{CompatibilityRoute, ExposureState, HarnessCapabilities},
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
            adaptable: [CapabilityKind::AgentSkill].into_iter().collect(),
            exposure: ExposureState::Unverified,
            evidence: "OpenCode documents native .agents/skills discovery; UZE store exposure would use an explicit runtime filesystem fallback and remains unverified."
                .to_owned(),
            ..HarnessCapabilities::default()
        }
    }

    fn exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        if resource.capability.kind != CapabilityKind::AgentSkill
            || resource.package_root().is_none()
        {
            return unsupported(
                resource,
                "OpenCode filesystem fallback is only modeled for UZE-stored Agent Skills.",
            );
        }
        let skill_directory = resource
            .capability
            .path
            .parent()
            .expect("SKILL.md has a parent");
        let skill_name = skill_directory
            .file_name()
            .expect("skill directory has a name");
        ExposurePlan {
            representation: resource.capability.representation,
            route: CompatibilityRoute::Adaptable,
            exposure: ExposureState::Unverified,
            mechanism: ExposureMechanism::FilesystemProjection {
                source: skill_directory.to_path_buf(),
                target_relative: std::path::PathBuf::from(".agents/skills").join(skill_name),
            },
            evidence: "OpenCode's native discovery path is used only through an explicit session-scoped UZE runtime projection."
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
