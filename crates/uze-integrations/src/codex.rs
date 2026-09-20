//! Codex peer integration. Its transparent-attachment strategy is a
//! UZE-managed reference at `<agents_home>/skills/<name>` (see ADR-006):
//! Codex documents a cwd-independent USER-scope Agent Skill directory that
//! explicitly follows symlinks. Until `uze setup` has completed, a Skill is
//! reported Unsupported with that instruction.
//!
//! Split by concern: [`mcp`] (MCP server registration/inspection),
//! [`skills`] (the managed skills-dir reference), [`plugin`] (the native
//! `.agents/plugins/marketplace.json` catalogue). This file is the
//! composition root: the `CodexIntegration` struct and its `IntegrationPort`
//! impl, delegating to each submodule.

use std::{fs, path::Path, path::PathBuf};

use crate::shared::plan::unsupported;
use uze_core::{
    Result, UzeError,
    capability::CapabilityKind,
    capability::Resource,
    exposure::{ExposureMechanism, ExposurePlan, PackageExposurePlan},
    home::UzeHome,
    integration::{
        AttachmentInspection, AttachmentReceipt, AttachmentState, ContextDelivery,
        HarnessDetection, IntegrationPort, ManagedArtifact, PublicationStatus, active_plugin_name,
        default_exposure_name_candidates, qualified_exposure_name_candidates,
    },
    preference::{
        PreferenceApplyOutcome, PreferencePlan, PreferencePort, PreferenceTranslation, Preferences,
    },
    provisioning::{ProcessRunner, ProcessSpec, ProvisioningResult},
    router::{CompatibilityRoute, HarnessCapabilities},
    state,
    store::StoredPackage,
};

mod generate;
mod mcp;
mod plugin;
mod preferences;
mod session;
mod skills;

pub use mcp::detach_mcp_entry;

use crate::hooks::{HookEntry, HookTarget};
use crate::shared::agent::agent_name;
use crate::shared::marketplace;
use crate::shared::process::{VersionToken, detect_version, real_executable};
use crate::shared::provision::provision_cli;
use crate::shared::skill::{head_value, split_frontmatter};
use mcp::attach_mcp_entry;
use plugin::CodexMarketplace;

/// Codex peer integration. Its transparent-attachment strategy is a
/// UZE-managed reference at `<agents_home>/skills/<name>` (see ADR-006):
/// Codex documents a cwd-independent USER-scope Agent Skill directory that
/// explicitly follows symlinks. Until `uze setup` has completed, a Skill is
/// reported Unsupported with that instruction.
#[derive(Clone)]
pub struct CodexIntegration {
    skills_dir: PathBuf,
    agents_dir: PathBuf,
    generated_agents_dir: PathBuf,
    /// `HOME` to set explicitly whenever a `codex` subcommand is shelled
    /// out to for MCP registration — see `ClaudeIntegration::command_home`
    /// for the full rationale; the same concern applies here since Codex
    /// derives `$CODEX_HOME` from `$HOME` by default.
    command_home: PathBuf,
    uze_home: UzeHome,
}

