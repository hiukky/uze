//! Gemini CLI custom-command exposure: Gemini natively discovers `.toml`
//! command files in user-global `~/.gemini/commands/` (project
//! `<project>/.gemini/commands/` is project-scoped; extension-internal
//! commands ride along with native extension delivery — see
//! [`super::generate`]). The canonical command file is markdown, so this
//! module generates the vendor TOML — a Derived Artifact (ADR-013 §4) in
//! the user-scope commands directory, never in the Store.
//!
//! Generation is deliberately prompt-only and deterministic:
//!
//! ```toml
//! description = "..."   # from canonical frontmatter, when present
//! prompt = "body with escaped newlines"
//! ```
//!
//! No `{{args}}`, `!{...}`, or `@{...}` is ever generated: those are
//! Gemini vendor semantics and are preserved verbatim only when the author
//! wrote them into the canonical body (which happens to use the same
//! syntax — see `docs/capabilities/commands.md`). UZE never invents shell
//! interpolation (ADR-025 §Security).

use uze_core::{
    exposure::{ExposureMechanism, ExposurePlan},
    integration::IntegrationPort,
    project::Resource,
    router::{CompatibilityRoute, VerificationStatus},
    state,
};

use crate::shared::command::{parse_command_body, toml_escape};

use super::GeminiIntegration;
use super::unsupported;

/// Gemini's own naming decision: bare logical name first (`/review`), then
/// fully package-qualified, with the `.toml` extension that makes the file
/// discoverable — the extension and the `:` namespacing rule are vendor
/// naming constraints, owned here, never in Application.
pub(super) fn gemini_command_exposure_name_candidates(resource: &Resource) -> Vec<String> {
    use uze_core::integration::short_then_qualified_exposure_name_candidates;
    short_then_qualified_exposure_name_candidates(resource)
        .into_iter()
        .map(|name| format!("{name}.toml"))
        .collect()
}

/// Deterministic TOML document for one canonical command, built from the
/// resource's own payload bytes — no disk read, so planning cannot fail
/// and cannot drift from what the Engine discovered.
pub(super) fn generated_toml(resource: &Resource) -> String {
    let (description, body) = parse_command_body(&resource.capability.payload);
    let mut document = String::new();
    if let Some(description) = description {
        document.push_str(&format!(
            "description = \"{}\"\n",
            toml_escape(&description)
        ));
    }
    document.push_str(&format!("prompt = \"{}\"\n", toml_escape(&body)));
    document
}

impl GeminiIntegration {
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
                "Gemini setup has not completed, so no user-scope commands directory exists yet.",
            );
        }
        let target_file = self.commands_dir.join(&entry_name);
        let expected_content = generated_toml(resource);
        ExposurePlan {
            representation: resource.capability.representation,
            route: CompatibilityRoute::Native,
            verification: VerificationStatus::Unverified,
            mechanism: ExposureMechanism::ManagedFile {
                target_file,
                expected_content,
            },
            evidence: "Gemini CLI natively discovers .toml command files in ~/.gemini/commands/. UZE generates the vendor TOML from the canonical markdown command (prompt plus optional description) — a Derived Artifact, never the Store's bytes, with no generated shell/file interpolation."
                .to_owned(),
        }
    }
}
