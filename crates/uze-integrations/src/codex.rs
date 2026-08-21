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
        IntegrationPort, ManagedArtifact, PublicationStatus, detach_standard_receipt,
        inspect_standard_receipt,
    },
    project::Resource,
    provisioning::{ProcessRunner, ProcessSpec, ProvisionAction, ProvisioningResult},
    router::{CompatibilityRoute, HarnessCapabilities, VerificationStatus},
    state,
    store::StoredPackage,
};

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

/// Name of the local catalogue this integration publishes. A Codex identity,
/// held by the Codex integration.
const MARKETPLACE_NAME: &str = "uze-local";

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
                .collect::<BTreeSet<_>>(),
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
            } => inspect_codex_mcp(
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
                    return blocked("plugin receipt has no marketplace root".to_owned());
                };
                let Some(package_root) = detail_path(detail, "package_root") else {
                    return blocked("plugin receipt has no package root".to_owned());
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
                detach_mcp_entry(&self.command_home, entry_name)?;
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
            "Codex automatic provisioning is currently supported on Unix and WSL only",
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

fn marketplace_exists(command_home: &Path, root: &Path) -> bool {
    let output = Command::new("codex")
        .env("HOME", command_home)
        .args(["plugin", "marketplace", "list", "--json"])
        .output();
    let Ok(output) = output else {
        return false;
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&output.stdout) else {
        return false;
    };
    value["marketplaces"].as_array().is_some_and(|entries| {
        entries.iter().any(|entry| {
            entry
                .get("root")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|candidate| Path::new(candidate) == root)
        })
    })
}

fn run_codex(command_home: &Path, prefix: [&str; 3], path: Option<&Path>) -> Result<()> {
    let mut command = Command::new("codex");
    command.env("HOME", command_home).args(prefix);
    if let Some(path) = path {
        command.arg(path);
    }
    match command.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(UzeError::ExposureUnavailable(format!(
            "`codex {}` exited with {status}",
            prefix.join(" ")
        ))),
        Err(error) => Err(UzeError::ExposureUnavailable(format!(
            "failed to run `codex {}`: {error}",
            prefix.join(" ")
        ))),
    }
}

impl CodexIntegration {
    fn skill_exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        if state::is_installed(&self.uze_home, self.id())
            && let Some(entry_name) = resource
                .resolved_exposure_name
                .clone()
                .or_else(|| self.exposure_name_candidates(resource).into_iter().next())
        {
            let skill_directory = resource
                .capability
                .path
                .parent()
                .expect("SKILL.md has a parent");
            return ExposurePlan {
                representation: resource.capability.representation,
                route: CompatibilityRoute::Adaptable,
                verification: VerificationStatus::Unverified,
                mechanism: ExposureMechanism::ManagedUserScopeReference {
                    discovery_root: self.skills_dir.clone(),
                    entry_name,
                    source: skill_directory.to_path_buf(),
                },
                evidence: "UZE symlinks <agents_home>/skills/<name> directly at the UZE store's skill directory once, per Codex's documented USER-scope, symlink-following discovery. No per-session preparation is required."
                    .to_owned(),
            };
        }
        let skill_directory = resource
            .capability
            .path
            .parent()
            .expect("SKILL.md has a parent");
        let skill_name = skill_directory
            .file_name()
            .expect("skill directory has a name");
        ExposurePlan {
            representation: resource.capability.representation,
            route: CompatibilityRoute::Adaptable,
            verification: VerificationStatus::Unverified,
            mechanism: ExposureMechanism::FilesystemProjection {
                source: skill_directory.to_path_buf(),
                target_relative: PathBuf::from(".agents/skills").join(skill_name),
            },
            evidence: "Codex has not completed `uze setup`; falling back to the per-session managed projection in the caller workspace rather than a persistent user-scope attachment."
                .to_owned(),
        }
    }

    fn mcp_exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        if !state::is_installed(&self.uze_home, self.id()) {
            return unsupported(
                resource,
                "Codex has not completed `uze setup`; MCP attachment has no per-session conformance-probe fallback (see ADR-007).",
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
            evidence: "UZE registers the store-owned MCP server once via `codex mcp add`, writing to ~/.codex/config.toml's [mcp_servers.*] (no --scope flag exists; global is the only destination). Available to every future session in any project."
                .to_owned(),
        }
    }
}