impl CodexIntegration {
    pub fn new(agents_home: PathBuf, uze_home: UzeHome) -> Self {
        let command_home = agents_home
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| agents_home.clone());
        Self {
            skills_dir: agents_home.join("skills"),
            agents_dir: command_home.join(".codex").join("agents"),
            generated_agents_dir: uze_home.generated_attachments_dir("codex").join("agents"),
            command_home,
            uze_home,
        }
    }

    /// The UZE-managed `hooks.json` at Codex's own config home — the
    /// standalone command-hook file Codex reads for its hook events
    /// (ADR-033). Only the `hooks` key is touched; foreign config files and
    /// entries are preserved.
    fn hooks_config_path(&self) -> PathBuf {
        self.command_home.join(".codex").join("hooks.json")
    }

    /// Where Codex files one rollout per conversation. Read only for the
    /// `session_meta` line that names a conversation and the directory it
    /// was started in — never for what was said in it.
    fn sessions_dir(&self) -> PathBuf {
        self.command_home.join(".codex").join("sessions")
    }

    /// Codex's own `config.toml` (`docs/config-file/config-basic`) — the
    /// user's shared config file. Only the keys UZE owns (`approval_policy`,
    /// `sandbox_mode`, `sandbox_workspace_write.network_access`) are ever
    /// touched; everything else, including comments, is preserved.
    fn config_toml_path(&self) -> PathBuf {
        self.command_home.join(".codex").join("config.toml")
    }

    /// Env-based constructor for the CLI composition root (`registry.rs`).
    pub fn from_env(uze_home: UzeHome) -> Result<Self> {
        let home = std::env::var_os("HOME").ok_or(UzeError::MissingHomeDirectory)?;
        Ok(Self::new(PathBuf::from(home).join(".agents"), uze_home))
    }

    fn provisioning_executable(&self) -> String {
        real_executable("codex", &self.uze_home.shims_dir(), None)
    }

    fn materialize_agent(&self, resource: &Resource) -> Result<PathBuf> {
        let name = agent_name(resource);
        let target = self.generated_agents_dir.join(format!("{name}.toml"));
        fs::create_dir_all(&self.generated_agents_dir).map_err(|source| UzeError::Write {
            path: self.generated_agents_dir.clone(),
            source,
        })?;
        let content = codex_agent_toml(resource, &name);
        match fs::read_to_string(&target) {
            Ok(existing) if existing == content => return Ok(target),
            Ok(_) => return Err(UzeError::ManagedEntryDrift(target)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(UzeError::Read {
                    path: target,
                    source,
                });
            }
        }
        fs::write(&target, content).map_err(|source| UzeError::Write {
            path: target.clone(),
            source,
        })?;
        Ok(target)
    }
}

