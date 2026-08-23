//! Codex Agent Skill exposure. Codex documents a cwd-independent USER-scope
//! Agent Skill directory (`<agents_home>/skills`) that follows symlinks, but
//! the user-facing/model-visible name comes from the SKILL.md frontmatter
//! `name` — verified against codex-cli 0.149.0 (`codex debug prompt-input`:
//! a directory named `flow:review` whose SKILL.md says `name: review` is
//! listed as `review`, with the path only in the source locator). To expose
//! the stable namespaced label `flow:review` without rewriting canonical
//! Store bytes, UZE materializes a generated wrapper SKILL.md (same
//! name/description/body, `name` = label) under `$UZE_HOME` and symlinks
//! `~/.agents/skills/flow:review` at it — the same Derived-Artifact
//! discipline as Command delivery, minus the explicit-only policy (a Skill
//! stays model-discoverable).

use std::path::PathBuf;

use uze_core::{
    exposure::{ExposureMechanism, ExposurePlan},
    integration::IntegrationPort,
    project::Resource,
    router::{CompatibilityRoute, VerificationStatus},
    state,
};

use super::CodexIntegration;
use super::commands::{codex_command_exposure_name_candidates, generated_artifact_dir};

impl CodexIntegration {
    pub(super) fn skill_exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        if state::is_installed(&self.uze_home, self.id())
            && let Some(entry_name) = resource
                .resolved_exposure_name
                .clone()
                .or_else(|| self.exposure_name_candidates(resource).into_iter().next())
        {
            let source = resource
                .resolved_artifact_target
                .clone()
                .unwrap_or_else(|| generated_artifact_dir(&self.uze_home, resource));
            return ExposurePlan {
                representation: resource.capability.representation,
                route: CompatibilityRoute::Native,
                verification: VerificationStatus::Unverified,
                mechanism: ExposureMechanism::ManagedUserScopeReference {
                    discovery_root: self.skills_dir.clone(),
                    entry_name,
                    source,
                },
                evidence: "UZE materializes a generated wrapper SKILL.md carrying the stable namespaced label as its `name` (Codex derives the model-visible name from frontmatter) and symlinks <agents_home>/skills/<label> once, per Codex's documented USER-scope, symlink-following discovery. The canonical Store bytes are never rewritten; the Skill stays model-discoverable with no explicit-only policy."
                    .to_owned(),
            };
        }
        let skill_directory = resource
            .capability
            .path
            .parent()
            .expect("SKILL.md has a parent");
        let label = codex_command_exposure_name_candidates(resource)
            .first()
            .cloned()
            .unwrap_or_else(|| {
                skill_directory
                    .file_name()
                    .expect("skill directory has a name")
                    .to_string_lossy()
                    .into_owned()
            });
        ExposurePlan {
            representation: resource.capability.representation,
            route: CompatibilityRoute::Adaptable,
            verification: VerificationStatus::Unverified,
            mechanism: ExposureMechanism::FilesystemProjection {
                source: skill_directory.to_path_buf(),
                target_relative: PathBuf::from(".agents/skills").join(label),
            },
            evidence: "Codex has not completed `uze setup`; falling back to the per-session managed projection in the caller workspace rather than a persistent user-scope attachment."
                .to_owned(),
        }
    }
}
