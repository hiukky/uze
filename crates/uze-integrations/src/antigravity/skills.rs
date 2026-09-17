//! Antigravity CLI Agent Skill exposure, invocation-policy-aware.
//!
//! The CLI's documented global skills root is
//! `~/.gemini/antigravity-cli/skills/` ("any markdown skill in this
//! directory is automatically imported as a global slash command whenever
//! you launch agy in any directory" — official CLI docs; the binary's own
//! builtin skills live beside it under
//! `~/.gemini/antigravity-cli/builtin/skills/`), so a UZE-managed reference
//! there is consumed natively.
//!
//! Invocation-policy reality (agy 1.1.27): both halves are controls the
//! CLI reads from a Skill's own front matter — `disable-slash-command:
//! true` hides it from `/` and `/name`, `disable-model-invocation: true`
//! withholds it from the model. Per ADR-030 this yields:
//!
//! - model+user (default) → Native;
//! - user-only (`model=false`) → Native via `disable-model-invocation: true`;
//! - model-only (`user=false`) → Native via `disable-slash-command: true`;
//! - invalid (`model=false,user=false`) → never projected.
//!
//! The user-only half was ADAPTED until 1.1.27: through 1.1.21 the CLI had
//! no inverse of `disable-slash-command`, so a user-invocable Skill stayed
//! model-discoverable and UZE said so rather than inventing the control.
//! The vendor has since shipped it (`DisableModelInvocation
//! yaml:"disable-model-invocation"`), and the Lab measures the promotion
//! rather than trusting this comment: `user-only-skill-hidden` asserts the
//! Skill is absent from the request the harness actually sends.
//!
//! The generated wrapper carries the stable namespaced label as its front
//! matter `name` (agy derives the invoked name from the SKILL.md front
//! matter, verified against 1.1.19) and the canonical description/body
//! verbatim. Always a Derived Artifact under `$UZE_HOME`, never the Store.

use std::path::{Path, PathBuf};

use uze_core::{
    Result,
    capability::Resource,
    exposure::{ExposureMechanism, ExposurePlan, ManagedArtifact},
    home::UzeHome,
    integration::IntegrationPort,
    router::CompatibilityRoute,
    state,
};

use super::AntigravityIntegration;
use crate::shared::skill::{
    entry_name, generated_skill_dir, invalid_policy_plan, recreate_dir, render_skill_wrapper,
    skill_label, skill_wrapper_root, write_file,
};

/// Deterministically materializes (or refreshes) one Skill's delivered
/// wrapper: `SKILL.md` carrying the stable namespaced label as its `name`
/// and the canonical description/body preserved — so the model-visible and
/// slash-invocable name is `flow:review`, never a bare alias or a
/// collision-prone `review` (the vendor derives the identity from front
/// matter, and `agy plugin validate` accepts `:` in skill names, verified
/// against 1.1.19). Idempotent and rebuilt wholesale — the directory is
/// entirely UZE-owned and non-authoritative (ADR-013 §5).
pub(super) fn materialize_generated_skill(
    uze_home: &UzeHome,
    resource: &Resource,
) -> Result<PathBuf> {
    let dir = generated_skill_dir(uze_home, "antigravity", resource);
    recreate_dir(&dir)?;
    let label = skill_label(uze_home, resource).unwrap_or_else(|| resource.name());
    let policy = resource.skill_invocation();
    let mut markers = Vec::new();
    if !policy.user {
        markers.push("disable-slash-command: true");
    }
    if !policy.model {
        markers.push("disable-model-invocation: true");
    }
    write_file(
        &dir.join("SKILL.md"),
        render_skill_wrapper(&label, &resource.capability.payload, &markers).as_bytes(),
    )?;
    Ok(dir)
}

impl AntigravityIntegration {
    /// Removes a generated wrapper directory once nothing references it —
    /// called when a resource leaves the managed skills root.
    pub(super) fn cleanup_unused_wrapper(&self, target: &Path) -> Result<()> {
        crate::shared::path::cleanup_unused_wrapper(
            target,
            &crate::shared::path::attachment_root(&self.uze_home, "antigravity"),
            &self.skills_dir,
            &skill_wrapper_root(&self.uze_home, "antigravity"),
            &|wrapper| wrapper.join("SKILL.md").is_file(),
        )
    }

