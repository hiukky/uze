use uze::{
    capability::CapabilityKind,
    exposure::{ExposureMechanism, ExposurePlan},
    integration::IntegrationPort,
    project::Resource,
    router::{CompatibilityRoute, ExposureState, HarnessCapabilities},
};

pub struct CodexIntegration;

impl IntegrationPort for CodexIntegration {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            adaptable: [CapabilityKind::AgentSkill].into_iter().collect(),
            exposure: ExposureState::Verified,
            evidence: "Codex CLI conformance verified the UZE store → explicit session-scoped runtime projection → harness path for one Agent Skill. Native .agents/skills discovery is tracked separately."
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
                "Codex filesystem fallback is only modeled for UZE-stored Agent Skills.",
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
            exposure: ExposureState::Verified,
            mechanism: ExposureMechanism::FilesystemProjection {
                source: skill_directory.to_path_buf(),
                target_relative: std::path::PathBuf::from(".agents/skills").join(skill_name),
            },
            evidence: "Codex's native .agents/skills discovery is reused only through an explicit session-scoped projection under UZE_HOME/runtime."
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
