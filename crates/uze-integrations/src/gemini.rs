//! Gemini CLI peer integration — EXPERIMENTAL / CONFORMANCE.
//!
//! This integration exists to falsify or confirm the vendor-neutral core, not
//! to claim v0 support. It was chosen as the fourth harness precisely because
//! its native package delivery is shaped unlike Codex's: Gemini points
//! directly at a package directory, so it needs no catalogue and therefore no
//! [`IntegrationPort::republish_packages`] at all. If a package delivers
//! natively here while that hook stays at its default, the two hooks are
//! demonstrably separate concepts rather than one Codex-shaped one.
//!
//! Everything below was confirmed against a pinned Gemini CLI 0.56.0 in the
//! conformance container, with no credential.
//!
//! Split by concern: [`mcp`] (MCP server registration/inspection),
//! [`skills`] (the managed skills-dir reference), [`commands`] (generated
//! user-scope `.toml` command delivery), [`extension`] (the native
//! `gemini extensions link` delivery), and [`provision`] (install/update
//! via the official npm package). This file is the composition root: the
//! `GeminiIntegration` struct and its `IntegrationPort` impl, delegating
//! to each submodule.

use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use uze_core::{
    Result, UzeError,
    capability::CapabilityKind,
    exposure::{ExposureMechanism, ExposurePlan, PackageExposurePlan},
    harness_runtime::resolve_real_executable,
    home::UzeHome,
    integration::{
        AttachmentInspection, AttachmentReceipt, AttachmentState, HarnessDetection,
        IntegrationPort, ManagedArtifact, default_exposure_name_candidates,
        detach_standard_receipt, inspect_standard_receipt, qualified_exposure_name_candidates,
    },
    project::Resource,
    provisioning::{ProcessRunner, ProvisioningResult},
    router::{CompatibilityRoute, HarnessCapabilities, VerificationStatus},
    state,
    store::StoredPackage,
};

mod commands;
mod extension;
mod generate;
mod mcp;
mod provision;
mod skills;

use crate::shared::process::run_quiet;
use extension::{
    extension_name, gemini_exact_coverage, inspect_linked_extension, linked_extension, run_gemini,
};
use generate::{
    GENERATED_LINKED_EXTENSION, generatable, generated_exact_coverage, generated_extension_receipt,
    materialize_generated_extension, remove_generated_extension_by_id,
};
use mcp::attach_mcp_entry;
use provision::{detect_binary, provision_npm};

/// The `kind` this integration stamps on its own receipts. Only this module
/// interprets it.
const LINKED_EXTENSION: &str = "linked-extension";

pub struct GeminiIntegration {
    /// Shared, cwd-independent Agent Skills discovery root. Gemini reads
    /// `~/.agents/skills` alongside its own `~/.gemini/skills`, which is why
    /// the Skill fallback needs no Gemini-specific mechanism at all.
    skills_dir: PathBuf,
    /// User-scope custom commands directory (`~/.gemini/commands/`), where
    /// Gemini discovers `.toml` command files.
    commands_dir: PathBuf,
    /// `HOME` set explicitly for every shelled-out `gemini` subcommand, for
    /// the same reason the Claude and Codex integrations do it: Gemini
    /// derives `~/.gemini` from `$HOME` and must never be pointed at the
    /// calling process's own environment by accident.
    command_home: PathBuf,
    uze_home: UzeHome,
}

