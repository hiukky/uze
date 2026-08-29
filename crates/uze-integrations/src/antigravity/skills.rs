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
//! Invocation-policy reality (agy 1.1.21): `disable-slash-command: true`
//! hides a Skill from `/` and `/name` while retaining model discovery. The
//! inverse control does not exist: every user-invocable Skill remains
//! model-discoverable. Per ADR-030 this yields:
//!
//! - model+user (default) → Native;
//! - user-only (`model=false`) → Adapted: user invocation is native, but
//!   the model can still discover/auto-select the Skill — the exact
//!   semantic degradation that used to characterize delivery of a
//!   canonical `Command`;
//! - model-only (`user=false`) → Native via `disable-slash-command: true`;
//! - invalid (`model=false,user=false`) → never projected.
//!
//! The generated wrapper carries the stable namespaced label as its front
//! matter `name` (agy derives the invoked name from the SKILL.md front
//! matter, verified against 1.1.19) and the canonical description/body
//! verbatim. Always a Derived Artifact under `$UZE_HOME`, never the Store.

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

use crate::shared::skill::parse_skill_body;

use super::AntigravityIntegration;

/// Root of every generated Skill wrapper directory. Under
/// `$UZE_HOME/state/attachments/antigravity/skills/` — the same convention
/// as every other integration's managed artifacts, never under the Store.
pub(super) fn generated_root(uze_home: &UzeHome) -> PathBuf {
    uze_home
        .state_dir()
        .join("attachments")
        .join("antigravity")
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

/// Antigravity's physical invocation label — the UZE semantic label
/// (`flow:review`) verbatim. `agy plugin validate` accepts `:` in skill
/// names (verified against 1.1.19).
pub(super) fn antigravity_invocation_label(
    uze_home: &UzeHome,
    resource: &Resource,
) -> Option<String> {
    use uze_core::integration::{active_plugin_name, qualified_capability_name};
    let active_name = active_plugin_name(uze_home, resource)?;
    let logical = resource.logical_capability_name()?;
    Some(qualified_capability_name(&active_name, &logical))
}

pub(super) fn antigravity_skill_exposure_name_candidates(
    uze_home: &UzeHome,
    resource: &Resource,
) -> Vec<String> {
    antigravity_invocation_label(uze_home, resource)
        .into_iter()
        .collect()
}

/// Deterministically materializes (or refreshes) one Skill's delivered
/// wrapper: `SKILL.md` carrying the stable namespaced label as its `name`
/// and the canonical description/body preserved — so the model-visible and
/// slash-invocable name is `flow:review`, never a bare alias or a
/// collision-prone `review` (the vendor derives the identity from front
/// matter). Idempotent and rebuilt wholesale — the directory is entirely
/// UZE-owned and non-authoritative (ADR-013 §4).
pub(super) fn materialize_generated_skill(
    uze_home: &UzeHome,
    resource: &Resource,
) -> Result<PathBuf> {
    let dir = generated_skill_dir(uze_home, resource);
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|source| UzeError::Write {
            path: dir.clone(),
            source,
        })?;
    }
    fs::create_dir_all(&dir).map_err(|source| UzeError::Write {
        path: dir.clone(),
        source,
    })?;
    let label = antigravity_invocation_label(uze_home, resource).unwrap_or_else(|| resource.name());
    let (description, body) = parse_skill_body(&resource.capability.payload);
    let mut skill = String::from("---\n");
    skill.push_str(&format!("name: {label}\n"));
    if let Some(description) = description {
        let escaped = crate::shared::skill::escape_yaml_double_quoted(&description);
        skill.push_str(&format!("description: \"{escaped}\"\n"));
    }
    if !resource.skill_invocation().user {
        skill.push_str("disable-slash-command: true\n");
    }
    skill.push_str("---\n");
    skill.push_str(&body);
    fs::write(dir.join("SKILL.md"), skill).map_err(|source| UzeError::Write {
        path: dir.join("SKILL.md"),
        source,
    })?;
    Ok(dir)
}

/// Cleans up a generated wrapper directory once nothing references it —
/// called when a resource leaves the managed skills root. Only ever touches
/// UZE-owned directories under `$UZE_HOME`.
impl AntigravityIntegration {
    pub(super) fn cleanup_unused_wrapper(&self, target: &Path) -> Result<()> {
        let managed_root = self
            .uze_home
            .state_dir()
            .join("attachments")
            .join("antigravity");
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
            let (route, mut evidence) = if policy.is_default() {
                (
                    CompatibilityRoute::Native,
                    "Antigravity CLI imports every markdown skill under ~/.gemini/antigravity-cli/skills as a global slash command, so a UZE-managed reference there is consumed natively. The generated wrapper carries the stable namespaced label and the canonical name/description/body."
                        .to_owned(),
                )
            } else if !policy.model {
                (
                    CompatibilityRoute::Adaptable,
                    "Antigravity has no explicit-invocation-only mechanism: a user-invocable Skill remains model-discoverable. The user-invocation half is native; invoke.model=false degrades — ADAPTED per ADR-030, reported honestly rather than invented."
                        .to_owned(),
                )
            } else {
                (
                    CompatibilityRoute::Native,
                    "Antigravity natively preserves invoke.user=false with disable-slash-command: true: the Skill remains model-discoverable while `/` and `/name` resolution omit it."
                        .to_owned(),
                )
            };
            if !policy.model {
                evidence.push_str(
                    " The canonical invoke.model=false cannot be enforced: the model may still discover and auto-select this Skill.",
                );
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
        ExposurePlan {
            representation: resource.capability.representation,
            route: CompatibilityRoute::Adaptable,
            verification: VerificationStatus::Unverified,
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
    fn generated_skill_preserves_body_and_carries_the_stable_label() {
        let root =
            std::env::temp_dir().join(format!("uze-antigravity-skill-{}", std::process::id()));
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
        let root =
            std::env::temp_dir().join(format!("uze-antigravity-skill-det-{}", std::process::id()));
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
