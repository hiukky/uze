//! Codex peer integration. Its transparent-attachment strategy is a
//! UZE-managed reference at `<agents_home>/skills/<name>` (see ADR-006):
//! Codex documents a cwd-independent USER-scope Agent Skill directory that
//! explicitly follows symlinks. Until `uze setup` has completed, exposure
//! falls back to the per-session managed projection from ADR-005.
//!
//! Split by concern: [`mcp`] (MCP server registration/inspection),
//! [`skills`] (the managed skills-dir reference), [`plugin`] (the native
//! `.agents/plugins/marketplace.json` catalogue), and [`provision`]
//! (install/update via the official installer). This file is the
//! composition root: the `CodexIntegration` struct and its `IntegrationPort`
//! impl, delegating to each submodule.

use std::{fs, path::Path, path::PathBuf};

use uze_core::{
    Result, UzeError,
    capability::CapabilityKind,
    exposure::{ExposureMechanism, ExposurePlan, PackageExposurePlan},
    harness_runtime::resolve_real_executable,
    home::UzeHome,
    hook::{HookAdapterPort, HookCommandInput, HookDispatchOutcome, HookEvent, HookNativeOutput},
    integration::{
        AttachmentInspection, AttachmentReceipt, AttachmentState, ContextDelivery,
        HarnessDetection, IntegrationPort, ManagedArtifact, PublicationStatus,
        default_exposure_name_candidates, detach_standard_receipt, inspect_standard_receipt,
    },
    project::Resource,
    provisioning::{ProcessRunner, ProcessSpec, ProvisioningResult},
    router::{CompatibilityRoute, HarnessCapabilities, VerificationStatus},
    state,
    store::StoredPackage,
};

mod generate;
mod mcp;
mod plugin;
mod provision;
mod skills;

pub use mcp::detach_mcp_entry;

use crate::hooks as hook_projection;
use crate::shared::process::run_quiet;
use generate::{
    GENERATED_MARKETPLACE_NAME, GENERATED_PLUGIN_KIND, generatable, generated_catalogue_matches,
    generated_exact_coverage, generated_package_receipt, generated_root,
    materialize_generated_package, remove_generated_package_by_id, write_generated_catalogue,
};
use mcp::attach_mcp_entry;
use plugin::{
    MARKETPLACE_NAME, catalogue_document, codex_exact_coverage, detail_path, inspect_codex_plugin,
    marketplace_exists, publishable, remove_plugin, run_codex, write_catalogue,
};
use provision::{detect_binary, provision_cli};
use skills::codex_skill_exposure_name_candidates;

