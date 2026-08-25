//! Claude Code peer integration. Its transparent-attachment strategy is a
//! UZE-managed "skills-dir plugin" reference at `<claude_home>/skills/<name>`
//! (see ADR-006): Claude auto-loads any directory there containing
//! `.claude-plugin/plugin.json` + `SKILL.md` at the start of every session,
//! with no per-session flag. Until `uze setup` has completed, exposure falls
//! back to the `--plugin-dir` conformance probe from ADR-005.
//!
//! Split by concern: [`mcp`] (MCP server registration/inspection),
//! [`skills`] (the managed skills-dir shim), [`plugin`] (the native
//! `.claude-plugin/marketplace.json` catalogue and its exact-coverage
//! computation), [`provision`] (install/update via the official installer),
//! and [`runtime`] (the experimental `--add-dir` runtime projection). This
//! file is the composition root: the `ClaudeIntegration` struct and its
//! `IntegrationPort` impl, delegating to each submodule.

use std::{fs, path::Path};

use uze_core::{
    Result, UzeError,
    capability::CapabilityKind,
    exposure::{ExposureMechanism, ExposurePlan, PackageExposurePlan},
    harness_runtime::{RuntimeContext, resolve_real_executable},
    home::UzeHome,
    integration::{
        AttachmentInspection, AttachmentReceipt, AttachmentState, HarnessDetection,
        IntegrationPort, ManagedArtifact, PublicationStatus, default_exposure_name_candidates,
        detach_standard_receipt, inspect_standard_receipt, qualified_exposure_name_candidates,
    },
    project::Resource,
    provisioning::{ProcessRunner, ProcessSpec, ProvisioningResult},
    router::{CompatibilityRoute, HarnessCapabilities, VerificationStatus},
    state,
    store::StoredPackage,
};

mod generate;
mod mcp;
mod plugin;
mod provision;
mod runtime;
mod skills;

pub use mcp::detach_mcp_entry;

use crate::shared::process::run_quiet;
use generate::{
    GENERATED_MARKETPLACE_NAME, GENERATED_PLUGIN_KIND, generatable, generated_catalogue_matches,
    generated_exact_coverage, generated_package_receipt, generated_root,
    materialize_generated_package, remove_generated_package_by_id, write_generated_catalogue,
};
use mcp::attach_mcp_entry;
use plugin::{
    claude_catalogue_document, claude_marketplace_exists, claude_package_receipt,
    claude_plugin_installed, claude_publishable, detail_path, inspect_claude_plugin,
    remove_claude_plugin, run_claude_marketplace_add, write_claude_catalogue,
};
use provision::{detect_binary, provision_cli};
use skills::materialize_shim;
const CLAUDE_MARKETPLACE_NAME: &str = "uze-local";

/// Claude Code peer integration. Its transparent-attachment strategy is a
/// UZE-managed "skills-dir plugin" reference at `<claude_home>/skills/<name>`
/// (see ADR-006): Claude auto-loads any directory there containing
/// `.claude-plugin/plugin.json` + `SKILL.md` at the start of every session,
/// with no per-session flag. Until `uze setup` has completed, exposure falls
/// back to the `--plugin-dir` conformance probe from ADR-005.
pub struct ClaudeIntegration {
    skills_dir: std::path::PathBuf,
    /// `HOME` to set explicitly whenever a `claude` subcommand is shelled
    /// out to for MCP registration (`mcp add`/`get`/`remove`) — unlike the
    /// Skills path (pure filesystem operations on `skills_dir`, no process
    /// spawn), MCP commands read `~/.claude.json` themselves, so a caller
    /// invoking this integration's methods directly (not via a spawned
    /// `uze` subprocess whose own environment was already isolated) must
    /// not have those commands silently fall back to the real `$HOME`.
    /// Derived from `claude_home`'s parent so an isolated test fixture
    /// (whose `claude_home` need not literally be `$HOME/.claude`) still
    /// gets a consistent, isolated value.
    command_home: std::path::PathBuf,
    uze_home: UzeHome,
}

