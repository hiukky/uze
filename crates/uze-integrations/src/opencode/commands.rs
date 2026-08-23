//! OpenCode custom-command exposure: OpenCode V2 natively discovers `.md`
//! command files in user-global `~/.config/opencode/commands/` (project
//! `.opencode/commands/` also exists but is project-scoped — machine-level
//! UZE attachment uses the global scope, the same choice as the shared
//! Agent Skills root). The canonical command file is byte-identical to what
//! OpenCode consumes (markdown body + optional `description` frontmatter),
//! so delivery is a direct standard reference: one UZE-managed symlink per
//! command, named `<command>.md`.
//!
//! Argument placeholders (`$ARGUMENTS`, `$1..$N`) and shell interpolation
//! (`` !`cmd` ``) are OpenCode vendor semantics. UZE never generates shell
//! interpolation; author-provided bodies are preserved verbatim, and a
//! body with no placeholder receives OpenCode's own default argument
//! appending behavior unchanged (ADR-025: no universal placeholder in v0).

use uze_core::{
    exposure::{ExposureMechanism, ExposurePlan},
    integration::IntegrationPort,
    project::Resource,
    router::{CompatibilityRoute, VerificationStatus},
    state,
};

use super::OpenCodeIntegration;
use super::unsupported;

/// OpenCode's own naming decision: the physical file name IS the command
/// name, so the stable namespaced label is dropped into the file name
/// verbatim (`flow:review.md`), never a bare alias and never a
/// collision-dependent qualification (ADR-026). The `.md` extension is a
/// vendor naming constraint, owned here, never in Application.
pub(super) fn opencode_command_exposure_name_candidates(resource: &Resource) -> Vec<String> {
    use uze_core::integration::qualified_exposure_name_candidates;
    qualified_exposure_name_candidates(resource)
        .into_iter()
        .map(|label| format!("{label}.md"))
        .collect()
}

impl OpenCodeIntegration {
    pub(super) fn command_plan(&self, resource: &Resource) -> ExposurePlan {
        let Some(entry_name) = resource
            .resolved_exposure_name
            .clone()
            .or_else(|| self.exposure_name_candidates(resource).into_iter().next())
        else {
            return unsupported(resource, "Resource has no derivable attachment entry name.");
        };
        if !state::is_installed(&self.uze_home, self.id()) {
            return unsupported(
                resource,
                "OpenCode setup has not completed, so no user-scope commands directory exists yet.",
            );
        }
        ExposurePlan {
            representation: resource.capability.representation,
            route: CompatibilityRoute::Native,
            verification: VerificationStatus::Unverified,
            mechanism: ExposureMechanism::ManagedUserScopeReference {
                discovery_root: self.commands_dir.clone(),
                entry_name,
                source: resource.capability.path.clone(),
            },
            evidence: "OpenCode V2 natively discovers .md command files in the user-global commands directory; the UZE-managed reference points at the canonical command file in the Store. Delivery is byte-preserving: no template translation, no generated shell interpolation."
                .to_owned(),
        }
    }
}