impl GeminiIntegration {
    pub fn new(agents_home: PathBuf, uze_home: UzeHome) -> Self {
        let command_home = agents_home
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| agents_home.clone());
        Self {
            skills_dir: agents_home.join("skills"),
            commands_dir: command_home.join(".gemini/commands"),
            command_home,
            uze_home,
        }
    }

    #[allow(dead_code)]
    pub fn from_env(uze_home: UzeHome) -> Result<Self> {
        let home = std::env::var_os("HOME").ok_or(UzeError::MissingHomeDirectory)?;
        Ok(Self::new(PathBuf::from(home).join(".agents"), uze_home))
    }

    fn settings_path(&self) -> PathBuf {
        self.command_home.join(".gemini/settings.json")
    }

    /// Same PATH-shim recursion hazard, and the same fix, as
    /// `ClaudeIntegration::provisioning_executable` and
    /// `CodexIntegration::provisioning_executable`: internal invocations must
    /// never risk re-entering UZE's own `~/.uze/shims/gemini`.
    fn provisioning_executable(&self) -> String {
        resolve_real_executable(&["gemini"], &self.uze_home.shims_dir())
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| "gemini".to_owned())
    }

    /// Links a package whose source ships its own `gemini-extension.json`
    /// directly from the Store. Unchanged behavior — extracted verbatim
    /// from the pre-generation `attach_package`.
    ///
    /// `link` rather than `install`, deliberately, and this is a real
    /// trade-off worth stating rather than hiding: Gemini documents `link`
    /// as a *development* workflow. UZE uses it as its managed integration
    /// mechanism because `install` copies the package into
    /// ~/.gemini/extensions — which would make a second copy of bytes the
    /// Store already owns, break install-once, and risk a detach removing
    /// content UZE could no longer distinguish from the original. `link`
    /// keeps the Store the single copy, is non-interactive with
    /// `--consent`, and leaves the stored package untouched on uninstall
    /// (confirmed empirically against 0.56.0).
    fn attach_explicit_extension(
        &self,
        executable: &str,
        package: &StoredPackage,
    ) -> Result<Option<AttachmentReceipt>> {
        let extension_name = extension_name(&package.root)?;
        if linked_extension(executable, &self.command_home, &extension_name).is_some() {
            return Ok(Some(self.receipt(package, &extension_name)));
        }
        let args: Vec<&OsStr> = vec![
            OsStr::new("extensions"),
            OsStr::new("link"),
            package.root.as_os_str(),
            OsStr::new("--consent"),
        ];
        run_quiet(
            Path::new(executable),
            &self.command_home,
            &format!("gemini extensions link `{extension_name}`"),
            &args,
        )?;
        Ok(Some(self.receipt(package, &extension_name)))
    }

    /// Links a package with no author-provided envelope from a UZE-owned
    /// derived directory, materializing (or refreshing) its generated
    /// extension first. No marketplace/catalogue is needed for Gemini
    /// either way — `link` points straight at whichever directory
    /// (Store-owned or UZE-generated) actually carries the manifest.
    fn attach_generated_extension(
        &self,
        executable: &str,
        package: &StoredPackage,
    ) -> Result<Option<AttachmentReceipt>> {
        let dir = materialize_generated_extension(&self.uze_home, package)?;
        let extension_name = package.id.as_str();
        if linked_extension(executable, &self.command_home, extension_name).is_some() {
            return Ok(Some(generated_extension_receipt(self.id(), package, &dir)));
        }
        let args: Vec<&OsStr> = vec![
            OsStr::new("extensions"),
            OsStr::new("link"),
            dir.as_os_str(),
            OsStr::new("--consent"),
        ];
        run_quiet(
            Path::new(executable),
            &self.command_home,
            &format!("gemini extensions link `{extension_name}`"),
            &args,
        )?;
        Ok(Some(generated_extension_receipt(self.id(), package, &dir)))
    }
}

