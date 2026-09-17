//! Codex Skill exposure — invocation-policy-aware delivery through Codex's
//! native Skill mechanism.
//!
//! Codex has **no custom-command file format**: its historical
//! `~/.codex/prompts/*.md` files are officially deprecated in favor of
//! Skills. What Codex *does* provide first-class is an invocation policy
//! sidecar: `agents/openai.yaml` beside `SKILL.md` with
//! `policy.allow_implicit_invocation: false` (Codex Build skills
//! documentation: *"Codex won't implicitly invoke the skill based on user
//! prompt; explicit `$skill` invocation still works"*), empirically honored
//! by codex-cli 0.149.0 (verified via `codex debug prompt-input`).
//!
//! Per ADR-030, the canonical capability is always a Skill and its
//! semantics are *who may invoke it*:
//!
//! - model+user (default) → plain Skill, Native;
//! - model=false → Skill + `agents/openai.yaml` with
//!   `allow_implicit_invocation: false`, Native (explicit `$skill`
//!   invocation stays the official mechanism);
//! - user=false → **Degraded**: Codex has no documented way to hide a
//!   skill from explicit `$skill` invocation, so model discovery is
//!   preserved and the user-invocation half of the canonical policy cannot
//!   be enforced — classified honestly, never invented;
//! - model=false,user=false → never projected (nobody can invoke it).
//!
//! The generated wrapper is always a Derived Artifact under `$UZE_HOME`,
//! never the Store. Because Codex and OpenCode consume the SAME physical
//! `~/.agents/skills` entry, the wrapper is the superset representation
//! (`crate::shared::skill::write_superset_skill_wrapper`): Codex's policy
//! sidecar *and* OpenCode's own invocation controls, so the entry is
//! correct whichever integration created it — single-harness behavior is
//! unchanged, and the shared entry can never silently degrade into model
//! visibility for either consumer (ADR-030 §25).

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

use super::CodexIntegration;
use crate::shared::skill::{
    SharedRootReader, entry_name, generated_skill_dir, invalid_policy_plan, setup_pending_plan,
    skill_label, skill_wrapper_root, verify_reused_wrapper, write_superset_skill_wrapper,
};

/// Deterministically materializes (or refreshes) one Skill's delivered
/// directory — the shared-root superset representation
/// (`crate::shared::skill::write_superset_skill_wrapper`): `SKILL.md`
/// carrying the stable namespaced invocation label as its `name` with the
/// canonical description/body preserved from the Store, plus
/// `agents/openai.yaml` with the implicit-invocation policy when the
/// canonical `invoke.model` is `false`. Codex derives the model-visible
/// name from frontmatter (verified against codex-cli 0.149.0), so
/// namespacing the directory alone is not enough; the generated wrapper is
/// the only way to show `flow:review` without rewriting the canonical
/// bytes. Codex accepts `:` in skill names (verified against codex-cli
/// 0.149.0). The wrapper also carries OpenCode's own invocation controls
/// because this directory lives in the shared `~/.agents/skills` root —
/// Codex ignores those fields, and OpenCode needs them when it reuses the
/// same physical entry (ADR-030 §25).
pub(super) fn materialize_generated_skill(
    uze_home: &UzeHome,
    resource: &Resource,
) -> Result<PathBuf> {
    let dir = generated_skill_dir(uze_home, "codex", resource);
    let canonical_dir = resource
        .capability
        .path
        .parent()
        .expect("SKILL.md has a parent");
    let label = skill_label(uze_home, resource).unwrap_or_else(|| resource.name());
    write_superset_skill_wrapper(
        &dir,
        canonical_dir,
        &resource.capability.payload,
        &label,
        &resource.skill_invocation(),
    )?;
    Ok(dir)
}

impl CodexIntegration {
    pub(super) fn cleanup_unused_wrapper(&self, target: &Path) -> Result<()> {
        crate::shared::path::cleanup_unused_wrapper(
            target,
            &crate::shared::path::attachment_root(&self.uze_home, "codex"),
            &self.skills_dir,
            &skill_wrapper_root(&self.uze_home, "codex"),
            &|wrapper| wrapper.join("SKILL.md").is_file(),
        )
    }

    /// Materializes this Skill's wrapper when this resource owns the shared
    /// entry; when the shared-root resolution reused another integration's
    /// artifact, that artifact is authoritative and nothing may replace it —
    /// but it must still carry Codex's own encoding of the policy.
    pub(super) fn materialize_or_verify_skill(&self, resource: &Resource) -> Result<()> {
        match &resource.resolved_artifact_target {
            Some(wrapper) => verify_reused_wrapper(
                resource,
                wrapper,
                &self.skills_dir,
                SharedRootReader::Codex,
                self.id(),
            ),
            None => materialize_generated_skill(&self.uze_home, resource).map(|_| ()),
        }
    }

