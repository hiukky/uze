//! The product-facing read models `UzeApplication` hands to the CLI and the
//! TUI, and the queries that build them.
//!
//! These types are the crate's public vocabulary: re-exported from
//! `application` for `src/` to name.

use uze_core::{
    Result,
    integration::{ContextDelivery, IntegrationPort},
    provisioning::ProvisioningResult,
};

use super::services::Plugins;
use super::*;

impl Plugins<'_> {
    #[tracing::instrument(name = "plugins.list", skip_all, err)]
    pub fn list(&self) -> Result<Vec<PluginSummary>> {
        self.0
            .store
            .package_ids()?
            .into_iter()
            .map(|id| self.0.plugin_summary(&self.0.store.package(&id)?))
            .collect()
    }

    #[tracing::instrument(name = "plugins.inspect", skip_all, fields(id = %id), err)]
    pub fn inspect(&self, id: &str) -> Result<PluginInspection> {
        let package = self.0.package_by_name(id)?;
        let resources = uze_core::engine::package_resources(&package)?;
        let resources: Vec<_> = resources.iter().collect();
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
        let mut plugin = self.0.plugin_summary(&package)?;
        // The listing says *whether* something newer exists, because that is
        // all it can afford. Here the distance is worth one more Git read —
        // this view is already spawning one to describe the revision.
        if plugin.freshness.state == (FreshnessState::Behind { commits: None }) {
            plugin.freshness.state = FreshnessState::Behind {
                commits: self.0.commits_behind(&package),
            };
        }
        Ok(PluginInspection {
            revision: self.0.installed_revision(&package),
            plugin,
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
    /// Whether the one installed is the one that exists, and when that was
    /// last established.
    pub freshness: Freshness,
}

/// What UZE can say about whether an installed package is current.
///
/// Every surface reports one of these, and `NotChecked` must never be drawn
/// the way `UpToDate` is: "we have not looked" and "we looked and it is
/// current" are different facts, and collapsing them is what made
/// "Installed" mean both.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Freshness {
    pub state: FreshnessState,
    /// When the comparison behind `state` was made — for a marketplace,
    /// when its mirror was last brought up to date. `None` when nothing was
    /// compared, which is the only honest answer for `Unpinned` and
    /// `NotChecked`.
    pub established_at_unix: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FreshnessState {
    /// The installed revision is the one the marketplace's declared ref
    /// points at.
    UpToDate,
    /// A newer revision exists. `commits` is how many, when the history to
    /// count them is on this machine; `None` when it is not — a snapshot
    /// compared by content rather than by history, or a mirror whose
    /// history was rewritten under the pin. A distance that might be wrong
    /// is worse than no distance, and "there is something newer" is true
    /// either way.
    Behind { commits: Option<usize> },
    /// The marketplace is a checkout this machine develops: its working
    /// tree is what exists, so "newer" means nothing.
    Linked { checkout: PathBuf },
    /// Nothing to compare against — a package installed straight from a
    /// path or a URL, which belongs to no marketplace catalogue. Distinct
    /// from `NotChecked`: there is no question to answer, rather than an
    /// answer UZE does not have.
    Unpinned,
    /// UZE has not established it, or tried and could not. Never a guess.
    NotChecked,
}

impl Freshness {
    pub fn not_checked() -> Self {
        Self {
            state: FreshnessState::NotChecked,
            established_at_unix: None,
        }
    }

    pub fn unpinned() -> Self {
        Self {
            state: FreshnessState::Unpinned,
            established_at_unix: None,
        }
    }

    /// Whether a newer revision exists — the one question every caller
    /// asking "is there an update" is really asking, answered without any
    /// of them re-deriving it from the state.
    pub fn behind(&self) -> bool {
        matches!(self.state, FreshnessState::Behind { .. })
    }
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
    /// Where this marketplace lives for a person: the manifest's
    /// `owner.url`, or its registered source when that is already a URL.
    /// `None` for one registered from a local path — there is nothing to
    /// open, and inventing a link is worse than admitting there is none.
    pub homepage: Option<String>,
    pub plugin_count: usize,
    /// The checkout this machine reads it from, when its operator
    /// develops it. Said out loud because it changes what every answer
    /// about this marketplace means: its plugins follow a working tree,
    /// and nothing pins from it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linked_to: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize)]
pub struct MarketplacePluginSummary {
    /// Which registered marketplace this plugin came from (`uze-official`
    /// for the embedded snapshot, or the name it was registered under via
    /// `marketplace add`). Needed once more than one marketplace can
    /// contribute plugins to the same list — see `Marketplace::plugins`.
    pub marketplace: String,
    pub name: String,
    pub description: Option<String>,
    pub keywords: Vec<String>,
    pub installed: bool,
    /// The installed package's freshness. `NotChecked` when the plugin is
    /// not installed at all: there is nothing of it here to be current.
    pub freshness: Freshness,
    /// Whether `bootstrap::DEFAULT_PLUGIN_IDS` installs this plugin on a
    /// fresh `UZE_HOME` — product policy, not a marketplace fact.
    pub is_default: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct MarketplacePluginDetail {
    pub summary: MarketplacePluginSummary,
    pub capabilities: Vec<PluginCapability>,
    /// When this plugin was last written in the marketplace that offers
    /// it — what you would be installing, and how old it is.
    pub revision: Option<Revision>,
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

/// When a plugin was last written, as something a person can place in
/// time.
///
/// The freshness state says *whether* there is something newer; this says
/// how old the thing in front of you is. Answered for a plugin that is
/// merely on offer too — "should I install this, or is it abandoned" is
/// the same question asked one step earlier.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Revision {
    /// The last commit that touched this plugin's own directory, with
    /// Git's account of how long ago it landed and what it was about.
    ///
    /// Its own directory, never the marketplace's head: a repository
    /// carrying several plugins moves whenever any of them does, so its
    /// head says nothing about this one.
    Commit {
        short: String,
        age: String,
        subject: String,
    },
    /// A checkout on this machine. There is no revision to name: what is
    /// installed is whatever its author last saved.
    Checkout { path: PathBuf },
    /// Shipped inside the binary. It has no repository to ask, and the
    /// release it came with is the only date that is true about it.
    Bundled { version: String },
}

#[derive(Clone, Debug, Serialize)]
pub struct PluginInspection {
    /// `None` for a package whose bytes came from nowhere a revision can
    /// be read from — a direct install from a path or URL — or whose
    /// mirror no longer holds the commit it was installed at. Absent
    /// rather than guessed.
    pub revision: Option<Revision>,
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
pub struct BlockedCapability {
    pub integration: String,
    pub capability: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct AddPluginReport {
    pub plugin: PluginSummary,
    pub package_plans: Vec<(String, PackageExposurePlan)>,
    pub attachments: Vec<AttachmentSummary>,
    pub publications: Vec<PublicationOutcome>,
    /// Capabilities whose vendor-visible name is held by something UZE
    /// does not own. The package is installed and everything else was
    /// delivered; these are what a person has to settle.
    pub blocked: Vec<BlockedCapability>,
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
    /// Registrations `packages.json` carries that this UZE cannot read —
    /// one sentence each, already carrying its remedy. The Store still
    /// works: every readable package is installed, listed and removable.
    /// These entries simply answer to nothing until they are cleared, and
    /// saying so is what keeps a package that quietly vanished from looking
    /// like a package that was never installed.
    Quarantined(Vec<String>),
    Blocked(String),
}

impl std::fmt::Display for StoreHealth {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreHealth::Ready => formatter.write_str("ready"),
            StoreHealth::Quarantined(entries) => {
                write!(
                    formatter,
                    "ready, with {} registration(s) that could not be read\n    {}",
                    entries.len(),
                    entries.join("\n    ")
                )
            }
            StoreHealth::Blocked(reason) => write!(formatter, "blocked: {reason}"),
        }
    }
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
    /// How this harness can receive a project's portable context on this
    /// machine — declared by the integration, like `capabilities` above,
    /// never resolved against a project. Whether one particular directory
    /// is actually being delivered is `AgentContextStatus`'s question.
    pub context_support: HarnessContextSupport,
}

/// The mechanism through which one portable project resource (`AGENTS.md`,
/// `.agents/`) reaches a harness on this machine. A property of the harness
/// and of this environment's `PATH` — never of any project, which is why
/// there is no "absent" variant: a machine-level row has nothing to be
/// absent from.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContextMechanism {
    /// The harness's own binary reads it straight out of the project.
    Native,
    /// UZE's runtime PATH shim projects it into every launch, without
    /// writing anything into the project.
    RuntimeShim,
    /// The shim would project it, but a real binary resolves ahead of the
    /// shim on this process's `PATH`, so a launch from here bypasses it.
    ShimShadowed,
    /// A persistent bridge file in the project root, maintained by
    /// `uze agent context reconcile`, carries it.
    Bridge,
    /// No delivery strategy exists for this harness and this resource.
    Unsupported,
}

/// One harness's declared context support, one mechanism per portable
/// resource — the two are answered independently because a harness may
/// discover `.agents/` natively while needing help with `AGENTS.md`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct HarnessContextSupport {
    pub instructions: ContextMechanism,
    pub agents_directory: ContextMechanism,
}

impl HarnessContextSupport {
    /// Derives the declaration from the integration's own answers plus the
    /// one environment fact that can defeat them (`runtime_shim_active`).
    pub fn declared(integration: &dyn IntegrationPort, runtime_shim_active: bool) -> Self {
        let projection = RuntimeProjection::of(integration, runtime_shim_active);
        Self {
            instructions: ContextMechanism::for_instructions(integration, projection),
            agents_directory: ContextMechanism::for_agents_directory(integration, projection),
        }
    }
}

/// Whether UZE's runtime shim carries project context into a launch of one
/// harness.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RuntimeProjection {
    /// Nothing is projected: the harness has no runtime projection of
    /// project context, or it has nothing to project here.
    Inactive,
    /// A launch goes through the shim, which projects the context.
    Active,
    /// The harness would be projected, but a real binary resolves ahead of
    /// the shim on this process's `PATH`.
    Shadowed,
}

impl RuntimeProjection {
    pub(crate) fn of(integration: &dyn IntegrationPort, runtime_shim_active: bool) -> Self {
        if !(integration.supports_runtime_integration()
            && integration.runtime_projects_project_context())
        {
            Self::Inactive
        } else if runtime_shim_active {
            Self::Active
        } else {
            Self::Shadowed
        }
    }
}

/// The one precedence every context read model applies: what the harness
/// reads itself, then a runtime projection, then a persistent bridge.
impl ContextMechanism {
    pub(crate) fn for_instructions(
        integration: &dyn IntegrationPort,
        projection: RuntimeProjection,
    ) -> Self {
        match (integration.context_delivery(), projection) {
            (ContextDelivery::Native { .. }, _) => Self::Native,
            (ContextDelivery::None, _) => Self::Unsupported,
            (ContextDelivery::Bridge { .. }, RuntimeProjection::Active) => Self::RuntimeShim,
            (ContextDelivery::Bridge { .. }, RuntimeProjection::Shadowed) => Self::ShimShadowed,
            (ContextDelivery::Bridge { .. }, RuntimeProjection::Inactive) => Self::Bridge,
        }
    }

    pub(crate) fn for_agents_directory(
        integration: &dyn IntegrationPort,
        projection: RuntimeProjection,
    ) -> Self {
        if integration.discovers_project_agents_directory() {
            return Self::Native;
        }
        match projection {
            RuntimeProjection::Active => Self::RuntimeShim,
            RuntimeProjection::Shadowed => Self::ShimShadowed,
            RuntimeProjection::Inactive => Self::Unsupported,
        }
    }
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
    /// UZE's runtime shim projects `AGENTS.md` into every launch from here,
    /// so a missing bridge is not a gap.
    Projected,
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
/// larger taxonomy. See `docs/capabilities/context-manager.md` ("Portability")
/// for why each variant exists and what evidence justified it.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "portability", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Portability {
    /// No recognized instructions file exists at all.
    NoContext,
    /// A shared `AGENTS.md` exists and every detected harness that needs
    /// something from it currently has it (natively, through the runtime
    /// shim, or via a matched bridge).
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
/// pending change would make `agent context plan` claim work that `reconcile`
/// will refuse to do.
fn is_mutating(action: &instruction_context::PlannedAction) -> bool {
    matches!(
        action,
        instruction_context::PlannedAction::Attach | instruction_context::PlannedAction::Remove
    )
}

/// The project's shared instruction file, as `uze status` carries it.
/// Deliberately three facts and not the source's full observation: a
/// person needs to know which document this is, whether it is there, and
/// how much of it UZE owns — the regions themselves are
/// `agent context inspect`'s answer.
#[derive(Clone, Debug, Serialize)]
pub struct InstructionsFile {
    pub path: PathBuf,
    pub exists: bool,
    /// How many managed regions UZE owns in it: each installed package's
    /// contribution, plus the worktree policy when one is declared.
    pub managed_regions: usize,
}

/// A project-scoped health summary. See `UzeApplication::status` for why
/// this is deliberately not folded into `doctor`.
#[derive(Clone, Debug, Serialize)]
pub struct StatusReport {
    pub root: PathBuf,
    /// The one file every harness's context is read from, and whether this
    /// project has written it yet. Named here because `status` is where a
    /// person asks whether the project is ready, and an answer about
    /// context that never says which document it is about cannot be acted
    /// on.
    pub instructions: InstructionsFile,
    pub portability: Portability,
    pub harnesses: Vec<HarnessContextStatus>,
    pub packages_installed: usize,
    pub packages_contributing_here: usize,
    pub project_lock: ProjectLockStatus,
    /// What `agents.yaml` asks for that the rest of the chain has not
    /// caught up to. Empty when the declaration, the lock, the Store and
    /// the projection all agree.
    pub drift: EnvironmentDrift,
    /// Human-readable, one-line-each context problems: a non-matched
    /// contribution, a bridge gap, a malformed or blocked orphan region.
    /// Empty means healthy. Never a substitute for the full detail of
    /// `agent context inspect` — this is the "does anything need my
    /// attention" view.
    pub issues: Vec<String>,
}

/// Drift along the chain a project's environment passes through:
/// declared -> locked -> installed -> delivered.
///
/// Every field is a normal state, never an error: a project mid-edit is a
/// project someone is working on. What makes drift worth reporting is that
/// nothing else names it, so a person editing `agents.yaml` gets no signal
/// that anything is owed.
#[derive(Clone, Debug, Default, Serialize)]
pub struct EnvironmentDrift {
    /// Declared in the manifest, and the lock does not answer for it.
    pub unresolved: Vec<String>,
    /// In the lock, and the manifest no longer declares it.
    pub surplus: Vec<String>,
    /// Recorded in the lock and absent from this machine's Store.
    pub missing: Vec<String>,
    /// The projected instruction region is behind the declared policy.
    pub stale_projection: bool,
    /// Marketplaces that resolve nowhere but the machine that declared
    /// them. Carried here so `uze status` and the overview say the same
    /// thing about it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unreproducible_marketplaces: Vec<String>,
}

/// The plan's answer, as `uze status` and the overview carry it: both read
/// the one plan, so the two surfaces cannot disagree about what is owed.
impl From<&ProjectEnvironmentPlan> for EnvironmentDrift {
    fn from(plan: &ProjectEnvironmentPlan) -> Self {
        Self {
            unresolved: plan.unresolved.clone(),
            surplus: plan.surplus.clone(),
            missing: plan.missing.clone(),
            unreproducible_marketplaces: plan.unreproducible_marketplaces.clone(),
            stale_projection: plan.stale_projection.is_some(),
        }
    }
}

impl EnvironmentDrift {
    pub fn is_clear(&self) -> bool {
        self.unresolved.is_empty()
            && self.surplus.is_empty()
            && self.missing.is_empty()
            && !self.stale_projection
    }
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
    /// A package whose region this pass could not write, with the reason —
    /// distinct from a region that is merely absent.
    pub failed: Vec<(String, String)>,
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

/// Records a previous version wrote that this one could not read, kept
/// where they were.
///
/// Reported because nothing reads them again: without somewhere to say so
/// they accumulate in silence, and the operator's first sign that anything
/// happened is work they cannot find.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct UpgradeLeftovers {
    /// The newest few, which are the ones an operator can still act on.
    pub set_aside: Vec<SetAsideRecord>,
    /// References into `$UZE_HOME` that resolve to nothing and that no
    /// receipt claims — `uze doctor` removes these, unlike `set_aside`,
    /// whose bytes only a person can judge.
    pub dangling: Vec<DanglingReferenceRecord>,
    /// How many there are in all, including the ones not listed: a report
    /// that names forty is one nobody reads.
    pub total: usize,
}

/// A reference UZE wrote into a harness's shared discovery root that
/// points into `$UZE_HOME` at something no longer there, and that no
/// receipt claims. Nothing reads it, and it holds a name another package
/// may need.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct DanglingReferenceRecord {
    pub path: PathBuf,
    pub target: PathBuf,
    pub remedy: &'static str,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SetAsideRecord {
    pub path: PathBuf,
    pub set_aside_at_unix: u64,
    /// What to do about it.
    pub remedy: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct DoctorReport {
    pub uze_home: PathBuf,
    pub store: StoreHealth,
    pub plugins: Vec<PluginSummary>,
    pub harnesses: Vec<HarnessHealth>,
    pub attachments: Vec<PackageManagedState>,
    pub ledger_error: Option<String>,
    pub provisioning_state_error: Option<String>,
    /// What a previous version left behind that this one did not adopt.
    pub leftovers: UpgradeLeftovers,
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

/// What one plugin's automatic update attempt did, from
/// [`Plugins::auto_update`].
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
