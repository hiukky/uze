//! The product-facing read models `UzeApplication` hands to the CLI and the
//! TUI, and the queries that build them.
//!
//! These types are the crate's public vocabulary — every one of them is
//! re-exported from `application` and named by `src/`. They lived inline in
//! `application.rs` until they were half of it, which buried the
//! orchestration surface the file exists for.

#![allow(clippy::empty_line_after_doc_comments)]

use uze_core::Result;

use super::services::Plugins;
use super::*;

impl Plugins<'_> {
    pub fn list(&self) -> Result<Vec<PluginSummary>> {
        self.0
            .store
            .package_ids()?
            .into_iter()
            .map(|id| self.0.plugin_summary(&self.0.store.package(&id)?))
            .collect()
    }

    pub fn inspect(&self, id: &str) -> Result<PluginInspection> {
        let package = self.0.package_by_name(id)?;
        let environment = self.0.engine().compose(std::slice::from_ref(&package.id))?;
        let resources: Vec<_> = environment.resources.iter().collect();
        let deliveries = self
            .0
            .integrations
            .iter()
            .map(|integration| {
                let package_plan = integration.package_exposure_plan(&package, &resources);
                let provided = package_plan
                    .as_ref()
                    .map(|plan| plan.provided_resource_identities.clone())
                    .unwrap_or_default();
                let capabilities = resources
                    .iter()
                    .map(|resource| CapabilityDelivery {
                        identity: resource.identity(),
                        kind: resource.capability.kind,
                        plan: (!provided.contains(&resource.identity()))
                            .then(|| integration.exposure_plan(resource)),
                        provided_by_package: provided.contains(&resource.identity()),
                    })
                    .collect();
                HarnessDelivery {
                    integration: integration.id().to_owned(),
                    display_name: integration.display_name().to_owned(),
                    package_plan,
                    capabilities,
                }
            })
            .collect();
        let reconciliation = self.0.reconcile_cached_report(package.id.as_str());
        Ok(PluginInspection {
            plugin: self.0.plugin_summary(&package)?,
            capabilities: resources
                .iter()
                .map(|resource| PluginCapability {
                    identity: resource.identity(),
                    name: capability_display_name(resource),
                    kind: resource.capability.kind,
                })
                .collect(),
            deliveries,
            managed_state: managed_state(&reconciliation),
            reconciliation,
        })
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct PluginSummary {
    pub id: String,
    /// The local name this plugin currently invokes under (ADR-038) — its
    /// own bare plugin name unless an install-time `alias` resolution gave
    /// it a different one to coexist with another marketplace's same-named
    /// plugin. Always present, never itself marketplace-qualified; `id`
    /// carries the real, marketplace-qualified identity (the origin).
    pub active_name: String,
    /// Human-facing description of where this package came from. Display
    /// only: the typed provenance stays in the registry, and nothing parses
    /// this back.
    pub source: String,
    pub store_path: PathBuf,
    pub capability_count: usize,
    /// Whether the official marketplace snapshot currently carries
    /// different content than what's installed — a pure read, computed by
    /// comparing directory trees, never re-applied automatically (see
    /// `ensure_default_plugins`). `None` for any package this composition
    /// root has no offline way to compare (anything not sourced from the
    /// embedded marketplace) — never re-acquired over the network or from
    /// a mutable local path just to answer this question.
    pub update_available: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PluginCapability {
    pub identity: String,
    pub name: String,
    pub kind: CapabilityKind,
}

#[derive(Clone, Debug, Serialize)]
pub struct MarketplaceSummary {
    pub name: String,
    pub source: String,
    pub plugin_count: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct MarketplacePluginSummary {
    /// Which registered marketplace this plugin came from (`uze-official`
    /// for the embedded snapshot, or the name it was registered under via
    /// `marketplace add`). Needed once more than one marketplace can
    /// contribute plugins to the same list — see `list_marketplace_plugins`.
    pub marketplace: String,
    pub name: String,
    pub description: Option<String>,
    pub keywords: Vec<String>,
    pub installed: bool,
    /// `None` when not installed (nothing to compare against) or when the
    /// comparison could not be made — never a guess.
    pub update_available: Option<bool>,
    /// Whether `bootstrap::DEFAULT_PLUGIN_IDS` installs this plugin on a
    /// fresh `UZE_HOME` — product policy, not a marketplace fact.
    pub is_default: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct MarketplacePluginDetail {
    pub summary: MarketplacePluginSummary,
    pub capabilities: Vec<PluginCapability>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CapabilityDelivery {
    pub identity: String,
    pub kind: CapabilityKind,
    pub provided_by_package: bool,
    pub plan: Option<ExposurePlan>,
}

#[derive(Clone, Debug, Serialize)]
pub struct HarnessDelivery {
    pub integration: String,
    /// The name a person recognizes (`IntegrationPort::display_name`) —
    /// display only, mirrors `HarnessHealth::display_name`.
    pub display_name: String,
    pub package_plan: Option<PackageExposurePlan>,
    pub capabilities: Vec<CapabilityDelivery>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PluginInspection {
    pub plugin: PluginSummary,
    pub capabilities: Vec<PluginCapability>,
    pub deliveries: Vec<HarnessDelivery>,
    pub managed_state: ManagedStateSummary,
    pub reconciliation: ReconciliationReport,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ManagedStateSummary {
    pub matched: usize,
    pub missing: usize,
    pub drifted: usize,
    pub conflicts: usize,
    pub blocked: usize,
    pub ledger_error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AttachmentSummary {
    pub integration: String,
    pub location: PathBuf,
}

#[derive(Clone, Debug, Serialize)]
pub struct PublicationOutcome {
    pub integration: String,
    /// `None` when the derived view refreshed cleanly. A message here is
    /// actionable on its own: the package is installed and only the view
    /// needs rebuilding.
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AddPluginReport {
    pub plugin: PluginSummary,
    pub package_plans: Vec<(String, PackageExposurePlan)>,
    pub attachments: Vec<AttachmentSummary>,
    pub publications: Vec<PublicationOutcome>,
}

#[derive(Clone, Debug, Serialize)]
pub enum UpdatePluginReport {
    Updated {
        plugin: PluginSummary,
        attachments: Vec<AttachmentSummary>,
        publications: Vec<PublicationOutcome>,
    },
    /// The installed package could not be safely detached, so nothing was
    /// replaced. The newly resolved revision is discarded with its scratch
    /// directory.
    Blocked {
        report: ReconciliationReport,
        plan: PackageRemovalPlan,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct SetupResult {
    pub integration: String,
    pub detection: HarnessDetection,
    pub configured: bool,
    pub provisioning: ProvisioningResult,
    /// `Some` when this integration opted into `EXPERIMENTAL RUNTIME
    /// DELIVERY STRATEGY` (`IntegrationPort::supports_runtime_integration`)
    /// and `ensure_runtime_shim` created/refreshed its PATH shim as an
    /// ordinary part of this `setup` call — see this module's
    /// `INSTRUCTION_BRIDGE_IDENTITY` doc for how this relates to the
    /// existing, still-default,
    /// persistent `CLAUDE.md` bridge. `None` for every
    /// integration with no runtime-integration story (not an error).
    pub runtime_shim: Option<RuntimeShimSetup>,
    /// `Some` when attachment of at least one stored package failed for this
    /// harness (e.g. a foreign `uze` plugin already occupies the Antigravity
    /// name). The harness stays `configured` and other harnesses still
    /// complete — the error is surfaced as a warning, not a fatal `Err`,
    /// matching the production-resilience contract added for real user
    /// environments where `setup` must never abort the whole run on one
    /// harness.
    pub attach_error: Option<String>,
    /// `Some` when the experimental runtime shim failed to be created for
    /// this harness (e.g. no real executable on PATH outside the shim dir).
    /// Also non-fatal: the harness setup completed, the shim just couldn't be
    /// wired.
    pub shim_error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RuntimeShimSetup {
    pub shim_path: PathBuf,
    pub resolved_executable: PathBuf,
    /// Set only when this call actually wrote a change into a detected
    /// shell rc file (`shell_path::ensure_path_line`) — the file that was
    /// touched. A marked, reversible block; see `shell_path` for the exact
    /// shape and safety guarantees. Absent when the shim dir was already
    /// on `PATH`, when the rc file already had the right line (idempotent
    /// no-op), or when no rc file was touched at all.
    pub rc_file_updated: Option<PathBuf>,
    /// Set whenever the shim dir isn't yet on `PATH` for the *current*
    /// shell session — a change to a shell rc file only takes effect in a
    /// new shell, so this is always the instruction for finishing that
    /// (open a new terminal / `source <rc>`), or the raw manual `export`
    /// line when no supported shell rc file was detected at all.
    pub path_hint: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "outcome", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RemovePluginReport {
    /// No Store registration or attachment receipt remained. This is a safe
    /// idempotent outcome, not historical evidence that the package existed.
    AlreadyAbsent { plugin: String },
    Removed {
        plugin: String,
        detached_receipts: Vec<String>,
        already_missing_receipts: Vec<String>,
    },
    Blocked {
        report: ReconciliationReport,
        plan: PackageRemovalPlan,
    },
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StoreHealth {
    Ready,
    Blocked(String),
}

#[derive(Clone, Debug, Serialize)]
pub struct HarnessHealth {
    pub integration: String,
    /// The name a person recognizes (`IntegrationPort::display_name`) —
    /// display only. `integration` above stays the stable id everything
    /// else (setup, state, doctor matching) is keyed on.
    pub display_name: String,
    /// A one-line, human-facing description (`IntegrationPort::description`)
    /// — display only, never a lookup key.
    pub description: String,
    pub detection: HarnessDetection,
    pub setup: String,
    pub strategy: Option<String>,
    pub provisioning: Option<state::ProvisioningRecord>,
    pub publication: PublicationStatus,
    /// This harness's own declared compatibility, independent of any
    /// installed plugin — what a Skill/MCP resource would route to if one
    /// existed. Compare against `PluginInspection::deliveries`, which is
    /// the same routing decision but for one specific installed resource.
    pub capabilities: HarnessCapabilities,
    /// Whether invoking this harness's command name resolves to UZE's
    /// runtime shim. A configured harness with a shadowed shim is not ready:
    /// its runtime context projection would be bypassed.
    pub runtime_shim_active: bool,
}

/// One recognized instructions file's observed state — never whether UZE
/// *should* do anything about it, just what is actually there.
#[derive(Clone, Debug, Serialize)]
pub struct InstructionSourceObservation {
    pub file_name: String,
    pub path: PathBuf,
    pub exists: bool,
    /// Content outside any well-formed UZE-managed region — i.e. something
    /// a user (or another tool) wrote independently of UZE.
    pub has_user_content: bool,
    pub managed_region_identities: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "delivery", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HarnessContextDelivery {
    /// Reads the shared `AGENTS.md` directly; nothing else is needed.
    Native,
    /// Needs a bridge region in its own file. `needed` is whether
    /// `AGENTS.md` currently carries at least one matched contribution
    /// worth bridging to; `state` is the bridge region's own observed
    /// state, checked regardless of whether it is currently needed (an
    /// unneeded-but-present bridge is real, reportable state, not silently
    /// folded into "needed").
    Bridge {
        needed: bool,
        state: AttachmentState,
    },
    /// This harness was not found on the machine at all; nothing here is
    /// evaluated as a gap.
    NotDetected,
}

#[derive(Clone, Debug, Serialize)]
pub struct HarnessContextStatus {
    pub integration: String,
    /// The name a person recognizes (`IntegrationPort::display_name`) —
    /// display only, mirrors `HarnessHealth::display_name`.
    pub display_name: String,
    pub delivery: HarnessContextDelivery,
}

/// The smallest classification that separates "context exists at all" from
/// "context reaches every harness that could use it" — deliberately not a
/// larger taxonomy. See `docs/capabilities/context-manager.md` Fase 3 for
/// why each variant exists and what evidence justified it.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "portability", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Portability {
    /// No recognized instructions file exists at all.
    NoContext,
    /// A shared `AGENTS.md` exists and every detected harness that needs
    /// something from it currently has it (natively, or via a matched
    /// bridge).
    Portable,
    /// A shared `AGENTS.md` exists, but at least one detected harness that
    /// needs a bridge does not currently have a working one.
    PartiallyPortable { gaps: Vec<String> },
    /// No shared `AGENTS.md` exists, but one or more vendor-specific files
    /// hold their own content — the original problem this capability set
    /// out to make visible.
    VendorLocked { files: Vec<PathBuf> },
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectContextStatus {
    pub root: PathBuf,
    pub canonical: PathBuf,
    pub sources: Vec<InstructionSourceObservation>,
    pub contributions: Vec<PackageInstructionStatus>,
    pub orphaned_regions: Vec<String>,
    pub malformed_regions: Vec<String>,
    pub harnesses: Vec<HarnessContextStatus>,
    /// The project's worktree policy, when `agents.lock` declares one.
    /// `None` means no policy is declared — not that isolation is forbidden.
    pub worktrees: Option<WorktreePolicyStatus>,
    pub portability: Portability,
    /// Human-readable notices for a state worth surfacing but that is not
    /// itself a gap or an error — e.g. a harness carrying legitimate
    /// vendor-specific content alongside its bridge. Never a suggestion to
    /// consolidate or an automatic action.
    pub warnings: Vec<String>,
}

/// The project's isolation declaration as it currently stands in the shared
/// instruction file. There is no per-harness row: UZE performs the isolation
/// itself, at launch, identically everywhere, so there is nothing left for a
/// harness to preserve or lose.
#[derive(Clone, Debug, Serialize)]
pub struct WorktreePolicyStatus {
    /// Where isolated checkouts live for this project. Fixed layout,
    /// resolved against the project root — reported, never configured.
    pub directory: PathBuf,
    pub completion: uze_core::worktree::CompletionBehavior,
    pub state: AttachmentState,
    pub reason: String,
    /// Regions left by a previous declaration, still present in the file.
    pub superseded_regions: Vec<String>,
}

/// The planned or performed change to the worktree policy's managed region.
#[derive(Clone, Debug, Serialize)]
pub struct WorktreeRegionPlan {
    pub file: PathBuf,
    pub action: instruction_context::PlannedAction,
    /// Regions a previous policy owned, which this pass would remove. A
    /// policy edit reads as one region going stale and another appearing,
    /// never as drift inside the region that already exists.
    pub superseded: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct WorktreeRegionStatus {
    pub file: PathBuf,
    pub state: AttachmentState,
    pub reason: String,
    pub removed_superseded: Vec<String>,
    /// A superseded region whose markers were malformed, so ownership could
    /// not be proven and nothing was removed.
    pub blocked_superseded: Vec<(String, String)>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BridgePlan {
    pub integration: String,
    pub file: PathBuf,
    pub action: instruction_context::PlannedAction,
}

#[derive(Clone, Debug, Serialize)]
pub struct ContextPlan {
    pub agents_md: PathBuf,
    pub agents_md_plan: instruction_context::AgentsMdPlan,
    /// Present whenever the shared file would gain, keep, lose, or is
    /// blocked from changing the worktree policy region.
    pub worktree_region: Option<WorktreeRegionPlan>,
    pub bridges: Vec<BridgePlan>,
}

impl ContextPlan {
    pub fn has_changes(&self) -> bool {
        self.agents_md_plan.has_changes()
            || self
                .worktree_region
                .as_ref()
                .is_some_and(|region| is_mutating(&region.action) || !region.superseded.is_empty())
            || self
                .bridges
                .iter()
                .any(|bridge| is_mutating(&bridge.action))
    }
}

/// Whether a planned region action would actually write. `Blocked` is not
/// mutating: it is a reason nothing can be applied, and reporting it as a
/// pending change would make `context plan` claim work that `reconcile`
/// will refuse to do.
fn is_mutating(action: &instruction_context::PlannedAction) -> bool {
    matches!(
        action,
        instruction_context::PlannedAction::Attach | instruction_context::PlannedAction::Remove
    )
}

/// A project-scoped health summary. See `UzeApplication::status` for why
/// this is deliberately not folded into `doctor`.
#[derive(Clone, Debug, Serialize)]
pub struct StatusReport {
    pub root: PathBuf,
    pub portability: Portability,
    pub harnesses: Vec<HarnessContextStatus>,
    pub packages_installed: usize,
    pub packages_contributing_here: usize,
    pub project_lock: ProjectLockStatus,
    /// Human-readable, one-line-each context problems: a non-matched
    /// contribution, a bridge gap, a malformed or blocked orphan region.
    /// Empty means healthy. Never a substitute for `context inspect`'s
    /// full detail — this is the "does anything need my attention" view.
    pub issues: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PackageInstructionStatus {
    pub package_id: String,
    pub state: AttachmentState,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct BridgeStatus {
    pub integration: String,
    pub file: PathBuf,
    pub state: AttachmentState,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ContextReconciliationReport {
    pub agents_md: PathBuf,
    pub packages: Vec<PackageInstructionStatus>,
    /// Regions this pass removed because no currently-installed package
    /// claims them any more. See `text_region::remove_unconditionally` for
    /// the exact (structural, not content-drift-verified) safety guarantee
    /// this carries.
    pub removed_orphans: Vec<String>,
    /// An orphaned-looking region this pass found but refused to touch —
    /// its markers were malformed, so ownership could not be proven.
    pub blocked_orphans: Vec<(String, String)>,
    pub worktree_region: Option<WorktreeRegionStatus>,
    pub bridges: Vec<BridgeStatus>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PackageManagedState {
    pub plugin: String,
    pub state: ManagedStateSummary,
    /// One row per canonical hook group × harness — the doctor's Hook
    /// attachment report (ADR-033): semantic event, compatibility verdict,
    /// the exact guarantee weakened on a degraded/unsupported route, and
    /// the receipt-owned artifact and its state when attached.
    pub hooks: Vec<HookHealth>,
}

/// Per-(hook group, harness) diagnostic row in the doctor report. `weakened`
/// is `Some` exactly when the route is Degraded/Unsupported — a semantic
/// loss is always stated, never hidden. `artifact`/`state` are `Some` only
/// for an attached hook (a degraded hook attaches nothing, honestly).
#[derive(Clone, Debug, Serialize)]
pub struct HookHealth {
    /// The canonical hook group id (`<package>:<group>` in receipts).
    pub hook: String,
    /// The semantic event this group listens to (abi name, e.g.
    /// `pre_tool_use`).
    pub event: String,
    pub harness: String,
    pub route: CompatibilityRoute,
    pub weakened: Option<String>,
    /// Something about how the hook is delivered that a person should know:
    /// the packager runtime carrying it instead of a generated wrapper, or
    /// a wrapper whose system dependency is not installed. `None` when the
    /// delivery is native and everything it needs is present.
    pub delivery: Option<String>,
    pub artifact: Option<PathBuf>,
    pub state: Option<AttachmentState>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DoctorReport {
    pub uze_home: PathBuf,
    pub store: StoreHealth,
    pub plugins: Vec<PluginSummary>,
    pub harnesses: Vec<HarnessHealth>,
    pub attachments: Vec<PackageManagedState>,
    pub ledger_error: Option<String>,
    pub integration_state_error: Option<String>,
    pub provisioning_state_error: Option<String>,
    pub maintenance: MaintenanceReport,
}

/// The friendliest name available for a resource in a `PluginCapability`
/// read model: a Skill's own directory name or a named MCP server's name
/// (`Resource::logical_capability_name`) when one exists, falling back to
/// `Resource::name` (typically a bare file name like `SKILL.md`) otherwise.
/// Display-only — never used for exposure naming, which stays entirely
/// `IntegrationPort::exposure_name_candidates`'s decision.
pub(crate) fn capability_display_name(resource: &uze_core::Resource) -> String {
    resource
        .logical_capability_name()
        .unwrap_or_else(|| resource.name())
}

pub(crate) fn managed_state(report: &ReconciliationReport) -> ManagedStateSummary {
    let mut summary = ManagedStateSummary {
        ledger_error: report.ledger_error.clone(),
        ..ManagedStateSummary::default()
    };
    for receipt in &report.receipts {
        match receipt.inspection.state {
            AttachmentState::Matched => summary.matched += 1,
            AttachmentState::Missing => summary.missing += 1,
            AttachmentState::Drifted => summary.drifted += 1,
            AttachmentState::Conflict => summary.conflicts += 1,
            AttachmentState::Blocked => summary.blocked += 1,
        }
    }
    summary
}

pub(crate) fn integration_status(status: IntegrationStatus) -> String {
    match status {
        IntegrationStatus::NotConfigured => "not configured",
        IntegrationStatus::InstalledUnverified => "installed / unverified",
        IntegrationStatus::InstalledVerified => "installed / verified",
    }
    .to_owned()
}

/// Idempotently points `link` at `target`, refusing to overwrite anything
/// at `link` that is not already a UZE-created symlink to something else —
/// the same conflict-safety shape `ClaudeIntegration`'s own skill symlink
/// helper uses.
#[cfg(unix)]
pub(crate) fn refresh_shim_symlink(target: &Path, link: &Path) -> Result<()> {
    match fs::symlink_metadata(link) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let current = fs::read_link(link).map_err(|source| UzeError::Read {
                path: link.to_path_buf(),
                source,
            })?;
            if current == target {
                return Ok(());
            }
            fs::remove_file(link).map_err(|source| UzeError::Write {
                path: link.to_path_buf(),
                source,
            })?;
        }
        Ok(_) => return Err(UzeError::ManagedEntryConflict(link.to_path_buf())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(UzeError::Read {
                path: link.to_path_buf(),
                source: error,
            });
        }
    }
    std::os::unix::fs::symlink(target, link).map_err(|source| UzeError::Write {
        path: link.to_path_buf(),
        source,
    })
}

#[cfg(not(unix))]
pub(crate) fn refresh_shim_symlink(_target: &Path, link: &Path) -> Result<()> {
    Err(UzeError::UnsupportedRuntimeProjection(link.to_path_buf()))
}

pub(crate) fn package_receipt_key(package: &str, integration: &str) -> String {
    format!("{package}:{integration}:package")
}

pub(crate) fn resource_receipt_key(
    package: &str,
    integration: &str,
    resource: &uze_core::Resource,
) -> String {
    format!("{package}:{integration}:{}", resource.identity())
}

pub(crate) fn package_store_inconsistency(package: &StoredPackage) -> Option<String> {
    if !package.root.is_dir() {
        return Some(format!(
            "package `{}` store directory is missing",
            package.id.as_str()
        ));
    }
    if !package.manifest.is_file() {
        return Some(format!(
            "package `{}` plugin.json is missing",
            package.id.as_str()
        ));
    }
    None
}

/// What one plugin's automatic update attempt did, from
/// [`UzeApplication::auto_update_plugins`].
#[derive(Clone, Debug, Serialize)]
pub struct AutoUpdateOutcome {
    pub plugin: String,
    /// `true` only when the new revision is installed and re-attached.
    pub applied: bool,
    /// Why the pending update was left for a person to apply — trust it
    /// cannot grant on their behalf, managed state it refused to disturb,
    /// or a failure. `None` when `applied`.
    pub detail: Option<String>,
}
