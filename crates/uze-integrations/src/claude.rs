use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use uze_core::{
    Result, UzeError,
    capability::CapabilityKind,
    exposure::{ExposureMechanism, ExposurePlan},
    harness_runtime::{self, HarnessRuntimeContribution, RuntimeContext},
    home::UzeHome,
    integration::{
        AttachmentInspection, AttachmentReceipt, AttachmentState, HarnessDetection,
        IntegrationPort, ManagedArtifact, default_exposure_name_candidates,
        detach_standard_receipt, inspect_standard_receipt,
        short_then_qualified_exposure_name_candidates,
    },
    project::Resource,
    provisioning::{ProcessRunner, ProcessSpec, ProvisionAction, ProvisioningResult},
    router::{CompatibilityRoute, HarnessCapabilities, VerificationStatus},
    state,
};

/// The vendor-documented (but undocumented-in-`--help`, empirically
/// confirmed — see the Checkpoint report) environment variable that makes
/// Claude Code treat a `--add-dir` directory's `CLAUDE.md` as loaded
/// instructions rather than only granting tool/file access to it.
const RUNTIME_PROJECTION_ENV_VAR: &str = "CLAUDE_CODE_ADDITIONAL_DIRECTORIES_CLAUDE_MD";

/// `EXPERIMENTAL RUNTIME DELIVERY STRATEGY` — see `CONTEXT DELIVERY POLICY`
/// note on `runtime_contribution` below. This is intentionally not wired
/// into `exposure_plan`/`attach` (the persistent, project-root `CLAUDE.md`
/// bridge that `uze context reconcile` still owns, see
/// `uze-application::application::BRIDGE_INTEGRATIONS` — that remains the
/// `LEGACY/PERSISTENT CONTEXT DELIVERY STRATEGY` until an empirical
/// interactive comparison decides otherwise).
///
/// Builds (or refreshes) `$UZE_HOME/runtime/claude-code/projects/<id>/
/// CLAUDE.md` importing the current project's `AGENTS.md`, entirely outside
/// the project's own working tree. Returns `Ok(None)` when `ctx.cwd` is not
/// inside a project that has an `AGENTS.md` at all — that is not an error,
/// it is the correct passthrough case. Every fallible step returns `Err`
/// with a short, human-readable reason instead of a typed `UzeError`,
/// because the only thing the caller (`runtime_contribution`) ever does
/// with it is fold it into a fail-open passthrough note.
fn claude_runtime_projection(ctx: &RuntimeContext) -> std::result::Result<Option<PathBuf>, String> {
    let Some(agents_md) = harness_runtime::discover_project_agents_md(ctx.cwd) else {
        return Ok(None);
    };
    let agents_md = agents_md
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let project_root = agents_md
        .parent()
        .ok_or_else(|| "AGENTS.md has no parent directory".to_owned())?;
    let project_id = harness_runtime::project_id_for(project_root);
    let runtime_dir = ctx.home.runtime_projection_dir("claude-code", &project_id);
    fs::create_dir_all(&runtime_dir).map_err(|error| error.to_string())?;

    let claude_md = runtime_dir.join("CLAUDE.md");
    let desired = format!("@{}\n", agents_md.display());
    // Idempotent: two concurrent sessions on the same project compute the
    // same `project_id`, the same `runtime_dir`, and the same content —
    // there is nothing to reference-count or coordinate. A same-content
    // write is skipped entirely so a second session never even touches the
    // file the first one is using.
    let already_current = fs::read_to_string(&claude_md)
        .map(|current| current == desired)
        .unwrap_or(false);
    if !already_current {
        uze_core::persistence::write_atomic(&claude_md, desired.as_bytes())
            .map_err(|error| error.to_string())?;
    }
    Ok(Some(runtime_dir))
}

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

    /// `claude` is the name people type; `claude-code` is the stable id the
    /// receipts and state records carry.
    fn aliases(&self) -> &'static [&'static str] {
        &["claude"]
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

    /// `CONTEXT DELIVERY POLICY`: this is the `EXPERIMENTAL RUNTIME
    /// DELIVERY STRATEGY` — see `claude_runtime_projection`'s doc comment.
    /// Building this shim path does not by itself replace the existing
    /// project-root `CLAUDE.md` bridge; that decision waits on the
    /// interactive dogfood comparison in the Checkpoint 2 report.
    fn runtime_contribution(&self, ctx: &RuntimeContext) -> HarnessRuntimeContribution {
        match claude_runtime_projection(ctx) {
            Ok(Some(runtime_dir)) => HarnessRuntimeContribution {
                extra_args: vec![OsString::from("--add-dir"), runtime_dir.into_os_string()],
                extra_env: vec![(
                    OsString::from(RUNTIME_PROJECTION_ENV_VAR),
                    OsString::from("1"),
                )],
                note: None,
            },
            Ok(None) => HarnessRuntimeContribution::passthrough(),
            Err(reason) => HarnessRuntimeContribution::passthrough_with_note(reason),
        }
    }

    fn supports_runtime_integration(&self) -> bool {
        true
    }

    fn provision(&self, runner: &dyn ProcessRunner) -> Result<ProvisioningResult> {
        provision_cli(
            runner,
            "claude",
            self.detect(),
            ProcessSpec::new(
                "sh",
                ["-c", "curl -fsSL https://claude.ai/install.sh | bash"],
            )
            .with_inherited_output(),
            ProcessSpec::new("claude", ["update"]).with_inherited_output(),
            "official-native-installer",
        )
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

    /// Claude Code is the one integration where a decomposed Skill's
    /// physical directory name is what the user actually types
    /// (`/<name>` — see `docs/capabilities/uze-skill.md`), so it alone
    /// tries the bare logical name first. MCP deliberately stays on the
    /// shared default (fully qualified only): its physical name never
    /// reaches the terminal UX the same way, and mixing the two policies
    /// just because both are `Resource`s would be exactly the "não
    /// misture Skill e MCP" mistake the design explicitly rules out.
    fn exposure_name_candidates(&self, resource: &Resource) -> Vec<String> {
        if resource.capability.kind != CapabilityKind::AgentSkill {
            return default_exposure_name_candidates(resource);
        }
        short_then_qualified_exposure_name_candidates(resource)
    }

    fn attach(&self, resource: &Resource) -> Result<Option<PathBuf>> {
        let plan = self.exposure_plan(resource);
        match &plan.mechanism {
            ExposureMechanism::ManagedUserScopeReference {
                source, entry_name, ..
            } => {
                let skill_source_dir = resource
                    .capability
                    .path
                    .parent()
                    .expect("SKILL.md has a parent");
                // Read directly from the plan just built, rather than
                // recomputing: the materialized shim's name must match
                // exactly what the mechanism is about to place at, and the
                // plan is already the single source of truth for that.
                materialize_shim(source, skill_source_dir, entry_name)?;
                Ok(Some(plan.mechanism.attach()?))
            }
            ExposureMechanism::ManagedVendorConfig {
                entry_name,
                command,
                args,
                ..
            } => attach_mcp_entry(&self.command_home, entry_name, command, args),
            _ => Ok(None),
        }
    }

    fn inspect_receipt(&self, receipt: &AttachmentReceipt) -> AttachmentInspection {
        let ManagedArtifact::VendorConfigEntry {
            entry_name,
            command,
            args,
            transport,
            cwd,
            environment,
            enabled,
        } = &receipt.artifact
        else {
            return inspect_standard_receipt(receipt);
        };
        inspect_claude_mcp(
            &self.command_home.join(".claude.json"),
            entry_name,
            transport,
            command,
            args,
            cwd.as_deref(),
            environment,
            *enabled,
        )
    }

    fn detach_receipt(&self, receipt: &AttachmentReceipt) -> Result<AttachmentInspection> {
        let inspection = self.inspect_receipt(receipt);
        if inspection.state != AttachmentState::Matched {
            return Ok(inspection);
        }
        let ManagedArtifact::VendorConfigEntry { entry_name, .. } = &receipt.artifact else {
            let detached = detach_standard_receipt(receipt)?;
            if detached.state == AttachmentState::Missing
                && let ManagedArtifact::SymlinkReference { target, .. } = &receipt.artifact
            {
                self.cleanup_unused_shim(target)?;
            }
            return Ok(detached);
        };
        detach_mcp_entry(&self.command_home, entry_name)?;
        Ok(AttachmentInspection {
            state: AttachmentState::Missing,
            reason: "Claude managed MCP entry detached via CLI".to_owned(),
        })
    }
}

fn provision_cli(
    runner: &dyn ProcessRunner,
    executable: &str,
    before: HarnessDetection,
    install: ProcessSpec,
    update: ProcessSpec,
    method: &str,
) -> Result<ProvisioningResult> {
    if !cfg!(unix) {
        return Ok(ProvisioningResult::blocked(
            "Claude Code automatic provisioning is currently supported on Unix and WSL only",
        ));
    }
    let action = if before.present {
        ProvisionAction::Update
    } else {
        ProvisionAction::Install
    };
    let command = if before.present { update } else { install };
    let outcome = match runner.run(&command) {
        Ok(outcome) => outcome,
        Err(_) => {
            return Ok(ProvisioningResult::failed(
                action,
                method,
                "official installer could not be started",
            ));
        }
    };
    if !outcome.success {
        let reason = if outcome.timed_out {
            "official installer timed out"
        } else {
            "official installer exited unsuccessfully"
        };
        return Ok(ProvisioningResult::failed(action, method, reason));
    }
    let verified = runner.run(&ProcessSpec::new(executable, ["--version"]));
    if !matches!(verified, Ok(output) if output.success) {
        return Ok(ProvisioningResult::failed(
            action,
            method,
            "installer finished but the executable could not be verified",
        ));
    }
    Ok(ProvisioningResult::verified(
        action,
        method,
        detect_binary(executable),
    ))
}

impl ClaudeIntegration {
    fn cleanup_unused_shim(&self, shim_root: &Path) -> Result<()> {
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

    fn skill_exposure_plan(&self, resource: &Resource) -> ExposurePlan {
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

    fn mcp_exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        if !state::is_installed(&self.uze_home, self.id()) {
            return unsupported(
                resource,
                "Claude Code has not completed `uze setup`; MCP attachment has no per-session conformance-probe fallback (see ADR-007).",
            );
        }
        let Some(entry_name) = resource
            .resolved_exposure_name
            .clone()
            .or_else(|| self.exposure_name_candidates(resource).into_iter().next())
        else {
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
                transport: "stdio".to_owned(),
                command,
                args,
                cwd: None,
                environment: Vec::new(),
                enabled: None,
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

/// Claude has no structured `mcp get` output. This is deliberately read-only:
/// attachment/removal still go through the official CLI, while inspection
/// reads only the one expected `mcpServers.<name>` entry.
#[allow(clippy::too_many_arguments)]
fn inspect_claude_mcp(
    path: &Path,
    entry_name: &str,
    transport: &str,
    command: &Path,
    args: &[String],
    cwd: Option<&Path>,
    environment: &[uze_core::exposure::McpEnvironmentReference],
    enabled: Option<bool>,
) -> AttachmentInspection {
    if transport != "stdio" || cwd.is_some() || !environment.is_empty() || enabled.is_some() {
        return AttachmentInspection {
            state: AttachmentState::Blocked,
            reason: "Claude MCP receipt requests state this integration cannot verify safely"
                .to_owned(),
        };
    }
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return AttachmentInspection {
                state: AttachmentState::Missing,
                reason: "Claude config is missing".to_owned(),
            };
        }
        Err(error) => {
            return AttachmentInspection {
                state: AttachmentState::Blocked,
                reason: error.to_string(),
            };
        }
    };
    let config: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(_) => {
            return AttachmentInspection {
                state: AttachmentState::Blocked,
                reason: "Claude config is malformed".to_owned(),
            };
        }
    };
    let Some(entry) = config
        .get("mcpServers")
        .and_then(serde_json::Value::as_object)
        .and_then(|servers| servers.get(entry_name))
    else {
        return AttachmentInspection {
            state: AttachmentState::Missing,
            reason: "Claude MCP entry is missing".to_owned(),
        };
    };
    let command_matches = entry.get("command").and_then(serde_json::Value::as_str)
        == Some(command.to_string_lossy().as_ref());
    let args_match = entry
        .get("args")
        .and_then(serde_json::Value::as_array)
        .map(|actual| {
            actual
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
                == args.iter().map(String::as_str).collect::<Vec<_>>()
        })
        .unwrap_or(args.is_empty());
    if command_matches && args_match {
        AttachmentInspection {
            state: AttachmentState::Matched,
            reason: "Claude MCP entry matches receipt".to_owned(),
        }
    } else {
        AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "Claude MCP command or args differ from receipt".to_owned(),
        }
    }
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
    // `claude --version` prints "2.1.239 (Claude Code)" — the version leads.
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

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use std::sync::Mutex;
    use uze_core::provisioning::{ProcessResult, ProcessRunner};

    struct RecordingRunner {
        commands: Mutex<Vec<ProcessSpec>>,
    }

    impl ProcessRunner for RecordingRunner {
        fn run(&self, spec: &ProcessSpec) -> Result<ProcessResult> {
            self.commands.lock().unwrap().push(spec.clone());
            Ok(ProcessResult {
                success: true,
                timed_out: false,
            })
        }
    }

    #[test]
    fn missing_harness_uses_its_documented_official_install_route_then_verifies() {
        let runner = RecordingRunner {
            commands: Mutex::new(Vec::new()),
        };
        let result = provision_cli(
            &runner,
            "claude-test-does-not-exist",
            HarnessDetection::default(),
            ProcessSpec::new("sh", ["-c", "official-install"]),
            ProcessSpec::new("claude", ["update"]),
            "official-native-installer",
        )
        .unwrap();
        if cfg!(unix) {
            assert_eq!(result.action, ProvisionAction::Install);
            assert_eq!(
                result.status,
                uze_core::provisioning::ProvisionStatus::Verified
            );
            let commands = runner.commands.lock().unwrap();
            assert_eq!(commands[0].program, "sh");
            assert_eq!(
                commands[0].output,
                uze_core::provisioning::ProcessOutput::Quiet,
                "the helper's synthetic test command is intentionally quiet"
            );
            assert_eq!(commands[1].program, "claude-test-does-not-exist");
            assert_eq!(commands[1].arguments, ["--version"]);
        }
    }
    fn check(value: &str) -> AttachmentState {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("uze-claude-config-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join(".claude.json");
        fs::write(&path, value).unwrap();
        let state = inspect_claude_mcp(
            &path,
            "uze-x",
            "stdio",
            Path::new("tool"),
            &["a".to_owned()],
            None,
            &[],
            None,
        )
        .state;
        let _ = fs::remove_dir_all(root);
        state
    }
    #[test]
    fn intact_is_matched_and_unknown_fields_tolerated() {
        assert_eq!(
            check(
                r#"{"mcpServers":{"uze-x":{"command":"tool","args":["a"],"extra":true},"other":{"command":"x"}},"unknown":1}"#
            ),
            AttachmentState::Matched
        );
    }
    #[test]
    fn absent_is_missing() {
        assert_eq!(check(r#"{"mcpServers":{}}"#), AttachmentState::Missing);
    }
    #[test]
    fn command_or_args_change_is_drifted() {
        assert_eq!(
            check(r#"{"mcpServers":{"uze-x":{"command":"other","args":["a"]}}}"#),
            AttachmentState::Drifted
        );
        assert_eq!(
            check(r#"{"mcpServers":{"uze-x":{"command":"tool","args":["b"]}}}"#),
            AttachmentState::Drifted
        );
    }
    #[test]
    fn malformed_is_blocked() {
        assert_eq!(check("{bad"), AttachmentState::Blocked);
    }

    #[cfg(unix)]
    #[test]
    fn detaching_a_skill_reference_cleans_an_unreferenced_owned_shim() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!("uze-claude-shim-{}", std::process::id()));
        let uze_home = UzeHome::at(root.join("uze"));
        let integration = ClaudeIntegration::new(root.join("claude"), uze_home.clone());
        fs::create_dir_all(&integration.skills_dir).unwrap();
        let shim = uze_home.state_dir().join("attachments/claude/uze-example");
        fs::create_dir_all(shim.join(".claude-plugin")).unwrap();
        fs::write(shim.join(".claude-plugin/plugin.json"), "{}").unwrap();
        let source = root.join("source/SKILL.md");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, "skill").unwrap();
        symlink(&source, shim.join("SKILL.md")).unwrap();
        let reference = integration.skills_dir.join("uze-example");
        symlink(&shim, &reference).unwrap();
        let receipt = AttachmentReceipt {
            package_id: "example".to_owned(),
            resource_identity: Some("skill:example".to_owned()),
            integration: integration.id().to_owned(),
            strategy: "managed-user-scope-reference".to_owned(),
            artifact: ManagedArtifact::SymlinkReference {
                path: reference,
                target: shim.clone(),
            },
        };
        assert_eq!(
            integration.detach_receipt(&receipt).unwrap().state,
            AttachmentState::Missing
        );
        assert!(!shim.exists());
        let _ = fs::remove_dir_all(root);
    }
}

#[cfg(test)]
mod runtime_projection_tests {
    use super::*;

    fn scratch_dir(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "uze-claude-runtime-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn no_agents_md_is_pure_passthrough() {
        let root = scratch_dir("no-agents-md");
        let home = UzeHome::at(root.join("uze-home"));
        let ctx = RuntimeContext {
            cwd: &root,
            home: &home,
        };
        let contribution = ClaudeIntegration::new(root.join("claude-home"), home.clone())
            .runtime_contribution(&ctx);
        assert!(contribution.is_passthrough());
        assert!(contribution.note.is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn agents_md_projects_an_import_and_the_project_working_tree_stays_untouched() {
        let root = scratch_dir("with-agents-md");
        let project = root.join("project");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("AGENTS.md"), "canary content\n").unwrap();
        let home = UzeHome::at(root.join("uze-home"));
        let ctx = RuntimeContext {
            cwd: &project,
            home: &home,
        };

        let contribution = ClaudeIntegration::new(root.join("claude-home"), home.clone())
            .runtime_contribution(&ctx);
        assert!(contribution.note.is_none(), "{:?}", contribution.note);
        assert_eq!(
            contribution.extra_env,
            vec![(
                OsString::from(RUNTIME_PROJECTION_ENV_VAR),
                OsString::from("1"),
            )]
        );
        assert_eq!(contribution.extra_args[0], OsString::from("--add-dir"));
        let runtime_dir = PathBuf::from(&contribution.extra_args[1]);
        assert!(runtime_dir.starts_with(home.runtime_dir()));

        let claude_md = fs::read_to_string(runtime_dir.join("CLAUDE.md")).unwrap();
        let canonical_agents_md = project.join("AGENTS.md").canonicalize().unwrap();
        assert_eq!(claude_md, format!("@{}\n", canonical_agents_md.display()));

        // The project's own working tree must never gain a file from this.
        let project_entries: Vec<_> = fs::read_dir(&project)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(project_entries, vec![std::ffi::OsString::from("AGENTS.md")]);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn repeated_projection_for_the_same_project_is_idempotent() {
        let root = scratch_dir("idempotent");
        let project = root.join("project");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("AGENTS.md"), "v1\n").unwrap();
        let home = UzeHome::at(root.join("uze-home"));
        let ctx = RuntimeContext {
            cwd: &project,
            home: &home,
        };
        let integration = ClaudeIntegration::new(root.join("claude-home"), home.clone());

        let first = integration.runtime_contribution(&ctx);
        let second = integration.runtime_contribution(&ctx);
        assert_eq!(first, second, "same project must yield the same plan");

        let runtime_dir = PathBuf::from(&first.extra_args[1]);
        // Same project id both times: no second directory was created.
        let projects_dir = runtime_dir.parent().unwrap();
        let entries: Vec<_> = fs::read_dir(projects_dir).unwrap().collect();
        assert_eq!(entries.len(), 1);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn project_path_containing_spaces_is_handled_safely() {
        let root = scratch_dir("spaces");
        let project = root.join("a project with spaces");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("AGENTS.md"), "canary\n").unwrap();
        let home = UzeHome::at(root.join("uze-home"));
        let ctx = RuntimeContext {
            cwd: &project,
            home: &home,
        };

        let contribution = ClaudeIntegration::new(root.join("claude-home"), home.clone())
            .runtime_contribution(&ctx);
        assert!(contribution.note.is_none(), "{:?}", contribution.note);
        let runtime_dir = PathBuf::from(&contribution.extra_args[1]);
        let claude_md = fs::read_to_string(runtime_dir.join("CLAUDE.md")).unwrap();
        assert!(claude_md.contains("a project with spaces"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn unwritable_runtime_dir_falls_open_to_passthrough_with_a_note() {
        let root = scratch_dir("unwritable");
        let project = root.join("project");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("AGENTS.md"), "canary\n").unwrap();
        let home = UzeHome::at(root.join("uze-home"));

        // Occupy the exact path the projection needs as a *file* instead of
        // a directory, so `create_dir_all` fails deterministically without
        // needing real permission games.
        let agents_md = project.join("AGENTS.md").canonicalize().unwrap();
        let project_id = harness_runtime::project_id_for(agents_md.parent().unwrap());
        let blocked_path = home.runtime_projection_dir("claude-code", &project_id);
        fs::create_dir_all(blocked_path.parent().unwrap()).unwrap();
        fs::write(&blocked_path, b"not a directory").unwrap();

        let ctx = RuntimeContext {
            cwd: &project,
            home: &home,
        };
        let contribution = ClaudeIntegration::new(root.join("claude-home"), home.clone())
            .runtime_contribution(&ctx);
        assert!(contribution.is_passthrough());
        assert!(contribution.note.is_some(), "expected a fail-open note");

        let _ = fs::remove_dir_all(&root);
    }
}
