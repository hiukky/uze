use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use uze_core::{
    Result, UzeError,
    capability::CapabilityKind,
    exposure::{ExposureMechanism, ExposurePlan},
    home::UzeHome,
    integration::{
        AttachmentInspection, AttachmentReceipt, AttachmentState, HarnessDetection,
        IntegrationPort, ManagedArtifact, default_exposure_name_candidates,
        detach_standard_receipt, inspect_standard_receipt,
        short_then_qualified_exposure_name_candidates,
    },
    project::Resource,
    provisioning::{ProcessRunner, ProcessSpec, ProvisionAction, ProvisioningResult},
    router::{CompatibilityRoute, HarnessCapabilities, VerificationStatus},
    state,
};

/// OpenCode does not consume the external plugin envelope. It does natively
/// discover user Agent Skills at `~/.agents/skills` and natively reads local
/// MCP definitions from its global config, so this integration decomposes
/// only those portable capabilities.
pub struct OpenCodeIntegration {
    skills_dir: PathBuf,
    config_path: PathBuf,
    uze_home: UzeHome,
}

impl OpenCodeIntegration {
    pub fn new(agents_home: PathBuf, config_path: PathBuf, uze_home: UzeHome) -> Self {
        Self {
            skills_dir: agents_home.join("skills"),
            config_path,
            uze_home,
        }
    }
    #[allow(dead_code)]
    pub fn from_env(uze_home: UzeHome) -> Result<Self> {
        let home = PathBuf::from(std::env::var_os("HOME").ok_or(UzeError::MissingHomeDirectory)?);
        let config_root = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        Ok(Self::new(
            home.join(".agents"),
            config_root.join("opencode/opencode.json"),
            uze_home,
        ))
    }
}

