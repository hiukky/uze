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
    integration::{
        AttachmentInspection, AttachmentReceipt, AttachmentState, HarnessDetection,
        IntegrationPort, ManagedArtifact, PublicationStatus, default_exposure_name_candidates,
        detach_standard_receipt, inspect_standard_receipt,
    },
    project::Resource,
    provisioning::{ProcessRunner, ProcessSpec, ProvisioningResult},
    router::{CompatibilityRoute, HarnessCapabilities, VerificationStatus},
    state,
    store::StoredPackage,
};

mod commands;
mod generate;
mod mcp;
mod plugin;
mod provision;
mod skills;

pub use mcp::detach_mcp_entry;

use crate::shared::process::run_quiet;
use commands::{
    codex_command_exposure_name_candidates, materialize_generated_command,
    materialize_generated_skill,
};
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

/// Codex peer integration. Its transparent-attachment strategy is a
/// UZE-managed reference at `<agents_home>/skills/<name>` (see ADR-006):
/// Codex documents a cwd-independent USER-scope Agent Skill directory that
/// explicitly follows symlinks. Until `uze setup` has completed, exposure
/// falls back to the per-session managed projection from ADR-005.
pub struct CodexIntegration {
    skills_dir: PathBuf,
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
        let selector = format!("{}@{MARKETPLACE_NAME}", package.id.as_str());
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
    /// second, UZE-owned `uze-local-generated` marketplace, materializing
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
        let selector = format!("{}@{GENERATED_MARKETPLACE_NAME}", package.id.as_str());
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
}

impl IntegrationPort for CodexIntegration {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            native: [
                CapabilityKind::AgentSkill,
                CapabilityKind::Mcp,
                CapabilityKind::Command,
            ]
            .into_iter()
            .collect(),
            verification: VerificationStatus::Unverified,
            evidence: "Codex consumes UZE's derived marketplaces: a package shipping .codex-plugin/plugin.json is added as a native plugin covering its declared skills/mcpServers (`codex plugin add <sel>@uze-local`); one without gets a deterministically synthesized envelope published through the generated-only `uze-local-generated` marketplace (ADR-021) — both confirmed against real Codex 0.148.0 dogfood (`codex plugin list --json`). Commands are NATIVE via Codex's official explicit-invocation-only Skill mechanism: a canonical Command becomes a generated user-invokable Skill carrying `agents/openai.yaml` → `policy.allow_implicit_invocation: false` (Codex Build skills documentation; empirically honored by codex-cli 0.149.0 via `codex debug prompt-input` — the skill leaves the model-visible list only when that policy file is present and well-formed), so the model cannot auto-select it while explicit `$skill` invocation keeps working. Per ADR-025, Native means an officially supported primitive that preserves the canonical capability semantics — not an identical vendor file format or primitive name. Capability-level fallbacks (USER-scope `~/.agents/skills` reference, `codex mcp add`) remain only for resources outside the envelope's coverage."
                .to_owned(),
            ..HarnessCapabilities::default()
        }
    }

    fn detect(&self) -> HarnessDetection {
        detect_binary(&self.provisioning_executable())
    }

    /// OpenCode and Gemini CLI also discover Skills from this exact same
    /// `~/.agents/skills` directory; see `OpenCodeIntegration`'s override of
    /// the same method for why this must be reported.
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
            CapabilityKind::Command => self.command_exposure_plan(resource),
            CapabilityKind::Mcp => self.mcp_exposure_plan(resource),
            _ => unsupported(
                resource,
                "Codex attachment is only modeled for Agent Skills, Commands, and MCP servers.",
            ),
        }
    }

    /// Codex's naming decision: every UZE-projected Skill and Command gets
    /// its stable namespaced invocation label (`flow:review`) as the single
    /// candidate — never a bare alias, never collision-dependent naming
    /// (ADR-026). Codex accepts `:` in skill names (verified against
    /// codex-cli 0.149.0). MCP stays on the default fully-qualified policy.
    fn exposure_name_candidates(&self, resource: &Resource) -> Vec<String> {
        if matches!(
            resource.capability.kind,
            CapabilityKind::AgentSkill | CapabilityKind::Command
        ) {
            return codex_command_exposure_name_candidates(resource);
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
                // new may replace it.
                if resource.resolved_artifact_target.is_none() {
                    match resource.capability.kind {
                        CapabilityKind::Command => {
                            materialize_generated_command(&self.uze_home, resource)?;
                        }
                        CapabilityKind::AgentSkill => {
                            materialize_generated_skill(&self.uze_home, resource)?;
                        }
                        _ => {}
                    }
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
                    self.cleanup_unused_command_adaptation(target)?;
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
