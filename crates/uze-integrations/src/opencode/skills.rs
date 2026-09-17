//! OpenCode Agent Skill exposure — invocation-policy-aware delivery through
//! OpenCode's native Skill mechanism.
//!
//! OpenCode discovers Skills natively at `~/.agents/skills` (the shared
//! root it shares with Codex), and its SKILL.md frontmatter natively
//! expresses both halves of the canonical invocation policy:
//!
//! - `metadata.opencode/autoinvoke: false` omits the skill from
//!   model-facing discovery while it stays registered and explicitly
//!   activatable by ID (model=false preserved — documented, and this
//!   wrapper's exact syntax is the documented `metadata: { opencode/autoinvoke: <bool> }`
//!   shape);
//! - `slash: false` hides the skill from the `/` command catalog — which
//!   is *not* the whole of user invocation on V2, see below.
//!
//! A canonical user-only Skill is projected as an OpenCode **Skill**,
//! never as a vendor Command — the vendor Command primitive remains a
//! projection detail UZE does not need for this harness (ADR-030 §9).
//!
//! # `invoke.user: false` degrades, and says so (measured 2026-09-06)
//!
//! V2 has two explicit-invocation paths, and `slash` gates one of them.
//! A Skill is `/id` when `slash === true`, and `@id` — a mention —
//! always: the picker renders every discovered Skill as `"@" + id`, and
//! `SessionPrompt.prepare` expands a mentioned Skill's body into the user
//! message whatever its `slash` value. So `slash: false` removes a Skill
//! from the `/` catalog and leaves it invocable by mention.
//!
//! This was `Native` here until the Lab learned to *invoke* rather than to
//! read a catalog: `skill-model-only-is-not-invocable` types `@flow:analyze`
//! and finds its body in the request. The claim was never wrong on purpose
//! — nothing measured it. It is now Adaptable, with the degradation
//! stated, and the Lab holds it there.
//!
//! UZE materializes every Skill as one wrapper SKILL.md under `$UZE_HOME` —
//! loading the canonical name/description/body, never rewriting the Store
//! — and symlinks the shared root entry at it. OpenCode uses `name` as the
//! visible label, so direct Store links would lose the stable qualified
//! label whenever the canonical skill has a bare name. Because Codex reads the
//! SAME physical entry from `~/.agents/skills`, the wrapper is the superset
//! representation (`crate::shared::skill::write_superset_skill_wrapper`):
//! OpenCode's own controls AND Codex's `agents/openai.yaml` policy sidecar
//! for `model=false`, so the entry is correct whichever integration
//! created it (ADR-030 §25).

use std::path::{Path, PathBuf};

use uze_core::{
    Result, UzeError,
    capability::Resource,
    exposure::{ExposureMechanism, ExposurePlan, ManagedArtifact},
    home::UzeHome,
    integration::IntegrationPort,
    router::CompatibilityRoute,
    state,
};

use super::OpenCodeIntegration;
use crate::shared::plan::unsupported;
use crate::shared::skill::{
    SharedRootReader, entry_name, generated_skill_dir, invalid_policy_plan, setup_pending_plan,
    skill_label, verify_reused_wrapper, write_superset_skill_wrapper,
};

/// Deterministically materializes (or refreshes) one Skill's
/// wrapper directory — the shared-root superset representation
/// (`crate::shared::skill::write_superset_skill_wrapper`): a real SKILL.md
/// carrying the stable namespaced label as its `name`, the canonical
/// description/body, and OpenCode's own invocation
/// controls — plus Codex's `agents/openai.yaml` policy sidecar, because
/// this directory lives in the shared `~/.agents/skills` root Codex reads
/// too (`model=false` must stay hidden there; ADR-030 §25). Idempotent and
/// rebuilt wholesale — the
/// directory is entirely UZE-owned and non-authoritative (ADR-013 §5).
pub(super) fn materialize_generated_skill(
    uze_home: &UzeHome,
    resource: &Resource,
) -> Result<PathBuf> {
    let policy = resource.skill_invocation();
    if policy.is_invalid() {
        return Err(UzeError::ExposureUnavailable(
            "a Skill nobody may invoke is never projected".to_owned(),
        ));
    }
    let dir = generated_skill_dir(uze_home, "opencode", resource);
    let canonical_dir = resource
        .capability
        .path
        .parent()
        .expect("SKILL.md has a parent");
    let label = skill_label(uze_home, resource).unwrap_or_else(|| {
        canonical_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("skill")
            .to_owned()
    });
    write_superset_skill_wrapper(
        &dir,
        canonical_dir,
        &resource.capability.payload,
        &label,
        &policy,
    )?;
    Ok(dir)
}

impl OpenCodeIntegration {
    pub(super) fn cleanup_unused_wrapper(&self, target: &Path) -> Result<()> {
        let managed_root = crate::shared::path::attachment_root(&self.uze_home, "opencode");
        crate::shared::path::cleanup_unused_wrapper(
            target,
            &managed_root,
            &self.skills_dir,
            &managed_root.join("skills"),
            &|wrapper| wrapper.join("SKILL.md").is_file(),
        )
    }

    /// Materializes this Skill's wrapper when this resource owns the shared
    /// entry; when the shared-root resolution reused another integration's
    /// artifact, verifies that the reused artifact still carries OpenCode's
    /// own invocation encoding — otherwise the canonical policy would
    /// silently degrade (ADR-030 §25).
    pub(super) fn materialize_or_verify_skill(&self, resource: &Resource) -> Result<()> {
        if resource.skill_invocation().is_invalid() {
            return Ok(());
        }
        match &resource.resolved_artifact_target {
            Some(wrapper) => verify_reused_wrapper(
                resource,
                wrapper,
                &self.skills_dir,
                SharedRootReader::OpenCode,
                self.id(),
            ),
            None => materialize_generated_skill(&self.uze_home, resource).map(|_| ()),
        }
    }

    pub(super) fn skill_plan(&self, resource: &Resource) -> ExposurePlan {
        let policy = resource.skill_invocation();
        if policy.is_invalid() {
            return invalid_policy_plan();
        }
        let Some(entry_name) = entry_name(self, resource) else {
            return unsupported("Resource has no derivable attachment entry name.");
        };
        if !state::is_installed(&self.uze_home, self.id()) {
            return setup_pending_plan("OpenCode");
        }
        let source = resource
            .resolved_artifact_target
            .clone()
            .unwrap_or_else(|| generated_skill_dir(&self.uze_home, "opencode", resource));
        let mut evidence = String::from(
            "OpenCode natively discovers the UZE-managed symlink in ~/.agents/skills (the same shared root Codex uses). UZE generates a wrapper carrying the stable qualified label as its `name`, while preserving the canonical description and body without rewriting the Store.",
        );
        let mut route = CompatibilityRoute::Native;
        if !policy.is_default() {
            evidence.push_str(
                " A non-default policy is translated into OpenCode's own SKILL.md fields on a generated wrapper (metadata.opencode/autoinvoke: false for model=false; slash: false for user=false) without touching the canonical Store bytes.",
            );
        }
        if !policy.user {
            // Measured, not assumed: `slash: false` removes the Skill from
            // the `/` catalog, and a mention (`@id`) still expands its body —
            // V2's picker offers every discovered Skill that way. Half the
            // policy is carried; half is not.
            route = CompatibilityRoute::Adaptable;
            evidence.push_str(
                " invoke.user=false degrades on OpenCode V2: `slash: false` withholds the Skill from the `/` catalog, but a mention (`@<label>`) still invokes it, so a user can reach it anyway — ADAPTED per ADR-030, reported rather than claimed.",
            );
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