impl IntegrationPort for OpenCodeIntegration {
    fn id(&self) -> &'static str {
        "opencode"
    }
    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            direct_standard: [CapabilityKind::AgentSkill].into_iter().collect(),
            adaptable: [CapabilityKind::Mcp].into_iter().collect(),
            verification: VerificationStatus::Unverified,
            evidence: "OpenCode documents global Agent Skills discovery at ~/.agents/skills and global local-MCP configuration under `mcp` in opencode.json. It does not consume the external plugin envelope, so UZE decomposes only these portable components.".to_owned(),
            ..HarnessCapabilities::default()
        }
    }
    fn detect(&self) -> HarnessDetection {
        resolve_opencode_binary()
            .map(|(_, detection)| detection)
            .unwrap_or_default()
    }

    /// OpenCode V2 discovers `~/.agents/skills` via path-derived ID and
    /// exposes Skills as slash commands (`/id`) by default (`slash: true`
    /// unless opted out). The physical directory is now user-visible, like
    /// Claude, so try bare logical first then qualified fallback. MCP stays
    /// on the default fully-qualified policy — Skill and MCP naming are
    /// never mixed just because both are `Resource`s.
    fn exposure_name_candidates(&self, resource: &Resource) -> Vec<String> {
        if resource.capability.kind != CapabilityKind::AgentSkill {
            return default_exposure_name_candidates(resource);
        }
        short_then_qualified_exposure_name_candidates(resource)
    }

    /// Codex and Gemini CLI both discover Skills from this exact same
    /// `~/.agents/skills` directory (see their own overrides of this
    /// method), so a name this integration claims here must be treated as
    /// claimed for them too — otherwise OpenCode's own preference for the
    /// bare name collides with their always-qualified default, leaving two
    /// symlinks for one skill in the one folder OpenCode scans.
    fn shared_agent_skill_root(&self) -> Option<PathBuf> {
        Some(self.skills_dir.clone())
    }

    /// OpenCode's v2 installer names the binary `opencode2`; UZE's canonical
    /// invocation stays `opencode` with no version suffix, so provisioning
    /// installs or upgrades normally and then ensures that alias exists —
    /// success is only reported once `opencode` itself resolves, not merely
    /// `opencode2`, since that's the one guarantee this method's callers
    /// (and the doc comment above the struct) actually rely on.
    fn provision(&self, runner: &dyn ProcessRunner) -> Result<ProvisioningResult> {
        if !cfg!(unix) {
            return Ok(ProvisioningResult::blocked(
                "OpenCode automatic provisioning is currently supported on Unix and WSL only",
            ));
        }
        let method = "official-install-script";
        let resolved = resolve_opencode_binary();
        let before = resolved
            .as_ref()
            .map(|(_, detection)| detection.clone())
            .unwrap_or_default();
        let action = if before.present {
            ProvisionAction::Update
        } else {
            ProvisionAction::Install
        };
        let command = match &resolved {
            Some((which, _)) => ProcessSpec::new(*which, ["upgrade"]).with_inherited_output(),
            None => ProcessSpec::new(
                "sh",
                ["-c", "curl -fsSL https://opencode.ai/v2/install | bash"],
            )
            .with_inherited_output(),
        };
        let outcome = match runner.run(&command) {
            Ok(o) => o,
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
        if let Err(reason) = ensure_opencode_alias() {
            return Ok(ProvisioningResult::failed(action, method, reason));
        }
        let verified = runner.run(&ProcessSpec::new("opencode", ["--version"]));
        if !matches!(verified, Ok(o) if o.success) {
            return Ok(ProvisioningResult::failed(
                action,
                method,
                "installer finished but the `opencode` executable could not be verified",
            ));
        }
        Ok(ProvisioningResult::verified(action, method, self.detect()))
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
                strategy: "native-user-scope-skills-plus-managed-mcp-config".to_owned(),
                installed: true,
            },
        )
    }
    fn exposure_plan(&self, resource: &Resource) -> ExposurePlan {
        if resource.package_root().is_none() {
            return unsupported(resource, "OpenCode attachment needs a UZE-stored package.");
        }
        match resource.capability.kind {
            CapabilityKind::AgentSkill => self.skill_plan(resource),
            CapabilityKind::Mcp => self.mcp_plan(resource),
            _ => unsupported(
                resource,
                "OpenCode portability is implemented only for Agent Skills and MCP in this slice.",
            ),
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
            } => attach_mcp_config(&self.config_path, entry_name, command, args),
            _ => Ok(None),
        }
    }

    fn inspect_receipt(&self, receipt: &AttachmentReceipt) -> AttachmentInspection {
        let ManagedArtifact::VendorConfigEntry {
            entry_name,
            command,
            args,
            transport,
            cwd,
            environment,
            enabled,
        } = &receipt.artifact
        else {
            return inspect_standard_receipt(receipt);
        };
        let bytes = match fs::read(&self.config_path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return AttachmentInspection {
                    state: AttachmentState::Missing,
                    reason: "OpenCode config is missing".to_owned(),
                };
            }
            Err(error) => {
                return AttachmentInspection {
                    state: AttachmentState::Blocked,
                    reason: format!("OpenCode config cannot be read: {error}"),
                };
            }
        };
        let Ok(config) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
            return AttachmentInspection {
                state: AttachmentState::Blocked,
                reason: "OpenCode config is not readable JSON".to_owned(),
            };
        };
        match config.get("mcp").and_then(|m| m.get(entry_name)) {
            Some(current) => inspect_opencode_mcp_value(
                current,
                transport,
                command,
                args,
                cwd.as_deref(),
                environment,
                *enabled,
            ),
            None => AttachmentInspection {
                state: AttachmentState::Missing,
                reason: "OpenCode MCP entry is missing".to_owned(),
            },
        }
    }

    fn detach_receipt(&self, receipt: &AttachmentReceipt) -> Result<AttachmentInspection> {
        let inspection = self.inspect_receipt(receipt);
        if inspection.state != AttachmentState::Matched {
            return Ok(inspection);
        }
        let ManagedArtifact::VendorConfigEntry {
            entry_name,
            command,
            args,
            transport,
            cwd,
            environment,
            enabled,
        } = &receipt.artifact
        else {
            return detach_standard_receipt(receipt);
        };
        let bytes = fs::read(&self.config_path).map_err(|source| UzeError::Read {
            path: self.config_path.clone(),
            source,
        })?;
        let mut config: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|source| UzeError::Json {
                path: self.config_path.clone(),
                source,
            })?;
        let current = config.get("mcp").and_then(|mcp| mcp.get(entry_name));
        let fresh = match current {
            Some(current) => inspect_opencode_mcp_value(
                current,
                transport,
                command,
                args,
                cwd.as_deref(),
                environment,
                *enabled,
            ),
            None => AttachmentInspection {
                state: AttachmentState::Missing,
                reason: "OpenCode MCP entry disappeared before detach".to_owned(),
            },
        };
        if fresh.state != AttachmentState::Matched {
            return Ok(fresh);
        }
        config
            .get_mut("mcp")
            .and_then(serde_json::Value::as_object_mut)
            .expect("matched entry has mcp object")
            .remove(entry_name);
        fs::write(
            &self.config_path,
            serde_json::to_vec_pretty(&config).expect("config serializable"),
        )
        .map_err(|source| UzeError::Write {
            path: self.config_path.clone(),
            source,
        })?;
        Ok(AttachmentInspection {
            state: AttachmentState::Missing,
            reason: "OpenCode managed MCP entry detached".to_owned(),
        })
    }
}

