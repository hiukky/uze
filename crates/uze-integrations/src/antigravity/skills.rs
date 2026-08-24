//! Antigravity CLI Agent Skill exposure. The CLI's documented global
//! skills root is `~/.gemini/antigravity-cli/skills/` ("any markdown skill
//! in this directory is automatically imported as a global slash command
//! whenever you launch agy in any directory" — official CLI docs; the
//! binary's own builtin skills live beside it under
//! `~/.gemini/antigravity-cli/builtin/skills/`), so a UZE-managed
//! reference there is consumed natively — no adaptation mechanism needed.

use uze_core::{
    exposure::{ExposureMechanism, ExposurePlan},
    integration::IntegrationPort,
    project::Resource,
    router::{CompatibilityRoute, VerificationStatus},
    state,
};

use super::AntigravityIntegration;
use super::unsupported;

impl AntigravityIntegration {
    pub(super) fn skill_exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        if state::is_installed(&self.uze_home, self.id()) {
            let skill_directory = resource
                .capability
                .path
                .parent()
                .expect("SKILL.md has a parent");
            let entry_name = resource
                .resolved_exposure_name
                .clone()
                .or_else(|| self.exposure_name_candidates(resource).into_iter().next());
            if let Some(entry_name) = entry_name {
                return ExposurePlan {
                    representation: resource.capability.representation,
                    route: CompatibilityRoute::Native,
                    verification: VerificationStatus::Unverified,
                    mechanism: ExposureMechanism::ManagedUserScopeReference {
                        discovery_root: self.skills_dir.clone(),
                        entry_name,
                        source: skill_directory.to_path_buf(),
                    },
                    evidence: "Antigravity CLI imports every markdown skill under ~/.gemini/antigravity-cli/skills as a global slash command, so a UZE-managed reference there is consumed natively. The wrapper (see commands.rs) carries the stable namespaced label and the canonical name/description/body."
                        .to_owned(),
                };
            }
        }
        unsupported(
            resource,
            "Antigravity setup has not completed, so no managed skill reference exists yet.",
        )
    }
}