fn attach_mcp_entry(
    command_home: &Path,
    entry_name: &str,
    command: &PathBuf,
    args: &[String],
) -> Result<Option<PathBuf>> {
    if mcp_entry_exists(command_home, entry_name) {
        return Ok(Some(PathBuf::from(format!("mcp:{entry_name}"))));
    }
    let status = Command::new("codex")
        .env("HOME", command_home)
        .args(["mcp", "add", entry_name, "--"])
        .arg(command)
        .args(args)
        .status();
    match status {
        Ok(status) if status.success() => Ok(Some(PathBuf::from(format!("mcp:{entry_name}")))),
        Ok(status) => Err(UzeError::ExposureUnavailable(format!(
            "`codex mcp add` exited with {status} for entry `{entry_name}`"
        ))),
        Err(error) => Err(UzeError::ExposureUnavailable(format!(
            "failed to run `codex mcp add` for entry `{entry_name}`: {error}"
        ))),
    }
}

/// Idempotently checked before ever calling `codex mcp add` — Codex's
/// overwrite behavior for a colliding, differently-configured name was not
/// confirmed by research, so UZE never relies on it (see ADR-007).
fn mcp_entry_exists(command_home: &Path, entry_name: &str) -> bool {
    Command::new("codex")
        .env("HOME", command_home)
        .args(["mcp", "get", entry_name])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

/// Inspects `codex mcp get --json`, the documented structured Codex surface.
/// No TOML is read or written by UZE; an unavailable or malformed response is
/// deliberately BLOCKED rather than interpreted as absence.
#[allow(clippy::too_many_arguments)]
fn inspect_codex_mcp(
    command_home: &Path,
    entry_name: &str,
    transport: &str,
    command: &Path,
    args: &[String],
    cwd: Option<&Path>,
    environment: &[uze_core::exposure::McpEnvironmentReference],
    enabled: Option<bool>,
) -> AttachmentInspection {
    let output = match Command::new("codex")
        .env("HOME", command_home)
        .args(["mcp", "get", entry_name, "--json"])
        .output()
    {
        Ok(output) => output,
        Err(error) => return blocked(format!("failed to run `codex mcp get --json`: {error}")),
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Current Codex identifies an absent name with exit 1 and this
        // stable diagnostic. Any other non-zero result is not positive
        // absence evidence, so removal must remain blocked.
        if output.status.code() == Some(1)
            && stderr.contains("No MCP server named")
            && stderr.contains(entry_name)
        {
            return AttachmentInspection {
                state: AttachmentState::Missing,
                reason: "Codex MCP entry is absent".to_owned(),
            };
        }
        return blocked(format!(
            "`codex mcp get --json` could not verify entry: {}",
            stderr.trim()
        ));
    }
    let value = match serde_json::from_slice::<serde_json::Value>(&output.stdout) {
        Ok(value) => value,
        Err(error) => return blocked(format!("Codex MCP JSON is invalid: {error}")),
    };
    inspect_codex_mcp_value(
        &value,
        entry_name,
        transport,
        command,
        args,
        cwd,
        environment,
        enabled,
    )
}

#[allow(clippy::too_many_arguments)]
fn inspect_codex_mcp_value(
    value: &serde_json::Value,
    entry_name: &str,
    expected_transport: &str,
    command: &Path,
    args: &[String],
    expected_cwd: Option<&Path>,
    expected_environment: &[uze_core::exposure::McpEnvironmentReference],
    expected_enabled: Option<bool>,
) -> AttachmentInspection {
    let object = value
        .as_object()
        .or_else(|| value.get("server")?.as_object());
    let Some(object) = object else {
        return blocked("Codex MCP JSON has no server object".to_owned());
    };
    if let Some(name) = object
        .get("name")
        .or_else(|| object.get("id"))
        .and_then(serde_json::Value::as_str)
        && name != entry_name
    {
        return AttachmentInspection {
            state: AttachmentState::Conflict,
            reason: "Codex MCP JSON identifies a different entry".to_owned(),
        };
    }
    if expected_enabled.is_some()
        && object.get("enabled").and_then(serde_json::Value::as_bool) != expected_enabled
    {
        return AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "Codex MCP entry is disabled".to_owned(),
        };
    }
    let transport = object
        .get("transport")
        .and_then(serde_json::Value::as_object)
        .unwrap_or(object);
    if transport
        .get("type")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|actual| actual != expected_transport)
    {
        return AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "Codex MCP transport differs from stdio receipt".to_owned(),
        };
    }
    let Some(actual_command) = transport.get("command").and_then(serde_json::Value::as_str) else {
        return blocked("Codex MCP JSON has no stdio command".to_owned());
    };
    let Some(actual_args) = transport.get("args").and_then(serde_json::Value::as_array) else {
        return blocked("Codex MCP JSON has no args array".to_owned());
    };
    let actual_args = actual_args
        .iter()
        .map(serde_json::Value::as_str)
        .collect::<Option<Vec<_>>>();
    let Some(actual_args) = actual_args else {
        return blocked("Codex MCP JSON args are not strings".to_owned());
    };
    if let Some(expected_cwd) = expected_cwd
        && transport.get("cwd").and_then(serde_json::Value::as_str)
            != Some(expected_cwd.to_string_lossy().as_ref())
    {
        return AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "Codex MCP cwd differs from receipt".to_owned(),
        };
    }
    if !expected_environment.is_empty() {
        let actual = transport
            .get("env_vars")
            .and_then(serde_json::Value::as_array)
            .and_then(|values| {
                values
                    .iter()
                    .map(serde_json::Value::as_str)
                    .collect::<Option<Vec<_>>>()
            });
        let expected = expected_environment
            .iter()
            .map(|reference| reference.name.as_str())
            .collect::<Vec<_>>();
        if actual.as_deref() != Some(expected.as_slice()) {
            return AttachmentInspection {
                state: AttachmentState::Drifted,
                reason: "Codex MCP environment references differ from receipt".to_owned(),
            };
        }
    }
    if actual_command == command.to_string_lossy() && actual_args == args {
        AttachmentInspection {
            state: AttachmentState::Matched,
            reason: "Codex MCP entry matches receipt".to_owned(),
        }
    } else {
        AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "Codex MCP command or args differ from receipt".to_owned(),
        }
    }
}

