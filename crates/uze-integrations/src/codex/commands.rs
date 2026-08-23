//! Codex Command exposure — NATIVE delivery through Codex's official
//! explicit-invocation-only Skill mechanism.
//!
//! Codex has **no custom-command file format**: its historical
//! `~/.codex/prompts/*.md` files are officially deprecated in favor of
//! Skills. What Codex *does* provide first-class is an
//! **explicit-invocation-only Skill**: `agents/openai.yaml` beside
//! `SKILL.md` with `policy.allow_implicit_invocation: false` (Codex Build
//! skills documentation: *"Codex won't implicitly invoke the skill based on
//! user prompt; explicit `$skill` invocation still works"*), empirically
//! honored by codex-cli 0.149.0 (verified via `codex debug prompt-input`
//! against a malformed-metadata control — see
//! `tests/command_capability_conformance.rs`).
//!
//! Per ADR-025, Native means *an officially supported primitive that
//! preserves the canonical capability semantics* — not an identical vendor
//! file format or primitive name. UZE Command semantics are: explicit user
//! invocation (✓ official `$name`/`/skills`), model never auto-selects it
//! (✓ policy, proven), stable identity (✓ canonical `Command` identity),
//! body preserved (✓ verbatim), description (✓). So a canonical Command is
//! delivered as a generated user-invokable explicit-only Skill: `SKILL.md`
//! plus the policy file, materialized under `$UZE_HOME` as a Derived
//! Artifact — never the Store — and symlinked into the shared
//! `~/.agents/skills` root. The route is **Native**; the canonical identity
//! remains `CapabilityKind::Command` and is never replaced by a Skill
//! identity.

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

use super::CodexIntegration;

/// Codex's official explicit-only skill metadata: `agents/openai.yaml`
/// beside `SKILL.md`, with `policy.allow_implicit_invocation: false`. Per
/// Codex's Build skills documentation this makes the skill *not* implicitly
/// invocable by the model while explicit `$skill` invocation still works.
/// Deterministically verified against codex-cli 0.149.0 via
/// `codex debug prompt-input` (the skill disappears from the model-visible
/// list only when this file is present and well-formed).
pub(super) const EXPLICIT_ONLY_POLICY_YAML: &str = "policy:\n  allow_implicit_invocation: false\n";

/// Root of every generated Command-as-Skill adaptation directory. Under
/// `$UZE_HOME/state/attachments/codex/commands/` — the same convention as
/// every other integration managed artifact, never under the Store.
pub(super) fn generated_root(uze_home: &UzeHome) -> PathBuf {
    uze_home
        .state_dir()
        .join("attachments")
        .join("codex")
        .join("commands")
}

