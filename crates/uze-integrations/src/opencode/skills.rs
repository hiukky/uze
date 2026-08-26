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
//! - `slash: false` hides the skill from interactive command catalogs
//!   (user=false preserved).
//!
//! Because every combination is natively representable, a canonical
//! user-only Skill is projected as an OpenCode **Skill**, never as a
//! vendor Command — the vendor Command primitive remains a projection
//! detail UZE does not need for this harness (ADR-030 §9).
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
    exposure::{ExposureMechanism, ExposurePlan},
    home::UzeHome,
    integration::IntegrationPort,
    project::Resource,
    router::{CompatibilityRoute, VerificationStatus},
    state,
};

use super::OpenCodeIntegration;
use super::unsupported;

/// Root of every generated OpenCode Skill wrapper directory. Under
/// `$UZE_HOME/state/attachments/opencode/skills/`, never under the Store.
pub(super) fn generated_skill_dir(uze_home: &UzeHome, resource: &Resource) -> PathBuf {
    let package_id = Resource::package_root(resource)
        .and_then(|root| root.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("unknown");
    let name = resource
        .logical_capability_name()
        .unwrap_or_else(|| resource.name());
    uze_home
        .state_dir()
        .join("attachments")
        .join("opencode")
        .join("skills")
        .join(package_id)
        .join(name)
}

/// Deterministically materializes (or refreshes) one Skill's
/// wrapper directory — the shared-root superset representation
/// (`crate::shared::skill::write_superset_skill_wrapper`): a real SKILL.md
/// carrying the stable namespaced label as its `name`, the canonical
/// description/body, and OpenCode's own invocation
/// controls — plus Codex's `agents/openai.yaml` policy sidecar, because
/// this directory lives in the shared `~/.agents/skills` root Codex reads
/// too (`model=false` must stay hidden there; ADR-030 §25). Idempotent and
/// rebuilt wholesale — the
/// directory is entirely UZE-owned and non-authoritative (ADR-013 §4).
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
    let dir = generated_skill_dir(uze_home, resource);
    if dir.exists() {
        fs_remove_dir_all(&dir)?;
    }
    fs_create_dir_all(&dir)?;
    let canonical_dir = resource
        .capability
        .path
        .parent()
        .expect("SKILL.md has a parent");
    let fallback_name = canonical_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("skill");
    let bytes = std::fs::read(canonical_dir.join("SKILL.md")).map_err(|error| UzeError::Read {
        path: canonical_dir.join("SKILL.md"),
        source: error,
    })?;
    let label = uze_core::integration::qualified_exposure_name_candidates(resource)
        .into_iter()
        .next()
        .unwrap_or_else(|| fallback_name.to_owned());
    crate::shared::skill::write_superset_skill_wrapper(
        &dir,
        canonical_dir,
        &bytes,
        &label,
        &policy,
    )?;
    Ok(dir)
}

impl OpenCodeIntegration {
    fn is_generated_skill_wrapper(&self, target: &Path) -> bool {
        target.starts_with(self.uze_home.state_dir().join("attachments"))
            && target.join("SKILL.md").is_file()
    }

    pub(super) fn cleanup_unused_skill_wrapper(&self, target: &Path) -> Result<()> {
        let managed_root = self
            .uze_home
            .state_dir()
            .join("attachments")
            .join("opencode");
        if !target.starts_with(&managed_root) || !target.is_dir() {
            return Ok(());
        }
        let referenced = std::fs::read_dir(&self.skills_dir)
            .map_err(|source| UzeError::Read {
                path: self.skills_dir.clone(),
                source,
            })?
            .filter_map(std::result::Result::ok)
            .any(|entry| std::fs::read_link(entry.path()).ok().as_deref() == Some(target));
        if referenced {
            return Ok(());
        }
        if target.join("SKILL.md").is_file() {
            fs_remove_dir_all(target)?;
        }
        Ok(())
    }

