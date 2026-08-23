//! Claude Code Agent Skill exposure — the managed skills-dir plugin shim
//! (see ADR-006): a small owned manifest plus a `SKILL.md` reference,
//! symlinked once into `<claude_home>/skills/<name>`.

use std::{fs, path::Path};

use uze_core::{
    Result, UzeError,
    exposure::{ExposureMechanism, ExposurePlan},
    integration::IntegrationPort,
    project::Resource,
    router::{CompatibilityRoute, VerificationStatus},
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
        if manifest.is_file() && skill.is_symlink() {
            fs::remove_dir_all(shim_root).map_err(|source| UzeError::Write {
                path: shim_root.to_path_buf(),
                source,
            })?;
        }
        Ok(())
    }

    pub(super) fn skill_exposure_plan(&self, resource: &Resource) -> ExposurePlan {
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
            return ExposurePlan {
                representation: resource.capability.representation,
                route: CompatibilityRoute::Adaptable,
                verification: VerificationStatus::Unverified,
                mechanism: ExposureMechanism::ManagedUserScopeReference {
                    discovery_root: self.skills_dir.clone(),
                    entry_name,
                    source: shim_root,
                },
                evidence: "UZE materializes a small owned manifest shim (.claude-plugin/plugin.json plus a SKILL.md reference into the UZE store) and symlinks it once into <claude_home>/skills/. Claude auto-loads it on every future session with no --plugin-dir flag."
                    .to_owned(),
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

pub(super) fn materialize_shim(
    shim_root: &Path,
    skill_source_dir: &Path,
    entry_name: &str,
    namespace: Option<&str>,
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
    let skill_source = skill_source_dir.join("SKILL.md");
    match fs::symlink_metadata(&skill_link) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let current = fs::read_link(&skill_link).map_err(|source| UzeError::Read {
                path: skill_link.clone(),
                source,
            })?;
            if current != skill_source {
                fs::remove_file(&skill_link).map_err(|source| UzeError::Write {
                    path: skill_link.clone(),
                    source,
                })?;
                symlink(&skill_source, &skill_link)?;
            }
        }
        Ok(_) => return Err(UzeError::ManagedEntryConflict(skill_link)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            symlink(&skill_source, &skill_link)?;
        }
        Err(error) => {
            return Err(UzeError::Read {
                path: skill_link,
                source: error,
            });
        }
    }
    Ok(())
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