/// Codex peer integration. Its transparent-attachment strategy is a
/// UZE-managed reference at `<agents_home>/skills/<name>` (see ADR-006):
/// Codex documents a cwd-independent USER-scope Agent Skill directory that
/// explicitly follows symlinks. Until `uze setup` has completed, exposure
/// falls back to the per-session managed projection from ADR-005.
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
    /// Root Codex is pointed at. It must contain the package tree: Codex
    /// resolves a catalogue entry's `source.path` relative to this root and
    /// rejects both absolute paths and relative paths escaping it —
    /// confirmed empirically against Codex 0.148.0. That constraint is why
    /// the catalogue sits beside the packages rather than in a directory of
    /// its own; the layout is UZE's, the file is Codex's.
    fn catalogue_root(&self) -> PathBuf {
        self.uze_home.store_dir()
    }

    fn catalogue_path(&self) -> PathBuf {
        self.catalogue_root()
            .join(".agents/plugins/marketplace.json")
    }

    pub fn new(agents_home: PathBuf, uze_home: UzeHome) -> Self {
        let command_home = agents_home
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| agents_home.clone());
        Self {
            skills_dir: agents_home.join("skills"),
            agents_dir: command_home.join(".codex").join("agents"),
            generated_agents_dir: uze_home
                .state_dir()
                .join("attachments")
                .join("codex")
                .join("agents"),
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

    /// Convenience constructor for the CLI composition root. Unused when
    /// this module is compiled into a test binary via `#[path]`, where
    /// tests construct with `new` directly against a temporary home.
    #[allow(dead_code)]
    pub fn from_env(uze_home: UzeHome) -> Result<Self> {
        let home = std::env::var_os("HOME").ok_or(UzeError::MissingHomeDirectory)?;
        Ok(Self::new(PathBuf::from(home).join(".agents"), uze_home))
    }

    /// The real `codex` executable, resolved explicitly rather than through
    /// a bare `Command::new("codex")` PATH lookup — same rationale, same
    /// recursion hazard, as `ClaudeIntegration::provisioning_executable`:
    /// once `uze setup codex` has ever succeeded, `~/.uze/shims/codex` can
    /// sit ahead of the real binary on `PATH`, and an internal integration
    /// call must never re-enter UZE's own runtime shim. Falls back to the
    /// bare name (previous behavior) if no real binary can be found outside
    /// the shims directory.
    fn provisioning_executable(&self) -> String {
        resolve_real_executable(&["codex"], &self.uze_home.shims_dir())
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| "codex".to_owned())
    }

    /// Installs a package whose source ships its own
    /// `.codex-plugin/plugin.json`, through the existing `uze-local`
    /// marketplace rooted at the Store itself. Unchanged behavior —
    /// extracted verbatim from the pre-generation `attach_package`.
    fn attach_explicit_package(
        &self,
        executable: &Path,
        package: &StoredPackage,
    ) -> Result<Option<AttachmentReceipt>> {
        let catalogue_root = self.catalogue_root();
        if !marketplace_exists(executable, &self.command_home, &catalogue_root) {
            run_codex(
                executable,
                &self.command_home,
                ["plugin", "marketplace", "add"],
                Some(&catalogue_root),
            )?;
        }
        let selector = format!("{}@{MARKETPLACE_NAME}", package.active_name.as_str());
        run_quiet(
            executable,
            &self.command_home,
            &format!("codex plugin add `{selector}`"),
            &["plugin", "add", selector.as_str()],
        )?;
        Ok(Some(AttachmentReceipt {
            package_id: package.id.as_str().to_owned(),
            resource_identity: None,
            integration: self.id().to_owned(),
            strategy: "native-plugin-marketplace".to_owned(),
            artifact: ManagedArtifact::IntegrationOwned {
                kind: "marketplace-plugin".to_owned(),
                selector,
                detail: [
                    (
                        "marketplace_root".to_owned(),
                        serde_json::json!(catalogue_root),
                    ),
                    ("package_root".to_owned(), serde_json::json!(package.root)),
                ]
                .into_iter()
                .collect(),
            },
        }))
    }

    /// Installs a package with no author-provided envelope through the
    /// second, UZE-owned `uze-store` marketplace, materializing
    /// (or refreshing) its generated envelope directory first.
    fn attach_generated_package(
        &self,
        executable: &Path,
        package: &StoredPackage,
    ) -> Result<Option<AttachmentReceipt>> {
        let generated_dir = materialize_generated_package(&self.uze_home, package)?;
        let marketplace_root = generated_root(&self.uze_home);
        if !marketplace_exists(executable, &self.command_home, &marketplace_root) {
            run_codex(
                executable,
                &self.command_home,
                ["plugin", "marketplace", "add"],
                Some(&marketplace_root),
            )?;
        }
        let selector = format!(
            "{}@{GENERATED_MARKETPLACE_NAME}",
            package.active_name.as_str()
        );
        run_quiet(
            executable,
            &self.command_home,
            &format!("codex plugin add `{selector}`"),
            &["plugin", "add", selector.as_str()],
        )?;
        Ok(Some(generated_package_receipt(
            self.id(),
            package,
            &marketplace_root,
            &generated_dir,
            &selector,
        )))
    }

    /// Materializes this Skill's wrapper when this resource owns the shared
    /// entry; when the shared-root resolution reused another integration's
    /// artifact, verifies that the reused artifact still carries Codex's own
    /// invocation encoding for a user-only Skill — otherwise the canonical
    /// `invoke.model=false` would silently degrade into model visibility.
    fn materialize_or_verify_skill(&self, resource: &Resource) -> Result<()> {
        let policy = resource.skill_invocation();
        let Some(target) = &resource.resolved_artifact_target else {
            return skills::materialize_generated_skill(&self.uze_home, resource).map(|_| ());
        };
        if policy.is_invalid() {
            return Ok(());
        }
        if !policy.model && !target.join("agents/openai.yaml").is_file() {
            let entry = resource
                .resolved_exposure_name
                .clone()
                .map(|name| self.skills_dir.join(name))
                .unwrap_or_else(|| target.to_path_buf());
            return Err(projection_conflict(
                resource,
                &entry,
                target,
                "Codex needs agents/openai.yaml with policy.allow_implicit_invocation: false for a user-only Skill",
                self.id(),
            ));
        }
        Ok(())
    }

    fn materialize_agent(&self, resource: &Resource) -> Result<PathBuf> {
        let name = resource
            .logical_capability_name()
            .unwrap_or_else(|| resource.name());
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
    /// openai/codex, only a README splash banner) — this is OpenAI's own
    /// mark, fetched directly from openai.com's favicon, not a third
    /// party's redistribution.
    fn icon_path(&self) -> Option<&'static str> {
        Some("/harnesses/codex.png")
    }

    /// Reads the shared `AGENTS.md` natively (it is the origin harness for
    /// the convention); UZE maintains no artifact for it.
    fn context_delivery(&self) -> ContextDelivery {
        ContextDelivery::Native { files: &[] }
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
            verification: VerificationStatus::Unverified,
            evidence: "Codex consumes UZE's derived marketplaces: a package shipping .codex-plugin/plugin.json is added as a native plugin covering its declared skills/mcpServers (`codex plugin add <sel>@uze-local`); one without gets a deterministically synthesized envelope published through the generated-only `uze-store` marketplace (ADR-021) — both confirmed against real Codex 0.148.0 dogfood (`codex plugin list --json`). Canonical Agents are generated as Codex's documented standalone TOML files under ~/.codex/agents/, with name, description, and developer_instructions derived from the portable Markdown definition. Invocation policy is translated into Codex's own agents/openai.yaml → policy.allow_implicit_invocation: false for a canonical user-only Skill (Codex Build skills documentation; empirically honored by codex-cli 0.149.0 via `codex debug prompt-input`); the user=false combination is honestly Degraded since Codex has no documented way to disable explicit `$skill` invocation. Per ADR-025/ADR-030, Native means an officially supported primitive that preserves the canonical capability semantics — not an identical vendor file format. Portable Hooks are projected into Codex's own `~/.codex/hooks.json` command form through a hook-exec wrapper carrying the portable ABI (ADR-033; deterministic emission, real-binary verification pending in the conformance lab). Capability-level fallbacks (USER-scope `~/.agents/skills` reference, `codex mcp add`) remain only for resources outside the envelope's coverage."
                .to_owned(),
            ..HarnessCapabilities::default()
        }
    }

    fn hook_capabilities(&self) -> uze_core::hook::HookCapabilities {
        hook_projection::codex_capabilities()
    }

    fn detect(&self) -> HarnessDetection {
        detect_binary(&self.provisioning_executable())
    }

    /// OpenCode also discovers Skills from this exact same
    /// `~/.agents/skills` directory; see `OpenCodeIntegration`'s override
    /// of the same method for why this must be reported.
    fn shared_agent_skill_root(&self) -> Option<PathBuf> {
        Some(self.skills_dir.clone())
    }

    fn provision(&self, runner: &dyn ProcessRunner) -> Result<ProvisioningResult> {
        provision_cli(
            runner,
            "codex",
            self.detect(),
            ProcessSpec::new(
                "sh",
                ["-c", "curl -fsSL https://chatgpt.com/codex/install.sh | sh"],
            )
            .with_inherited_output(),
            // Real-CLI dogfood against codex-cli 0.148.0 (the version this
            // repo's own docs list as last-validated) found `--upgrade` is
            // not a recognized flag — `codex --help` lists `update` as a
            // subcommand instead. `research.md`'s original `--upgrade`
            // finding predates that vendor rename; left as a historical
            // record, not edited.
            ProcessSpec::new("codex", ["update"]).with_inherited_output(),
            "official-native-installer",
        )
    }

    fn install(&self, home: &UzeHome, detection: &HarnessDetection) -> Result<()> {
        fs::create_dir_all(&self.skills_dir).map_err(|source| UzeError::Write {
            path: self.skills_dir.clone(),
            source,
        })?;
        state::record(
            home,
            state::IntegrationRecord {
                harness: self.id().to_owned(),
                version: detection.version.clone(),
                strategy: "managed-user-scope-skills-dir".to_owned(),
                installed: true,
            },
        )
    }

    fn exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        if resource.package_root().is_none() {
            return unsupported(
                resource,
                "Codex attachment needs a UZE-stored Agent Plugin package.",
            );
        }
        match resource.capability.kind {
            CapabilityKind::AgentSkill => self.skill_exposure_plan(resource),
            CapabilityKind::Mcp => self.mcp_exposure_plan(resource),
            CapabilityKind::Agent => self.agent_exposure_plan(resource),
            CapabilityKind::Hook => self.hook_exposure_plan(resource),
            _ => unsupported(
                resource,
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
            return codex_skill_exposure_name_candidates(&self.uze_home, resource);
        }
        default_exposure_name_candidates(resource)
    }

    fn package_exposure_plan(
        &self,
        package: &StoredPackage,
        resources: &[&Resource],
    ) -> Option<PackageExposurePlan> {
        if package.root.join(".codex-plugin/plugin.json").is_file() {
            let provided = codex_exact_coverage(package, resources);
            return Some(PackageExposurePlan {
                package_id: package.id.clone(),
                route: CompatibilityRoute::Native,
                verification: VerificationStatus::Unverified,
                provided_resource_identities: provided,
                evidence: "The preserved external .codex-plugin/plugin.json is exposed through UZE's generated, standard Codex local marketplace catalog for exactly the skills/mcpServers it declares; undeclared resources fall back to individual attachment.".to_owned(),
            });
        }
        // No author-provided envelope. Check whether UZE can safely
        // synthesize one (ADR-020/ADR-021, refining ADR-013 §2: Explicit
        // Native Package > Generated Native Package > Native Capability >
        // Safe Adaptation > Unsupported). Stays read-only either way.
        if !generatable(package) {
            return None;
        }
        let provided = generated_exact_coverage(package, resources);
        Some(PackageExposurePlan {
            package_id: package.id.clone(),
            route: CompatibilityRoute::Native,
            verification: VerificationStatus::Unverified,
            provided_resource_identities: provided,
            evidence: "No .codex-plugin/plugin.json was provided. UZE synthesizes one deterministically into a UZE-owned derived directory (never the Store) covering exactly the package's conventional skills/ directory and mcp.json-declared servers, published through a second, generated-only Codex marketplace.".to_owned(),
        })
    }

    fn attach(&self, resource: &Resource) -> Result<Option<PathBuf>> {
        let plan = self.exposure_plan(resource);
        match &plan.mechanism {
            ExposureMechanism::ManagedUserScopeReference { .. } => {
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
                Ok(Some(plan.mechanism.attach()?))
            }
            ExposureMechanism::ManagedVendorConfig {
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
                )
            }
            ExposureMechanism::ManagedHookConfig {
                config_file,
                entry_name,
                event,
                expected,
            } => {
                let path = hook_projection::attach_event_entry(
                    &self.uze_home,
                    self.id(),
                    config_file,
                    event.expect("Codex hook entries are event-keyed"),
                    entry_name,
                    expected,
                )?;
                Ok(Some(path))
            }
            _ => Ok(None),
        }
    }

    fn attach_package(
        &self,
        package: &StoredPackage,
        _plan: &PackageExposurePlan,
    ) -> Result<Option<AttachmentReceipt>> {
        let executable = self.provisioning_executable();
        let executable = Path::new(&executable);
        if package.root.join(".codex-plugin/plugin.json").is_file() {
            return self.attach_explicit_package(executable, package);
        }
        self.attach_generated_package(executable, package)
    }

    fn republish_packages(&self, packages: &[StoredPackage]) -> Result<()> {
        write_catalogue(&self.catalogue_path(), packages)?;
        write_generated_catalogue(&self.uze_home, packages)
    }

    fn publication(&self, packages: &[StoredPackage]) -> PublicationStatus {
        let expected = catalogue_document(packages);
        let explicit_published = match fs::read(self.catalogue_path()) {
            Ok(bytes) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
                Ok(actual) if actual == expected => Ok(()),
                Ok(_) => Err(
                    "the Codex catalogue does not match the installed package set; re-run `uze setup codex`".to_owned(),
                ),
                Err(error) => Err(format!(
                    "the Codex catalogue is unreadable ({error}); re-run `uze setup codex`"
                )),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if publishable(packages).is_empty() {
                    Ok(())
                } else {
                    Err(
                        "no Codex catalogue has been written for the installed packages; re-run `uze setup codex`".to_owned(),
                    )
                }
            }
            Err(error) => Err(error.to_string()),
        };
        if let Err(reason) = explicit_published {
            return PublicationStatus::Unpublished(reason);
        }
        if !generated_catalogue_matches(&self.uze_home, packages) {
            return PublicationStatus::Unpublished(
                "the generated Codex catalogue does not match the installed package set; re-run `uze setup codex`"
                    .to_owned(),
            );
        }
        PublicationStatus::Published
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
                event,
                expected,
                ..
            } => hook_projection::inspect_event_entry(
                config_file,
                event.expect("Codex hook entries are event-keyed"),
                expected,
            ),
            ManagedArtifact::IntegrationOwned {
                kind,
                selector,
                detail,
            } if kind == "marketplace-plugin" || kind == GENERATED_PLUGIN_KIND => {
                let Some(marketplace_root) = detail_path(detail, "marketplace_root") else {
                    return plugin::blocked("plugin receipt has no marketplace root".to_owned());
                };
                let Some(package_root) = detail_path(detail, "package_root") else {
                    return plugin::blocked("plugin receipt has no package root".to_owned());
                };
                let executable = self.provisioning_executable();
                inspect_codex_plugin(
                    Path::new(&executable),
                    &self.command_home,
                    selector,
                    &marketplace_root,
                    &package_root,
                )
            }
            _ => inspect_standard_receipt(receipt),
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
                event,
                expected,
                ..
            } => {
                return hook_projection::remove_event_entry(
                    config_file,
                    event.expect("Codex hook entries are event-keyed"),
                    expected,
                );
            }
            ManagedArtifact::IntegrationOwned { kind, selector, .. }
                if kind == "marketplace-plugin" || kind == GENERATED_PLUGIN_KIND =>
            {
                let executable = self.provisioning_executable();
                remove_plugin(Path::new(&executable), &self.command_home, selector)?;
                if kind == GENERATED_PLUGIN_KIND {
                    // The generated envelope directory is a Derived Artifact
                    // (ADR-013 §4): non-authoritative, rebuildable, and
                    // never the canonical Store — safe to remove outright
                    // now that Codex no longer references it.
                    remove_generated_package_by_id(&self.uze_home, &receipt.package_id)?;
                }
            }
            _ => {
                let detached = detach_standard_receipt(receipt)?;
                if detached.state == AttachmentState::Missing
                    && let ManagedArtifact::SymlinkReference { target, .. } = &receipt.artifact
                {
                    self.cleanup_unused_skill_adaptation(target)?;
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
        let entry_name = resource
            .logical_capability_name()
            .unwrap_or_else(|| resource.name());
        ExposurePlan {
            representation: resource.capability.representation,
            route: CompatibilityRoute::Native,
            verification: VerificationStatus::Unverified,
            mechanism: ExposureMechanism::ManagedUserScopeReference {
                discovery_root: self.agents_dir.clone(),
                entry_name: format!("{entry_name}.toml"),
                source: self.generated_agents_dir.join(format!("{entry_name}.toml")),
            },
            evidence: "Codex natively loads standalone custom-agent TOML files from ~/.codex/agents/. UZE deterministically generates that native TOML from the portable Markdown definition and exposes it through a receipt-owned reference.".to_owned(),
        }
    }

    fn hook_exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        hook_projection::hook_exposure_plan(
            resource,
            &self.hook_capabilities(),
            self.hooks_config_path(),
            "codex",
            self.id(),
            false,
            "Codex's own hooks.json command form reads PreToolUse/PostToolUse/Stop command hooks; UZE merges one group entry per canonical hook (command + matcher + timeout preserved through the hook-exec wrapper carrying the portable ABI) and keeps the exact entry receipt-owned.",
        )
    }
}