/// Resolves whichever name currently answers to `--version`: `opencode`
/// (the canonical alias, once created) first, then `opencode2` (the raw v2
/// binary name) as a fallback for a fresh v2 install that has no alias yet.
fn resolve_opencode_binary() -> Option<(&'static str, HarnessDetection)> {
    let primary = detect_binary("opencode");
    if primary.present {
        return Some(("opencode", primary));
    }
    let fallback = detect_binary("opencode2");
    fallback.present.then_some(("opencode2", fallback))
}

fn inspect_opencode_mcp_value(
    current: &serde_json::Value,
    transport: &str,
    command: &Path,
    args: &[String],
    cwd: Option<&Path>,
    environment: &[uze_core::exposure::McpEnvironmentReference],
    enabled: Option<bool>,
) -> AttachmentInspection {
    let expected_command = std::iter::once(command.to_string_lossy().into_owned())
        .chain(args.iter().cloned())
        .collect::<Vec<_>>();
    let matches = transport == "stdio"
        && current.get("type").and_then(serde_json::Value::as_str) == Some("local")
        && enabled.is_none_or(|expected| {
            current.get("enabled").and_then(serde_json::Value::as_bool) == Some(expected)
        })
        && current
            .get("command")
            .and_then(serde_json::Value::as_array)
            .and_then(|values| {
                values
                    .iter()
                    .map(serde_json::Value::as_str)
                    .collect::<Option<Vec<_>>>()
            })
            .is_some_and(|actual| actual == expected_command)
        && cwd.is_none_or(|expected| {
            current.get("cwd").and_then(serde_json::Value::as_str)
                == Some(expected.to_string_lossy().as_ref())
        })
        && (environment.is_empty()
            || current
                .get("env")
                .and_then(serde_json::Value::as_object)
                .is_some_and(|env| {
                    environment
                        .iter()
                        .all(|reference| env.contains_key(&reference.name))
                }));
    if matches {
        AttachmentInspection {
            state: AttachmentState::Matched,
            reason: "OpenCode MCP entry matches receipt".to_owned(),
        }
    } else {
        AttachmentInspection {
            state: AttachmentState::Drifted,
            reason: "OpenCode MCP entry differs from receipt".to_owned(),
        }
    }
}

