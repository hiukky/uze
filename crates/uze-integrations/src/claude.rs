//! Claude Code peer integration. Its transparent-attachment strategy is a
//! UZE-managed "skills-dir plugin" reference at `<claude_home>/skills/<name>`
//! (see ADR-006): Claude auto-loads any directory there containing
//! `.claude-plugin/plugin.json` + `SKILL.md` at the start of every session,
//! with no per-session flag. Until `uze setup` has completed, a Skill is
//! reported Unsupported with that instruction.
//!
//! Split by concern: [`mcp`] (MCP server registration/inspection),
//! [`skills`] (the managed skills-dir shim), [`plugin`] (the native
//! `.claude-plugin/marketplace.json` catalogue and its exact-coverage
//! computation), and [`runtime`] (the experimental `--add-dir` runtime
//! projection). This
//! file is the composition root: the `ClaudeIntegration` struct and its
//! `IntegrationPort` impl, delegating to each submodule.

use std::{fs, path::Path};

use crate::shared::plan::unsupported;
use uze_core::{
    Result, UzeError,
    capability::CapabilityKind,
    capability::Resource,
    exposure::{ExposureMechanism, ExposurePlan, PackageExposurePlan},
    harness_runtime::RuntimeContext,
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
    router::HarnessCapabilities,
    state,
    store::StoredPackage,
};

mod generate;
mod mcp;
mod plugin;
mod preferences;
mod runtime;
mod session;
mod skills;

pub use mcp::detach_mcp_entry;

use crate::hooks::{HookEntry, HookTarget};
use crate::shared::agent::{agent_name, markdown_agent_plan};
use crate::shared::marketplace;
use crate::shared::process::{VersionToken, detect_version, real_executable};
use crate::shared::provision::provision_cli;
use mcp::attach_mcp_entry;
use plugin::ClaudeMarketplace;
use skills::materialize_shim;
/// Claude Code peer integration. Its transparent-attachment strategy is a
/// UZE-managed "skills-dir plugin" reference at `<claude_home>/skills/<name>`
/// (see ADR-006): Claude auto-loads any directory there containing
/// `.claude-plugin/plugin.json` + `SKILL.md` at the start of every session,
/// with no per-session flag. Until `uze setup` has completed, a Skill is
/// reported Unsupported with that instruction.
#[derive(Clone)]
pub struct ClaudeIntegration {
    skills_dir: std::path::PathBuf,
    agents_dir: std::path::PathBuf,
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
    command_home: std::path::PathBuf,
    /// The user-scope settings file carrying the `hooks` key Claude Code
    /// reads (ADR-033): `<claude_home>/settings.json` — inside the Claude
    /// config directory, not the HOME root where `~/.claude.json` lives.
    hooks_config: std::path::PathBuf,
    /// Where Claude Code keeps one transcript directory per working
    /// directory. Read only to prove a recorded conversation still exists
    /// and to notice the agent moving to another one — never for content.
    transcripts_root: std::path::PathBuf,
    uze_home: UzeHome,
}

impl ClaudeIntegration {
    pub fn new(claude_home: std::path::PathBuf, uze_home: UzeHome) -> Self {
        let command_home = claude_home
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| claude_home.clone());
        Self {
            skills_dir: claude_home.join("skills"),
            agents_dir: claude_home.join("agents"),
            command_home,
            hooks_config: claude_home.join("settings.json"),
            transcripts_root: claude_home.join("projects"),
            uze_home,
        }
    }

    /// The shared user-scope settings file whose `hooks` key receives every
    /// managed group entry (ADR-033): Claude Code documents hook
    /// configuration through `settings.json`, and UZE namespaces its
    /// entries by exact content so foreign hooks and ordering never change.
    fn hooks_config_path(&self) -> std::path::PathBuf {
        self.hooks_config.clone()
    }

    /// Env-based constructor for the CLI composition root (`registry.rs`).
    pub fn from_env(uze_home: UzeHome) -> Result<Self> {
        let home = std::env::var_os("HOME").ok_or(UzeError::MissingHomeDirectory)?;
        Ok(Self::new(
            std::path::PathBuf::from(home).join(".claude"),
            uze_home,
        ))
    }

    fn provisioning_executable(&self) -> String {
        real_executable("claude", &self.uze_home.shims_dir(), None)
    }
}

