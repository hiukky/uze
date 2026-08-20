use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use uze::{
    Result, UzeError,
    capability::CapabilityKind,
    exposure::{ExposureMechanism, ExposurePlan},
    home::UzeHome,
    integration::{HarnessDetection, IntegrationPort},
    project::Resource,
    router::{CompatibilityRoute, HarnessCapabilities, VerificationStatus},
    state,
};

/// Claude Code peer integration. Its transparent-attachment strategy is a
/// UZE-managed "skills-dir plugin" reference at `<claude_home>/skills/<name>`
/// (see ADR-006): Claude auto-loads any directory there containing
/// `.claude-plugin/plugin.json` + `SKILL.md` at the start of every session,
/// with no per-session flag. Until `uze setup` has completed, exposure falls
/// back to the `--plugin-dir` conformance probe from ADR-005.
pub struct ClaudeIntegration {
    skills_dir: PathBuf,
    uze_home: UzeHome,
}

impl ClaudeIntegration {
    pub fn new(claude_home: PathBuf, uze_home: UzeHome) -> Self {
        Self {
            skills_dir: claude_home.join("skills"),
            uze_home,
        }
    }

    /// Convenience constructor for the CLI composition root. Unused when
    /// this module is compiled into a test binary via `#[path]`, where
    /// tests construct with `new` directly against a temporary home.
    #[allow(dead_code)]
    pub fn from_env(uze_home: UzeHome) -> Result<Self> {
        let home = std::env::var_os("HOME").ok_or(UzeError::MissingHomeDirectory)?;
        Ok(Self::new(PathBuf::from(home).join(".claude"), uze_home))
    }
}

impl IntegrationPort for ClaudeIntegration {
    fn id(&self) -> &'static str {
        "claude-code"
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            adaptable: [CapabilityKind::AgentSkill].into_iter().collect(),
            verification: VerificationStatus::Unverified,
            evidence: "Claude Code auto-loads a skills-dir plugin from <claude_home>/skills/<name>/. A symlinked entry there was empirically confirmed loaded via `claude plugin validate`/`plugin list`; behavioral (prompted) verification remains a separate opt-in conformance probe."
                .to_owned(),
            ..HarnessCapabilities::default()
        }
    }

    fn detect(&self) -> HarnessDetection {
        detect_binary("claude")
    }

    fn install(&self, home: &UzeHome) -> Result<()> {
        fs::create_dir_all(&self.skills_dir).map_err(|source| UzeError::Write {
            path: self.skills_dir.clone(),
            source,
        })?;
        let detected = self.detect();
        state::record(
            home,
            state::IntegrationRecord {
                harness: self.id().to_owned(),
                version: detected.version,
                strategy: "managed-user-scope-skills-dir".to_owned(),
                installed: true,
                managed_artifacts: vec![self.skills_dir.clone()],
            },
        )
    }

    fn exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        if resource.package_root().is_none() {
            return unsupported(
                resource,
                "Claude Code needs a UZE-stored Agent Plugin package for this attachment.",
            );
        }
        if resource.capability.kind != CapabilityKind::AgentSkill {
            return unsupported(
                resource,
                "Claude Code attachment is only modeled for Agent Skills in this PoC.",
            );
        }
        if state::is_installed(&self.uze_home, self.id())
            && let Some(entry_name) = resource.attachment_entry_name()
        {
            let shim_root = self
                .uze_home
                .state_dir()
                .join("attachments")
                .join("claude")
                .join(&entry_name);
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

    fn attach(&self, resource: &Resource) -> Result<Option<PathBuf>> {
        let plan = self.exposure_plan(resource);
        let ExposureMechanism::ManagedUserScopeReference { source, .. } = &plan.mechanism else {
            return Ok(None);
        };
        let skill_source_dir = resource
            .capability
            .path
            .parent()
            .expect("SKILL.md has a parent");
        let entry_name = resource
            .attachment_entry_name()
            .expect("plan construction already required a valid entry name");
        materialize_shim(source, skill_source_dir, &entry_name)?;
        Ok(Some(plan.mechanism.attach()?))
    }
}

fn materialize_shim(shim_root: &Path, skill_source_dir: &Path, name: &str) -> Result<()> {
    let plugin_dir = shim_root.join(".claude-plugin");
    fs::create_dir_all(&plugin_dir).map_err(|source| UzeError::Write {
        path: plugin_dir.clone(),
        source,
    })?;
    let manifest = serde_json::json!({
        "$schema": "https://anthropic.com/claude-code/plugin.schema.json",
        "name": name,
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

fn detect_binary(program: &str) -> HarnessDetection {
    let Ok(output) = Command::new(program).arg("--version").output() else {
        return HarnessDetection::default();
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = stdout.split_whitespace().next().map(str::to_owned);
    HarnessDetection {
        present: true,
        version,
    }
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

fn unsupported(resource: &Resource, rationale: &str) -> ExposurePlan {
    ExposurePlan {
        representation: resource.capability.representation,
        route: CompatibilityRoute::Unsupported,
        verification: VerificationStatus::NotExposed,
        mechanism: ExposureMechanism::Unsupported {
            rationale: rationale.to_owned(),
        },
        evidence: rationale.to_owned(),
    }
}