impl OpenCodeIntegration {
    fn skill_plan(&self, resource: &Resource) -> ExposurePlan {
        let Some(entry_name) = resource
            .resolved_exposure_name
            .clone()
            .or_else(|| self.exposure_name_candidates(resource).into_iter().next())
        else {
            return unsupported(resource, "Resource has no derivable attachment entry name.");
        };
        let source = resource
            .capability
            .path
            .parent()
            .expect("SKILL.md has a parent")
            .to_path_buf();
        if state::is_installed(&self.uze_home, self.id()) {
            return ExposurePlan { representation: resource.capability.representation, route: CompatibilityRoute::Native, verification: VerificationStatus::Unverified, mechanism: ExposureMechanism::ManagedUserScopeReference { discovery_root: self.skills_dir.clone(), entry_name, source }, evidence: "OpenCode natively discovers the UZE-managed symlink in ~/.agents/skills. The symlink is delivery only; SKILL.md remains the preserved standard payload in the UZE store.".to_owned() };
        }
        ExposurePlan { representation: resource.capability.representation, route: CompatibilityRoute::Adaptable, verification: VerificationStatus::Unverified, mechanism: ExposureMechanism::FilesystemProjection { source, target_relative: PathBuf::from(".agents/skills").join(resource.capability.path.parent().and_then(Path::file_name).expect("skill dir name")) }, evidence: "OpenCode setup has not completed; the existing project-scope projection remains a conformance fallback.".to_owned() }
    }
    fn mcp_plan(&self, resource: &Resource) -> ExposurePlan {
        if !state::is_installed(&self.uze_home, self.id()) {
            return unsupported(
                resource,
                "OpenCode has not completed `uze setup`; its managed global MCP config is not yet enabled.",
            );
        }
        let Some(entry_name) = resource
            .resolved_exposure_name
            .clone()
            .or_else(|| self.exposure_name_candidates(resource).into_iter().next())
        else {
            return unsupported(resource, "Resource has no derivable attachment entry name.");
        };
        let Some((command, args)) = parse_mcp(&resource.capability.payload) else {
            return unsupported(
                resource,
                "mcp.json server entry is missing a usable `command` field.",
            );
        };
        ExposurePlan { representation: resource.capability.representation, route: CompatibilityRoute::Adaptable, verification: VerificationStatus::Unverified, mechanism: ExposureMechanism::ManagedVendorConfig { entry_name, transport: "stdio".to_owned(), command, args, cwd: None, environment: Vec::new(), enabled: Some(true) }, evidence: "UZE adapts the standard stdio command/args into OpenCode's documented global `mcp.<name>.command` array in opencode.json; the OpenCode MCP runtime remains native.".to_owned() }
    }
}