impl HookAdapterPort for CodexIntegration {
    fn adapter_id(&self) -> &'static str {
        IntegrationPort::id(self)
    }

    fn normalize_input(
        &self,
        native: &serde_json::Value,
        event: HookEvent,
    ) -> std::result::Result<HookCommandInput, String> {
        hook_projection::codex_normalize_input(native, event)
    }

    fn render_output(
        &self,
        outcome: &HookDispatchOutcome,
        event: HookEvent,
    ) -> std::result::Result<HookNativeOutput, String> {
        hook_projection::codex_render_output(outcome, event)
    }
}

fn codex_agent_toml(resource: &Resource, fallback_name: &str) -> String {
    let markdown = String::from_utf8_lossy(&resource.capability.payload);
    let (frontmatter, instructions) = markdown_frontmatter(&markdown);
    let name = frontmatter_value(frontmatter, "name").unwrap_or(fallback_name);
    let description =
        frontmatter_value(frontmatter, "description").unwrap_or("Portable UZE custom agent.");
    format!(
        "name = {}\ndescription = {}\ndeveloper_instructions = {}\n",
        toml_string(name),
        toml_string(description),
        toml_string(instructions.trim()),
    )
}

fn markdown_frontmatter(markdown: &str) -> (&str, &str) {
    let Some(rest) = markdown.strip_prefix("---\n") else {
        return ("", markdown);
    };
    let Some(end) = rest.find("\n---\n") else {
        return ("", markdown);
    };
    (&rest[..end], &rest[end + 5..])
}

