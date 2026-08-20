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
    /// `HOME` to set explicitly whenever a `claude` subcommand is shelled
    /// out to for MCP registration (`mcp add`/`get`/`remove`) — unlike the
    /// Skills path (pure filesystem operations on `skills_dir`, no process
    /// spawn), MCP commands read `~/.claude.json` themselves, so a caller
    /// invoking this integration's methods directly (not via a spawned
    /// `uze` subprocess whose own environment was already isolated) must
    /// not have those commands silently fall back to the real `$HOME`.
    /// Derived from `claude_home`'s parent so an isolated test fixture
    /// (whose `claude_home` need not literally be `$HOME/.claude`) still
    /// gets a consistent, isolated value.
    command_home: PathBuf,
    uze_home: UzeHome,
}

impl ClaudeIntegration {
    pub fn new(claude_home: PathBuf, uze_home: UzeHome) -> Self {
        let command_home = claude_home
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| claude_home.clone());
        Self {
            skills_dir: claude_home.join("skills"),
            command_home,
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
            adaptable: [CapabilityKind::AgentSkill, CapabilityKind::Mcp]
                .into_iter()
                .collect(),
            verification: VerificationStatus::Unverified,
            evidence: "Claude Code auto-loads a skills-dir plugin from <claude_home>/skills/<name>/. A symlinked entry there was empirically confirmed loaded via `claude plugin validate`/`plugin list`. `claude mcp add --scope user` registers an MCP server globally, documented and confirmed non-interactive. Behavioral (prompted) verification for either remains a separate opt-in conformance probe."
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
        match resource.capability.kind {
            CapabilityKind::AgentSkill => self.skill_exposure_plan(resource),
            CapabilityKind::Mcp => self.mcp_exposure_plan(resource),
            _ => unsupported(
                resource,
                "Claude Code attachment is only modeled for Agent Skills and MCP servers.",
            ),
        }
    }

    fn attach(&self, resource: &Resource) -> Result<Option<PathBuf>> {
        let plan = self.exposure_plan(resource);
        match &plan.mechanism {
            ExposureMechanism::ManagedUserScopeReference { source, .. } => {
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
            ExposureMechanism::ManagedVendorConfig {
                entry_name,
                command,
                args,
            } => attach_mcp_entry(&self.command_home, entry_name, command, args),
            _ => Ok(None),
        }
    }
}

impl ClaudeIntegration {
    fn skill_exposure_plan(&self, resource: &Resource) -> ExposurePlan {
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

    fn mcp_exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        if !state::is_installed(&self.uze_home, self.id()) {
            return unsupported(
                resource,
                "Claude Code has not completed `uze setup`; MCP attachment has no per-session conformance-probe fallback (see ADR-007).",
            );
        }
        let Some(entry_name) = resource.attachment_entry_name() else {
            return unsupported(resource, "Resource has no derivable attachment entry name.");
        };
        let Some((command, args)) = parse_mcp_server_config(&resource.capability.payload) else {
            return unsupported(
                resource,
                "mcp.json server entry is missing a usable `command` field.",
            );
        };
        ExposurePlan {
            representation: resource.capability.representation,
            route: CompatibilityRoute::Adaptable,
            verification: VerificationStatus::Unverified,
            mechanism: ExposureMechanism::ManagedVendorConfig {
                entry_name,
                command,
                args,
            },
            evidence: "UZE registers the store-owned MCP server once via `claude mcp add --scope user --transport stdio`, writing to ~/.claude.json's mcpServers. Available to every future session in any project with no --plugin-dir-style flag."
                .to_owned(),
        }
    }
}

fn attach_mcp_entry(
    command_home: &Path,
    entry_name: &str,
    command: &Path,
    args: &[String],
) -> Result<Option<PathBuf>> {
    if mcp_entry_exists(command_home, entry_name) {
        return Ok(Some(PathBuf::from(format!("mcp:{entry_name}"))));
    }
    let status = Command::new("claude")
        .env("HOME", command_home)
        .args([
            "mcp",
            "add",
            "--scope",
            "user",
            "--transport",
            "stdio",
            entry_name,
            "--",
        ])
        .arg(command)
        .args(args)
        .status();
    match status {
        Ok(status) if status.success() => Ok(Some(PathBuf::from(format!("mcp:{entry_name}")))),
        Ok(status) => Err(UzeError::ExposureUnavailable(format!(
            "`claude mcp add` exited with {status} for entry `{entry_name}`"
        ))),
        Err(error) => Err(UzeError::ExposureUnavailable(format!(
            "failed to run `claude mcp add` for entry `{entry_name}`: {error}"
        ))),
    }
}

/// Idempotently checked before ever calling `claude mcp add` — Claude's
/// overwrite behavior for a colliding, differently-configured name was not
/// confirmed by research, so UZE never relies on it (see ADR-007).
fn mcp_entry_exists(command_home: &Path, entry_name: &str) -> bool {
    Command::new("claude")
        .env("HOME", command_home)
        .args(["mcp", "get", entry_name])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Removes a UZE-registered MCP entry. Not wired to a CLI verb yet — same
/// precedent as `ExposureMechanism::detach` for Agent Skills. Unused by the
/// `uze` binary for the same reason; exercised directly by
/// `tests/integration_contract.rs`. `command_home` is set explicitly as
/// `HOME` for the same reason `attach_mcp_entry` does — never relies on the
/// calling process's own environment.
#[allow(dead_code)]
pub fn detach_mcp_entry(command_home: &Path, entry_name: &str) -> Result<()> {
    let status = Command::new("claude")
        .env("HOME", command_home)
        .args(["mcp", "remove", entry_name])
        .status();
    match status {
        Ok(status) if status.success() => Ok(()),
        // Already absent is not an error — removal is idempotent.
        Ok(_) if !mcp_entry_exists(command_home, entry_name) => Ok(()),
        Ok(status) => Err(UzeError::ExposureUnavailable(format!(
            "`claude mcp remove` exited with {status} for entry `{entry_name}`"
        ))),
        Err(error) => Err(UzeError::ExposureUnavailable(format!(
            "failed to run `claude mcp remove` for entry `{entry_name}`: {error}"
        ))),
    }
}

/// Parses `{"command": "...", "args": [...]}` from a payload produced by
/// `UzeEngine`'s MCP resource discovery (one server's config object,
/// already extracted from `mcp.json`'s `mcpServers` map).
fn parse_mcp_server_config(payload: &[u8]) -> Option<(PathBuf, Vec<String>)> {
    let value: serde_json::Value = serde_json::from_slice(payload).ok()?;
    let command = value.get("command")?.as_str()?.to_owned();
    let args = value
        .get("args")
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Some((PathBuf::from(command), args))
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