pub(super) fn generated_command_dir(uze_home: &UzeHome, resource: &Resource) -> PathBuf {
    let package_id = Resource::package_root(resource)
        .and_then(|root| root.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("unknown");
    let name = resource
        .logical_capability_name()
        .unwrap_or_else(|| resource.name());
    generated_root(uze_home).join(package_id).join(name)
}

pub(super) use generated_command_dir as generated_artifact_dir;

/// Deterministically materializes (or refreshes) one command's delivered
/// skill directory: `SKILL.md` with the stable namespaced invocation label
/// as its `name` (`flow:review`), `description` from the canonical
/// frontmatter, the canonical prompt body verbatim, plus `agents/openai.yaml`
/// carrying `policy.allow_implicit_invocation: false` so the model never
/// auto-selects a Command. Idempotent and rebuilt wholesale — the directory
/// is entirely UZE-owned and non-authoritative (ADR-013 §4).
pub(super) fn materialize_generated_command(
    uze_home: &UzeHome,
    resource: &Resource,
) -> Result<PathBuf> {
    materialize_generated_skill_file(uze_home, resource, true)
}

/// Deterministically materializes (or refreshes) one Skill's delivered
/// directory in the shared skills root: `SKILL.md` carrying the stable
/// namespaced invocation label as its `name` and the canonical
/// name/description/body preserved from the Store — **without** the
/// explicit-only policy, so the Skill stays model-discoverable exactly as a
/// plain Skill. Codex derives the model-visible name from frontmatter
/// (verified against codex-cli 0.149.0), so namespacing the directory alone
/// is not enough; the generated wrapper is the only way to show
/// `flow:review` without rewriting the canonical bytes.
pub(super) fn materialize_generated_skill(
    uze_home: &UzeHome,
    resource: &Resource,
) -> Result<PathBuf> {
    materialize_generated_skill_file(uze_home, resource, false)
}

fn materialize_generated_skill_file(
    uze_home: &UzeHome,
    resource: &Resource,
    explicit_only: bool,
) -> Result<PathBuf> {
    let dir = generated_command_dir(uze_home, resource);
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
    let label = codex_invocation_label(resource).unwrap_or_else(|| resource.name());
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
    if explicit_only {
        fs::create_dir_all(dir.join("agents")).map_err(|source| UzeError::Write {
            path: dir.join("agents"),
            source,
        })?;
        fs::write(dir.join("agents/openai.yaml"), EXPLICIT_ONLY_POLICY_YAML).map_err(|source| {
            UzeError::Write {
                path: dir.join("agents/openai.yaml"),
                source,
            }
        })?;
    }
    Ok(dir)
}

/// Codex's physical invocation label — the UZE semantic label
/// (`flow:review`) verbatim: Codex accepted colon-named skills in
/// codex-cli 0.149.0 (verified: `flow:review` appears in the model-visible
/// list exactly as named, and the explicit-only policy keeps working).
pub(super) fn codex_invocation_label(resource: &Resource) -> Option<String> {
    use uze_core::integration::qualified_capability_name;
    let uze_core::project::ResourceOrigin::Package { id, .. } = &resource.origin else {
        return None;
    };
    let logical = resource.logical_capability_name()?;
    Some(qualified_capability_name(id.as_str(), &logical))
}

/// Codex derives a skill's user-facing identity from its `name` (verified:
/// frontmatter `name` wins over the directory name), so the single
/// candidate is the stable namespaced label itself — no bare alias, no
/// collision-dependent qualification (ADR-026).
pub(super) fn codex_command_exposure_name_candidates(resource: &Resource) -> Vec<String> {
    codex_invocation_label(resource).into_iter().collect()
}

impl CodexIntegration {
    pub(super) fn cleanup_unused_command_adaptation(&self, target: &Path) -> Result<()> {
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
                .unwrap_or_else(|| generated_command_dir(&self.uze_home, resource));
            return ExposurePlan {
                representation: resource.capability.representation,
                route: CompatibilityRoute::Native,
                verification: VerificationStatus::Unverified,
                mechanism: ExposureMechanism::ManagedUserScopeReference {
                    discovery_root: self.skills_dir.clone(),
                    entry_name,
                    source,
                },
                evidence: "Codex has no custom-command file format (~/.codex/prompts are deprecated in favor of Skills), but it has Codex's official explicit-invocation-only Skill mechanism (agents/openai.yaml → policy.allow_implicit_invocation: false; explicit `$skill` invocation still works — Codex Build skills documentation, empirically honored by codex-cli 0.149.0). UZE delivers the canonical Command as a generated explicit-only Skill under ~/.agents/skills — NATIVE per ADR-025 (an officially supported primitive preserving the canonical Command semantics), with the command's name/description/body preserved and its canonical identity inspectable in the receipt."
                    .to_owned(),
            };
        }
        ExposurePlan {
            representation: resource.capability.representation,
            route: CompatibilityRoute::Native,
            verification: VerificationStatus::Unverified,
            mechanism: ExposureMechanism::Unsupported {
                rationale: "Codex setup has not completed, so there is no user-scope skills directory for the command delivery yet.".to_owned(),
            },
            evidence: "Codex has not completed `uze setup`; the explicit-only Skill delivery needs the managed user-scope skills root."
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
    fn generated_skill_preserves_body_carries_identity_and_is_explicit_only() {
        let root = std::env::temp_dir().join(format!("uze-codex-cmd-{}", std::process::id()));
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
        // The explicit-only policy is the semantic load-bearing piece: it
        // keeps the model from auto-selecting a Command.
        assert_eq!(
            fs::read_to_string(dir.join("agents/openai.yaml")).unwrap(),
            EXPLICIT_ONLY_POLICY_YAML
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn adaptation_is_deterministic_across_rebuilds() {
        let root = std::env::temp_dir().join(format!("uze-codex-cmd-det-{}", std::process::id()));
        let home = UzeHome::at(root.join("uze"));
        let resource = command_resource(
            "flow",
            "/store/packages/flow/commands/review.md",
            b"body only\n",
        );
        let a = materialize_generated_command(&home, &resource).unwrap();
        let first_skill = fs::read(a.join("SKILL.md")).unwrap();
        let first_policy = fs::read(a.join("agents/openai.yaml")).unwrap();
        let b = materialize_generated_command(&home, &resource).unwrap();
        let second_skill = fs::read(b.join("SKILL.md")).unwrap();
        let second_policy = fs::read(b.join("agents/openai.yaml")).unwrap();
        assert_eq!(first_skill, second_skill);
        assert_eq!(first_policy, second_policy);
        let _ = fs::remove_dir_all(root);
    }
}
