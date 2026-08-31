//! Antigravity CLI peer integration — the Google-family v0 harness.
//!
//! Antigravity CLI (`agy`, validated against 1.1.19) is the Go-based,
//! terminal-runtime of the Antigravity 2.0 agent harness. Its native
//! package delivery is a **Plugin**: a directory with a mandatory
//! `plugin.json` plus optional `skills/`, `commands/` (converted to skills
//! by the CLI), `mcp_config.json`, `agents/`, `hooks.json` and `rules/`.
//!
//! Every fact below was confirmed empirically against `agy` 1.1.19 in an
//! isolated `$HOME` (see `docs/architecture/antigravity-compatibility.md`):
//!
//! - `agy plugin install <dir>` stages a **byte copy** at
//!   `~/.gemini/config/plugins/<name>/` (the vendor keeps the legacy
//!   Google config area) and registers it in
//!   `~/.gemini/config/import_manifest.json`; it dereferences symlinks, so
//!   there is no link-preserving install route at all. The staged tree is
//!   therefore always a Derived Artifact (ADR-013 §4): integration-owned,
//!   rebuildable from the Store, never authoritative.
//! - `agy plugin list` prints machine-readable JSON on stdout
//!   (`{"imports":[{name,source,importedAt,components}]}`) — inspection
//!   and ownership proofs are cheap and reliable. `plugin uninstall`
//!   removes the staged tree and the registration.
//! - The canonical UZE `plugin.json` (name + description) **is** a valid
//!   Antigravity plugin manifest — extra fields are tolerated — so the
//!   North Star package ships no Antigravity-specific file and still takes
//!   the explicit native route. The one surface needing translation is
//!   MCP: the plugin system reads `mcp_config.json`, never canonical
//!   `mcp.json`, so a package with a canonical MCP surface is delivered
//!   through a generated plugin carrying a translated `mcp_config.json`.
//! - `agy` has **no independent custom-command primitive**: the official
//!   migration path converts legacy commands to skills
//!   (`commands: N legacy commands converted to skills`, verified against
//!   1.1.19). Skills are model-discoverable (progressive disclosure)
//!   *and* slash-invocable, and no explicit-only mechanism is documented
//!   or observable, so a canonical Command delivered through this physical
//!   primitive is classified **Adapted** — user invocation is native, the
//!   explicit-only property degrades.
//! - MCP servers are managed through `agy mcp add <name> <cmd> [args...]`
//!   (global `~/.gemini/config/mcp_config.json`, schema `command/args/
//!   disabled`, remote `serverUrl`), inspected by reading that JSON file
//!   directly (`agy mcp list` is human-readable only) and removed via
//!   `agy mcp remove`.
//! - Workspace context is read directly from `AGENTS.md` and `GEMINI.md`
//!   (official docs: identical workspace context rules, no modifications
//!   needed), so UZE's context route is Native — no bridge file is
//!   generated for Antigravity.
//! - Global skills live under `~/.gemini/antigravity-cli/skills/` (CLI
//!   docs; the binary's own builtin skills live under
//!   `~/.gemini/antigravity-cli/builtin/skills/`).
//!
//! Split by concern: [`provision`] (official installer + detection),
//! [`plugin`] (explicit plugin delivery + `agy plugin list` inspection),
//! [`generate`] (generated plugin for canonical-MCP translation),
//! [`skills`] (managed global-skills reference, invocation-policy-aware)
//! and [`mcp`] (`agy mcp add` registration). This file is the composition
//! root.

use std::{collections::BTreeMap, fs, path::Path, path::PathBuf};

use uze_core::{
    Result, UzeError,
    capability::CapabilityKind,
    exposure::{ExposureMechanism, ExposurePlan, PackageExposurePlan},
    harness_runtime::resolve_real_executable,
    home::UzeHome,
    hook::{HookAdapterPort, HookCommandInput, HookDispatchOutcome, HookEvent, HookNativeOutput},
    integration::{
        AttachmentInspection, AttachmentReceipt, AttachmentState, ContextDelivery,
        HarnessDetection, IntegrationPort, ManagedArtifact, default_exposure_name_candidates,
        detach_standard_receipt, inspect_standard_receipt,
    },
    preference::{PreferenceApplyOutcome, PreferencePort, PreferenceTranslation, Preferences},
    project::Resource,
    provisioning::{ProcessRunner, ProcessSpec, ProvisioningResult},
    router::{CompatibilityRoute, HarnessCapabilities, VerificationStatus},
    state,
    store::StoredPackage,
};

