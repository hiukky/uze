//! OpenCode does not consume the external plugin envelope. It does natively
//! discover user Agent Skills at `~/.agents/skills` and natively reads local
//! MCP definitions from its global config, so this integration decomposes
//! only those portable capabilities.
//!
//! Split by concern: [`mcp`] (the global `mcp.<name>` config entries),
//! [`skills`] (the managed skills-dir reference), and [`provision`]
//! (install/update, plus detecting whichever of `opencode`/`opencode2` is
//! present). Dispatching `opencode` to a binary actually named `opencode2`
//! is handled by the generic PATH shim (`runtime_executable_aliases` below),
//! not by anything in this module. This file is the composition root: the
//! `OpenCodeIntegration` struct and its `IntegrationPort` impl, delegating
//! to each submodule.

use std::{fs, path::PathBuf};

use uze_core::{
    Result, UzeError,
    capability::CapabilityKind,
    exposure::{ExposureMechanism, ExposurePlan},
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
};

mod commands;
mod mcp;
mod provision;
mod skills;

use mcp::attach_mcp_config;
use provision::{provision_opencode, resolve_opencode_binary};

/// OpenCode does not consume the external plugin envelope. It does natively
/// discover user Agent Skills at `~/.agents/skills`, natively reads local
/// MCP definitions from its global config, and natively reads `.md` custom
/// commands from its user-global commands directory, so this integration
/// decomposes only those portable capabilities.
pub struct OpenCodeIntegration {
    skills_dir: PathBuf,
    commands_dir: PathBuf,
    config_path: PathBuf,
    uze_home: UzeHome,
}

impl OpenCodeIntegration {
    pub fn new(agents_home: PathBuf, config_path: PathBuf, uze_home: UzeHome) -> Self {
        let commands_dir = config_path
            .parent()
            .map(|parent| parent.join("commands"))
            .unwrap_or_else(|| PathBuf::from("commands"));
        Self {
            skills_dir: agents_home.join("skills"),
            commands_dir,
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
            direct_standard: [CapabilityKind::AgentSkill, CapabilityKind::Command]
                .into_iter()
                .collect(),
            adaptable: [CapabilityKind::Mcp].into_iter().collect(),
            verification: VerificationStatus::Unverified,
            evidence: "OpenCode documents global Agent Skills discovery at ~/.agents/skills, global local-MCP configuration under `mcp` in opencode.json, and user-global custom commands as .md files under ~/.config/opencode/commands/ (also configurable via the `commands` key; project `.opencode/commands/` is project-scoped, so machine-level attachment uses the global scope). It does not consume the external plugin envelope, so UZE decomposes only these portable components.".to_owned(),
            ..HarnessCapabilities::default()
        }
    }
    fn detect(&self) -> HarnessDetection {
        resolve_opencode_binary(&self.uze_home.shims_dir())
            .map(|(_, detection)| detection)
            .unwrap_or_default()
    }

    /// Matches `resolve_opencode_binary`'s own preference order: the
    /// canonical `opencode` alias first, the raw v2 binary name
    /// `opencode2` as fallback for a fresh install with no alias yet.
    fn detection_program_candidates(&self) -> Vec<&'static str> {
        vec!["opencode", "opencode2"]
    }

    /// OpenCode's v2 installer names the binary `opencode2`, not `opencode`.
    /// Rather than a UZE-managed symlink alias placed next to the real
    /// binary (outside `$UZE_HOME`), the generic PATH shim resolves straight
    /// to whichever name is actually present — see `runtime_executable_aliases`.
    fn supports_runtime_integration(&self) -> bool {
        true
    }

    fn runtime_executable_aliases(&self) -> &'static [&'static str] {
        &["opencode2"]
    }

    /// OpenCode derives a skill's ID from its path and a command's name
    /// from its path relative to the commands directory — with no name
    /// regex enforced in V2 — so UZE exposes the stable namespaced
    /// invocation label verbatim (`flow:review`) as the single, deterministic
    /// candidate for both Skills (physical directory name) and Commands
    /// (physical file name, `.md` extension added). No bare alias, no
    /// collision-dependent qualification (ADR-026). MCP stays on the default
    /// fully-qualified policy — capability naming policies are never mixed
    /// just because all are `Resource`s.
    fn exposure_name_candidates(&self, resource: &Resource) -> Vec<String> {
        match resource.capability.kind {
            CapabilityKind::AgentSkill => qualified_exposure_name_candidates(resource),
            CapabilityKind::Command => {
                commands::opencode_command_exposure_name_candidates(resource)
            }
            _ => default_exposure_name_candidates(resource),
        }
    }

    /// Codex also discovers Skills from this exact same
    /// `~/.agents/skills` directory (see its own override of this
    /// method), so a name this integration claims here must be treated as
    /// claimed for it too — every member derives the same single
    /// namespaced label, so the group always converges on one entry.
    fn shared_agent_skill_root(&self) -> Option<PathBuf> {
        Some(self.skills_dir.clone())
    }

    /// OpenCode's v2 installer names the binary `opencode2`; provisioning
    /// installs or upgrades normally and reports success once either
    /// `opencode` or `opencode2` resolves — reconciling the name gap with
    /// UZE's canonical `opencode` invocation is the PATH shim's job
    /// (`runtime_executable_aliases`), not provisioning's.
    fn provision(&self, runner: &dyn ProcessRunner) -> Result<ProvisioningResult> {
        provision_opencode(runner, || self.detect(), &self.uze_home.shims_dir())
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
            CapabilityKind::Command => self.command_plan(resource),
            CapabilityKind::Mcp => self.mcp_plan(resource),
            _ => unsupported(
                resource,
                "OpenCode portability is implemented only for Agent Skills, Commands, and MCP in this slice.",
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
            Some(current) => mcp::inspect_opencode_mcp_value(
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
            Some(current) => mcp::inspect_opencode_mcp_value(
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
    use std::path::Path;

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
}