fn inspect_codex_plugin(
    command_home: &Path,
    selector: &str,
    marketplace_root: &Path,
    package_root: &Path,
) -> AttachmentInspection {
    let marketplace = match codex_json(command_home, ["plugin", "marketplace", "list", "--json"]) {
        Ok(value) => value,
        Err(reason) => return blocked(reason),
    };
    let marketplace_name = selector.rsplit_once('@').map(|(_, name)| name);
    let Some(marketplace_name) = marketplace_name else {
        return blocked("plugin receipt selector has no marketplace identity".to_owned());
    };
    let Some(entries) = marketplace
        .get("marketplaces")
        .and_then(serde_json::Value::as_array)
    else {
        return blocked("Codex marketplace JSON has no marketplaces array".to_owned());
    };
    let matching_name = entries.iter().find(|entry| {
        entry
            .get("name")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|name| name == marketplace_name)
    });
    let Some(matching_name) = matching_name else {
        return AttachmentInspection {
            state: AttachmentState::Missing,
            reason: "Codex marketplace is absent".to_owned(),
        };
    };
    if matching_name
        .get("root")
        .and_then(serde_json::Value::as_str)
        .is_none_or(|root| Path::new(root) != marketplace_root)
    {
        return AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "Codex marketplace root differs from receipt".to_owned(),
        };
    }
    let plugins = match codex_json(command_home, ["plugin", "list", "--json"]) {
        Ok(value) => value,
        Err(reason) => return blocked(reason),
    };
    inspect_codex_plugin_value(&plugins, selector, package_root)
}