mod generate;
mod mcp;
mod plugin;
mod preferences;
mod provision;
mod skills;

use crate::hooks as hook_projection;
use crate::shared::provision::provision_cli;
use generate::{canonical_hook_groups, remove_generated_plugin_by_id};
use mcp::attach_mcp_entry;
use plugin::{
    GENERATED_PLUGIN_KIND, PLUGIN_KIND, attach_explicit_plugin, attach_generated_plugin,
    exact_coverage, inspect_installed_plugin, installed_plugins, plugin_manifest_name, run_agy,
};

/// Antigravity CLI's stable integration id. Never changes in receipts.
pub const ID: &str = "antigravity";

/// Official Unix installer, invoked exactly as the vendor documents:
/// `curl -fsSL https://antigravity.google/cli/install.sh | bash`.
///
/// Note: the live installer (verified against the current script) accepts
/// only `-d/--dir` — the docs' `--skip-aliases`/`--skip-path` flags are
/// **rejected** by the script ("Unknown parameter"), so UZE cannot use
/// them. The installer therefore appends its own PATH export to the user's
/// shell profiles (`~/.bashrc`/`~/.zshrc`/`~/.profile`) — vendor behavior
/// surfaced in its own output, unavoidable in this version. Documented
/// destination: `~/.local/bin/agy`.
const INSTALLER_COMMAND: &str = "curl -fsSL https://antigravity.google/cli/install.sh | bash";

#[derive(Clone)]
pub struct AntigravityIntegration {
    /// CLI global skills root (`~/.gemini/antigravity-cli/skills`), where a
    /// UZE-managed reference is discovered natively.
    skills_dir: PathBuf,
    agents_dir: PathBuf,
    /// Global plugins directory (`~/.gemini/config/plugins`), where
    /// `agy plugin install` stages plugin byte copies.
    plugins_dir: PathBuf,
    /// Global MCP config (`~/.gemini/config/mcp_config.json`).
    mcp_config_path: PathBuf,
    /// `HOME` set explicitly for every shelled-out `agy` subcommand.
    /// Antigravity derives `~/.gemini` (its config area keeps this legacy
    /// directory name) from `$HOME` (confirmed against
    /// 1.1.19) and must never be pointed at the calling process's own
    /// environment by accident.
    command_home: PathBuf,
    uze_home: UzeHome,
}

impl AntigravityIntegration {
    pub fn new(agents_home: PathBuf, uze_home: UzeHome) -> Self {
        let command_home = agents_home
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| agents_home.clone());
        let gemini_root = command_home.join(".gemini");
        Self {
            skills_dir: gemini_root.join("antigravity-cli").join("skills"),
            agents_dir: gemini_root.join("antigravity-cli").join("agents"),
            plugins_dir: gemini_root.join("config").join("plugins"),
            mcp_config_path: gemini_root.join("config").join("mcp_config.json"),
            command_home,
            uze_home,
        }
    }

    #[allow(dead_code)]
    pub fn from_env(uze_home: UzeHome) -> Result<Self> {
        let home = std::env::var_os("HOME").ok_or(UzeError::MissingHomeDirectory)?;
        Ok(Self::new(PathBuf::from(home).join(".agents"), uze_home))
    }

    /// `~/.gemini/antigravity-cli/settings.json` — same directory as
    /// `skills_dir`/`agents_dir`'s parent (unverified against current docs
    /// beyond two independently reproduced fetches; see `preferences`'s
    /// module doc for the confidence caveat).
    fn preferences_config_path(&self) -> PathBuf {
        self.command_home
            .join(".gemini")
            .join("antigravity-cli")
            .join("settings.json")
    }

    /// Same PATH-shim recursion hazard and the same fix as every peer
    /// integration: internal invocations must never risk re-entering UZE's
    /// own `~/.uze/shims/agy`. Falls back to the installer's documented
    /// destination (`~/.local/bin/agy`) when the binary is not on `PATH` —
    /// a fresh official install lands there and should work even before the
    /// user reopens their shell (the installer's own rc-file PATH append
    /// only affects future shells).
    fn provisioning_executable(&self) -> String {
        resolve_real_executable(&["agy"], &self.uze_home.shims_dir())
            .or_else(|| provision::documented_install_path("agy"))
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| "agy".to_owned())
    }
}