fn attach_mcp_config(
    config_path: &Path,
    entry_name: &str,
    command: &Path,
    args: &[String],
) -> Result<Option<PathBuf>> {
    let mut config = if config_path.exists() {
        serde_json::from_slice(&fs::read(config_path).map_err(|source| UzeError::Read {
            path: config_path.to_path_buf(),
            source,
        })?)
        .map_err(|source| UzeError::Json {
            path: config_path.to_path_buf(),
            source,
        })?
    } else {
        serde_json::json!({ "$schema": "https://opencode.ai/config.json" })
    };
    let root = config.as_object_mut().ok_or_else(|| {
        UzeError::ExposureUnavailable("OpenCode config root must be a JSON object".to_owned())
    })?;
    let mcp = root
        .entry("mcp")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| {
            UzeError::ExposureUnavailable("OpenCode config `mcp` must be an object".to_owned())
        })?;
    let command_values: Vec<serde_json::Value> =
        std::iter::once(command.to_string_lossy().into_owned())
            .chain(args.iter().cloned())
            .map(serde_json::Value::String)
            .collect();
    let desired =
        serde_json::json!({ "type": "local", "command": command_values, "enabled": true });
    match mcp.get(entry_name) {
        Some(current) if current == &desired => return Ok(Some(config_path.to_path_buf())),
        Some(_) => {
            return Err(UzeError::ExposureUnavailable(format!(
                "OpenCode MCP entry `{entry_name}` already exists and is not owned by this UZE plan"
            )));
        }
        None => {
            mcp.insert(entry_name.to_owned(), desired);
        }
    }
    let parent = config_path.parent().expect("config path has a parent");
    fs::create_dir_all(parent).map_err(|source| UzeError::Write {
        path: parent.to_path_buf(),
        source,
    })?;
    fs::write(
        config_path,
        serde_json::to_vec_pretty(&config).expect("config serializable"),
    )
    .map_err(|source| UzeError::Write {
        path: config_path.to_path_buf(),
        source,
    })?;
    Ok(Some(config_path.to_path_buf()))
}
fn parse_mcp(payload: &[u8]) -> Option<(PathBuf, Vec<String>)> {
    let value: serde_json::Value = serde_json::from_slice(payload).ok()?;
    Some((
        PathBuf::from(value.get("command")?.as_str()?),
        value
            .get("args")
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
    ))
}
/// Resolves `name` to an absolute path via the shell's own `PATH` lookup —
/// the same mechanism the installer that put it there used.
fn resolve_executable_path(name: &str) -> Option<PathBuf> {
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {name}"))
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?;
    let path = path.trim();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

/// Creates or repairs a symlink at `alias_path` pointing to `target`.
/// Idempotent: a symlink already pointing at `target` is left untouched, a
/// stale symlink is replaced, and a real (non-symlink) file already
/// occupying `alias_path` is never touched — it isn't UZE's to overwrite.
#[cfg(unix)]
fn ensure_symlink_alias(alias_path: &Path, target: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(alias_path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            if fs::read_link(alias_path)? == target {
                return Ok(());
            }
            fs::remove_file(alias_path)?;
            std::os::unix::fs::symlink(target, alias_path)
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Some(parent) = alias_path.parent() {
                fs::create_dir_all(parent)?;
            }
            std::os::unix::fs::symlink(target, alias_path)
        }
        Err(error) => Err(error),
    }
}

#[cfg(not(unix))]
fn ensure_symlink_alias(_alias_path: &Path, _target: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "opencode alias creation is only supported on Unix",
    ))
}

/// Ensures `opencode` resolves to OpenCode v2's `opencode2` binary, since
/// UZE's canonical invocation never carries the version suffix. Tries both a
/// alias next to `opencode2` on `PATH` and one in `~/.local/bin`, and
/// succeeds if either lands — `provision`'s own final `opencode --version`
/// check is the actual proof this worked; this function's `Err` only short-
/// circuits that doomed check when neither location was writable.
fn ensure_opencode_alias() -> std::result::Result<(), String> {
    if detect_binary("opencode").present {
        return Ok(());
    }
    let target = resolve_executable_path("opencode2")
        .ok_or_else(|| "opencode2 executable not found on PATH after install".to_owned())?;
    let mut aliased = false;
    if let Some(dir) = target.parent()
        && ensure_symlink_alias(&dir.join("opencode"), &target).is_ok()
    {
        aliased = true;
    }
    if let Some(home) = std::env::var_os("HOME")
        && ensure_symlink_alias(&PathBuf::from(home).join(".local/bin/opencode"), &target).is_ok()
    {
        aliased = true;
    }
    if aliased {
        Ok(())
    } else {
        Err(format!(
            "could not create an `opencode` alias for {}",
            target.display()
        ))
    }
}