fn inspect_codex_plugin_value(
    value: &serde_json::Value,
    selector: &str,
    package_root: &Path,
) -> AttachmentInspection {
    let Some(installed) = value.get("installed").and_then(serde_json::Value::as_array) else {
        return blocked("Codex plugin JSON has no installed array".to_owned());
    };
    let Some(plugin) = installed.iter().find(|entry| {
        ["pluginId", "id", "plugin_id", "selector"]
            .iter()
            .filter_map(|field| entry.get(*field).and_then(serde_json::Value::as_str))
            .any(|candidate| candidate == selector)
    }) else {
        return AttachmentInspection {
            state: AttachmentState::Missing,
            reason: "Codex plugin is not installed".to_owned(),
        };
    };
    let Some(enabled) = plugin.get("enabled").and_then(serde_json::Value::as_bool) else {
        return blocked("Codex plugin JSON has no enabled state".to_owned());
    };
    let Some(installed_state) = plugin.get("installed").and_then(serde_json::Value::as_bool) else {
        return blocked("Codex plugin JSON has no installed state".to_owned());
    };
    let Some((_, marketplace_name)) = selector.rsplit_once('@') else {
        return blocked("plugin receipt selector has no marketplace identity".to_owned());
    };
    let Some(actual_marketplace) = plugin
        .get("marketplaceName")
        .or_else(|| plugin.get("marketplace_name"))
        .and_then(serde_json::Value::as_str)
    else {
        return blocked("Codex plugin JSON has no marketplace identity".to_owned());
    };
    let source = plugin
        .get("path")
        .or_else(|| plugin.pointer("/source/path"))
        .and_then(serde_json::Value::as_str);
    let Some(source) = source else {
        return blocked("Codex plugin JSON has no package source path".to_owned());
    };
    if !enabled
        || !installed_state
        || actual_marketplace != marketplace_name
        || Path::new(source) != package_root
    {
        return AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "Codex plugin enabled state or source differs from receipt".to_owned(),
        };
    }
    AttachmentInspection {
        state: AttachmentState::Matched,
        reason: "Codex native plugin matches receipt".to_owned(),
    }
}

fn codex_json<const N: usize>(
    command_home: &Path,
    args: [&str; N],
) -> std::result::Result<serde_json::Value, String> {
    let output = Command::new("codex")
        .env("HOME", command_home)
        .args(args)
        .output()
        .map_err(|error| format!("failed to run `codex`: {error}"))?;
    if !output.status.success() {
        return Err(format!("`codex` inspection exited with {}", output.status));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("Codex JSON is invalid: {error}"))
}

fn remove_plugin(command_home: &Path, selector: &str) -> Result<()> {
    match Command::new("codex")
        .env("HOME", command_home)
        .args(["plugin", "remove", selector])
        .status()
    {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(UzeError::ExposureUnavailable(format!(
            "`codex plugin remove` exited with {status} for `{selector}`"
        ))),
        Err(error) => Err(UzeError::ExposureUnavailable(format!(
            "failed to run `codex plugin remove` for `{selector}`: {error}"
        ))),
    }
}