    /// The single Skill route classification for one canonical invocation
    /// policy on Codex:
    ///
    /// - default / user-only → Native (the explicit-only policy sidecar is
    ///   Codex's official mechanism for the model half);
    /// - model-only → Degraded — Codex cannot prevent explicit `$skill`
    ///   invocation, so `user=false` cannot be enforced and must not be
    ///   reported as preserved;
    /// - invalid → Unsupported.
    pub(super) fn skill_exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        let policy = resource.skill_invocation();
        if policy.is_invalid() {
            return invalid_policy_plan();
        }
        if !state::is_installed(&self.uze_home, self.id()) {
            return setup_pending_plan("Codex");
        }
        let Some(entry_name) = entry_name(self, resource) else {
            return setup_pending_plan("Codex");
        };
        let source = resource
            .resolved_artifact_target
            .clone()
            .unwrap_or_else(|| generated_skill_dir(&self.uze_home, "codex", resource));
        let route = if policy.model && !policy.user {
            CompatibilityRoute::Degraded
        } else {
            CompatibilityRoute::Native
        };
        let mut evidence = String::from(
            "UZE materializes a generated wrapper SKILL.md carrying the stable namespaced label as its `name` (Codex derives the model-visible name from frontmatter) and symlinks <agents_home>/skills/<label> once, per Codex's documented USER-scope, symlink-following discovery. The canonical Store bytes are never rewritten.",
        );
        if !policy.model {
            evidence.push_str(
                " The canonical invoke.model=false is translated into Codex's own agents/openai.yaml → policy.allow_implicit_invocation: false (explicit `$skill` invocation still works) — NATIVE.",
            );
        } else if !policy.user {
            evidence.push_str(" Codex has no documented way to disable explicit `$skill` invocation, so the canonical invoke.user=false cannot be enforced — DEGRADED, reported honestly rather than invented.");
        }
        ExposurePlan {
            route,
            mechanism: ExposureMechanism::Managed(ManagedArtifact::SymlinkReference {
                path: self.skills_dir.join(entry_name),
                target: source,
            }),
            evidence,
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
    fn user_only_skill_wrapper_is_superset_and_preserves_body() {
        let root = uze_testkit::temp::scratch("codex-skill");
        let home = UzeHome::at(root.join("uze"));
        let resource = skill_resource(
            "flow",
            "/store/packages/flow/skills/review/SKILL.md",
            b"---\nname: review\ndescription: Review code\ninvoke:\n  model: false\n  user: true\n---\n\nReview this diff.\n",
        );
        let dir = materialize_generated_skill(&home, &resource).unwrap();
        let skill = fs::read_to_string(dir.join("SKILL.md")).unwrap();
        assert!(skill.starts_with("---\nname: flow:review\ndescription: \"Review code\"\n"));
        assert!(skill.ends_with("\nReview this diff.\n"));
        // The explicit-only policy is the semantic load-bearing piece: it
        // keeps the model from auto-selecting a user-only Skill.
        assert_eq!(
            fs::read_to_string(dir.join("agents/openai.yaml")).unwrap(),
            crate::shared::skill::EXPLICIT_ONLY_POLICY_YAML
        );
        // This wrapper lives in the shared root Codex and OpenCode both
        // read, so it must carry OpenCode's own encoding too — Codex
        // ignores the unknown frontmatter field (verified via `codex debug
        // prompt-input`), OpenCode needs it when it reuses the entry.
        assert!(
            skill.contains("metadata:\n  opencode/autoinvoke: false\n"),
            "the shared-root wrapper is the superset: {skill}"
        );
        assert!(
            !skill.contains("slash: false"),
            "user invocation stays enabled"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn default_skill_wrapper_has_no_policy_sidecar() {
        let root = uze_testkit::temp::scratch("codex-skill-def");
        let home = UzeHome::at(root.join("uze"));
        let resource = skill_resource(
            "flow",
            "/store/packages/flow/skills/review/SKILL.md",
            b"---\nname: review\ndescription: Review code\n---\n\nReview this diff.\n",
        );
        let dir = materialize_generated_skill(&home, &resource).unwrap();
        assert!(
            !dir.join("agents/openai.yaml").exists(),
            "a default model+user Skill stays model-discoverable: no policy sidecar"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn adaptation_is_deterministic_across_rebuilds() {
        let root = uze_testkit::temp::scratch("codex-skill-det");
        let home = UzeHome::at(root.join("uze"));
        let resource = skill_resource(
            "flow",
            "/store/packages/flow/skills/review/SKILL.md",
            b"---\ninvoke:\n  model: false\n  user: true\n---\nbody only\n",
        );
        let a = materialize_generated_skill(&home, &resource).unwrap();
        let first_skill = fs::read(a.join("SKILL.md")).unwrap();
        let first_policy = fs::read(a.join("agents/openai.yaml")).unwrap();
        let b = materialize_generated_skill(&home, &resource).unwrap();
        let second_skill = fs::read(b.join("SKILL.md")).unwrap();
        let second_policy = fs::read(b.join("agents/openai.yaml")).unwrap();
        assert_eq!(first_skill, second_skill);
        assert_eq!(first_policy, second_policy);
        let _ = fs::remove_dir_all(root);
    }
}
