//! OpenCode Agent Skill exposure — natively discovers a UZE-managed symlink
//! at `~/.agents/skills`, the same shared root Codex and Gemini CLI use.

use std::path::{Path, PathBuf};

use uze_core::{
    exposure::{ExposureMechanism, ExposurePlan},
    integration::IntegrationPort,
    project::Resource,
    router::{CompatibilityRoute, VerificationStatus},
    state,
};

use super::OpenCodeIntegration;
use super::unsupported;

impl OpenCodeIntegration {
    pub(super) fn skill_plan(&self, resource: &Resource) -> ExposurePlan {
        let Some(entry_name) = resource
            .resolved_exposure_name
            .clone()
            .or_else(|| self.exposure_name_candidates(resource).into_iter().next())
        else {
            return unsupported(resource, "Resource has no derivable attachment entry name.");
        };
        let source = resource
            .capability
            .path
            .parent()
            .expect("SKILL.md has a parent")
            .to_path_buf();
        if state::is_installed(&self.uze_home, self.id()) {
            return ExposurePlan {
                representation: resource.capability.representation,
                route: CompatibilityRoute::Native,
                verification: VerificationStatus::Unverified,
                mechanism: ExposureMechanism::ManagedUserScopeReference {
                    discovery_root: self.skills_dir.clone(),
                    entry_name,
                    source,
                },
                evidence: "OpenCode natively discovers the UZE-managed symlink in ~/.agents/skills. The symlink is delivery only; SKILL.md remains the preserved standard payload in the UZE store."
                    .to_owned(),
            };
        }
        ExposurePlan {
            representation: resource.capability.representation,
            route: CompatibilityRoute::Adaptable,
            verification: VerificationStatus::Unverified,
            mechanism: ExposureMechanism::FilesystemProjection {
                source,
                target_relative: PathBuf::from(".agents/skills").join(
                    resource
                        .capability
                        .path
                        .parent()
                        .and_then(Path::file_name)
                        .expect("skill dir name"),
                ),
            },
            evidence: "OpenCode setup has not completed; the existing project-scope projection remains a conformance fallback."
                .to_owned(),
        }
    }
}