impl IntegrationPort for ClaudeIntegration {
    fn id(&self) -> &'static str {
        "claude-code"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["claude"]
    }

    fn display_name(&self) -> &'static str {
        "Claude Code"
    }

    fn description(&self) -> &'static str {
        "Anthropic's official coding agent CLI"
    }

    fn invocation_prefix(&self) -> &'static str {
        "/"
    }

    /// simple-icons' `claudecode` mark (CC0-1.0), filled with Claude's own
    /// `#D97757`. Not `currentColor`: the docs matrix renders this through
    /// `<img>`, and an SVG loaded that way is its own document — it inherits
    /// no colour from the page, so `currentColor` resolved to black and the
    /// mark vanished on a dark theme. A brand colour is legible on both.
    fn icon_path(&self) -> Option<&'static str> {
        Some("/harnesses/claude-code.svg")
    }

    fn homepage(&self) -> Option<&'static str> {
        Some("https://code.claude.com")
    }

    /// Reads project context only through the `@AGENTS.md` bridge region in
    /// `CLAUDE.md` — the vendor's own documented interop path.
    fn context_delivery(&self) -> ContextDelivery {
        ContextDelivery::Bridge {
            file_name: "CLAUDE.md",
        }
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
            evidence: "Claude Code consumes UZE's derived marketplaces: a package shipping .claude-plugin/plugin.json is installed as a native plugin covering its declared skills/mcpServers (`claude plugin install <sel>@uze-local`, empirically confirmed via `claude plugin validate`/`plugin list`); one without gets a deterministically synthesized envelope published through the generated-only `uze-store` marketplace (ADR-013). Invocation policy is translated into Claude's own SKILL.md frontmatter (disable-model-invocation / user-invocable — both verified against the current Claude Code skill docs); an explicit-envelope Skill is only claimed as covered when its canonical policy is actually preserved by the vendor content it ships. Capability-level shims (`<claude_home>/skills` reference, `claude mcp add`) remain only as fallback for resources outside the envelope's coverage. Portable Hooks are projected into the `hooks` key of the user settings file as entries running the generated `hooks/exec` wrapper, which carries the portable ABI with no UZE binary on the execution path (ADR-040; deterministic emission, real-binary verification pending in the conformance lab). Behavioral (prompted) verification remains a separate opt-in conformance probe."
                .to_owned(),
            ..HarnessCapabilities::default()
        }
    }

    fn hook_capabilities(&self) -> uze_core::hook::HookCapabilities {
        HookTarget::Claude.capabilities()
    }

    fn detect(&self) -> HarnessDetection {
        claude_version(&self.provisioning_executable())
    }

    /// `id()` is `claude-code`; the binary people actually have on `PATH`
    /// is `claude`.
    fn detection_program_candidates(&self) -> Vec<&'static str> {
        vec!["claude"]
    }

    /// `CONTEXT DELIVERY POLICY`: this is the `EXPERIMENTAL RUNTIME
    /// DELIVERY STRATEGY` — see `runtime::claude_runtime_projection`'s doc
    /// comment. Building this shim path does not by itself replace the
    /// existing project-root `CLAUDE.md` bridge; that decision waits on an
    /// empirical interactive comparison.
    fn runtime_contribution(
        &self,
        ctx: &RuntimeContext,
    ) -> uze_core::harness_runtime::HarnessRuntimeContribution {
        runtime::runtime_contribution(ctx)
    }

    fn session_continuity(&self) -> uze_core::integration::SessionContinuity {
        uze_core::integration::SessionContinuity::Assigned
    }

    fn start_session_args(
        &self,
        session: &uze_core::conversation::SessionId,
    ) -> Vec<std::ffi::OsString> {
        session::start_args(session)
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
        session::observe(&self.transcripts_root, ctx)
    }

    fn session_exists(&self, session: &uze_core::conversation::SessionId, cwd: &Path) -> bool {
        session::exists(&self.transcripts_root, cwd, session)
    }

    /// Pure check for status views: `claude_runtime_projection` writes its
    /// files as an idempotent refresh, so the launch path and the status
    /// path must not share the write — the popup asks this instead of the
    /// real contribution (see `runtime::projection_would_activate`).
    fn runtime_contribution_would_activate(&self, ctx: &RuntimeContext) -> bool {
        runtime::projection_would_activate(ctx)
    }

    fn runtime_projects_project_context(&self) -> bool {
        true
    }

    fn provision(&self, runner: &dyn ProcessRunner) -> Result<ProvisioningResult> {
        let executable = self.provisioning_executable();
        provision_cli(
            runner,
            &executable,
            "Claude Code",
            self.detect(),
            ProcessSpec::new(
                "sh",
                ["-c", "curl -fsSL https://claude.ai/install.sh | bash"],
            )
            .with_inherited_output(),
            ProcessSpec::new(executable.clone(), ["update"]).with_inherited_output(),
            "official-native-installer",
            claude_version,
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
                "Claude Code attachment is only modeled for Agent Skills, Agents, MCP servers, and portable Hooks.",
            ),
        }
    }

    /// Claude namespaces plugin skills/commands itself (`/flow:review` for a
    /// plugin named `flow` — see `docs/capabilities/skill-invocation-policy.md`), so UZE
    /// never materializes the namespace into the plugin: the plugin declares
    /// the plain logical name and Claude owns the `plugin:` prefix. For the
    /// capability-level fallback shim the physical directory name is the
    /// stable namespaced label (ADR-026). MCP deliberately stays on the
    /// shared default (fully qualified only) — its physical name never
    /// reaches the terminal UX the same way, and mixing naming policies just
    /// because both are `Resource`s would be exactly the "não misture Skill
    /// e MCP" mistake the design explicitly rules out.
    fn exposure_name_candidates(&self, resource: &Resource) -> Vec<String> {
        if resource.capability.kind != CapabilityKind::AgentSkill {
            return default_exposure_name_candidates(resource);
        }
        let active_name = active_plugin_name(&self.uze_home, resource);
        qualified_exposure_name_candidates(resource, &active_name)
    }

    fn package_exposure_plan(
        &self,
        package: &StoredPackage,
        resources: &[&Resource],
    ) -> Option<PackageExposurePlan> {
        marketplace::package_plan::<ClaudeMarketplace>(package, resources)
    }

    fn attach_package(
        &self,
        package: &StoredPackage,
        _plan: &PackageExposurePlan,
    ) -> Result<Option<AttachmentReceipt>> {
        let executable = self.provisioning_executable();
        marketplace::attach_package::<ClaudeMarketplace>(
            Path::new(&executable),
            &self.command_home,
            &self.uze_home,
            self.id(),
            package,
        )
        .map(Some)
    }

    fn republish_packages(&self, packages: &[StoredPackage]) -> Result<()> {
        marketplace::republish::<ClaudeMarketplace>(&self.uze_home, packages)
    }

    fn publication(&self, packages: &[StoredPackage]) -> PublicationStatus {
        marketplace::publication::<ClaudeMarketplace>(&self.uze_home, packages)
    }

    fn attach(&self, resource: &Resource) -> Result<Option<ManagedArtifact>> {
        let ExposureMechanism::Managed(artifact) = self.exposure_plan(resource).mechanism else {
            return Ok(None);
        };
        let attached = match &artifact {
            ManagedArtifact::SymlinkReference { path, target } => {
                if resource.capability.kind == CapabilityKind::AgentSkill {
                    let skill_source_dir = resource
                        .capability
                        .path
                        .parent()
                        .expect("SKILL.md has a parent");
                    let entry_name = path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .expect("a managed Skill entry has a UTF-8 name");
                    // The shim's own plugin directory gets the stable
                    // namespaced label (`flow:review`), while the *manifest
                    // plugin name* stays the namespace (`flow`): Claude then
                    // exposes the skill as `/flow:review` (ADR-026) instead
                    // of double namespacing it (`/flow:flow:review`).
                    let namespace = active_plugin_name(&self.uze_home, resource);
                    let policy = resource.skill_invocation();
                    materialize_shim(
                        target,
                        skill_source_dir,
                        entry_name,
                        Some(&namespace),
                        &policy,
                    )?;
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
                HookTarget::Claude.attach_entry(
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
            } => mcp::inspect_claude_mcp(
                &self.command_home.join(".claude.json"),
                entry_name,
                transport,
                command,
                args,
                cwd.as_deref(),
                environment,
                *enabled,
            ),
            ManagedArtifact::HookConfigEntry {
                config_file,
                entry_name,
                event,
                expected,
                wrapper,
            } => HookTarget::Claude.inspect_entry(&HookEntry {
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
            } if marketplace::receipt_origin::<ClaudeMarketplace>(kind).is_some() => {
                let executable = self.provisioning_executable();
                marketplace::inspect_package::<ClaudeMarketplace>(
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
                Ok(AttachmentInspection {
                    state: AttachmentState::Missing,
                    reason: "Claude managed MCP entry detached via CLI".to_owned(),
                })
            }
            ManagedArtifact::HookConfigEntry {
                config_file,
                entry_name,
                event,
                expected,
                wrapper,
            } => HookTarget::Claude.detach_entry(
                &self.uze_home,
                self.id(),
                &HookEntry {
                    config_file,
                    entry_name,
                    event: *event,
                    expected,
                    wrapper,
                },
            ),
            ManagedArtifact::IntegrationOwned { kind, selector, .. }
                if let Some(origin) = marketplace::receipt_origin::<ClaudeMarketplace>(kind) =>
            {
                let executable = self.provisioning_executable();
                marketplace::detach_package::<ClaudeMarketplace>(
                    Path::new(&executable),
                    &self.command_home,
                    &self.uze_home,
                    receipt,
                    selector,
                    origin,
                )?;
                Ok(AttachmentInspection {
                    state: AttachmentState::Missing,
                    reason: "Claude native plugin detached".to_owned(),
                })
            }
            _ => {
                let detached = receipt.artifact.detach_standard()?;
                if detached.state == AttachmentState::Missing
                    && let ManagedArtifact::SymlinkReference { target, .. } = &receipt.artifact
                {
                    self.cleanup_unused_wrapper(target)?;
                }
                Ok(detached)
            }
        }
    }
}

impl ClaudeIntegration {
    fn agent_exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        let entry_name = resource
            .resolved_exposure_name
            .clone()
            .unwrap_or_else(|| agent_name(resource));
        markdown_agent_plan(
            &self.agents_dir,
            &entry_name,
            resource,
            "Claude Code natively discovers Markdown subagents from its user agents directory; UZE keeps a receipt-owned symlink to the canonical Store definition.",
        )
    }

    fn hook_exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        HookTarget::Claude.entry_plan(
            &self.uze_home,
            resource,
            self.hooks_config_path(),
            "Claude Code reads `hooks` from its user settings file; UZE merges one group entry per canonical hook (matcher and timeout preserved) whose command is the generated `hooks/exec` wrapper — the handlers run against the portable HOOK_* contract with no UZE binary on the execution path — and keeps the exact entry receipt-owned. The generated settings entry follows the plugin `hooks/hooks.json` group form.",
        )
    }
}

/// `claude --version` prints "2.1.239 (Claude Code)" — the version leads.
fn claude_version(program: &str) -> HarnessDetection {
    detect_version(program, VersionToken::First)
}

impl PreferencePort for ClaudeIntegration {
    fn preference_id(&self) -> &'static str {
        IntegrationPort::id(self)
    }

    fn translate(&self, preferences: &Preferences) -> PreferenceTranslation {
        preferences::translate(preferences, &preferences::SandboxHost::detect())
    }

    fn apply(&self, preferences: &Preferences) -> Result<PreferenceApplyOutcome> {
        // Preferences share Claude's user-scope settings file with Hooks
        // (ADR-033) — same file, disjoint keys.
        preferences::apply(
            &self.hooks_config_path(),
            preferences,
            &preferences::SandboxHost::detect(),
        )
    }

    fn plan(&self, preferences: &Preferences) -> Result<PreferencePlan> {
        preferences::plan(
            &self.hooks_config_path(),
            preferences,
            &preferences::SandboxHost::detect(),
        )
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use std::path::Path;

    use uze_core::exposure::McpEnvironmentReference;
    use uze_core::home::UzeHome;
    use uze_core::integration::{
        AttachmentReceipt, AttachmentState, IntegrationPort, ManagedArtifact,
    };

    use super::mcp::inspect_claude_mcp;
    use super::{ClaudeIntegration, fs};

    fn check(value: &str) -> AttachmentState {
        let root = uze_testkit::temp::scratch("claude-config");
        fs::create_dir_all(&root).unwrap();
        let path = root.join(".claude.json");
        fs::write(&path, value).unwrap();
        let environment: Vec<McpEnvironmentReference> = Vec::new();
        let state = inspect_claude_mcp(
            &path,
            "uze-x",
            "stdio",
            Path::new("tool"),
            &["a".to_owned()],
            None,
            &environment,
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

        let root = uze_testkit::temp::scratch("claude-shim");
        let uze_home = UzeHome::at(root.join("uze"));
        let integration = ClaudeIntegration::new(root.join("claude"), uze_home.clone());
        fs::create_dir_all(&integration.skills_dir).unwrap();
        let shim = uze_home
            .generated_attachments_dir("claude")
            .join("uze-example");
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