impl IntegrationPort for AntigravityIntegration {
    fn id(&self) -> &'static str {
        ID
    }

    fn display_name(&self) -> &'static str {
        "Antigravity"
    }

    fn description(&self) -> &'static str {
        "Google's agentic coding CLI"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["agy", "antigravity-cli"]
    }

    /// Skills stay model-discoverable and slash-invocable (ADR-031) — same
    /// surface as Claude Code and OpenCode; only Codex differs (`$`).
    fn invocation_prefix(&self) -> &'static str {
        "/"
    }

    /// Google Antigravity's own apple-touch-icon, fetched directly from
    /// antigravity.google — not a third party's redistribution.
    fn icon_path(&self) -> Option<&'static str> {
        Some("/harnesses/antigravity.png")
    }

    /// Reads the shared `AGENTS.md` natively (official docs: identical
    /// workspace context rules) plus the legacy `GEMINI.md` global-rules
    /// file, which is observed for portability reporting only.
    fn context_delivery(&self) -> ContextDelivery {
        ContextDelivery::Native {
            files: &["GEMINI.md"],
        }
    }

    /// Antigravity's own docs (antigravity.google/docs/cli/plugins, 2026)
    /// state Agent Skills are available "globally
    /// (~/.gemini/antigravity-cli/skills/) and per-workspace
    /// (.agents/skills/)" — the latter read directly by `agy` from the
    /// project, with no UZE involvement (superseding this crate's own
    /// earlier "not yet implemented/unverified" note, written before that
    /// documentation existed).
    fn discovers_project_agents_directory(&self) -> bool {
        true
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            // Agent Skills and MCP servers are delivered natively through
            // the Antigravity plugin (explicit canonical package or
            // UZE-generated envelope) — the package-level route. The
            // capability-level shims (global skills reference, `agy mcp
            // add`) are the fallback for resources outside the envelope's
            // coverage, never the primary route.
            native: [
                CapabilityKind::AgentSkill,
                CapabilityKind::Mcp,
                CapabilityKind::Agent,
                CapabilityKind::Hook,
            ]
                .into_iter()
                .collect(),
            // Non-default invocation policies are ADAPTED, never Native:
            // Antigravity has no explicit-invocation-only mechanism and no
            // way to hide a Skill from the model or the user's slash
            // surface (verified against agy 1.1.19 — the official
            // migration path converts legacy commands to Skills, which are
            // both model-discoverable and slash-invocable). Per ADR-030,
            // Native requires preserving the canonical invocation policy;
            // the non-default half degrades here. This is declared through
            // the per-resource exposure plan, kept honest per policy — a
            // default model+user Skill is fully Native.
            verification: VerificationStatus::Unverified,
            evidence: "Antigravity CLI consumes UZE's native plugins: the canonical package itself is a valid plugin (plugin.json name/description; extra fields tolerated), so an envelope-less package is installed straight from the Store via `agy plugin install`; one with a canonical mcp.json and/or canonical hooks.json gets a deterministically synthesized plugin carrying a translated mcp_config.json and a named-entry hooks.json respectively, installed from a UZE-owned derived directory (verified against real agy 1.1.19 dogfood: validate → install → list → uninstall; the hook projection itself is deterministic emission, real-binary verification pending in the conformance lab). Non-default invocation policies are ADAPTED (no explicit-invocation-only mechanism exists; Skills stay model-discoverable and slash-invocable — verified against 1.1.19). MCP falls back to `agy mcp add` (global ~/.gemini/config/mcp_config.json) for resources outside plugin coverage. AGENTS.md is read natively (official docs: identical workspace context rules), so context needs no bridge."
                .to_owned(),
            ..HarnessCapabilities::default()
        }
    }

    fn hook_capabilities(&self) -> uze_core::hook::HookCapabilities {
        hook_projection::antigravity_capabilities()
    }

    fn detect(&self) -> HarnessDetection {
        provision::detect_binary(&self.provisioning_executable())
    }

    fn detection_program_candidates(&self) -> Vec<&'static str> {
        vec!["agy"]
    }

    /// Antigravity's global skills root (`~/.gemini/antigravity-cli/skills`)
    /// is exclusive to this integration — unlike Codex/OpenCode it
    /// does not read `~/.agents/skills` — so no shared-root awareness is
    /// reported (and none is needed for naming resolution).
    fn shared_agent_skill_root(&self) -> Option<PathBuf> {
        None
    }

    fn provision(&self, runner: &dyn ProcessRunner) -> Result<ProvisioningResult> {
        // Install: the documented official Unix installer (curl | bash).
        // The installer appends its own PATH export to the user's shell
        // profiles — vendor behavior; the docs' `--skip-aliases`/
        // `--skip-path` flags are rejected by the current script (see
        // INSTALLER_COMMAND). Update: the installer exits early when the
        // binary already exists ("agy automatically self-updates in the
        // background"), so the update verb is the official `agy update`
        // subcommand (present in 1.1.19's `--help`).
        //
        // `executable` is the resolved real binary (PATH first, then the
        // installer's documented `~/.local/bin` destination), so the
        // post-install `--version` verification works even when the user's
        // current shell has not re-sourced its rc files yet.
        let executable = self.provisioning_executable();
        provision_cli(
            runner,
            &executable,
            "Antigravity CLI",
            self.detect(),
            ProcessSpec::new("sh", ["-c", INSTALLER_COMMAND]).with_inherited_output(),
            ProcessSpec::new(&executable, ["update"]).with_inherited_output(),
            "official-native-installer",
            provision::detect_binary,
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

    /// Antigravity's naming decision: every UZE-projected Skill gets its
    /// stable namespaced invocation label (`flow:review`) as the single
    /// candidate — never a bare alias, never collision-dependent naming
    /// (ADR-026). `agy plugin validate` accepts `:` in skill names
    /// (verified against 1.1.19). MCP stays on the default fully-qualified
    /// policy — capability naming policies are never mixed.
    fn exposure_name_candidates(&self, resource: &Resource) -> Vec<String> {
        if resource.capability.kind == CapabilityKind::AgentSkill {
            return skills::antigravity_skill_exposure_name_candidates(&self.uze_home, resource);
        }
        default_exposure_name_candidates(resource)
    }

    fn exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        if resource.package_root().is_none() {
            return unsupported(
                resource,
                "Antigravity attachment needs a UZE-stored Agent Plugin package.",
            );
        }
        match resource.capability.kind {
            CapabilityKind::AgentSkill => self.skill_exposure_plan(resource),
            CapabilityKind::Mcp => self.mcp_exposure_plan(resource),
            CapabilityKind::Agent => self.agent_exposure_plan(resource),
            CapabilityKind::Hook => self.hook_fallback_plan(resource),
            _ => unsupported(
                resource,
                "Antigravity attachment is only modeled for Agent Skills, Agents, MCP servers, and portable Hooks.",
            ),
        }
    }

    /// The canonical UZE `plugin.json` is itself a valid Antigravity plugin
    /// manifest (name pattern `^[a-zA-Z0-9-_]+$`; description optional;
    /// extra fields tolerated — verified against 1.1.19), so an
    /// envelope-less canonical package is delivered whole, straight from
    /// the Store, with NO synthesized envelope. The only surface the
    /// canonical layout does NOT satisfy is MCP: the plugin system reads
    /// `mcp_config.json`, not canonical `mcp.json`, so a package whose
    /// canonical MCP surface is discovered gets a deterministically
    /// synthesized plugin carrying the translated server declarations
    /// (ADR-020/ADR-021 discipline).
    fn package_exposure_plan(
        &self,
        package: &StoredPackage,
        resources: &[&Resource],
    ) -> Option<PackageExposurePlan> {
        // The canonical manifest's own name decides explicitness; a package
        // whose name is not a valid Antigravity plugin name has no native
        // package route at all (capability-level delivery only).
        plugin_manifest_name(package)?;
        // A plugin stages its entire skills/ tree unchanged. When any
        // Skill carries a non-default invocation policy, delivering that
        // tree would bypass the capability wrapper that translates (or
        // honestly adapts) the policy. Decompose the package instead: each
        // capability then gets exactly one policy-aware delivery.
        if resources.iter().any(|resource| {
            resource.capability.kind == CapabilityKind::AgentSkill
                && !resource.skill_invocation().is_default()
        }) {
            return None;
        }
        let canonical_mcp = generate::canonical_mcp_servers(package);
        let author_mcp = plugin::author_mcp_config_servers(package);
        if (canonical_mcp.is_some() && author_mcp.is_empty()) || canonical_hook_groups(package) {
            let provided = generate::generated_exact_coverage(package, resources);
            return Some(PackageExposurePlan {
                package_id: package.id.clone(),
                route: CompatibilityRoute::Native,
                verification: VerificationStatus::Unverified,
                provided_resource_identities: provided,
                evidence: "The canonical package's own plugin.json is a valid Antigravity plugin manifest, but its MCP servers live in canonical mcp.json (which the plugin system does not read) and/or its hooks live in canonical portable form (the plugin reads named-entry hooks.json). UZE synthesizes a deterministic plugin (plugin.json + translated mcp_config.json + translated named hooks.json + symlinked skills/) into a UZE-owned derived directory and installs that — never the Store."
                    .to_owned(),
            });
        }
        let provided = exact_coverage(package, resources);
        Some(PackageExposurePlan {
            package_id: package.id.clone(),
            route: CompatibilityRoute::Native,
            verification: VerificationStatus::Unverified,
            provided_resource_identities: provided,
            evidence: "The canonical plugin.json is a valid Antigravity plugin manifest, so the package is installed whole, straight from the UZE store, through `agy plugin install`; its conventional skills/ plus any author-shipped mcp_config.json are what it declares (default-policy Skills only — a non-default invoke policy degrades and is delivered capability-level, reported honestly). Undeclared resources fall back to individual attachment."
                .to_owned(),
        })
    }

    // `republish_packages` deliberately remains its default no-op, exactly
    // like every no-catalogue integration: Antigravity needs no catalogue —
    // `plugin install` points straight at a package directory. Publication
    // stays optional, not a Codex-shaped concept.

    fn attach_package(
        &self,
        package: &StoredPackage,
        _plan: &PackageExposurePlan,
    ) -> Result<Option<AttachmentReceipt>> {
        let executable = self.provisioning_executable();
        if generate::canonical_mcp_servers(package).is_some() || canonical_hook_groups(package) {
            attach_generated_plugin(&executable, self, package)
        } else {
            attach_explicit_plugin(&executable, self, package)
        }
    }

    fn attach(&self, resource: &Resource) -> Result<Option<PathBuf>> {
        let plan = self.exposure_plan(resource);
        match &plan.mechanism {
            ExposureMechanism::ManagedUserScopeReference { .. } => {
                // Materialize the generated wrapper first — and only when
                // this resource owns the physical entry (a resolved shared
                // artifact is authoritative; nothing new may replace it).
                if resource.resolved_artifact_target.is_none()
                    && resource.capability.kind == CapabilityKind::AgentSkill
                {
                    skills::materialize_generated_skill(&self.uze_home, resource)?;
                }
                Ok(Some(plan.mechanism.attach()?))
            }
            ExposureMechanism::ManagedVendorConfig {
                entry_name,
                command,
                args,
                ..
            } => attach_mcp_entry(
                &self.provisioning_executable(),
                &self.command_home,
                entry_name,
                command,
                args,
            ),
            _ => Ok(None),
        }
    }

    fn inspect_receipt(&self, receipt: &AttachmentReceipt) -> AttachmentInspection {
        match &receipt.artifact {
            ManagedArtifact::IntegrationOwned {
                kind,
                selector,
                detail,
            } if kind == PLUGIN_KIND || kind == GENERATED_PLUGIN_KIND => {
                let Some(expected_fingerprint) = detail_str(detail, "fingerprint") else {
                    return blocked("plugin receipt has no expected fingerprint".to_owned());
                };
                let staged_dir = self.plugins_dir.join(selector);
                match installed_plugins(&self.provisioning_executable(), &self.command_home) {
                    Ok(listing) => inspect_installed_plugin(
                        &listing,
                        selector,
                        &staged_dir,
                        &expected_fingerprint,
                    ),
                    Err(message) => blocked(message),
                }
            }
            ManagedArtifact::VendorConfigEntry {
                entry_name,
                transport,
                command,
                args,
                cwd,
                environment,
                enabled,
            } => mcp::inspect_antigravity_mcp(
                &self.mcp_config_path,
                entry_name,
                transport,
                command,
                args,
                cwd.as_deref(),
                environment,
                *enabled,
            ),
            _ => inspect_standard_receipt(receipt),
        }
    }

    fn detach_receipt(&self, receipt: &AttachmentReceipt) -> Result<AttachmentInspection> {
        // Re-inspect immediately before the destructive call, per ADR-009.
        let inspection = self.inspect_receipt(receipt);
        if inspection.state != AttachmentState::Matched {
            return Ok(inspection);
        }
        let executable = self.provisioning_executable();
        match &receipt.artifact {
            ManagedArtifact::IntegrationOwned { kind, selector, .. }
                if kind == PLUGIN_KIND || kind == GENERATED_PLUGIN_KIND =>
            {
                // Uninstalling removes only Antigravity's staged copy and
                // its registration; the stored package UZE owns stays
                // exactly where it is.
                run_agy(
                    &executable,
                    &self.command_home,
                    &["plugin", "uninstall", selector],
                    "agy plugin uninstall",
                )?;
                if kind == GENERATED_PLUGIN_KIND {
                    remove_generated_plugin_by_id(&self.uze_home, &receipt.package_id)?;
                }
            }
            ManagedArtifact::VendorConfigEntry { entry_name, .. } => {
                run_agy(
                    &executable,
                    &self.command_home,
                    &["mcp", "remove", entry_name],
                    "agy mcp remove",
                )?;
            }
            _ => {
                let detached = detach_standard_receipt(receipt)?;
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
            reason: "Antigravity managed artifact detached".to_owned(),
        })
    }
}

