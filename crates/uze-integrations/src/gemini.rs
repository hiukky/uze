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
    provisioning::{ProcessRunner, ProcessSpec, ProvisionAction, ProvisioningResult},
    router::{CompatibilityRoute, HarnessCapabilities, VerificationStatus},
    state,
    store::StoredPackage,
};

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

    fn skill_exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        if state::is_installed(&self.uze_home, self.id())
            && let Some(entry_name) = resource.attachment_entry_name()
        {
            let skill_directory = resource
                .capability
                .path
                .parent()
                .expect("SKILL.md has a parent");
            return ExposurePlan {
                representation: resource.capability.representation,
                // Gemini documents `~/.agents/skills` as one of its own skill
                // discovery roots, so a UZE-managed reference there is
                // consumed natively rather than adapted.
                route: CompatibilityRoute::Native,
                verification: VerificationStatus::Unverified,
                mechanism: ExposureMechanism::ManagedUserScopeReference {
                    discovery_root: self.skills_dir.clone(),
                    entry_name,
                    source: skill_directory.to_path_buf(),
                },
                evidence: "Gemini CLI discovers Agent Skills from the shared `~/.agents/skills` root, so UZE reuses the same managed reference it places for its other peers. SKILL.md remains the preserved standard payload in the UZE store."
                    .to_owned(),
            };
        }
        unsupported(
            resource,
            "Gemini setup has not completed, so no managed skill reference exists yet.",
        )
    }

    fn mcp_exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        let Some(entry_name) = resource.attachment_entry_name() else {
            return unsupported(resource, "MCP resource has no derivable entry name.");
        };
        if !state::is_installed(&self.uze_home, self.id()) {
            return unsupported(
                resource,
                "Gemini setup has not completed, so no managed MCP entry exists yet.",
            );
        }
        let Some((command, args)) = stdio_command(resource) else {
            return unsupported(
                resource,
                "Gemini MCP attachment is only modeled for a stdio command/args server.",
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
            evidence: "UZE registers the store-owned MCP server once via `gemini mcp add --scope user --transport stdio`, writing to ~/.gemini/settings.json's mcpServers. The Gemini MCP runtime remains native."
                .to_owned(),
        }
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

    fn provision(&self, runner: &dyn ProcessRunner) -> Result<ProvisioningResult> {
        if !cfg!(unix) {
            return Ok(ProvisioningResult::blocked(
                "Gemini automatic provisioning is currently supported on Unix and WSL only",
            ));
        }
        let action = if self.detect().present {
            ProvisionAction::Update
        } else {
            ProvisionAction::Install
        };
        let outcome = match runner.run(
            &ProcessSpec::new("npm", ["install", "-g", "@google/gemini-cli@latest"])
                .with_inherited_output(),
        ) {
            Ok(outcome) => outcome,
            Err(_) => {
                return Ok(ProvisioningResult::failed(
                    action,
                    "official-npm",
                    "official package installation could not be started",
                ));
            }
        };
        if !outcome.success {
            let reason = if outcome.timed_out {
                "official package installation timed out"
            } else {
                "official package installation exited unsuccessfully"
            };
            return Ok(ProvisioningResult::failed(action, "official-npm", reason));
        }
        let verified = runner.run(&ProcessSpec::new("gemini", ["--version"]));
        if !matches!(verified, Ok(output) if output.success) {
            return Ok(ProvisioningResult::failed(
                action,
                "official-npm",
                "installation finished but the executable could not be verified",
            ));
        }
        Ok(ProvisioningResult::verified(
            action,
            "official-npm",
            detect_binary("gemini"),
        ))
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
            } => inspect_gemini_mcp(
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

impl GeminiIntegration {
    fn receipt(&self, package: &StoredPackage, extension_name: &str) -> AttachmentReceipt {
        AttachmentReceipt {
            package_id: package.id.as_str().to_owned(),
            resource_identity: None,
            integration: self.id().to_owned(),
            strategy: "linked-native-extension".to_owned(),
            artifact: ManagedArtifact::IntegrationOwned {
                kind: LINKED_EXTENSION.to_owned(),
                selector: extension_name.to_owned(),
                detail: [
                    // What Gemini reports back as the extension's `path` for a
                    // link install. Ownership is proven by identity *and*
                    // source, never by name alone.
                    ("source_path".to_owned(), serde_json::json!(package.root)),
                    ("package_root".to_owned(), serde_json::json!(package.root)),
                ]
                .into_iter()
                .collect(),
            },
        }
    }
}

/// The extension's own declared name, which is the selector every Gemini
/// verb takes. Read from the preserved external manifest — never invented.
fn extension_name(package_root: &Path) -> Result<String> {
    let path = package_root.join("gemini-extension.json");
    let bytes = fs::read(&path).map_err(|source| UzeError::Read {
        path: path.clone(),
        source,
    })?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|source| UzeError::Json { path, source })?;
    manifest
        .get("name")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            UzeError::ExposureUnavailable("gemini-extension.json has no string name".to_owned())
        })
}

