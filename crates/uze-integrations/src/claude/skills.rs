//! Claude Code Agent Skill exposure — the managed skills-dir plugin shim
//! (see ADR-006): a small owned manifest plus a `SKILL.md` reference,
//! symlinked once into `<claude_home>/skills/<name>`. Invocation policy is
//! translated into Claude's own SKILL.md frontmatter fields
//! (`disable-model-invocation`, `user-invocable`) — see ADR-030.

use std::{fs, path::Path};

use uze_core::{
    Result, UzeError,
    exposure::{ExposureMechanism, ExposurePlan},
    integration::IntegrationPort,
    project::Resource,
    router::{CompatibilityRoute, VerificationStatus},
    skill::SkillInvocationPolicy,
    state,
};

use super::ClaudeIntegration;

impl ClaudeIntegration {
    pub(super) fn cleanup_unused_shim(&self, shim_root: &Path) -> Result<()> {
        let managed_root = self.uze_home.state_dir().join("attachments").join("claude");
        if !shim_root.starts_with(&managed_root) || !shim_root.is_dir() {
            return Ok(());
        }
        let referenced = fs::read_dir(&self.skills_dir)
            .map_err(|source| UzeError::Read {
                path: self.skills_dir.clone(),
                source,
            })?
            .filter_map(std::result::Result::ok)
            .any(|entry| fs::read_link(entry.path()).ok().as_deref() == Some(shim_root));
        if referenced {
            return Ok(());
        }
        let manifest = shim_root.join(".claude-plugin/plugin.json");
        let skill = shim_root.join("SKILL.md");
        if manifest.is_file() && (skill.is_symlink() || skill.is_file()) {
            fs::remove_dir_all(shim_root).map_err(|source| UzeError::Write {
                path: shim_root.to_path_buf(),
                source,
            })?;
        }
        Ok(())
    }

    pub(super) fn skill_exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        let policy = resource.skill_invocation();
        if policy.is_invalid() {
            return unsupported_invalid_policy(resource);
        }
        if state::is_installed(&self.uze_home, self.id())
            && let Some(entry_name) = resource
                .resolved_exposure_name
                .clone()
                .or_else(|| self.exposure_name_candidates(resource).into_iter().next())
        {
            let shim_root = resource
                .resolved_artifact_target
                .clone()
                .unwrap_or_else(|| {
                    self.uze_home
                        .state_dir()
                        .join("attachments")
                        .join("claude")
                        .join(&entry_name)
                });
            let mut evidence = String::from(
                "UZE materializes a small owned manifest shim (.claude-plugin/plugin.json plus a SKILL.md reference into the UZE store) and symlinks it once into <claude_home>/skills/. Claude auto-loads it on every future session with no --plugin-dir flag.",
            );
            if !policy.is_default() {
                evidence.push_str(
                    " The canonical invoke policy is translated into Claude's own frontmatter (disable-model-invocation: true when model=false, user-invocable: false when user=false) without touching the canonical Store bytes.",
                );
            }
            return ExposurePlan {
                representation: resource.capability.representation,
                route: route_for_policy(policy),
                verification: VerificationStatus::Unverified,
                mechanism: ExposureMechanism::ManagedUserScopeReference {
                    discovery_root: self.skills_dir.clone(),
                    entry_name,
                    source: shim_root,
                },
                evidence,
            };
        }
        ExposurePlan {
            representation: resource.capability.representation,
            route: CompatibilityRoute::Adaptable,
            verification: VerificationStatus::Unverified,
            mechanism: ExposureMechanism::RuntimeBridge {
                bridge: "Claude Code --plugin-dir".to_owned(),
                arguments: vec![
                    "--plugin-dir".to_owned(),
                    resource
                        .package_root()
                        .expect("guarded above")
                        .display()
                        .to_string(),
                ],
            },
            evidence: "Claude Code has not completed `uze setup`; falling back to the per-session --plugin-dir conformance probe rather than a managed attachment."
                .to_owned(),
        }
    }
}

/// Route classification for one canonical invocation policy on Claude Code.
///
/// Claude natively preserves every valid combination (ADR-030): a
/// user-only Skill gets `disable-model-invocation: true`, a model-only
/// Skill gets `user-invocable: false`, and the default needs nothing. An
/// invalid declaration (nobody can invoke it) is never projected.
pub(super) fn route_for_policy(policy: SkillInvocationPolicy) -> CompatibilityRoute {
    if policy.is_invalid() {
        return CompatibilityRoute::Unsupported;
    }
    CompatibilityRoute::Adaptable
}

fn unsupported_invalid_policy(resource: &Resource) -> ExposurePlan {
    ExposurePlan {
        representation: resource.capability.representation,
        route: CompatibilityRoute::Unsupported,
        verification: VerificationStatus::NotExposed,
        mechanism: ExposureMechanism::Unsupported {
            rationale: "This Skill declares invoke.model: false and invoke.user: false — nobody can invoke it, so UZE never projects it. Fix the `invoke:` block in SKILL.md.".to_owned(),
        },
        evidence: "Invalid canonical invocation policy: a Skill that nobody may invoke is not a projectable capability (ADR-030 §1).".to_owned(),
    }
}