impl IntegrationPort for CodexIntegration {
    fn id(&self) -> &'static str {
        "codex"
    }

    /// `codex` is both the stable id and the name people type — the label
    /// capitalizes the product name so every harness reads as one.
    fn display_name(&self) -> &'static str {
        "Codex"
    }

    fn description(&self) -> &'static str {
        "OpenAI's coding agent CLI"
    }

    fn invocation_prefix(&self) -> &'static str {
        "$"
    }

    /// Codex has no distinct mark of its own (no logo/icon file anywhere in
    /// openai/codex, only a README splash banner), so this is OpenAI's mark —
    /// simple-icons' `openai` path (CC0-1.0), replacing the vendor favicon
    /// whose white plate showed as a square on a dark page. It carries one
    /// dark fill rather than `currentColor`, because the docs matrix draws it
    /// through `<img>`: an SVG loaded that way is its own document and
    /// inherits no colour from the page. `global.css` inverts it for the dark
    /// theme.
    fn icon_path(&self) -> Option<&'static str> {
        Some("/harnesses/codex.svg")
    }

    fn homepage(&self) -> Option<&'static str> {
        Some("https://chatgpt.com/codex")
    }

    /// Reads the shared `AGENTS.md` natively (it is the origin harness for
    /// the convention); UZE maintains no artifact for it.
    fn context_delivery(&self) -> ContextDelivery {
        ContextDelivery::Native { files: &[] }
    }

    /// Codex's own Skills docs (developers.openai.com/codex/skills, 2026)
    /// document discovery from multiple scopes checked in order, including
    /// `./.agents/skills/`, `../.agents/skills/`, and `$REPO_ROOT/.agents/skills/`
    /// — a project-local convention read directly by the `codex` binary,
    /// with no UZE involvement, independent of the UZE-managed
    /// `$HOME/.agents/skills` symlink this integration writes elsewhere.
    fn discovers_project_agents_directory(&self) -> bool {
        true
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            native: [
                CapabilityKind::AgentSkill,
                CapabilityKind::Mcp,
                CapabilityKind::Agent,
                CapabilityKind::Hook,
            ]
                .into_iter()
                .collect(),
            evidence: "Codex consumes UZE's derived marketplaces: a package shipping .codex-plugin/plugin.json is added as a native plugin covering its declared skills/mcpServers (`codex plugin add <sel>@uze-local`); one without gets a deterministically synthesized envelope published through the generated-only `uze-store` marketplace (ADR-013) — both confirmed against real Codex 0.148.0 dogfood (`codex plugin list --json`). Canonical Agents are generated as Codex's documented standalone TOML files under ~/.codex/agents/, with name, description, and developer_instructions derived from the portable Markdown definition. Invocation policy is translated into Codex's own agents/openai.yaml → policy.allow_implicit_invocation: false for a canonical user-only Skill (Codex Build skills documentation; empirically honored by codex-cli 0.149.0 via `codex debug prompt-input`); the user=false combination is honestly Degraded since Codex has no documented way to disable explicit `$skill` invocation. Per ADR-030, Native means an officially supported primitive that preserves the canonical capability semantics — not an identical vendor file format. Portable Hooks are projected into Codex's own `~/.codex/hooks.json` command form as entries running the generated `hooks/exec` wrapper, which carries the portable ABI with no UZE binary on the execution path (ADR-040; deterministic emission, real-binary verification pending in the conformance lab). Capability-level fallbacks (USER-scope `~/.agents/skills` reference, `codex mcp add`) remain only for resources outside the envelope's coverage."
                .to_owned(),
            ..HarnessCapabilities::default()
        }
    }

    fn hook_capabilities(&self) -> uze_core::hook::HookCapabilities {
        HookTarget::Codex.capabilities()
    }

    fn session_continuity(&self) -> uze_core::integration::SessionContinuity {
        uze_core::integration::SessionContinuity::Observed
    }

    fn resume_session_args(
        &self,
        session: &uze_core::conversation::SessionId,
    ) -> Vec<std::ffi::OsString> {
        session::resume_args(session)
    }

    fn observe_session(
        &self,
        ctx: &uze_core::integration::ObservationContext,
    ) -> Option<uze_core::conversation::SessionId> {
        session::observe(&self.sessions_dir(), ctx)
    }

    fn session_exists(&self, session: &uze_core::conversation::SessionId, _cwd: &Path) -> bool {
        session::exists(&self.sessions_dir(), session)
    }

    fn detect(&self) -> HarnessDetection {
        codex_version(&self.provisioning_executable())
    }

    /// OpenCode also discovers Skills from this exact same
    /// `~/.agents/skills` directory; see `OpenCodeIntegration`'s override
    /// of the same method for why this must be reported.
    fn shared_agent_skill_root(&self) -> Option<PathBuf> {
        Some(self.skills_dir.clone())
    }

    fn provision(&self, runner: &dyn ProcessRunner) -> Result<ProvisioningResult> {
        let executable = self.provisioning_executable();
        provision_cli(
            runner,
            &executable,
            "Codex",
            self.detect(),
            ProcessSpec::new(
                "sh",
                ["-c", "curl -fsSL https://chatgpt.com/codex/install.sh | sh"],
            )
            .with_inherited_output(),
            // Real-CLI dogfood against codex-cli 0.148.0 found `--upgrade` is not
            // a recognized flag — `codex --help` lists `update` as a
            // subcommand instead.
            ProcessSpec::new(executable.clone(), ["update"]).with_inherited_output(),
            "official-native-installer",
            codex_version,
        )
    }

    fn install(&self, home: &UzeHome, detection: &HarnessDetection) -> Result<()> {
        fs::create_dir_all(&self.skills_dir).map_err(|source| UzeError::Write {
            path: self.skills_dir.clone(),
            source,
        })?;
        state::record(
            home,
            self.id(),
            state::IntegrationRecord {
                version: detection.version.clone(),
                strategy: "managed-user-scope-skills-dir".to_owned(),
            },
        )
    }

    fn exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        match resource.capability.kind {
            CapabilityKind::AgentSkill => self.skill_exposure_plan(resource),
            CapabilityKind::Mcp => self.mcp_exposure_plan(resource),
            CapabilityKind::Agent => self.agent_exposure_plan(resource),
            CapabilityKind::Hook => self.hook_exposure_plan(resource),
            CapabilityKind::Instruction => unsupported(
                "Codex attachment is only modeled for Agent Skills, Agents, MCP servers, and portable Hooks.",
            ),
        }
    }

    /// Codex's naming decision: every UZE-projected Skill gets its stable
    /// namespaced invocation label (`flow:review`) as the single candidate —
    /// never a bare alias, never collision-dependent naming (ADR-026). Codex
    /// accepts `:` in skill names (verified against codex-cli 0.149.0). MCP
    /// stays on the default fully-qualified policy.
    fn exposure_name_candidates(&self, resource: &Resource) -> Vec<String> {
        if resource.capability.kind == CapabilityKind::AgentSkill {
            let active_name = active_plugin_name(&self.uze_home, resource);
            return qualified_exposure_name_candidates(resource, &active_name);
        }
        default_exposure_name_candidates(resource)
    }

    fn package_exposure_plan(
        &self,
        package: &StoredPackage,
        resources: &[&Resource],
    ) -> Option<PackageExposurePlan> {
        marketplace::package_plan::<CodexMarketplace>(package, resources)
    }

    fn attach(&self, resource: &Resource) -> Result<Option<ManagedArtifact>> {
        let ExposureMechanism::Managed(artifact) = self.exposure_plan(resource).mechanism else {
            return Ok(None);
        };
        let attached = match &artifact {
            ManagedArtifact::SymlinkReference { .. } => {
                // Only materialize when this resource is the one that owns
                // the physical entry. When the shared-root resolution reused
                // another integration's receipt (resolved_artifact_target
                // set), the existing artifact is authoritative and nothing
                // new may replace it — but a user-only Skills must still
                // carry THIS integration's encoding, or the reuse would
                // silently drop the invocation policy (ADR-030 §25).
                if resource.capability.kind == CapabilityKind::AgentSkill {
                    self.materialize_or_verify_skill(resource)?;
                } else if resource.capability.kind == CapabilityKind::Agent {
                    self.materialize_agent(resource)?;
                }
                artifact.attach_standard()?;
                true
            }
            ManagedArtifact::VendorConfigEntry {
                entry_name,
                command,
                args,
                ..
            } => {
                let executable = self.provisioning_executable();
                attach_mcp_entry(
                    Path::new(&executable),
                    &self.command_home,
                    entry_name,
                    command,
                    args,
                )?;
                true
            }
            ManagedArtifact::HookConfigEntry {
                config_file,
                entry_name,
                event,
                expected,
                wrapper,
            } => {
                HookTarget::Codex.attach_entry(
                    &self.uze_home,
                    self.id(),
                    &HookEntry {
                        config_file,
                        entry_name,
                        event: *event,
                        expected,
                        wrapper,
                    },
                )?;
                true
            }
            _ => false,
        };
        Ok(attached.then_some(artifact))
    }

    fn attach_package(
        &self,
        package: &StoredPackage,
        _plan: &PackageExposurePlan,
    ) -> Result<Option<AttachmentReceipt>> {
        let executable = self.provisioning_executable();
        marketplace::attach_package::<CodexMarketplace>(
            Path::new(&executable),
            &self.command_home,
            &self.uze_home,
            self.id(),
            package,
        )
        .map(Some)
    }

    fn republish_packages(&self, packages: &[StoredPackage]) -> Result<()> {
        marketplace::republish::<CodexMarketplace>(&self.uze_home, packages)
    }

    fn publication(&self, packages: &[StoredPackage]) -> PublicationStatus {
        marketplace::publication::<CodexMarketplace>(&self.uze_home, packages)
    }

    fn inspect_receipt(&self, receipt: &AttachmentReceipt) -> AttachmentInspection {
        match &receipt.artifact {
            ManagedArtifact::VendorConfigEntry {
                entry_name,
                command,
                args,
                transport,
                cwd,
                environment,
                enabled,
            } => {
                let executable = self.provisioning_executable();
                mcp::inspect_codex_mcp(
                    Path::new(&executable),
                    &self.command_home,
                    entry_name,
                    transport,
                    command,
                    args,
                    cwd.as_deref(),
                    environment,
                    *enabled,
                )
            }
            ManagedArtifact::HookConfigEntry {
                config_file,
                entry_name,
                event,
                expected,
                wrapper,
            } => HookTarget::Codex.inspect_entry(&HookEntry {
                config_file,
                entry_name,
                event: *event,
                expected,
                wrapper,
            }),
            ManagedArtifact::IntegrationOwned {
                kind,
                selector,
                detail,
            } if marketplace::receipt_origin::<CodexMarketplace>(kind).is_some() => {
                let executable = self.provisioning_executable();
                marketplace::inspect_package::<CodexMarketplace>(
                    Path::new(&executable),
                    &self.command_home,
                    selector,
                    detail,
                )
            }
            _ => receipt.artifact.inspect_standard(),
        }
    }

    fn detach_receipt(&self, receipt: &AttachmentReceipt) -> Result<AttachmentInspection> {
        let inspection = self.inspect_receipt(receipt);
        if inspection.state != AttachmentState::Matched {
            return Ok(inspection);
        }
        match &receipt.artifact {
            ManagedArtifact::VendorConfigEntry { entry_name, .. } => {
                let executable = self.provisioning_executable();
                mcp::detach_mcp_entry(Path::new(&executable), &self.command_home, entry_name)?;
            }
            ManagedArtifact::HookConfigEntry {
                config_file,
                entry_name,
                event,
                expected,
                wrapper,
            } => {
                return HookTarget::Codex.detach_entry(
                    &self.uze_home,
                    self.id(),
                    &HookEntry {
                        config_file,
                        entry_name,
                        event: *event,
                        expected,
                        wrapper,
                    },
                );
            }
            ManagedArtifact::IntegrationOwned { kind, selector, .. }
                if let Some(origin) = marketplace::receipt_origin::<CodexMarketplace>(kind) =>
            {
                let executable = self.provisioning_executable();
                marketplace::detach_package::<CodexMarketplace>(
                    Path::new(&executable),
                    &self.command_home,
                    &self.uze_home,
                    receipt,
                    selector,
                    origin,
                )?;
            }
            _ => {
                let detached = receipt.artifact.detach_standard()?;
                if detached.state == AttachmentState::Missing
                    && let ManagedArtifact::SymlinkReference { target, .. } = &receipt.artifact
                {
                    self.cleanup_unused_wrapper(target)?;
                }
                return Ok(detached);
            }
        }
        Ok(AttachmentInspection {
            state: AttachmentState::Missing,
            reason: "Codex managed artifact detached".to_owned(),
        })
    }
}

