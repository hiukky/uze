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

use std::{fs, path::Path, path::PathBuf, process::Command};

use uze_core::{
    Result, UzeError,
    capability::CapabilityKind,
    exposure::{ExposureMechanism, ExposurePlan, PackageExposurePlan},
    home::UzeHome,
    integration::{
        AttachmentInspection, AttachmentReceipt, AttachmentState, HarnessDetection,
        IntegrationPort, ManagedArtifact, PublicationStatus, detach_standard_receipt,
        inspect_standard_receipt,
    },
    project::Resource,
    provisioning::{ProcessRunner, ProcessSpec, ProvisioningResult},
    router::{CompatibilityRoute, HarnessCapabilities, VerificationStatus},
    state,
    store::StoredPackage,
};

mod mcp;
mod plugin;
mod provision;
mod skills;

pub use mcp::detach_mcp_entry;

use mcp::attach_mcp_entry;
use plugin::{
    MARKETPLACE_NAME, catalogue_document, detail_path, inspect_codex_plugin, marketplace_exists,
    publishable, remove_plugin, run_codex, write_catalogue,
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
}

impl IntegrationPort for CodexIntegration {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            adaptable: [CapabilityKind::AgentSkill, CapabilityKind::Mcp]
                .into_iter()
                .collect(),
            verification: VerificationStatus::Unverified,
            evidence: "Codex documents a cwd-independent USER-scope Agent Skill directory (<agents_home>/skills) that follows symlinks; UZE places a managed reference there once setup completes. `codex mcp add` registers an MCP server globally (no --scope flag exists; global is the only destination), confirmed non-interactive and format-preserving."
                .to_owned(),
            ..HarnessCapabilities::default()
        }
    }

    fn detect(&self) -> HarnessDetection {
        detect_binary("codex")
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
            ProcessSpec::new("codex", ["--upgrade"]).with_inherited_output(),
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
                "Codex attachment needs a UZE-stored Agent Plugin package.",
            );
        }
        match resource.capability.kind {
            CapabilityKind::AgentSkill => self.skill_exposure_plan(resource),
            CapabilityKind::Mcp => self.mcp_exposure_plan(resource),
            _ => unsupported(
                resource,
                "Codex attachment is only modeled for Agent Skills and MCP servers.",
            ),
        }
    }

    fn package_exposure_plan(
        &self,
        package: &StoredPackage,
        resources: &[&Resource],
    ) -> Option<PackageExposurePlan> {
        if !package.root.join(".codex-plugin/plugin.json").is_file() {
            return None;
        }
        Some(PackageExposurePlan {
            package_id: package.id.clone(),
            route: CompatibilityRoute::Native,
            verification: VerificationStatus::Unverified,
            provided_resource_identities: resources
                .iter()
                .map(|resource| resource.identity())
                .collect::<std::collections::BTreeSet<_>>(),
            evidence: "The preserved external .codex-plugin/plugin.json is exposed through UZE's generated, standard Codex local marketplace catalog. Codex owns Skill and MCP loading, so UZE must not attach either resource a second time.".to_owned(),
        })
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

    fn attach_package(
        &self,
        package: &StoredPackage,
        _plan: &PackageExposurePlan,
    ) -> Result<Option<AttachmentReceipt>> {
        let catalogue_root = self.catalogue_root();
        if !marketplace_exists(&self.command_home, &catalogue_root) {
            run_codex(
                &self.command_home,
                ["plugin", "marketplace", "add"],
                Some(&catalogue_root),
            )?;
        }
        let selector = format!("{}@{MARKETPLACE_NAME}", package.id.as_str());
        match Command::new("codex")
            .env("HOME", &self.command_home)
            .args(["plugin", "add", &selector])
            .status()
        {
            Ok(status) if status.success() => Ok(Some(AttachmentReceipt {
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
            })),
            Ok(status) => Err(UzeError::ExposureUnavailable(format!(
                "`codex plugin add` exited with {status} for `{selector}`"
            ))),
            Err(error) => Err(UzeError::ExposureUnavailable(format!(
                "failed to run `codex plugin add` for `{selector}`: {error}"
            ))),
        }
    }

    fn republish_packages(&self, packages: &[StoredPackage]) -> Result<()> {
        write_catalogue(&self.catalogue_path(), packages)
    }

    fn publication(&self, packages: &[StoredPackage]) -> PublicationStatus {
        let expected = catalogue_document(packages);
        match fs::read(self.catalogue_path()) {
            Ok(bytes) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
                Ok(actual) if actual == expected => PublicationStatus::Published,
                Ok(_) => PublicationStatus::Unpublished(
                    "the Codex catalogue does not match the installed package set; re-run `uze setup codex`".to_owned(),
                ),
                Err(error) => PublicationStatus::Unpublished(format!(
                    "the Codex catalogue is unreadable ({error}); re-run `uze setup codex`"
                )),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if publishable(packages).is_empty() {
                    PublicationStatus::Published
                } else {
                    PublicationStatus::Unpublished(
                        "no Codex catalogue has been written for the installed packages; re-run `uze setup codex`".to_owned(),
                    )
                }
            }
            Err(error) => PublicationStatus::Unpublished(error.to_string()),
        }
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
            } => mcp::inspect_codex_mcp(
                &self.command_home,
                entry_name,
                transport,
                command,
                args,
                cwd.as_deref(),
                environment,
                *enabled,
            ),
            ManagedArtifact::IntegrationOwned {
                kind,
                selector,
                detail,
            } if kind == "marketplace-plugin" => {
                let Some(marketplace_root) = detail_path(detail, "marketplace_root") else {
                    return plugin::blocked("plugin receipt has no marketplace root".to_owned());
                };
                let Some(package_root) = detail_path(detail, "package_root") else {
                    return plugin::blocked("plugin receipt has no package root".to_owned());
                };
                inspect_codex_plugin(
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
                mcp::detach_mcp_entry(&self.command_home, entry_name)?;
            }
            ManagedArtifact::IntegrationOwned { kind, selector, .. }
                if kind == "marketplace-plugin" =>
            {
                remove_plugin(&self.command_home, selector)?;
            }
            _ => return detach_standard_receipt(receipt),
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
