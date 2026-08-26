//! OpenCode V2 does not consume the external plugin envelope. It does natively
//! discover user Agent Skills at `~/.agents/skills` and natively reads local
//! MCP definitions from its global config, so this integration decomposes
//! only those portable capabilities — including the canonical invocation
//! policy, which OpenCode V2 expresses natively in SKILL.md frontmatter
//! (`metadata.opencode/autoinvoke`, `slash`), so the vendor Command
//! primitive is never needed (ADR-030 §9).
//!
//! Split by concern: [`mcp`] (the global `mcp.<name>` config entries),
//! [`skills`] (the managed skills-dir reference), and [`provision`]
//! (install/update, plus detecting whichever of `opencode`/`opencode2` is
//! present). Dispatching `opencode` to a binary actually named `opencode2`
//! is handled by the generic PATH shim (`runtime_executable_aliases` below),
//! not by anything in this module. This file is the composition root: the
//! `OpenCodeIntegration` struct and its `IntegrationPort` impl, delegating
//! to each submodule.

use std::{
    fs,
    path::{Path, PathBuf},
};

use uze_core::{
    Result, UzeError,
    capability::CapabilityKind,
    exposure::{ExposureMechanism, ExposurePlan},
    home::UzeHome,
    integration::{
        AttachmentInspection, AttachmentReceipt, AttachmentState, ContextDelivery,
        HarnessDetection, IntegrationPort, ManagedArtifact, default_exposure_name_candidates,
        detach_standard_receipt, inspect_standard_receipt, qualified_exposure_name_candidates,
    },
    project::Resource,
    provisioning::{ProcessRunner, ProvisioningResult},
    router::{CompatibilityRoute, HarnessCapabilities, VerificationStatus},
    state,
};

mod mcp;
mod provision;
mod skills;

use mcp::{
    attach_mcp_config, attach_mcp_entry, provisioning_executable_for_attach, resolve_home_and_xdg,
};
use provision::{provision_opencode, resolve_opencode_binary};

/// OpenCode does not consume the external plugin envelope. It does natively
/// discover user Agent Skills at `~/.agents/skills` and natively reads local
/// MCP definitions from its global config, so this integration decomposes
/// only those portable capabilities.
pub struct OpenCodeIntegration {
    skills_dir: PathBuf,
    agents_dir: PathBuf,
    config_path: PathBuf,
    uze_home: UzeHome,
}

