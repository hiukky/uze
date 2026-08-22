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
//! [`skills`] (the managed skills-dir reference), [`extension`] (the native
//! `gemini extensions link` delivery), and [`provision`] (install/update via
//! the official npm package). This file is the composition root: the
//! `GeminiIntegration` struct and its `IntegrationPort` impl, delegating to
//! each submodule.

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use uze_core::{
    Result, UzeError,
    capability::CapabilityKind,
    exposure::{ExposureMechanism, ExposurePlan, PackageExposurePlan},
    home::UzeHome,
    integration::{
        AttachmentInspection, AttachmentReceipt, AttachmentState, HarnessDetection,
        IntegrationPort, ManagedArtifact, detach_standard_receipt, inspect_standard_receipt,
    },
    project::Resource,
    provisioning::{ProcessRunner, ProvisioningResult},
    router::{CompatibilityRoute, HarnessCapabilities, VerificationStatus},
    state,
    store::StoredPackage,
};

mod extension;
mod mcp;
mod provision;
mod skills;

use extension::{extension_name, inspect_linked_extension, linked_extension, run_gemini};
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
            direct_standard: [CapabilityKind::AgentSkill].into_iter().collect(),
            adaptable: [CapabilityKind::Mcp].into_iter().collect(),
            evidence: "Gemini CLI reads Agent Skills from the shared `~/.agents/skills` root directly, and accepts stdio MCP servers through its own `gemini mcp add --scope user`."
                .to_owned(),
            ..HarnessCapabilities::default()
        }
    }

    fn detect(&self) -> HarnessDetection {
        detect_binary("gemini")
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
                "Gemini CLI needs a UZE-stored Agent Plugin package for this attachment.",
            );
        }
        match resource.capability.kind {
            CapabilityKind::AgentSkill => self.skill_exposure_plan(resource),
            CapabilityKind::Mcp => self.mcp_exposure_plan(resource),
            _ => unsupported(
                resource,
                "Gemini attachment is only modeled for Agent Skills and MCP servers.",
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
        if !package.root.join("gemini-extension.json").is_file() {
            return None;
        }
        // An extension's `mcpServers` live inside its own manifest and its
        // skills inside its own `skills/`, so Gemini owns every capability in
        // the package once linked. Attaching any of them again would produce
        // a duplicate the harness never asked for.
        Some(PackageExposurePlan {
            package_id: package.id.clone(),
            route: CompatibilityRoute::Native,
            verification: VerificationStatus::Unverified,
            provided_resource_identities: resources
                .iter()
                .map(|resource| resource.identity())
                .collect::<BTreeSet<_>>(),
            evidence: "The preserved external gemini-extension.json is linked directly from the UZE store through `gemini extensions link`. Gemini owns Skill and MCP loading for a linked extension, so UZE must not attach either resource a second time."
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
        let extension_name = extension_name(&package.root)?;
        // `link` rather than `install`, deliberately, and this is a real
        // trade-off worth stating rather than hiding: Gemini documents
        // `link` as a *development* workflow. UZE uses it as its managed
        // integration mechanism because `install` copies the package into
        // ~/.gemini/extensions — which would make a second copy of bytes the
        // Store already owns, break install-once, and risk a detach removing
        // content UZE could no longer distinguish from the original. `link`
        // keeps the Store the single copy, is non-interactive with
        // `--consent`, and leaves the stored package untouched on uninstall
        // (confirmed empirically against 0.56.0).
        if linked_extension(&self.command_home, &extension_name).is_some() {
            return Ok(Some(self.receipt(package, &extension_name)));
        }
        let status = Command::new("gemini")
            .env("HOME", &self.command_home)
            .args(["extensions", "link"])
            .arg(&package.root)
            .arg("--consent")
            .status();
        match status {
            Ok(status) if status.success() => Ok(Some(self.receipt(package, &extension_name))),
            Ok(status) => Err(UzeError::ExposureUnavailable(format!(
                "`gemini extensions link` exited with {status} for `{extension_name}`"
            ))),
            Err(error) => Err(UzeError::ExposureUnavailable(format!(
                "failed to run `gemini extensions link` for `{extension_name}`: {error}"
            ))),
        }
    }

    fn attach(&self, resource: &Resource) -> Result<Option<PathBuf>> {
        let plan = self.exposure_plan(resource);
        match &plan.mechanism {
            ExposureMechanism::ManagedUserScopeReference { .. } => {
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
        match &receipt.artifact {
            ManagedArtifact::IntegrationOwned {
                kind,
                selector,
                detail,
            } if kind == LINKED_EXTENSION => {
                let Some(source) = detail_path(detail, "source_path") else {
                    return blocked("extension receipt has no expected source path".to_owned());
                };
                inspect_linked_extension(&self.command_home, selector, &source)
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
        match &receipt.artifact {
            ManagedArtifact::IntegrationOwned { kind, selector, .. }
                if kind == LINKED_EXTENSION =>
            {
                // Uninstalling a *linked* extension removes only Gemini's own
                // reference. The stored package this integration never owns
                // stays exactly where it is.
                run_gemini(
                    &self.command_home,
                    &["extensions", "uninstall", selector],
                    "gemini extensions uninstall",
                )?;
            }
            ManagedArtifact::VendorConfigEntry { entry_name, .. } => {
                run_gemini(
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
