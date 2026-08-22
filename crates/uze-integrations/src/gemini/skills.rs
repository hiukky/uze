//! Gemini CLI Agent Skill exposure. Gemini documents `~/.agents/skills` as
//! one of its own native skill discovery roots, so a UZE-managed reference
//! there is consumed natively — no adaptation mechanism is needed.

use uze_core::{
    exposure::{ExposureMechanism, ExposurePlan},
    integration::IntegrationPort,
    project::Resource,
    router::{CompatibilityRoute, VerificationStatus},
    state,
};

use super::GeminiIntegration;
use super::unsupported;

impl GeminiIntegration {
    pub(super) fn skill_exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        if state::is_installed(&self.uze_home, self.id())
            && let Some(entry_name) = resource
                .resolved_exposure_name
                .clone()
                .or_else(|| self.exposure_name_candidates(resource).into_iter().next())
        {
            let skill_directory = resource
                .capability
                .path
                .parent()
                .expect("SKILL.md has a parent");
            return ExposurePlan {
                representation: resource.capability.representation,
                // Gemini documents `~/.agents/skills` as one of its own skill
                // discovery roots, so a UZE-managed reference there is
                // consumed natively rather than adapted.
                route: CompatibilityRoute::Native,
                verification: VerificationStatus::Unverified,
                mechanism: ExposureMechanism::ManagedUserScopeReference {
                    discovery_root: self.skills_dir.clone(),
                    entry_name,
                    source: skill_directory.to_path_buf(),
                },
                evidence: "Gemini CLI discovers Agent Skills from the shared `~/.agents/skills` root, so UZE reuses the same managed reference it places for its other peers. SKILL.md remains the preserved standard payload in the UZE store."
                    .to_owned(),
            };
        }
        unsupported(
            resource,
            "Gemini setup has not completed, so no managed skill reference exists yet.",
        )
    }
}