impl ClaudeIntegration {
    pub fn new(claude_home: std::path::PathBuf, uze_home: UzeHome) -> Self {
        let command_home = claude_home
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| claude_home.clone());
        Self {
            skills_dir: claude_home.join("skills"),
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
        Ok(Self::new(
            std::path::PathBuf::from(home).join(".claude"),
            uze_home,
        ))
    }

    fn catalogue_root(&self) -> std::path::PathBuf {
        self.uze_home.store_dir()
    }

    fn catalogue_path(&self) -> std::path::PathBuf {
        self.catalogue_root()
            .join(".claude-plugin/marketplace.json")
    }

    /// The real `claude` executable, resolved explicitly rather than through
    /// a bare `Command::new("claude")` PATH lookup. Once `uze setup claude`
    /// has ever succeeded, `~/.uze/shims` sits ahead of the real binary on
    /// `PATH` (see `UzeApplication::ensure_runtime_shim`), so a bare lookup
    /// here would re-enter UZE's own runtime shim instead of the vendor CLI.
    /// The shim then prepends `--add-dir <dir>` before whatever argument
    /// follows — for `["update"]`, since `--add-dir` is a variadic option,
    /// the real CLI swallows `update` into that directory list instead of
    /// recognizing it as a subcommand, and falls through to its default
    /// action: starting a full interactive session instead of checking for
    /// updates. Falls back to the bare name (previous behavior) if no real
    /// binary can be found outside the shims directory.
    fn provisioning_executable(&self) -> String {
        resolve_real_executable(&["claude"], &self.uze_home.shims_dir())
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| "claude".to_owned())
    }

    /// Installs a package whose source ships its own `.claude-plugin/
    /// plugin.json`, through the existing `uze-local` marketplace rooted at
    /// the Store itself. Unchanged behavior — extracted verbatim from the
    /// pre-generation `attach_package` so the explicit-envelope path stays
    /// exactly as proven by the existing 12 native-package tests.
    fn attach_explicit_package(
        &self,
        executable: &Path,
        package: &StoredPackage,
    ) -> Result<Option<AttachmentReceipt>> {
        let catalogue_root = self.catalogue_root();
        if !claude_marketplace_exists(executable, &self.command_home, &catalogue_root) {
            run_claude_marketplace_add(executable, &self.command_home, &catalogue_root)?;
        }
        let selector = format!("{}@{CLAUDE_MARKETPLACE_NAME}", package.id.as_str());
        if claude_plugin_installed(executable, &self.command_home, &selector) {
            return Ok(Some(claude_package_receipt(
                self.id(),
                package,
                &catalogue_root,
                &selector,
            )));
        }
        run_quiet(
            executable,
            &self.command_home,
            &format!("claude plugin install `{selector}`"),
            &["plugin", "install", selector.as_str()],
        )?;
        Ok(Some(claude_package_receipt(
            self.id(),
            package,
            &catalogue_root,
            &selector,
        )))
    }

    /// Installs a package with no author-provided envelope through the
    /// second, UZE-owned `uze-store` marketplace, materializing
    /// (or refreshing) its generated envelope directory first.
    fn attach_generated_package(
        &self,
        executable: &Path,
        package: &StoredPackage,
    ) -> Result<Option<AttachmentReceipt>> {
        materialize_generated_package(&self.uze_home, package)?;
        let marketplace_root = generated_root(&self.uze_home);
        if !claude_marketplace_exists(executable, &self.command_home, &marketplace_root) {
            run_claude_marketplace_add(executable, &self.command_home, &marketplace_root)?;
        }
        let selector = format!("{}@{GENERATED_MARKETPLACE_NAME}", package.id.as_str());
        if claude_plugin_installed(executable, &self.command_home, &selector) {
            return Ok(Some(generated_package_receipt(
                self.id(),
                package,
                &marketplace_root,
                &selector,
            )));
        }
        run_quiet(
            executable,
            &self.command_home,
            &format!("claude plugin install `{selector}`"),
            &["plugin", "install", selector.as_str()],
        )?;
        Ok(Some(generated_package_receipt(
            self.id(),
            package,
            &marketplace_root,
            &selector,
        )))
    }
}

