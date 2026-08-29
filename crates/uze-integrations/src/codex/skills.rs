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

use std::{fs, path::Path, path::PathBuf};

use uze_core::{
    Result, UzeError,
    exposure::{ExposureMechanism, ExposurePlan},
    home::UzeHome,
    integration::IntegrationPort,
    project::Resource,
    router::{CompatibilityRoute, VerificationStatus},
    state,
};

use super::CodexIntegration;

/// Codex's official invocation-policy metadata: `agents/openai.yaml` beside
/// `SKILL.md`, with `policy.allow_implicit_invocation: false`. Per Codex's
/// Build skills documentation this makes the skill *not* implicitly
/// invocable by the model while explicit `$skill` invocation still works.
/// Deterministically verified against codex-cli 0.149.0 via
/// `codex debug prompt-input`.
pub(super) const EXPLICIT_ONLY_POLICY_YAML: &str = "policy:\n  allow_implicit_invocation: false\n";

/// Root of every generated Skill wrapper directory. Under
/// `$UZE_HOME/state/attachments/codex/skills/` — the same convention as
/// every other integration's managed artifacts, never under the Store.
pub(super) fn generated_root(uze_home: &UzeHome) -> PathBuf {
    uze_home
        .state_dir()
        .join("attachments")
        .join("codex")
        .join("skills")
}

pub(super) fn generated_skill_dir(uze_home: &UzeHome, resource: &Resource) -> PathBuf {
    let package_id = Resource::package_root(resource)
        .and_then(|root| root.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("unknown");
    let name = resource
        .logical_capability_name()
        .unwrap_or_else(|| resource.name());
    generated_root(uze_home).join(package_id).join(name)
}

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
/// bytes. The wrapper also carries OpenCode's own invocation controls
/// because this directory lives in the shared `~/.agents/skills` root —
/// Codex ignores those fields, and OpenCode needs them when it reuses the
/// same physical entry (ADR-030 §25). Idempotent and rebuilt wholesale —
/// the directory is entirely UZE-owned and non-authoritative (ADR-013 §4).
pub(super) fn materialize_generated_skill(
    uze_home: &UzeHome,
    resource: &Resource,
) -> Result<PathBuf> {
    let dir = generated_skill_dir(uze_home, resource);
    let canonical_dir = resource
        .capability
        .path
        .parent()
        .expect("SKILL.md has a parent");
    let label = codex_invocation_label(uze_home, resource).unwrap_or_else(|| resource.name());
    crate::shared::skill::write_superset_skill_wrapper(
        &dir,
        canonical_dir,
        &resource.capability.payload,
        &label,
        &resource.skill_invocation(),
    )?;
    Ok(dir)
}

/// Codex's physical invocation label — the UZE semantic label
/// (`flow:review`) verbatim: Codex accepted colon-named skills in
/// codex-cli 0.149.0 (verified: `flow:review` appears in the model-visible
/// list exactly as named, and the explicit-only policy keeps working).
pub(super) fn codex_invocation_label(uze_home: &UzeHome, resource: &Resource) -> Option<String> {
    use uze_core::integration::{active_plugin_name, qualified_capability_name};
    let active_name = active_plugin_name(uze_home, resource)?;
    let logical = resource.logical_capability_name()?;
    Some(qualified_capability_name(&active_name, &logical))
}

/// Codex derives a skill's user-facing identity from its `name` (verified:
/// frontmatter `name` wins over the directory name), so the single
/// candidate is the stable namespaced label itself — no bare alias, no
/// collision-dependent qualification (ADR-026).
pub(super) fn codex_skill_exposure_name_candidates(
    uze_home: &UzeHome,
    resource: &Resource,
) -> Vec<String> {
    codex_invocation_label(uze_home, resource)
        .into_iter()
        .collect()
}

impl CodexIntegration {
    pub(super) fn cleanup_unused_skill_adaptation(&self, target: &Path) -> Result<()> {
        let managed_root = self.uze_home.state_dir().join("attachments").join("codex");
        if !target.starts_with(&managed_root) || !target.is_dir() {
            return Ok(());
        }
        let referenced = fs::read_dir(&self.skills_dir)
            .map_err(|source| UzeError::Read {
                path: self.skills_dir.clone(),
                source,
            })?
            .filter_map(std::result::Result::ok)
            .any(|entry| fs::read_link(entry.path()).ok().as_deref() == Some(target));
        if referenced {
            return Ok(());
        }
        if target.join("SKILL.md").is_file() {
            fs::remove_dir_all(target).map_err(|source| UzeError::Write {
                path: target.to_path_buf(),
                source,
            })?;
        }
        Ok(())
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
            return ExposurePlan {
                representation: resource.capability.representation,
                route: CompatibilityRoute::Unsupported,
                verification: VerificationStatus::NotExposed,
                mechanism: ExposureMechanism::Unsupported {
                    rationale: "This Skill declares invoke.model: false and invoke.user: false — nobody can invoke it, so UZE never projects it. Fix the `invoke:` block in SKILL.md.".to_owned(),
                },
                evidence: "Invalid canonical invocation policy: a Skill that nobody may invoke is not a projectable capability (ADR-030 §1).".to_owned(),
            };
        }
        if state::is_installed(&self.uze_home, self.id())
            && let Some(entry_name) = resource
                .resolved_exposure_name
                .clone()
                .or_else(|| self.exposure_name_candidates(resource).into_iter().next())
        {
            let source = resource
                .resolved_artifact_target
                .clone()
                .unwrap_or_else(|| generated_skill_dir(&self.uze_home, resource));
            // model-only (user cannot invoke) is the one combination Codex
            // cannot preserve; every other valid combination is Native.
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
            return ExposurePlan {
                representation: resource.capability.representation,
                route,
                verification: VerificationStatus::Unverified,
                mechanism: ExposureMechanism::ManagedUserScopeReference {
                    discovery_root: self.skills_dir.clone(),
                    entry_name,
                    source,
                },
                evidence,
            };
        }
        let skill_directory = resource
            .capability
            .path
            .parent()
            .expect("SKILL.md has a parent");
        let label = codex_skill_exposure_name_candidates(&self.uze_home, resource)
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

#[cfg(test)]
mod tests {
    use super::*;
    use uze_core::capability::{Capability, CapabilityKind, Representation};
    use uze_core::store::PackageId;

    fn skill_resource(package_id: &str, path_string: &str, payload: &[u8]) -> Resource {
        let id = PackageId::from_plugin_name(package_id, Path::new("plugin.json")).unwrap();
        Resource::from_package(
            id,
            PathBuf::from("/store/packages").join(package_id),
            Capability {
                kind: CapabilityKind::AgentSkill,
                representation: Representation::Standard,
                path: PathBuf::from(path_string),
                payload: payload.to_vec(),
            },
        )
    }

    #[test]
    fn user_only_skill_wrapper_is_superset_and_preserves_body() {
        let root = std::env::temp_dir().join(format!("uze-codex-skill-{}", std::process::id()));
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
            EXPLICIT_ONLY_POLICY_YAML
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
        let root = std::env::temp_dir().join(format!("uze-codex-skill-def-{}", std::process::id()));
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
        let root = std::env::temp_dir().join(format!("uze-codex-skill-det-{}", std::process::id()));
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
