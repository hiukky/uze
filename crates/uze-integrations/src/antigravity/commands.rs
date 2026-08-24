//! Antigravity Command exposure — ADAPTED delivery through the vendor's
//! official commands→Skills conversion.
//!
//! Antigravity has **no custom-command file format**: its official
//! migration path converts legacy commands to Skills (`commands: N legacy
//! commands converted to skills` — verified against agy 1.1.19, which
//! turns a legacy `review.toml` into `skills/review/SKILL.md`), and Skills
//! are model-discoverable (progressive disclosure: the agent decides based
//! on the description) *and* slash-invocable. No explicit-invocation-only
//! mechanism is documented or observable, so the canonical Command's
//! explicit-only property cannot be preserved on this physical primitive.
//!
//! Per ADR-025, Native means an officially supported primitive preserving
//! the canonical capability semantics; the Command semantics degrade here
//! (the model may auto-select it), so the route is **Adapted**: user
//! invocation is fully native, explicit-only is lost, and the canonical
//! body/description/identity are preserved. The capability declaration
//! reports Command as `adaptable`, never `native` (see
//! `AntigravityIntegration::capabilities`).

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

use crate::shared::command::parse_command_body;

use super::AntigravityIntegration;

/// Root of every generated Command/Skill wrapper directory. Under
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
pub(super) fn antigravity_invocation_label(resource: &Resource) -> Option<String> {
    use uze_core::integration::qualified_capability_name;
    let uze_core::project::ResourceOrigin::Package { id, .. } = &resource.origin else {
        return None;
    };
    let logical = resource.logical_capability_name()?;
    Some(qualified_capability_name(id.as_str(), &logical))
}

pub(super) fn antigravity_command_exposure_name_candidates(resource: &Resource) -> Vec<String> {
    antigravity_invocation_label(resource).into_iter().collect()
}

/// Deterministically materializes (or refreshes) one Command's delivered
/// skill directory: `SKILL.md` carrying the stable namespaced invocation
/// label as its `name`, the canonical description, and the canonical prompt
/// body verbatim. Idempotent and rebuilt wholesale — the directory is
/// entirely UZE-owned and non-authoritative (ADR-013 §4).
pub(super) fn materialize_generated_command(
    uze_home: &UzeHome,
    resource: &Resource,
) -> Result<PathBuf> {
    materialize_generated_skill_file(uze_home, resource)
}

/// Deterministically materializes (or refreshes) one Skill's delivered
/// wrapper: `SKILL.md` carrying the stable namespaced label as its `name`
/// and the canonical description/body preserved — so the model-visible and
/// slash-invocable name is `flow:review`, never a bare alias or a
/// collision-prone `review` (same reason Codex wraps: the vendor derives
/// the identity from frontmatter).
pub(super) fn materialize_generated_skill(
    uze_home: &UzeHome,
    resource: &Resource,
) -> Result<PathBuf> {
    materialize_generated_skill_file(uze_home, resource)
}

fn materialize_generated_skill_file(uze_home: &UzeHome, resource: &Resource) -> Result<PathBuf> {
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
    let label = antigravity_invocation_label(resource).unwrap_or_else(|| resource.name());
    let (description, body) = parse_command_body(&resource.capability.payload);
    let mut skill = String::from("---\n");
    skill.push_str(&format!("name: {label}\n"));
    if let Some(description) = description {
        skill.push_str(&format!("description: {description}\n"));
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
}

impl AntigravityIntegration {
    pub(super) fn command_exposure_plan(&self, resource: &Resource) -> ExposurePlan {
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
            return ExposurePlan {
                representation: resource.capability.representation,
                // ADAPTED, deliberately: Antigravity converts commands to
                // Skills and Skills are model-discoverable with no
                // explicit-only switch, so the canonical Command's
                // explicit-only property degrades (see module doc).
                route: CompatibilityRoute::Adaptable,
                verification: VerificationStatus::Unverified,
                mechanism: ExposureMechanism::ManagedUserScopeReference {
                    discovery_root: self.skills_dir.clone(),
                    entry_name,
                    source,
                },
                evidence: "Antigravity has no custom-command primitive (its official migration path converts commands to Skills), and Skills are model-discoverable with no observable explicit-invocation-only mechanism. UZE delivers the canonical Command as a generated Skill under ~/.gemini/antigravity-cli/skills with the stable namespaced label, canonical name/description/body — ADAPTED per ADR-025: native user invocation, degraded explicit-only property."
                    .to_owned(),
            };
        }
        ExposurePlan {
            representation: resource.capability.representation,
            route: CompatibilityRoute::Adaptable,
            verification: VerificationStatus::Unverified,
            mechanism: ExposureMechanism::Unsupported {
                rationale: "Antigravity setup has not completed, so there is no global skills root for the command delivery yet."
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

    fn command_resource(package_id: &str, path_string: &str, payload: &[u8]) -> Resource {
        let id = PackageId::from_plugin_name(package_id, Path::new("plugin.json")).unwrap();
        let stem = Path::new(path_string)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap()
            .to_owned();
        Resource::from_package_named(
            id,
            PathBuf::from("/store/packages").join(package_id),
            Capability {
                kind: CapabilityKind::Command,
                representation: Representation::Standard,
                path: PathBuf::from(path_string),
                payload: payload.to_vec(),
            },
            stem,
        )
    }

    #[test]
    fn generated_skill_preserves_body_and_carries_the_stable_label() {
        let root = std::env::temp_dir().join(format!("uze-antigravity-cmd-{}", std::process::id()));
        let home = UzeHome::at(root.join("uze"));
        let resource = command_resource(
            "flow",
            "/store/packages/flow/commands/review.md",
            b"---\ndescription: Review code\n---\n\nReview this diff.\n",
        );
        let dir = materialize_generated_command(&home, &resource).unwrap();
        let skill = fs::read_to_string(dir.join("SKILL.md")).unwrap();
        assert!(skill.starts_with("---\nname: flow:review\ndescription: Review code\n---\n"));
        assert!(skill.ends_with("\nReview this diff.\n"));
        assert!(
            !dir.join("agents/openai.yaml").exists(),
            "Antigravity has no explicit-only policy file; the wrapper carries only SKILL.md"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn adaptation_is_deterministic_across_rebuilds() {
        let root =
            std::env::temp_dir().join(format!("uze-antigravity-cmd-det-{}", std::process::id()));
        let home = UzeHome::at(root.join("uze"));
        let resource = command_resource(
            "flow",
            "/store/packages/flow/commands/review.md",
            b"body only\n",
        );
        let a = materialize_generated_command(&home, &resource).unwrap();
        let first = fs::read(a.join("SKILL.md")).unwrap();
        let b = materialize_generated_command(&home, &resource).unwrap();
        let second = fs::read(b.join("SKILL.md")).unwrap();
        assert_eq!(first, second);
        let _ = fs::remove_dir_all(root);
    }
}