    /// Materializes this Skill's wrapper when this resource owns the shared
    /// entry; when the shared-root resolution reused another integration's
    /// artifact, verifies that the reused artifact still carries OpenCode's
    /// own invocation encoding — otherwise the canonical policy would
    /// silently degrade (ADR-030 §25).
    pub(super) fn materialize_or_verify_skill(&self, resource: &Resource) -> Result<()> {
        let policy = resource.skill_invocation();
        if policy.is_invalid() {
            return Ok(());
        }
        let Some(target) = &resource.resolved_artifact_target else {
            return materialize_generated_skill(&self.uze_home, resource).map(|_| ());
        };
        if !self.is_generated_skill_wrapper(target) {
            return materialize_generated_skill(&self.uze_home, resource).map(|_| ());
        }
        if policy.is_default() {
            return Ok(());
        }
        let bytes = std::fs::read(target.join("SKILL.md")).map_err(|error| UzeError::Read {
            path: target.join("SKILL.md"),
            source: error,
        })?;
        let entry = resource
            .resolved_exposure_name
            .clone()
            .map(|name| self.skills_dir.join(name))
            .unwrap_or_else(|| target.to_path_buf());
        if !policy.model && !crate::shared::skill::has_opencode_autoinvoke_false(&bytes) {
            return Err(projection_conflict(
                resource,
                &entry,
                target,
                "OpenCode needs metadata.opencode/autoinvoke: false for a user-only Skill",
            ));
        }
        if !policy.user && !crate::shared::skill::has_slash_false(&bytes) {
            return Err(projection_conflict(
                resource,
                &entry,
                target,
                "OpenCode needs slash: false for a model-only Skill",
            ));
        }
        Ok(())
    }

    pub(super) fn skill_plan(&self, resource: &Resource) -> ExposurePlan {
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
        let Some(entry_name) = resource
            .resolved_exposure_name
            .clone()
            .or_else(|| self.exposure_name_candidates(resource).into_iter().next())
        else {
            return unsupported(resource, "Resource has no derivable attachment entry name.");
        };
        let canonical_source = resource
            .capability
            .path
            .parent()
            .expect("SKILL.md has a parent")
            .to_path_buf();
        let source = resource
            .resolved_artifact_target
            .as_ref()
            .filter(|target| self.is_generated_skill_wrapper(target))
            .cloned()
            .unwrap_or_else(|| generated_skill_dir(&self.uze_home, resource));
        if state::is_installed(&self.uze_home, self.id()) {
            let mut evidence = String::from(
                "OpenCode natively discovers the UZE-managed symlink in ~/.agents/skills (the same shared root Codex uses). UZE generates a wrapper carrying the stable qualified label as its `name`, while preserving the canonical description and body without rewriting the Store.",
            );
            if !policy.is_default() {
                evidence.push_str(
                    " A non-default policy is translated into OpenCode's own SKILL.md fields on a generated wrapper (metadata.opencode/autoinvoke: false for model=false; slash: false for user=false) without touching the canonical Store bytes — Native per ADR-030.",
                );
            }
            return ExposurePlan {
                representation: resource.capability.representation,
                route: CompatibilityRoute::Native,
                verification: VerificationStatus::Unverified,
                mechanism: ExposureMechanism::ManagedUserScopeReference {
                    discovery_root: self.skills_dir.clone(),
                    entry_name,
                    source,
                },
                evidence,
            };
        }
        ExposurePlan {
            representation: resource.capability.representation,
            route: CompatibilityRoute::Adaptable,
            verification: VerificationStatus::Unverified,
            mechanism: ExposureMechanism::FilesystemProjection {
                source: canonical_source,
                target_relative: PathBuf::from(".agents/skills").join(
                    self.exposure_name_candidates(resource)
                        .first()
                        .cloned()
                        .unwrap_or_else(|| {
                            resource
                                .capability
                                .path
                                .parent()
                                .and_then(Path::file_name)
                                .expect("skill dir name")
                                .to_string_lossy()
                                .into_owned()
                        }),
                ),
            },
            evidence: "OpenCode setup has not completed; the existing project-scope projection remains a conformance fallback."
                .to_owned(),
        }
    }
}

/// Deterministic, pre-attach projection conflict: the shared
/// `~/.agents/skills` entry this resource would reuse is already owned by
/// another integration's artifact that lacks OpenCode's invocation
/// encoding (ADR-030 §25 — never degrade silently).
fn projection_conflict(
    resource: &Resource,
    entry: &Path,
    reused_target: &Path,
    requirement: &str,
) -> UzeError {
    let requested_target = resource
        .capability
        .path
        .parent()
        .map(|parent| parent.to_path_buf())
        .unwrap_or_else(|| resource.capability.path.clone());
    UzeError::ProjectionConflict(Box::new(uze_core::error::ProjectionConflictDetails {
        entry: entry.to_path_buf(),
        requested: format!("{} ({requirement})", resource.identity()),
        requested_integration: "opencode".to_owned(),
        requested_target,
        existing: format!("{} ({requirement})", resource.identity()),
        existing_integration: "shared-root owner".to_owned(),
        existing_target: reused_target.to_path_buf(),
    }))
}

fn fs_create_dir_all(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path).map_err(|source| UzeError::Write {
        path: path.to_path_buf(),
        source,
    })
}

fn fs_remove_dir_all(path: &Path) -> Result<()> {
    std::fs::remove_dir_all(path).map_err(|source| UzeError::Write {
        path: path.to_path_buf(),
        source,
    })
}