/// Materializes the UZE-owned shim directory: the small plugin manifest
/// plus a `SKILL.md` that is either a symlink to the canonical Store bytes
/// (default model+user policy — byte-preserving) or a UZE-generated file
/// carrying the canonical name/description/body plus Claude's own
/// invocation markers (non-default policy — the canonical bytes stay in
/// the Store; this wrapper is a Derived Artifact, ADR-013 §4).
pub(super) fn materialize_shim(
    shim_root: &Path,
    canonical_skill_dir: &Path,
    entry_name: &str,
    namespace: Option<&str>,
    policy: &SkillInvocationPolicy,
) -> Result<()> {
    let plugin_dir = shim_root.join(".claude-plugin");
    fs::create_dir_all(&plugin_dir).map_err(|source| UzeError::Write {
        path: plugin_dir.clone(),
        source,
    })?;
    // The manifest plugin `name` is what Claude uses to namespace
    // components (plugins-reference: "This name is used for namespacing
    // components"), so it must be the *namespace* (`flow`) while the shim
    // directory carries the full stable label (`flow:review`). Together
    // with the skill's own frontmatter `name` (`review`) Claude exposes
    // `/flow:review` — never `/flow:flow:review` (ADR-026). A canonical
    // SKILL.md without a `name` field relies on Claude's directory-name
    // fallback for the skill name; packages should ship `name` frontmatter
    // (documented residual risk).
    let plugin_name = namespace.unwrap_or(entry_name);
    let manifest = serde_json::json!({
        "$schema": "https://anthropic.com/claude-code/plugin.schema.json",
        "name": plugin_name,
        "version": "0.1.0",
        "description": "UZE-managed skill, referencing the UZE store.",
        "skills": ["./"],
    });
    fs::write(
        plugin_dir.join("plugin.json"),
        serde_json::to_vec_pretty(&manifest).expect("plugin manifest serialization is infallible"),
    )
    .map_err(|source| UzeError::Write {
        path: plugin_dir.join("plugin.json"),
        source,
    })?;

    let skill_link = shim_root.join("SKILL.md");
    if policy.is_default() {
        let skill_source = canonical_skill_dir.join("SKILL.md");
        link_or_repair(&skill_link, &skill_source)?;
        return Ok(());
    }
    // Non-default policy: the delivered SKILL.md must carry Claude's own
    // frontmatter markers, so it is materialized — never a symlink — while
    // everything else in the canonical skill directory stays referenced.
    let bytes = fs::read(canonical_skill_dir.join("SKILL.md")).map_err(|error| UzeError::Read {
        path: canonical_skill_dir.join("SKILL.md"),
        source: error,
    })?;
    let fallback_name = canonical_skill_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(entry_name);
    let document = claude_wrapper_skill_document(&bytes, policy, fallback_name);
    write_or_replace_file(&skill_link, document.as_bytes())?;
    Ok(())
}

/// Renders the generated SKILL.md for one canonical Skill under a
/// non-default invocation policy: the canonical `name`/`description` are
/// preserved (with the description safely re-quoted — never
/// raw-interpolated), Claude's own invocation markers are injected, and
/// the canonical body is preserved verbatim. Used by both the capability
/// shim and the generated native package envelope.
pub(super) fn claude_wrapper_skill_document(
    canonical_bytes: &[u8],
    policy: &SkillInvocationPolicy,
    fallback_name: &str,
) -> String {
    let (description, body) = crate::shared::skill::parse_skill_body(canonical_bytes);
    let name = crate::shared::skill::frontmatter_value(canonical_bytes, "name")
        .unwrap_or_else(|| fallback_name.to_owned());
    let mut frontmatter = String::from("---\n");
    frontmatter.push_str(&format!("name: {name}\n"));
    if let Some(description) = &description {
        let escaped = crate::shared::skill::escape_yaml_double_quoted(description);
        frontmatter.push_str(&format!("description: \"{escaped}\"\n"));
    }
    if !policy.model {
        frontmatter.push_str("disable-model-invocation: true\n");
    }
    if !policy.user {
        frontmatter.push_str("user-invocable: false\n");
    }
    frontmatter.push_str("---\n");
    frontmatter.push_str(&body);
    frontmatter
}

fn link_or_repair(link: &Path, source: &Path) -> Result<()> {
    match fs::symlink_metadata(link) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let current = fs::read_link(link).map_err(|source_error| UzeError::Read {
                path: link.to_path_buf(),
                source: source_error,
            })?;
            if current != source {
                fs::remove_file(link).map_err(|source_error| UzeError::Write {
                    path: link.to_path_buf(),
                    source: source_error,
                })?;
                symlink(source, link)?;
            }
        }
        Ok(_) => return Err(UzeError::ManagedEntryConflict(link.to_path_buf())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            symlink(source, link)?;
        }
        Err(error) => {
            return Err(UzeError::Read {
                path: link.to_path_buf(),
                source: error,
            });
        }
    }
    Ok(())
}

fn write_or_replace_file(target: &Path, content: &[u8]) -> Result<()> {
    match fs::symlink_metadata(target) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            fs::remove_file(target).map_err(|source_error| UzeError::Write {
                path: target.to_path_buf(),
                source: source_error,
            })?;
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(UzeError::Read {
                path: target.to_path_buf(),
                source: error,
            });
        }
    }
    fs::write(target, content).map_err(|source_error| UzeError::Write {
        path: target.to_path_buf(),
        source: source_error,
    })
}

#[cfg(unix)]
fn symlink(source: &Path, target: &Path) -> Result<()> {
    std::os::unix::fs::symlink(source, target).map_err(|source_error| UzeError::Write {
        path: target.to_path_buf(),
        source: source_error,
    })
}

#[cfg(not(unix))]
fn symlink(_source: &Path, target: &Path) -> Result<()> {
    Err(UzeError::UnsupportedRuntimeProjection(target.to_path_buf()))
}