impl IntegrationPort for GeminiIntegration {
    fn id(&self) -> &'static str {
        "gemini"
    }

    fn aliases(&self) -> &'static [&'static str] {
        &["gemini-cli"]
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            native: [CapabilityKind::AgentSkill, CapabilityKind::Mcp, CapabilityKind::Command]
                .into_iter()
                .collect(),
            verification: VerificationStatus::Unverified,
            evidence: "Gemini CLI consumes UZE's linked extensions: a package shipping gemini-extension.json is linked straight from the Store via `gemini extensions link`; one without gets a deterministically synthesized extension linked from a UZE-owned derived directory (ADR-021) — both deliver the extension's conventional skills/, its commands/ TOML files, and its declared mcpServers natively (confirmed against real Gemini CLI 0.56.0 dogfood). A canonical command outside any extension reaches Gemini as a generated user-scope `~/.gemini/commands/<name>.toml` (ADR-025). Capability-level fallback (`gemini mcp add --scope user`) remains only for resources outside the extension's coverage."
                .to_owned(),
            ..HarnessCapabilities::default()
        }
    }

    fn detect(&self) -> HarnessDetection {
        detect_binary(&self.provisioning_executable())
    }

    /// OpenCode and Codex also discover Skills from this exact same
    /// `~/.agents/skills` directory; see `OpenCodeIntegration`'s override of
    /// the same method for why this must be reported.
    fn shared_agent_skill_root(&self) -> Option<PathBuf> {
        Some(self.skills_dir.clone())
    }

    fn provision(&self, runner: &dyn ProcessRunner) -> Result<ProvisioningResult> {
        provision_npm(runner, self.detect().present)
    }

    fn install(&self, home: &UzeHome, detection: &HarnessDetection) -> Result<()> {
        fs::create_dir_all(&self.skills_dir).map_err(|source| UzeError::Write {
            path: self.skills_dir.clone(),
            source,
        })?;
        fs::create_dir_all(&self.commands_dir).map_err(|source| UzeError::Write {
            path: self.commands_dir.clone(),
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

    /// Gemini's naming decision: commands nest under their plugin namespace
    /// as vendor paths (Gemini converts the path separator to a colon, so
    /// `flow/review.toml` → `/flow:review`); Skills in the shared root get
    /// the stable namespaced label verbatim as their directory name. MCP
    /// stays fully qualified — capability naming policies are never mixed
    /// just because all are `Resource`s.
    fn exposure_name_candidates(&self, resource: &Resource) -> Vec<String> {
        match resource.capability.kind {
            CapabilityKind::Command => commands::gemini_command_exposure_name_candidates(resource),
            CapabilityKind::AgentSkill => qualified_exposure_name_candidates(resource),
            _ => default_exposure_name_candidates(resource),
        }
    }

    fn exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        if resource.package_root().is_none() {
            return unsupported(
                resource,
                "Gemini CLI needs a UZE-stored Agent Plugin package for this attachment.",
            );
        }
        match resource.capability.kind {
            CapabilityKind::AgentSkill => self.skill_exposure_plan(resource),
            CapabilityKind::Command => self.command_plan(resource),
            CapabilityKind::Mcp => self.mcp_exposure_plan(resource),
            _ => unsupported(
                resource,
                "Gemini attachment is only modeled for Agent Skills, Commands, and MCP servers.",
            ),
        }
    }

    /// A package carrying a source-provided `gemini-extension.json` is
    /// delivered whole. UZE never synthesizes that manifest: a package that
    /// does not ship one has no native route here and is decomposed
    /// capability by capability instead.
    fn package_exposure_plan(
        &self,
        package: &StoredPackage,
        resources: &[&Resource],
    ) -> Option<PackageExposurePlan> {
        if package.root.join("gemini-extension.json").is_file() {
            // An extension's `mcpServers` live inside its own manifest
            // (declared inline, by name) and its skills inside its own
            // conventional `skills/` directory — Gemini owns exactly those,
            // not everything the Engine happened to discover in the same
            // package tree.
            let provided = gemini_exact_coverage(package, resources);
            return Some(PackageExposurePlan {
                package_id: package.id.clone(),
                route: CompatibilityRoute::Native,
                verification: VerificationStatus::Unverified,
                provided_resource_identities: provided,
                evidence: "The preserved external gemini-extension.json is linked directly from the UZE store through `gemini extensions link`, for exactly the skills/mcpServers it declares; undeclared resources fall back to individual attachment."
                    .to_owned(),
            });
        }
        // No author-provided envelope. Check whether UZE can safely
        // synthesize one (ADR-020/ADR-021, refining ADR-013 §2: Explicit
        // Native Package/Extension > Generated Native Package/Extension >
        // Native Capability > Safe Adaptation > Unsupported). Stays
        // read-only either way.
        if !generatable(package) {
            return None;
        }
        let provided = generated_exact_coverage(package, resources);
        Some(PackageExposurePlan {
            package_id: package.id.clone(),
            route: CompatibilityRoute::Native,
            verification: VerificationStatus::Unverified,
            provided_resource_identities: provided,
            evidence: "No gemini-extension.json was provided. UZE synthesizes one deterministically into a UZE-owned derived directory (never the Store) covering exactly the package's conventional skills/ directory and mcp.json-declared servers, linked directly from that derived directory."
                .to_owned(),
        })
    }

    // `republish_packages` deliberately remains its default no-op. Gemini
    // needs no catalogue: `attach_package` points it straight at the stored
    // package directory. This proves publication is optional rather than a
    // Codex marketplace concept under a general name.

    fn attach_package(
        &self,
        package: &StoredPackage,
        _plan: &PackageExposurePlan,
    ) -> Result<Option<AttachmentReceipt>> {
        let executable = self.provisioning_executable();
        if package.root.join("gemini-extension.json").is_file() {
            return self.attach_explicit_extension(&executable, package);
        }
        self.attach_generated_extension(&executable, package)
    }

    fn attach(&self, resource: &Resource) -> Result<Option<PathBuf>> {
        let plan = self.exposure_plan(resource);
        match &plan.mechanism {
            ExposureMechanism::ManagedUserScopeReference { .. } => {
                Ok(Some(plan.mechanism.attach()?))
            }
            ExposureMechanism::ManagedFile { .. } => {
                Ok(Some(plan.mechanism.attach_managed_file()?))
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
            } if kind == LINKED_EXTENSION || kind == GENERATED_LINKED_EXTENSION => {
                let Some(source) = detail_path(detail, "source_path") else {
                    return blocked("extension receipt has no expected source path".to_owned());
                };
                inspect_linked_extension(
                    &self.provisioning_executable(),
                    &self.command_home,
                    selector,
                    &source,
                )
            }
            ManagedArtifact::VendorConfigEntry {
                entry_name,
                transport,
                command,
                args,
                cwd,
                environment,
                enabled,
            } => mcp::inspect_gemini_mcp(
                &self.settings_path(),
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
                if kind == LINKED_EXTENSION || kind == GENERATED_LINKED_EXTENSION =>
            {
                // Uninstalling a *linked* extension removes only Gemini's own
                // reference. The stored package this integration never owns
                // stays exactly where it is.
                run_gemini(
                    &executable,
                    &self.command_home,
                    &["extensions", "uninstall", selector],
                    "gemini extensions uninstall",
                )?;
                if kind == GENERATED_LINKED_EXTENSION {
                    // The generated envelope directory is a Derived Artifact
                    // (ADR-013 §4): non-authoritative, rebuildable, and
                    // never the canonical Store — safe to remove outright
                    // now that Gemini no longer references it.
                    remove_generated_extension_by_id(&self.uze_home, &receipt.package_id)?;
                }
            }
            ManagedArtifact::VendorConfigEntry { entry_name, .. } => {
                run_gemini(
                    &executable,
                    &self.command_home,
                    &["mcp", "remove", "--scope", "user", entry_name],
                    "gemini mcp remove",
                )?;
            }
            _ => return detach_standard_receipt(receipt),
        }
        Ok(AttachmentInspection {
            state: AttachmentState::Missing,
            reason: "Gemini managed artifact detached".to_owned(),
        })
    }
}

fn detail_path(
    detail: &std::collections::BTreeMap<String, serde_json::Value>,
    key: &str,
) -> Option<PathBuf> {
    detail
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
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