fn blocked(reason: String) -> AttachmentInspection {
    AttachmentInspection {
        state: AttachmentState::Blocked,
        reason,
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
    let status = Command::new("codex")
        .env("HOME", command_home)
        .args(["mcp", "remove", entry_name])
        .status();
    match status {
        Ok(status) if status.success() => Ok(()),
        // Already absent is not an error — removal is idempotent.
        Ok(_) if !mcp_entry_exists(command_home, entry_name) => Ok(()),
        Ok(status) => Err(UzeError::ExposureUnavailable(format!(
            "`codex mcp remove` exited with {status} for entry `{entry_name}`"
        ))),
        Err(error) => Err(UzeError::ExposureUnavailable(format!(
            "failed to run `codex mcp remove` for entry `{entry_name}`: {error}"
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

fn detect_binary(program: &str) -> HarnessDetection {
    let Ok(output) = Command::new(program).arg("--version").output() else {
        return HarnessDetection::default();
    };
    // `codex --version` prints "codex-cli 0.148.0" — the version trails.
    let stdout = String::from_utf8_lossy(&output.stdout);
    let version = stdout.split_whitespace().last().map(str::to_owned);
    HarnessDetection {
        present: true,
        version,
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

/// Packages carrying the Codex-native envelope. Deciding which packages
/// belong in the catalogue is Codex policy, so it lives here rather than in
/// the Store.
fn publishable(packages: &[StoredPackage]) -> Vec<&StoredPackage> {
    packages
        .iter()
        .filter(|package| package.root.join(".codex-plugin/plugin.json").is_file())
        .collect()
}

/// The catalogue document, derived purely from the installed package set.
/// Nothing here exists only in the catalogue: delete the file and this
/// rebuilds it byte for byte from the Store.
fn catalogue_document(packages: &[StoredPackage]) -> serde_json::Value {
    let plugins: Vec<serde_json::Value> = publishable(packages)
        .into_iter()
        .map(|package| {
            serde_json::json!({
                "name": package.id.as_str(),
                // Relative to the catalogue root by necessity — see
                // `CodexIntegration::catalogue_root`.
                "source": { "source": "local", "path": format!("./packages/{}", package.id.as_str()) },
                "policy": { "installation": "AVAILABLE", "authentication": "ON_INSTALL" },
                "category": "Developer tools"
            })
        })
        .collect();
    serde_json::json!({
        "name": MARKETPLACE_NAME,
        "interface": { "displayName": "UZE Local" },
        "plugins": plugins,
    })
}

fn write_catalogue(path: &Path, packages: &[StoredPackage]) -> Result<()> {
    let parent = path.parent().expect("catalogue path has a parent");
    fs::create_dir_all(parent).map_err(|source| UzeError::Write {
        path: parent.to_path_buf(),
        source,
    })?;
    uze_core::persistence::write_atomic(
        path,
        &serde_json::to_vec_pretty(&catalogue_document(packages))
            .expect("catalogue is serializable"),
    )
}

/// Reads one integration-defined detail out of an opaque receipt payload.
/// Only this integration interprets these keys.
fn detail_path(
    detail: &std::collections::BTreeMap<String, serde_json::Value>,
    key: &str,
) -> Option<PathBuf> {
    detail
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    #[test]
    fn structured_mcp_inspection_distinguishes_match_drift_and_conflict() {
        let expected_command = PathBuf::from("/bin/example");
        let expected_args = vec!["--serve".to_owned()];
        let exact = serde_json::json!({
            "name": "uze-example",
            "command": "/bin/example",
            "args": ["--serve"],
            "unrelated": {"future": true}
        });
        assert_eq!(
            inspect_codex_mcp_value(
                &exact,
                "uze-example",
                "stdio",
                &expected_command,
                &expected_args,
                None,
                &[],
                None
            )
            .state,
            AttachmentState::Matched
        );
        let official_shape = serde_json::json!({
            "name": "uze-example", "enabled": true,
            "transport": {"type":"stdio", "command":"/bin/example", "args":["--serve"], "env":null}
        });
        assert_eq!(
            inspect_codex_mcp_value(
                &official_shape,
                "uze-example",
                "stdio",
                &expected_command,
                &expected_args,
                None,
                &[],
                None,
            )
            .state,
            AttachmentState::Matched
        );
        let changed =
            serde_json::json!({"name":"uze-example", "command":"/bin/changed", "args":["--serve"]});
        assert_eq!(
            inspect_codex_mcp_value(
                &changed,
                "uze-example",
                "stdio",
                &expected_command,
                &expected_args,
                None,
                &[],
                None
            )
            .state,
            AttachmentState::Drifted
        );
        let foreign =
            serde_json::json!({"name":"foreign", "command":"/bin/example", "args":["--serve"]});
        assert_eq!(
            inspect_codex_mcp_value(
                &foreign,
                "uze-example",
                "stdio",
                &expected_command,
                &expected_args,
                None,
                &[],
                None
            )
            .state,
            AttachmentState::Conflict
        );
        assert_eq!(
            inspect_codex_mcp_value(
                &serde_json::json!({}),
                "uze-example",
                "stdio",
                &expected_command,
                &expected_args,
                None,
                &[],
                None,
            )
            .state,
            AttachmentState::Blocked
        );
    }

    #[test]
    fn native_plugin_receipt_requires_installed_identity_and_expected_source() {
        let package = Path::new("/uze/store/example");
        let exact = serde_json::json!({
            "installed": [{"pluginId":"example@uze-local", "enabled":true, "installed":true, "marketplaceName":"uze-local", "source":{"path":"/uze/store/example"}}]
        });
        assert_eq!(
            inspect_codex_plugin_value(&exact, "example@uze-local", package).state,
            AttachmentState::Matched
        );
        let changed = serde_json::json!({
            "installed": [{"id":"example@uze-local", "enabled":false, "installed":true, "marketplaceName":"uze-local", "path":"/uze/store/example"}]
        });
        assert_eq!(
            inspect_codex_plugin_value(&changed, "example@uze-local", package).state,
            AttachmentState::Drifted
        );
        let absent = serde_json::json!({"installed": []});
        assert_eq!(
            inspect_codex_plugin_value(&absent, "example@uze-local", package).state,
            AttachmentState::Missing
        );
    }
}