impl CodexIntegration {
    fn agent_exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        let entry_name = agent_name(resource);
        ExposurePlan {
            route: CompatibilityRoute::Native,
            mechanism: ExposureMechanism::Managed(ManagedArtifact::SymlinkReference { path: self.agents_dir.clone().join(format!("{entry_name}.toml")), target: self.generated_agents_dir.join(format!("{entry_name}.toml")) }),
            evidence: "Codex natively loads standalone custom-agent TOML files from ~/.codex/agents/. UZE deterministically generates that native TOML from the portable Markdown definition and exposes it through a receipt-owned reference.".to_owned(),
        }
    }

    fn hook_exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        HookTarget::Codex.entry_plan(
            &self.uze_home,
            resource,
            self.hooks_config_path(),
            "Codex's own hooks.json command form reads PreToolUse/PostToolUse/Stop command hooks; UZE merges one group entry per canonical hook (matcher and timeout preserved) whose command is the generated `hooks/exec` wrapper — the handlers run against the portable HOOK_* contract with no UZE binary on the execution path — and keeps the exact entry receipt-owned.",
        )
    }
}

/// `codex --version` prints "codex-cli 0.148.0" — the version trails.
fn codex_version(program: &str) -> HarnessDetection {
    detect_version(program, VersionToken::Last)
}