impl IntegrationPort for ClaudeIntegration {
    fn id(&self) -> &'static str {
        "claude-code"
    }

    /// `claude` is the name people type; `claude-code` is the stable id the
    /// receipts and state records carry.
    fn aliases(&self) -> &'static [&'static str] {
        &["claude"]
    }

    fn display_name(&self) -> &'static str {
        "claude"
    }

    fn capabilities(&self) -> HarnessCapabilities {
        HarnessCapabilities {
            native: [CapabilityKind::AgentSkill, CapabilityKind::Mcp]
                .into_iter()
                .collect(),
            verification: VerificationStatus::Unverified,
            evidence: "Claude Code consumes UZE's derived marketplaces: a package shipping .claude-plugin/plugin.json is installed as a native plugin covering its declared skills/mcpServers (`claude plugin install <sel>@uze-local`, empirically confirmed via `claude plugin validate`/`plugin list`); one without gets a deterministically synthesized envelope published through the generated-only `uze-store` marketplace (ADR-020). Invocation policy is translated into Claude's own SKILL.md frontmatter (disable-model-invocation / user-invocable — both verified against the current Claude Code skill docs); an explicit-envelope Skill is only claimed as covered when its canonical policy is actually preserved by the vendor content it ships. Capability-level shims (`<claude_home>/skills` reference, `claude mcp add`) remain only as fallback for resources outside the envelope's coverage. Behavioral (prompted) verification remains a separate opt-in conformance probe."
                .to_owned(),
            ..HarnessCapabilities::default()
        }
    }

    fn detect(&self) -> HarnessDetection {
        detect_binary(&self.provisioning_executable())
    }

    /// `id()` is `claude-code`; the binary people actually have on `PATH`
    /// is `claude`.
    fn detection_program_candidates(&self) -> Vec<&'static str> {
        vec!["claude"]
    }

    /// `CONTEXT DELIVERY POLICY`: this is the `EXPERIMENTAL RUNTIME
    /// DELIVERY STRATEGY` — see `runtime::claude_runtime_projection`'s doc
    /// comment. Building this shim path does not by itself replace the
    /// existing project-root `CLAUDE.md` bridge; that decision waits on the
    /// interactive dogfood comparison in the Checkpoint 2 report.
    fn runtime_contribution(
        &self,
        ctx: &RuntimeContext,
    ) -> uze_core::harness_runtime::HarnessRuntimeContribution {
        runtime::runtime_contribution(ctx)
    }

    fn supports_runtime_integration(&self) -> bool {
        true
    }

    fn provision(&self, runner: &dyn ProcessRunner) -> Result<ProvisioningResult> {
        let executable = self.provisioning_executable();
        provision_cli(
            runner,
            &executable,
            self.detect(),
            ProcessSpec::new(
                "sh",
                ["-c", "curl -fsSL https://claude.ai/install.sh | bash"],
            )
            .with_inherited_output(),
            ProcessSpec::new(executable.clone(), ["update"]).with_inherited_output(),
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
                "Claude Code needs a UZE-stored Agent Plugin package for this attachment.",
            );
        }
        match resource.capability.kind {
            CapabilityKind::AgentSkill => self.skill_exposure_plan(resource),
            CapabilityKind::Mcp => self.mcp_exposure_plan(resource),
            _ => unsupported(
                resource,
                "Claude Code attachment is only modeled for Agent Skills and MCP servers.",
            ),
        }
    }

    /// Claude namespaces plugin skills/commands itself (`/flow:review` for a
    /// plugin named `flow` — see `docs/capabilities/skill-invocation-policy.md`), so UZE
    /// never materializes the namespace into the plugin: the plugin declares
    /// the plain logical name and Claude owns the `plugin:` prefix. For the
    /// capability-level fallback shim the physical directory name is the
    /// stable namespaced label (ADR-026). MCP deliberately stays on the
    /// shared default (fully qualified only) — its physical name never
    /// reaches the terminal UX the same way, and mixing naming policies just
    /// because both are `Resource`s would be exactly the "não misture Skill
    /// e MCP" mistake the design explicitly rules out.
    fn exposure_name_candidates(&self, resource: &Resource) -> Vec<String> {
        if resource.capability.kind != CapabilityKind::AgentSkill {
            return default_exposure_name_candidates(resource);
        }
        qualified_exposure_name_candidates(resource)
    }

    fn package_exposure_plan(
        &self,
        package: &StoredPackage,
        resources: &[&Resource],
    ) -> Option<PackageExposurePlan> {
        if package.root.join(".claude-plugin/plugin.json").is_file() {
            let provided = plugin::claude_exact_coverage(package, resources);
            return Some(PackageExposurePlan {
                package_id: package.id.clone(),
                route: CompatibilityRoute::Native,
                verification: VerificationStatus::Unverified,
                provided_resource_identities: provided,
                evidence: "The preserved external .claude-plugin/plugin.json is exposed through UZE's derived Claude marketplace. Claude Code owns Skill and MCP loading for this plugin, so UZE must not attach them a second time."
                    .to_owned(),
            });
        }
        // No author-provided envelope. Rather than falling straight to
        // capability decomposition, check whether UZE can safely synthesize
        // one (ADR-020, refining ADR-013 §2: Explicit Native Package >
        // Generated Native Package > Native Capability > Safe Adaptation >
        // Unsupported). This method stays read-only either way — it
        // computes what *would* be covered, never materializes anything.
        if !generatable(package) {
            return None;
        }
        let provided = generated_exact_coverage(package, resources);
        Some(PackageExposurePlan {
            package_id: package.id.clone(),
            route: CompatibilityRoute::Native,
            verification: VerificationStatus::Unverified,
            provided_resource_identities: provided,
            evidence: "No .claude-plugin/plugin.json was provided. UZE synthesizes one deterministically into a UZE-owned derived directory (never the Store) covering exactly the package's conventional skills/ directory and mcp.json-declared servers, published through a second, generated-only Claude marketplace."
                .to_owned(),
        })
    }

    fn attach_package(
        &self,
        package: &StoredPackage,
        _plan: &PackageExposurePlan,
    ) -> Result<Option<AttachmentReceipt>> {
        let executable = self.provisioning_executable();
        let executable = Path::new(&executable);
        if package.root.join(".claude-plugin/plugin.json").is_file() {
            return self.attach_explicit_package(executable, package);
        }
        self.attach_generated_package(executable, package)
    }

    fn republish_packages(&self, packages: &[StoredPackage]) -> Result<()> {
        write_claude_catalogue(&self.catalogue_path(), packages)?;
        write_generated_catalogue(&self.uze_home, packages)
    }

    fn publication(&self, packages: &[StoredPackage]) -> PublicationStatus {
        let expected = claude_catalogue_document(packages);
        let explicit_published = match fs::read(self.catalogue_path()) {
            Ok(bytes) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
                Ok(actual) if actual == expected => Ok(()),
                Ok(_) => Err(
                    "the Claude marketplace does not match the installed package set; re-run `uze setup claude`"
                        .to_owned(),
                ),
                Err(error) => Err(format!(
                    "the Claude marketplace is unreadable ({error}); re-run `uze setup claude`"
                )),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if claude_publishable(packages).is_empty() {
                    Ok(())
                } else {
                    Err(
                        "no Claude marketplace has been written for the installed packages; re-run `uze setup claude`"
                            .to_owned(),
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
                "the generated Claude marketplace does not match the installed package set; re-run `uze setup claude`"
                    .to_owned(),
            );
        }
        PublicationStatus::Published
    }

    fn attach(&self, resource: &Resource) -> Result<Option<std::path::PathBuf>> {
        let plan = self.exposure_plan(resource);
        match &plan.mechanism {
            ExposureMechanism::ManagedUserScopeReference {
                source, entry_name, ..
            } => {
                let skill_source_dir = resource
                    .capability
                    .path
                    .parent()
                    .expect("SKILL.md has a parent");
                // The shim's own plugin directory gets the stable namespaced
                // label (`flow:review`), while the *manifest plugin name*
                // stays the namespace (`flow`): Claude then exposes the
                // skill as `/flow:review` (ADR-026) instead of double
                // namespacing it (`/flow:flow:review`).
                let namespace = resource_package_id(resource);
                let policy = resource.skill_invocation();
                materialize_shim(
                    source,
                    skill_source_dir,
                    entry_name,
                    namespace.as_deref(),
                    &policy,
                )?;
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
            } => mcp::inspect_claude_mcp(
                &self.command_home.join(".claude.json"),
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
            } if kind == "claude-plugin" || kind == GENERATED_PLUGIN_KIND => {
                let Some(marketplace_root) = detail_path(detail, "marketplace_root") else {
                    return plugin::blocked("plugin receipt has no marketplace root".to_owned());
                };
                let Some(package_root) = detail_path(detail, "package_root") else {
                    return plugin::blocked("plugin receipt has no package root".to_owned());
                };
                let executable = self.provisioning_executable();
                inspect_claude_plugin(
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
                Ok(AttachmentInspection {
                    state: AttachmentState::Missing,
                    reason: "Claude managed MCP entry detached via CLI".to_owned(),
                })
            }
            ManagedArtifact::IntegrationOwned { kind, selector, .. }
                if kind == "claude-plugin" || kind == GENERATED_PLUGIN_KIND =>
            {
                let executable = self.provisioning_executable();
                remove_claude_plugin(Path::new(&executable), &self.command_home, selector)?;
                if kind == GENERATED_PLUGIN_KIND {
                    // The generated envelope directory is a Derived Artifact
                    // (ADR-013 §4): non-authoritative, rebuildable, and
                    // never the canonical Store — safe to remove outright
                    // now that Claude no longer references it.
                    remove_generated_package_by_id(&self.uze_home, &receipt.package_id)?;
                }
                Ok(AttachmentInspection {
                    state: AttachmentState::Missing,
                    reason: "Claude native plugin detached".to_owned(),
                })
            }
            _ => {
                let detached = detach_standard_receipt(receipt)?;
                if detached.state == AttachmentState::Missing
                    && let ManagedArtifact::SymlinkReference { target, .. } = &receipt.artifact
                {
                    self.cleanup_unused_shim(target)?;
                }
                Ok(detached)
            }
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

/// The plugin id of a package-owned resource — the namespace half of the
/// stable invocation label (ADR-026). `None` for a project-owned resource,
/// which has no managed attachment.
fn resource_package_id(resource: &Resource) -> Option<String> {
    match &resource.origin {
        uze_core::project::ResourceOrigin::Package { id, .. } => Some(id.as_str().to_owned()),
        uze_core::project::ResourceOrigin::Project { .. } => None,
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use std::path::Path;
    use std::sync::Mutex;

    use uze_core::exposure::McpEnvironmentReference;
    use uze_core::home::UzeHome;
    use uze_core::integration::{
        AttachmentReceipt, AttachmentState, IntegrationPort, ManagedArtifact,
    };
    use uze_core::provisioning::{ProcessResult, ProcessRunner, ProcessSpec};

    use super::mcp::inspect_claude_mcp;
    use super::provision::provision_cli;
    use super::{ClaudeIntegration, fs};

    struct RecordingRunner {
        commands: Mutex<Vec<ProcessSpec>>,
    }

    impl ProcessRunner for RecordingRunner {
        fn run(&self, spec: &ProcessSpec) -> Result<ProcessResult, uze_core::UzeError> {
            self.commands.lock().unwrap().push(spec.clone());
            Ok(ProcessResult {
                success: true,
                timed_out: false,
            })
        }
    }

    #[test]
    fn missing_harness_uses_its_documented_official_install_route_then_verifies() {
        let runner = RecordingRunner {
            commands: Mutex::new(Vec::new()),
        };
        let result = provision_cli(
            &runner,
            "claude-test-does-not-exist",
            uze_core::integration::HarnessDetection::default(),
            ProcessSpec::new("sh", ["-c", "official-install"]),
            ProcessSpec::new("claude", ["update"]),
            "official-native-installer",
        )
        .unwrap();
        if cfg!(unix) {
            assert_eq!(
                result.action,
                uze_core::provisioning::ProvisionAction::Install
            );
            assert_eq!(
                result.status,
                uze_core::provisioning::ProvisionStatus::Verified
            );
            let commands = runner.commands.lock().unwrap();
            assert_eq!(commands[0].program, "sh");
            assert_eq!(
                commands[0].output,
                uze_core::provisioning::ProcessOutput::Quiet,
                "the helper's synthetic test command is intentionally quiet"
            );
            assert_eq!(commands[1].program, "claude-test-does-not-exist");
            assert_eq!(commands[1].arguments, ["--version"]);
        }
    }
    fn check(value: &str) -> AttachmentState {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("uze-claude-config-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join(".claude.json");
        fs::write(&path, value).unwrap();
        let environment: Vec<McpEnvironmentReference> = Vec::new();
        let state = inspect_claude_mcp(
            &path,
            "uze-x",
            "stdio",
            Path::new("tool"),
            &["a".to_owned()],
            None,
            &environment,
            None,
        )
        .state;
        let _ = fs::remove_dir_all(root);
        state
    }
    #[test]
    fn intact_is_matched_and_unknown_fields_tolerated() {
        assert_eq!(
            check(
                r#"{"mcpServers":{"uze-x":{"command":"tool","args":["a"],"extra":true},"other":{"command":"x"}},"unknown":1}"#
            ),
            AttachmentState::Matched
        );
    }
    #[test]
    fn absent_is_missing() {
        assert_eq!(check(r#"{"mcpServers":{}}"#), AttachmentState::Missing);
    }
    #[test]
    fn command_or_args_change_is_drifted() {
        assert_eq!(
            check(r#"{"mcpServers":{"uze-x":{"command":"other","args":["a"]}}}"#),
            AttachmentState::Drifted
        );
        assert_eq!(
            check(r#"{"mcpServers":{"uze-x":{"command":"tool","args":["b"]}}}"#),
            AttachmentState::Drifted
        );
    }
    #[test]
    fn malformed_is_blocked() {
        assert_eq!(check("{bad"), AttachmentState::Blocked);
    }

    #[cfg(unix)]
    #[test]
    fn detaching_a_skill_reference_cleans_an_unreferenced_owned_shim() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!("uze-claude-shim-{}", std::process::id()));
        let uze_home = UzeHome::at(root.join("uze"));
        let integration = ClaudeIntegration::new(root.join("claude"), uze_home.clone());
        fs::create_dir_all(&integration.skills_dir).unwrap();
        let shim = uze_home.state_dir().join("attachments/claude/uze-example");
        fs::create_dir_all(shim.join(".claude-plugin")).unwrap();
        fs::write(shim.join(".claude-plugin/plugin.json"), "{}").unwrap();
        let source = root.join("source/SKILL.md");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, "skill").unwrap();
        symlink(&source, shim.join("SKILL.md")).unwrap();
        let reference = integration.skills_dir.join("uze-example");
        symlink(&shim, &reference).unwrap();
        let receipt = AttachmentReceipt {
            package_id: "example".to_owned(),
            resource_identity: Some("skill:example".to_owned()),
            integration: integration.id().to_owned(),
            strategy: "managed-user-scope-reference".to_owned(),
            artifact: ManagedArtifact::SymlinkReference {
                path: reference,
                target: shim.clone(),
            },
        };
        assert_eq!(
            integration.detach_receipt(&receipt).unwrap().state,
            AttachmentState::Missing
        );
        assert!(!shim.exists());
        let _ = fs::remove_dir_all(root);
    }
}