impl OpenCodeIntegration {
    pub fn new(agents_home: PathBuf, config_path: PathBuf, uze_home: UzeHome) -> Self {
        Self {
            skills_dir: agents_home.join("skills"),
            agents_dir: config_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("agents"),
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

    fn invocation_prefix(&self) -> &'static str {
        "/"
    }

    /// Reads the shared `AGENTS.md` natively (preferred over `CLAUDE.md`
    /// per its own docs); UZE maintains no artifact for it.
    fn context_delivery(&self) -> ContextDelivery {
        ContextDelivery::Native { files: &[] }
    }
    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            direct_standard: [CapabilityKind::AgentSkill, CapabilityKind::Agent].into_iter().collect(),
            native: [CapabilityKind::Mcp].into_iter().collect(),
            verification: VerificationStatus::Unverified,
            evidence: "OpenCode V2 documents global Agent Skills at ~/.agents/skills and local MCP via `opencode mcp add <name> -- <command>` into global `mcp.servers.<name>.command` in opencode.json (verified `opencode mcp add --help` requires ` -- ` separator; no `remove` verb so detach stays file rewrite). Skills preserve invocation policy natively in SKILL.md frontmatter (metadata.opencode/autoinvoke/slash — ADR-030 §9) without Command primitive."
                .to_owned(),
            ..HarnessCapabilities::default()
        }
    }
    fn detect(&self) -> HarnessDetection {
        resolve_opencode_binary(&self.uze_home.shims_dir())
            .map(|(_, detection)| detection)
            .unwrap_or_default()
    }

    /// OpenCode V2 installs as `opencode` (current, 1.18.x) with a legacy
    /// `opencode2` alias. Both are probed in PATH order excluding the shim.
    fn detection_program_candidates(&self) -> Vec<&'static str> {
        vec!["opencode", "opencode2"]
    }

    /// UZE keeps `opencode` as its stable shim name; the legacy `opencode2`
    /// alias is still resolved generically without mutating vendor paths.
    fn supports_runtime_integration(&self) -> bool {
        true
    }

    fn runtime_executable_aliases(&self) -> &'static [&'static str] {
        &["opencode2"]
    }

    /// OpenCode derives a skill's ID from its path (verified in the V2
    /// docs: the ID comes from the path, never the frontmatter `name`), so
    /// UZE exposes the stable namespaced invocation label verbatim
    /// (`flow:review`) as the single, deterministic candidate. No bare
    /// alias, no collision-dependent qualification (ADR-026). MCP stays on
    /// the default fully-qualified policy — capability naming policies are
    /// never mixed just because all are `Resource`s.
    fn exposure_name_candidates(&self, resource: &Resource) -> Vec<String> {
        if resource.capability.kind == CapabilityKind::AgentSkill {
            return qualified_exposure_name_candidates(resource);
        }
        default_exposure_name_candidates(resource)
    }

    /// Codex also discovers Skills from this exact same
    /// `~/.agents/skills` directory (see its own override of this
    /// method), so a name this integration claims here must be treated as
    /// claimed for it too — every member derives the same single
    /// namespaced label, so the group always converges on one entry.
    fn shared_agent_skill_root(&self) -> Option<PathBuf> {
        Some(self.skills_dir.clone())
    }

    /// Provisioning targets the current V2 beta channel and verifies its
    /// distinct `opencode2` executable. Runtime invocation remains through
    /// UZE's stable `opencode` shim above.
    fn provision(&self, runner: &dyn ProcessRunner) -> Result<ProvisioningResult> {
        provision_opencode(runner, || self.detect(), &self.uze_home.shims_dir())
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
            CapabilityKind::Agent => self.agent_plan(resource),
            _ => unsupported(
                resource,
                "OpenCode portability is implemented only for Agent Skills, Agents, and MCP in this slice.",
            ),
        }
    }
    fn attach(&self, resource: &Resource) -> Result<Option<PathBuf>> {
        let plan = self.exposure_plan(resource);
        match &plan.mechanism {
            ExposureMechanism::ManagedUserScopeReference { .. } => {
                if resource.capability.kind == CapabilityKind::AgentSkill {
                    self.materialize_or_verify_skill(resource)?;
                }
                Ok(Some(plan.mechanism.attach()?))
            }
            ExposureMechanism::ManagedVendorConfig {
                entry_name,
                command,
                args,
                ..
            } => {
                if let Some(exe) = provisioning_executable_for_attach(&self.uze_home.shims_dir()) {
                    let (home_opt, xdg_opt) = resolve_home_and_xdg(&self.config_path);
                    // Only use the native CLI when we can derive a HOME/XDG that
                    // matches this integration's config_path (production:
                    // $HOME/.config/opencode/opencode.json or
                    // $XDG_CONFIG_HOME/opencode/opencode.json). Isolated tests
                    // use <tmp>/config/opencode.json — there we keep the
                    // direct file path so inspection stays on the same file.
                    // If the CLI fails (e.g. shim mis-resolution in a test
                    // without a real opencode on PATH), fall back to the
                    // direct file path so tests stay deterministic.
                    if let Some(home) = home_opt
                        && let Ok(path) = attach_mcp_entry(
                            &exe,
                            &home,
                            xdg_opt.as_deref(),
                            entry_name,
                            command,
                            args,
                            &self.config_path,
                        )
                    {
                        return Ok(path);
                    }
                }
                attach_mcp_config(&self.config_path, entry_name, command, args)
            }
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
        match mcp::configured_server(&config, entry_name) {
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
            let detached = detach_standard_receipt(receipt)?;
            if detached.state == AttachmentState::Missing
                && let ManagedArtifact::SymlinkReference { target, .. } = &receipt.artifact
            {
                self.cleanup_unused_skill_wrapper(target)?;
            }
            return Ok(detached);
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
        let current = mcp::configured_server(&config, entry_name);
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
            .and_then(|mcp| mcp.get_mut("servers"))
            .and_then(serde_json::Value::as_object_mut)
            .expect("matched entry has mcp.servers object")
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

impl OpenCodeIntegration {
    fn agent_plan(&self, resource: &Resource) -> ExposurePlan {
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
            evidence: "OpenCode natively discovers Markdown agents from its configuration agents directory; UZE keeps a receipt-owned symlink to the canonical Store definition.".to_owned(),
        }
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
            r#"{"mcp":{"servers":{"uze-example":{"type":"local","command":["/bin/example","--serve"],"future":true},"foreign":{"type":"local","command":["foreign"]}},"unrelated":true},"unrelated":true}"#,
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
        assert!(after.pointer("/mcp/servers/uze-example").is_none());
        assert!(after.pointer("/mcp/servers/foreign").is_some());
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
            r#"{"mcp":{"servers":{"uze-example":{"type":"local","command":["/bin/changed","--serve"]}}}}"#,
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