    pub(super) fn skill_exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        let policy = resource.skill_invocation();
        if policy.is_invalid() {
            return invalid_policy_plan();
        }
        if state::is_installed(&self.uze_home, self.id())
            && let Some(entry_name) = entry_name(self, resource)
        {
            let source = resource
                .resolved_artifact_target
                .clone()
                .unwrap_or_else(|| generated_skill_dir(&self.uze_home, "antigravity", resource));
            let evidence = if policy.is_default() {
                "Antigravity CLI imports every markdown skill under ~/.gemini/antigravity-cli/skills as a global slash command, so a UZE-managed reference there is consumed natively. The generated wrapper carries the stable namespaced label and the canonical name/description/body."
            } else if !policy.model {
                "Antigravity natively preserves invoke.model=false with disable-model-invocation: true: the Skill stays `/`-invocable while the model is not offered it."
            } else {
                "Antigravity natively preserves invoke.user=false with disable-slash-command: true: the Skill remains model-discoverable while `/` and `/name` resolution omit it."
            };
            return ExposurePlan {
                route: CompatibilityRoute::Native,
                mechanism: ExposureMechanism::Managed(ManagedArtifact::SymlinkReference {
                    path: self.skills_dir.join(entry_name),
                    target: source,
                }),
                evidence: evidence.to_owned(),
            };
        }
        ExposurePlan {
            route: CompatibilityRoute::Adaptable,
            mechanism: ExposureMechanism::Unsupported {
                rationale: "Antigravity setup has not completed, so there is no global skills root for the Skill delivery yet."
                    .to_owned(),
            },
            evidence: "Antigravity has not completed `uze setup`; the generated-Skill delivery needs the managed global skills root."
                .to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use uze_core::capability::{Capability, CapabilityKind};
    use uze_core::store::PackageId;

    fn skill_resource(package_id: &str, path_string: &str, payload: &[u8]) -> Resource {
        let id = PackageId::from_plugin_name(package_id, Path::new("plugin.json")).unwrap();
        Resource::from_package(
            id,
            PathBuf::from("/store/packages").join(package_id),
            Capability {
                kind: CapabilityKind::AgentSkill,
                path: PathBuf::from(path_string),
                payload: payload.to_vec(),
            },
        )
    }

    #[test]
    fn generated_skill_preserves_body_and_carries_the_stable_label() {
        let root = uze_testkit::temp::scratch("antigravity-skill");
        let home = UzeHome::at(root.join("uze"));
        let resource = skill_resource(
            "flow",
            "/store/packages/flow/skills/review/SKILL.md",
            b"---\nname: review\ndescription: Review code\n---\n\nReview this diff.\n",
        );
        let dir = materialize_generated_skill(&home, &resource).unwrap();
        let skill = fs::read_to_string(dir.join("SKILL.md")).unwrap();
        assert!(skill.starts_with("---\nname: flow:review\ndescription: \"Review code\"\n---\n"));
        assert!(skill.ends_with("\nReview this diff.\n"));
        assert!(
            !dir.join("agents/openai.yaml").exists(),
            "Antigravity has no policy sidecar; the wrapper carries only SKILL.md"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn adaptation_is_deterministic_across_rebuilds() {
        let root = uze_testkit::temp::scratch("antigravity-skill-det");
        let home = UzeHome::at(root.join("uze"));
        let resource = skill_resource(
            "flow",
            "/store/packages/flow/skills/review/SKILL.md",
            b"body only\n",
        );
        let a = materialize_generated_skill(&home, &resource).unwrap();
        let first = fs::read(a.join("SKILL.md")).unwrap();
        let b = materialize_generated_skill(&home, &resource).unwrap();
        let second = fs::read(b.join("SKILL.md")).unwrap();
        assert_eq!(first, second);
        let _ = fs::remove_dir_all(root);
    }
}