/// Reads Gemini's machine-readable output.
///
/// `gemini extensions list --output-format=json` exits 0 and writes its JSON
/// to **stderr**, leaving stdout empty (confirmed against 0.56.0). Preferring
/// stdout and falling back to stderr keeps this correct either way, so a
/// future release that moves the payload to stdout needs no change here.
fn gemini_json(
    command_home: &Path,
    args: &[&str],
) -> std::result::Result<serde_json::Value, String> {
    let output = Command::new("gemini")
        .env("HOME", command_home)
        .args(args)
        .output()
        .map_err(|error| format!("failed to run `gemini`: {error}"))?;
    if !output.status.success() {
        return Err(format!("`gemini` inspection exited with {}", output.status));
    }
    let payload = if output.stdout.iter().any(|byte| !byte.is_ascii_whitespace()) {
        &output.stdout
    } else {
        &output.stderr
    };
    serde_json::from_slice(payload).map_err(|error| format!("Gemini JSON is invalid: {error}"))
}

/// One installed extension, by name, from Gemini's own machine-readable
/// listing. `None` when absent or when the listing itself is unavailable —
/// callers distinguish those two cases.
fn linked_extension(command_home: &Path, name: &str) -> Option<serde_json::Value> {
    let listing = gemini_json(
        command_home,
        &["extensions", "list", "--output-format=json"],
    )
    .ok()?;
    listing
        .as_array()?
        .iter()
        .find(|entry| entry.get("name").and_then(serde_json::Value::as_str) == Some(name))
        .cloned()
}

fn inspect_linked_extension(
    command_home: &Path,
    name: &str,
    expected_source: &Path,
) -> AttachmentInspection {
    let listing = match gemini_json(
        command_home,
        &["extensions", "list", "--output-format=json"],
    ) {
        Ok(listing) => listing,
        Err(reason) => return blocked(reason),
    };
    inspect_listing(&listing, name, expected_source)
}

/// The ownership decision, separated from the process call so every branch is
/// testable without a Gemini binary.
fn inspect_listing(
    listing: &serde_json::Value,
    name: &str,
    expected_source: &Path,
) -> AttachmentInspection {
    let Some(entries) = listing.as_array() else {
        return blocked("Gemini extension listing is not an array".to_owned());
    };
    let matching: Vec<&serde_json::Value> = entries
        .iter()
        .filter(|entry| entry.get("name").and_then(serde_json::Value::as_str) == Some(name))
        .collect();
    let entry = match matching.as_slice() {
        [] => {
            return AttachmentInspection {
                state: AttachmentState::Missing,
                reason: "Gemini extension is not installed".to_owned(),
            };
        }
        [entry] => entry,
        // Two extensions answering to one name is ambiguous ownership: UZE
        // cannot prove which one it created, so it must not touch either.
        _ => {
            return AttachmentInspection {
                state: AttachmentState::Conflict,
                reason: "more than one Gemini extension answers to this name".to_owned(),
            };
        }
    };
    let actual_source = entry
        .pointer("/installMetadata/source")
        .and_then(serde_json::Value::as_str);
    let install_type = entry
        .pointer("/installMetadata/type")
        .and_then(serde_json::Value::as_str);
    let Some(actual_source) = actual_source else {
        return blocked("Gemini extension entry has no install source".to_owned());
    };
    if Path::new(actual_source) != expected_source {
        return AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "Gemini extension points at a different source than the receipt".to_owned(),
        };
    }
    if install_type != Some("link") {
        // A name and source UZE recognizes, delivered by a mechanism it did
        // not use. Someone else owns this entry.
        return AttachmentInspection {
            state: AttachmentState::Conflict,
            reason: "Gemini extension was installed by a different mechanism than UZE's link"
                .to_owned(),
        };
    }
    // Enablement is deliberately *not* part of this ownership proof, for two
    // separate reasons.
    //
    // It is not observable: the listing's `isActive` stays `true` after
    // `gemini extensions disable` (confirmed on 0.56.0). The real state lives
    // in `extension-enablement.json` as path-scoped override globs, and
    // reimplementing that resolution would mean guessing at vendor internals
    // — precisely the guessing ADR-009 exists to forbid.
    //
    // And it is not an ownership signal even if it were observable. Drift
    // means someone repointed the artifact; disabling means the user turned
    // off an artifact UZE still demonstrably created. Treating that as drift
    // would block `uze remove` from detaching UZE's own extension, which is
    // worse behaviour, not safer behaviour. Identity, source and install type
    // together already prove UZE created this entry.
    AttachmentInspection {
        state: AttachmentState::Matched,
        reason: "Gemini linked extension matches receipt (enablement is a user preference, not an ownership signal)".to_owned(),
    }
}

