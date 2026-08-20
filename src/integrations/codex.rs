use uze::{
    capability::CapabilityKind,
    exposure::{ExposureMechanism, ExposurePlan},
    integration::IntegrationPort,
    project::Resource,
    router::{CompatibilityRoute, HarnessCapabilities, VerificationStatus},
};

pub struct CodexIntegration;

impl IntegrationPort for CodexIntegration {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            adaptable: [CapabilityKind::AgentSkill].into_iter().collect(),
            verification: VerificationStatus::Unverified,
            evidence: "Codex documents project Agent Skill discovery. UZE currently has only an explicit compatibility projection; successful exposure must be recorded by each real conformance run."
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
            verification: VerificationStatus::Unverified,
            mechanism: ExposureMechanism::FilesystemProjection {
                source: skill_directory.to_path_buf(),
                target_relative: std::path::PathBuf::from(".agents/skills").join(skill_name),
            },
            evidence: "Codex's project .agents/skills discovery is reused only through an explicit UZE-managed projection in the real caller workspace. The project cwd is preserved; no shadow workspace is created."
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