fn frontmatter_value<'a>(frontmatter: &'a str, key: &str) -> Option<&'a str> {
    frontmatter.lines().find_map(|line| {
        let (found, value) = line.split_once(':')?;
        (found.trim() == key).then(|| value.trim().trim_matches('"').trim_matches('\''))
    })
}

fn toml_string(value: &str) -> String {
    serde_json::to_string(value).expect("strings are JSON serializable")
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

/// Deterministic, pre-attach projection conflict: the shared
/// `~/.agents/skills` entry this resource would reuse is already owned by
/// another integration's artifact that cannot preserve this integration's
/// invocation encoding (ADR-030 §25 — never degrade silently).
fn projection_conflict(
    resource: &Resource,
    entry: &std::path::Path,
    reused_target: &std::path::Path,
    requirement: &str,
    integration: &str,
) -> UzeError {
    let requested_target = resource
        .capability
        .path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| resource.capability.path.clone());
    UzeError::ProjectionConflict(Box::new(uze_core::error::ProjectionConflictDetails {
        entry: entry.to_path_buf(),
        requested: format!("{} ({requirement})", resource.identity()),
        requested_integration: integration.to_owned(),
        requested_target,
        existing: format!("{} ({requirement})", resource.identity()),
        existing_integration: "shared-root owner".to_owned(),
        existing_target: reused_target.to_path_buf(),
    }))
}