fn attach_mcp_entry(
    command_home: &Path,
    entry_name: &str,
    command: &Path,
    args: &[String],
) -> Result<Option<PathBuf>> {
    // Checked before ever calling `mcp add`: Gemini's overwrite behavior for
    // a colliding, differently-configured name is unconfirmed, so UZE never
    // relies on it (same discipline as ADR-007 for the other peers).
    if mcp_entry_exists(command_home, entry_name) {
        return Ok(Some(PathBuf::from(format!("mcp:{entry_name}"))));
    }
    let status = Command::new("gemini")
        .env("HOME", command_home)
        .args([
            "mcp",
            "add",
            "--scope",
            "user",
            "--transport",
            "stdio",
            entry_name,
        ])
        .arg(command)
        .args(args)
        .status();
    match status {
        Ok(status) if status.success() => Ok(Some(PathBuf::from(format!("mcp:{entry_name}")))),
        Ok(status) => Err(UzeError::ExposureUnavailable(format!(
            "`gemini mcp add` exited with {status} for entry `{entry_name}`"
        ))),
        Err(error) => Err(UzeError::ExposureUnavailable(format!(
            "failed to run `gemini mcp add` for entry `{entry_name}`: {error}"
        ))),
    }
}

fn mcp_entry_exists(command_home: &Path, entry_name: &str) -> bool {
    read_user_mcp_entry(&command_home.join(".gemini/settings.json"), entry_name).is_some()
}

fn read_user_mcp_entry(path: &Path, entry_name: &str) -> Option<serde_json::Value> {
    let bytes = fs::read(path).ok()?;
    let config: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    config
        .get("mcpServers")
        .and_then(serde_json::Value::as_object)
        .and_then(|servers| servers.get(entry_name))
        .cloned()
}

/// `gemini mcp list` has no machine-readable output, so inspection reads the
/// one expected `mcpServers.<name>` entry out of user settings. Attachment
/// and removal still go through the official CLI verbs.
#[allow(clippy::too_many_arguments)]
fn inspect_gemini_mcp(
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
        return blocked(
            "Gemini MCP receipt requests state this integration cannot verify safely".to_owned(),
        );
    }
    if !path.exists() {
        return AttachmentInspection {
            state: AttachmentState::Missing,
            reason: "Gemini user settings are missing".to_owned(),
        };
    }
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => return blocked(error.to_string()),
    };
    if serde_json::from_slice::<serde_json::Value>(&bytes).is_err() {
        return blocked("Gemini user settings are malformed".to_owned());
    }
    let Some(entry) = read_user_mcp_entry(path, entry_name) else {
        return AttachmentInspection {
            state: AttachmentState::Missing,
            reason: "Gemini MCP entry is absent".to_owned(),
        };
    };
    inspect_gemini_mcp_value(&entry, command, args)
}

fn inspect_gemini_mcp_value(
    entry: &serde_json::Value,
    command: &Path,
    args: &[String],
) -> AttachmentInspection {
    let actual_command = entry.get("command").and_then(serde_json::Value::as_str);
    if actual_command != command.to_str() {
        return AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "Gemini MCP command differs from receipt".to_owned(),
        };
    }
    let actual_args: Vec<&str> = entry
        .get("args")
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect()
        })
        .unwrap_or_default();
    if actual_args != args.iter().map(String::as_str).collect::<Vec<_>>() {
        return AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "Gemini MCP args differ from receipt".to_owned(),
        };
    }
    // The receipt declares no env, cwd or explicit enabled state, so any of
    // them present is state UZE did not create and cannot claim.
    for unexpected in ["env", "cwd"] {
        if entry.get(unexpected).is_some_and(|value| !value.is_null()) {
            return AttachmentInspection {
                state: AttachmentState::Drifted,
                reason: format!("Gemini MCP entry carries an unexpected `{unexpected}`"),
            };
        }
    }
    AttachmentInspection {
        state: AttachmentState::Matched,
        reason: "Gemini MCP entry matches receipt".to_owned(),
    }
}

fn run_gemini(command_home: &Path, args: &[&str], label: &str) -> Result<()> {
    match Command::new("gemini")
        .env("HOME", command_home)
        .args(args)
        .status()
    {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(UzeError::ExposureUnavailable(format!(
            "`{label}` exited with {status}"
        ))),
        Err(error) => Err(UzeError::ExposureUnavailable(format!(
            "failed to run `{label}`: {error}"
        ))),
    }
}