fn detect_binary(program: &str) -> HarnessDetection {
    let Ok(output) = Command::new(program).arg("--version").output() else {
        return HarnessDetection::default();
    };
    // `opencode --version` prints "opencode2 v0.0.0-beta-17823" — the
    // version trails, and comes with a `v` prefix intact.
    HarnessDetection {
        present: true,
        version: String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .last()
            .map(str::to_owned),
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

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    fn temp(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "uze-opencode-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    fn receipt() -> AttachmentReceipt {
        AttachmentReceipt {
            package_id: "plugin".to_owned(),
            resource_identity: Some("mcp:example".to_owned()),
            integration: "opencode".to_owned(),
            strategy: "managed-vendor-config".to_owned(),
            artifact: ManagedArtifact::VendorConfigEntry {
                entry_name: "uze-example".to_owned(),
                transport: "stdio".to_owned(),
                command: PathBuf::from("/bin/example"),
                args: vec!["--serve".to_owned()],
                cwd: None,
                environment: Vec::new(),
                enabled: Some(true),
            },
        }
    }

    fn integration(root: &Path) -> OpenCodeIntegration {
        OpenCodeIntegration::new(
            root.join("agents"),
            root.join("config/opencode.json"),
            UzeHome::at(root.join("uze")),
        )
    }

    #[test]
    fn mcp_inspection_tolerates_unrelated_fields_and_detaches_only_owned_entry() {
        let root = temp("mcp");
        fs::create_dir_all(root.join("config")).unwrap();
        let integration = integration(&root);
        let receipt = receipt();
        fs::write(
            &integration.config_path,
            r#"{"mcp":{"uze-example":{"type":"local","command":["/bin/example","--serve"],"enabled":true,"future":true},"foreign":{"type":"local","command":["foreign"],"enabled":true}},"unrelated":true}"#,
        )
        .unwrap();

        assert_eq!(
            integration.inspect_receipt(&receipt).state,
            AttachmentState::Matched
        );
        assert_eq!(
            integration.detach_receipt(&receipt).unwrap().state,
            AttachmentState::Missing
        );
        let after: serde_json::Value =
            serde_json::from_slice(&fs::read(&integration.config_path).unwrap()).unwrap();
        assert!(after.pointer("/mcp/uze-example").is_none());
        assert!(after.pointer("/mcp/foreign").is_some());
        assert_eq!(after["unrelated"], true);
        assert_eq!(
            integration.detach_receipt(&receipt).unwrap().state,
            AttachmentState::Missing
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mcp_drift_and_invalid_config_are_preserved() {
        let root = temp("drift");
        fs::create_dir_all(root.join("config")).unwrap();
        let integration = integration(&root);
        let receipt = receipt();
        fs::write(
            &integration.config_path,
            r#"{"mcp":{"uze-example":{"type":"local","command":["/bin/changed","--serve"],"enabled":true}}}"#,
        )
        .unwrap();
        assert_eq!(
            integration.inspect_receipt(&receipt).state,
            AttachmentState::Drifted
        );
        assert_eq!(
            integration.detach_receipt(&receipt).unwrap().state,
            AttachmentState::Drifted
        );
        assert!(
            serde_json::from_slice::<serde_json::Value>(
                &fs::read(&integration.config_path).unwrap()
            )
            .is_ok()
        );

        fs::write(&integration.config_path, "not json").unwrap();
        assert_eq!(
            integration.inspect_receipt(&receipt).state,
            AttachmentState::Blocked
        );
        assert_eq!(
            integration.detach_receipt(&receipt).unwrap().state,
            AttachmentState::Blocked
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn managed_cwd_and_environment_reference_drift_are_detected() {
        let expected = PathBuf::from("/bin/example");
        let args = vec!["--serve".to_owned()];
        let current = serde_json::json!({
            "type": "local",
            "command": ["/bin/example", "--serve"],
            "enabled": true,
            "cwd": "/other",
            "env": {"OTHER": "opaque"}
        });
        assert_eq!(
            inspect_opencode_mcp_value(
                &current,
                "stdio",
                &expected,
                &args,
                Some(Path::new("/expected")),
                &[uze_core::exposure::McpEnvironmentReference {
                    name: "TOKEN".to_owned()
                }],
                Some(true),
            )
            .state,
            AttachmentState::Drifted
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_alias_is_created_repaired_and_leaves_foreign_files_alone() {
        use std::os::unix::fs::symlink;

        let root = temp("alias");
        fs::create_dir_all(&root).unwrap();
        let target = root.join("opencode2");
        fs::write(&target, "binary").unwrap();

        // Absent -> created.
        let alias = root.join("bin/opencode");
        ensure_symlink_alias(&alias, &target).unwrap();
        assert_eq!(fs::read_link(&alias).unwrap(), target);

        // Already correct -> left untouched (idempotent).
        ensure_symlink_alias(&alias, &target).unwrap();
        assert_eq!(fs::read_link(&alias).unwrap(), target);

        // Stale symlink (e.g. still pointing at a removed v1 binary) -> repaired.
        let stale_target = root.join("opencode-v1");
        fs::write(&stale_target, "old").unwrap();
        let stale_alias = root.join("bin2/opencode");
        fs::create_dir_all(stale_alias.parent().unwrap()).unwrap();
        symlink(&stale_target, &stale_alias).unwrap();
        ensure_symlink_alias(&stale_alias, &target).unwrap();
        assert_eq!(fs::read_link(&stale_alias).unwrap(), target);

        // A real (non-symlink) file already at the alias path is not UZE's
        // to overwrite.
        let foreign = root.join("bin3/opencode");
        fs::create_dir_all(foreign.parent().unwrap()).unwrap();
        fs::write(&foreign, "not managed by uze").unwrap();
        ensure_symlink_alias(&foreign, &target).unwrap();
        assert_eq!(fs::read_to_string(&foreign).unwrap(), "not managed by uze");

        fs::remove_dir_all(root).unwrap();
    }

    struct RecordingRunner {
        commands: std::sync::Mutex<Vec<ProcessSpec>>,
    }

    impl ProcessRunner for RecordingRunner {
        fn run(&self, spec: &ProcessSpec) -> Result<uze_core::provisioning::ProcessResult> {
            self.commands.lock().unwrap().push(spec.clone());
            Ok(uze_core::provisioning::ProcessResult {
                success: true,
                timed_out: false,
            })
        }
    }

    /// `resolve_opencode_binary` genuinely shells out to whatever `opencode`/
    /// `opencode2` resolve to on the machine running the test, so — like
    /// `detect()` itself — there's no fake executable name to substitute
    /// (unlike the sibling integrations' `provision_cli`, which is
    /// parameterized). This asserts the dispatch logic is *consistent* with
    /// whatever `detect()` observes, rather than assuming one fixed starting
    /// state.
    #[test]
    fn provision_dispatches_install_or_update_consistently_with_detected_state() {
        if !cfg!(unix) {
            return;
        }
        // `provision`'s install/upgrade command is mocked below, so nothing
        // is genuinely installed — `ensure_opencode_alias` still resolves
        // `opencode`/`opencode2` for real. On a machine with neither on
        // PATH (a bare CI runner, unlike a dev box that has one installed)
        // that real resolution fails and `Verified` is unreachable no
        // matter what the mock records; skip rather than assert a status
        // this test has no way to produce here.
        if !detect_binary("opencode").present && resolve_executable_path("opencode2").is_none() {
            return;
        }
        let root = temp("provision");
        let integration = integration(&root);
        let runner = RecordingRunner {
            commands: std::sync::Mutex::new(Vec::new()),
        };
        let before = integration.detect();
        let result = integration.provision(&runner).unwrap();
        assert_eq!(
            result.action,
            if before.present {
                ProvisionAction::Update
            } else {
                ProvisionAction::Install
            }
        );
        assert_eq!(
            result.status,
            uze_core::provisioning::ProvisionStatus::Verified
        );
        let commands = runner.commands.lock().unwrap();
        if before.present {
            assert_eq!(commands[0].arguments, ["upgrade"]);
        } else {
            assert_eq!(commands[0].program, "sh");
            assert!(commands[0].arguments[1].contains("opencode.ai/v2/install"));
        }
        assert_eq!(commands[1].program, "opencode");
        assert_eq!(commands[1].arguments, ["--version"]);
    }
}