impl PreferencePort for CodexIntegration {
    fn preference_id(&self) -> &'static str {
        IntegrationPort::id(self)
    }

    fn translate(&self, preferences: &Preferences) -> PreferenceTranslation {
        preferences::translate(preferences)
    }

    fn apply(&self, preferences: &Preferences) -> Result<PreferenceApplyOutcome> {
        preferences::apply(&self.config_toml_path(), preferences)
    }

    fn plan(&self, preferences: &Preferences) -> Result<PreferencePlan> {
        preferences::plan(&self.config_toml_path(), preferences)
    }
}

fn codex_agent_toml(resource: &Resource, fallback_name: &str) -> String {
    let markdown = String::from_utf8_lossy(&resource.capability.payload);
    let (frontmatter, instructions) = split_frontmatter(&markdown).unwrap_or(("", &markdown));
    let unquoted =
        |key| head_value(frontmatter, key).map(|value| value.trim_matches('"').trim_matches('\''));
    let name = unquoted("name").unwrap_or(fallback_name);
    let description = unquoted("description").unwrap_or("Portable UZE custom agent.");
    format!(
        "name = {}\ndescription = {}\ndeveloper_instructions = {}\n",
        toml_string(name),
        toml_string(description),
        toml_string(instructions.trim()),
    )
}

fn toml_string(value: &str) -> String {
    serde_json::to_string(value).expect("strings are JSON serializable")
}