fn detail_str(detail: &BTreeMap<String, serde_json::Value>, key: &str) -> Option<String> {
    detail
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

fn blocked(reason: String) -> AttachmentInspection {
    AttachmentInspection {
        state: AttachmentState::Blocked,
        reason,
    }
}

fn unsupported(resource: &Resource, rationale: &str) -> ExposurePlan {
    ExposurePlan {
        representation: resource.capability.representation,
        route: CompatibilityRoute::Unsupported,
        verification: VerificationStatus::Unverified,
        mechanism: ExposureMechanism::Unsupported {
            rationale: rationale.to_owned(),
        },
        evidence: rationale.to_owned(),
    }
}

impl AntigravityIntegration {
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
                entry_name: format!("{entry_name}.md"),
                source: resource.capability.path.clone(),
            },
            evidence: "Antigravity CLI natively discovers Markdown custom agents from its global agents directory; UZE keeps a receipt-owned symlink to the canonical Store definition.".to_owned(),
        }
    }

    /// The capability-level fallback for a Hook resource. Antigravity
    /// exposes hooks only through its native Plugin surface — a global
    /// hook config is not part of the documented model — so a hook is
    /// always delivered at package level through the UZE-generated plugin
    /// (see the package exposure plan); this plan exists to state that
    /// honestly if a resource ever surfaces outside package coverage.
    fn hook_fallback_plan(&self, resource: &Resource) -> ExposurePlan {
        let mut plan = unsupported(
            resource,
            "Antigravity carries hooks only inside its native Plugin: this package's hooks are delivered through the UZE-generated named-entry plugin at package level (see Plugin inspection).",
        );
        plan.route = CompatibilityRoute::Native;
        plan.verification = VerificationStatus::Unverified;
        plan
    }
}

impl HookAdapterPort for AntigravityIntegration {
    fn adapter_id(&self) -> &'static str {
        "antigravity"
    }

    fn normalize_input(
        &self,
        native: &serde_json::Value,
        event: HookEvent,
    ) -> std::result::Result<HookCommandInput, String> {
        hook_projection::antigravity_normalize_input(native, event)
    }

    fn render_output(
        &self,
        outcome: &HookDispatchOutcome,
        event: HookEvent,
    ) -> std::result::Result<HookNativeOutput, String> {
        hook_projection::antigravity_render_output(outcome, event)
    }
}

impl PreferencePort for AntigravityIntegration {
    fn preference_id(&self) -> &'static str {
        IntegrationPort::id(self)
    }

    fn translate(&self, preferences: &Preferences) -> PreferenceTranslation {
        preferences::translate(preferences)
    }

    fn apply(&self, preferences: &Preferences) -> Result<PreferenceApplyOutcome> {
        preferences::apply(&self.preferences_config_path(), preferences)
    }
}