fn stdio_command(resource: &Resource) -> Option<(PathBuf, Vec<String>)> {
    let config: serde_json::Value = serde_json::from_slice(&resource.capability.payload).ok()?;
    let command = config.get("command").and_then(serde_json::Value::as_str)?;
    let args = config
        .get("args")
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    Some((PathBuf::from(command), args))
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

fn detect_binary(program: &str) -> HarnessDetection {
    match Command::new(program).arg("--version").output() {
        Ok(output) if output.status.success() => HarnessDetection {
            present: true,
            version: String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .next()
                .map(str::to_owned),
        },
        _ => HarnessDetection::default(),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(command: &str, args: &[&str]) -> serde_json::Value {
        serde_json::json!({ "command": command, "args": args })
    }

    fn listing(name: &str, source: &str, install_type: &str) -> serde_json::Value {
        serde_json::json!([{
            "name": name,
            "installMetadata": { "source": source, "type": install_type },
            "isActive": true
        }])
    }

    /// Ownership is proven by identity *and* source, never by name alone.
    #[test]
    fn a_same_named_extension_from_another_source_is_drift() {
        let entries = listing("x", "/somewhere/else", "link");
        let inspection = inspect_listing(&entries, "x", Path::new("/uze/store/packages/x"));
        assert_eq!(inspection.state, AttachmentState::Drifted);
    }

    /// A name and source UZE recognizes, delivered by a mechanism it did not
    /// use, is foreign ownership — never something to detach.
    #[test]
    fn a_differently_installed_extension_is_a_conflict() {
        let entries = listing("x", "/uze/store/packages/x", "local");
        let inspection = inspect_listing(&entries, "x", Path::new("/uze/store/packages/x"));
        assert_eq!(inspection.state, AttachmentState::Conflict);
    }

    #[test]
    fn two_extensions_answering_to_one_name_are_ambiguous() {
        let entries = serde_json::json!([
            { "name": "x", "installMetadata": { "source": "/a", "type": "link" }, "isActive": true },
            { "name": "x", "installMetadata": { "source": "/b", "type": "link" }, "isActive": true },
        ]);
        let inspection = inspect_listing(&entries, "x", Path::new("/a"));
        assert_eq!(inspection.state, AttachmentState::Conflict);
    }

    #[test]
    fn an_absent_extension_is_missing_not_blocked() {
        let entries = serde_json::json!([]);
        let inspection = inspect_listing(&entries, "x", Path::new("/a"));
        assert_eq!(inspection.state, AttachmentState::Missing);
    }

    /// Disabling is a user preference on an artifact UZE still owns, so it
    /// must stay MATCHED — otherwise `uze remove` could never detach it.
    #[test]
    fn a_disabled_extension_is_still_owned_by_uze() {
        let mut entries = listing("x", "/uze/store/packages/x", "link");
        entries[0]["isActive"] = serde_json::json!(false);
        let inspection = inspect_listing(&entries, "x", Path::new("/uze/store/packages/x"));
        assert_eq!(inspection.state, AttachmentState::Matched);
    }

    #[test]
    fn a_matching_mcp_entry_is_matched() {
        assert_eq!(
            inspect_gemini_mcp_value(
                &entry("/bin/server", &["--serve"]),
                Path::new("/bin/server"),
                &["--serve".to_owned()],
            )
            .state,
            AttachmentState::Matched
        );
    }

    #[test]
    fn a_changed_command_or_args_is_drift_not_a_match() {
        assert_eq!(
            inspect_gemini_mcp_value(
                &entry("/bin/other", &["--serve"]),
                Path::new("/bin/server"),
                &["--serve".to_owned()],
            )
            .state,
            AttachmentState::Drifted
        );
        assert_eq!(
            inspect_gemini_mcp_value(
                &entry("/bin/server", &["--different"]),
                Path::new("/bin/server"),
                &["--serve".to_owned()],
            )
            .state,
            AttachmentState::Drifted
        );
    }

    #[test]
    fn state_uze_never_created_is_drift() {
        let mut value = entry("/bin/server", &[]);
        value["env"] = serde_json::json!({ "TOKEN": "x" });
        assert_eq!(
            inspect_gemini_mcp_value(&value, Path::new("/bin/server"), &[]).state,
            AttachmentState::Drifted
        );
    }

    #[test]
    fn an_extension_name_is_read_from_the_preserved_manifest_never_invented() {
        let root = std::env::temp_dir().join(format!("uze-gemini-name-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("gemini-extension.json"),
            r#"{"name":"declared-name","version":"1.0.0"}"#,
        )
        .unwrap();
        assert_eq!(extension_name(&root).unwrap(), "declared-name");

        fs::write(root.join("gemini-extension.json"), r#"{"version":"1.0.0"}"#).unwrap();
        assert!(extension_name(&root).is_err());
        let _ = fs::remove_dir_all(root);
    }
}
