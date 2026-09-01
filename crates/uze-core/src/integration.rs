//! Harness-agnostic integration and managed-attachment contracts.

use std::{collections::BTreeMap, fs, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    capability::CapabilityKind,
    error::Result,
    exposure::{ExposureMechanism, ExposurePlan, McpEnvironmentReference, PackageExposurePlan},
    harness_runtime::{HarnessRuntimeContribution, RuntimeContext},
    home::UzeHome,
    project::EffectiveEnvironment,
    provisioning::ProvisionStatus,
    router::{HarnessCapabilities, RouteDecision, route},
    state,
    store::StoredPackage,
};

/// Stable receipt for one harness-owned side effect. The ledger persists this
/// intent; every destructive operation still asks the integration to inspect
/// the real harness state first.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AttachmentReceipt {
    pub package_id: String,
    pub resource_identity: Option<String>,
    pub integration: String,
    pub strategy: String,
    pub artifact: ManagedArtifact,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ManagedArtifact {
    SymlinkReference {
        path: PathBuf,
        target: PathBuf,
    },
    VendorConfigEntry {
        entry_name: String,
        transport: String,
        command: PathBuf,
        args: Vec<String>,
        cwd: Option<PathBuf>,
        environment: Vec<McpEnvironmentReference>,
        enabled: Option<bool>,
    },
    /// UZE owns a delimited region inside a shared text file, never the
    /// whole file. See `ExposureMechanism::ManagedTextRegion` and
    /// `crate::text_region`, which every safety rule for this variant lives
    /// in — this receipt shape carries no knowledge of what kind of file
    /// `target_file` is or why.
    ManagedTextRegion {
        target_file: PathBuf,
        region_identity: String,
        expected_content: String,
    },
    /// A delivery whose ownership proof only the owning integration can
    /// interpret. The Core routes it by `receipt.integration`, never reads
    /// `detail`, and refuses to inspect or detach it generically.
    ///
    /// `MARKETPLACE_PLUGIN` is accepted on read so a ledger written before
    /// this variant existed stays interpretable. Its `marketplace_root` and
    /// `package_root` land in `detail` through the flattened capture, and
    /// `kind` falls back to the name that artifact had. Reading a legacy
    /// receipt never rewrites it; only a genuinely new attachment writes,
    /// and a new write always emits this representation.
    #[serde(alias = "MARKETPLACE_PLUGIN")]
    IntegrationOwned {
        #[serde(default = "legacy_artifact_kind")]
        kind: String,
        selector: String,
        #[serde(flatten, default)]
        detail: BTreeMap<String, serde_json::Value>,
    },
    /// A UZE-namespaced entry inside the harness's shared hook
    /// configuration (ADR-033). `entry_name` is the stable UZE identity,
    /// `event` the manifest group's semantic event where the target shape
    /// is event-keyed, and `expected` the exact serialized entry content
    /// the owning integration wrote. Inspection and detach are
    /// integration-owned: the Core knows the identity, never the file's
    /// shape.
    HookConfigEntry {
        config_file: PathBuf,
        entry_name: String,
        event: Option<crate::hook::HookEvent>,
        expected: String,
    },
    /// A whole, UZE-owned derived file the harness loads from its own
    /// discovery directory (the OpenCode bridge): no configuration entry
    /// exists to merge, so `path` is the entire artifact. The owning
    /// integration interprets inspection and detach.
    ManagedHookFile {
        path: PathBuf,
    },
}

/// The `kind` a receipt predating [`ManagedArtifact::IntegrationOwned`]
/// implicitly had. Deliberately not a schema version: it is one default for
/// one superseded variant, not a migration framework.
fn legacy_artifact_kind() -> String {
    "marketplace-plugin".to_owned()
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AttachmentState {
    Matched,
    Missing,
    Drifted,
    Conflict,
    Blocked,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AttachmentInspection {
    pub state: AttachmentState,
    pub reason: String,
}

/// Read-only detection of a harness binary. No side effects.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct HarnessDetection {
    pub present: bool,
    pub version: Option<String>,
}

/// Diagnosable, machine-level integration status for `uze doctor`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IntegrationStatus {
    NotConfigured,
    InstalledUnverified,
    InstalledVerified,
}

/// Whether an integration's derived view of the installed package set is
/// currently in place. `NotApplicable` is the honest default: most
/// integrations publish no such view at all.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE", tag = "state", content = "reason")]
pub enum PublicationStatus {
    NotApplicable,
    Published,
    /// The package set is installed but the harness cannot see it. Actionable
    /// by re-running the publication, never by reinstalling the package.
    Unpublished(String),
}

/// How an integration's harness consumes a project's shared `AGENTS.md`
/// context — the delivery half of the Context Manager's per-harness model
/// (`context inspect|plan|reconcile`). The Core only defines the
/// vocabulary; which harness has which delivery is each integration's own
/// declaration, never the Application's.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextDelivery {
    /// Reads the shared `AGENTS.md` directly; UZE maintains no artifact and
    /// writes nothing for this harness. `files` names any *additional*
    /// native context files the harness reads (e.g. a hand-written vendor
    /// instructions file), observed for portability reporting only — never
    /// written by UZE.
    Native { files: &'static [&'static str] },
    /// Reaches the shared context only through a delimited bridge region
    /// (an `@AGENTS.md` import) inside the harness's own native file, which
    /// `context inspect|plan|reconcile` maintain.
    Bridge { file_name: &'static str },
    /// No project-context delivery is modeled for this harness; `context`
    /// commands never report it.
    None,
}

pub trait IntegrationPort {
    fn id(&self) -> &'static str;

    /// The name a person recognizes — shown anywhere a harness is displayed
    /// to a human (`uze doctor`, the TUI). Defaults to `id()`; an
    /// integration overrides this when its stable id carries a disambiguator
    /// (`id()` and state/receipts must stay keyed on that id regardless —
    /// this is display-only and never used for lookup or matching).
    fn display_name(&self) -> &'static str {
        self.id()
    }

    /// A one-line, human-facing description (`uze doctor`, the TUI
    /// Harnesses list) — never hardcoded outside `uze-integrations`
    /// (`cli_and_tui_never_name_a_vendor_harness`/
    /// `application_never_names_a_vendor_harness` enforce this). Defaults
    /// to empty; an integration overrides it with its own short blurb.
    fn description(&self) -> &'static str {
        ""
    }

    fn capabilities(&self) -> HarnessCapabilities;

    /// The hook semantics this harness can preserve (ADR-033): the semantic
    /// events, effects, matcher translation, input transformation,
    /// ordering, and handler types an integration can honestly deliver.
    /// The Core composes per-resource compatibility from this declaration
    /// (`crate::hook::assess`) instead of overloading the coarse
    /// `HarnessCapabilities` kind sets, because a Hook route varies per
    /// event/effect axis. The vendor-neutral default is the empty profile
    /// — no Hook semantics preservable — which each integration overrides
    /// exactly as far as its observed contract reaches.
    fn hook_capabilities(&self) -> crate::hook::HookCapabilities {
        crate::hook::HookCapabilities::default()
    }

    /// This integration's opt-in contribution to a shim-mediated harness
    /// launch (`RUNTIME INFRASTRUCTURE`, see `harness_runtime`) — entirely
    /// separate from `exposure_plan`, which governs package/skill delivery.
    /// The default is a pure passthrough. Never fallible — see
    /// `HarnessRuntimeContribution`'s own documentation for why fail-open
    /// is structural here.
    fn runtime_contribution(&self, _ctx: &RuntimeContext) -> HarnessRuntimeContribution {
        HarnessRuntimeContribution::passthrough()
    }

    /// Read-only twin of [`Self::runtime_contribution`]: whether a
    /// shim-mediated launch for `ctx` would project anything, *without
    /// performing the contribution itself*. Status views (the agent-support
    /// popup) must use this so rendering a row never mutates state; the
    /// default mirrors the real contribution, which is correct for
    /// integrations whose contribution is cheap and side-effect-free, and
    /// is overridden with a pure predicate by integrations whose
    /// contribution performs writes.
    fn runtime_contribution_would_activate(&self, ctx: &RuntimeContext) -> bool {
        !self.runtime_contribution(ctx).is_passthrough()
    }

    /// Whether `uze setup <harness>` should also create the PATH shim
    /// (`UzeHome::shims_dir`) for this harness, as an ordinary part of that
    /// one command — no separate flag or persisted enabled/disabled state.
    /// The shim symlink's own presence is the entire "is this on" answer:
    /// removing it is how one turns it back off. Default `true`: every
    /// registered harness receives the same transparent, generic launch
    /// boundary after explicit setup. Integrations only override
    /// `runtime_contribution` when they have extra runtime behavior.
    fn supports_runtime_integration(&self) -> bool {
        true
    }

    /// Alternate names the real binary behind this harness's shim may be
    /// installed under, tried in order after the shim's own invoked name
    /// when resolving which executable to `exec`. Default: none — the
    /// invoked name is the only candidate. Exists for a harness whose
    /// installer names the binary differently from the name users type
    /// (e.g. OpenCode's v2 installer produces `opencode2`, not `opencode`):
    /// the shim can dispatch straight to the real name without a physical
    /// alias file ever being created outside `$UZE_HOME`.
    fn runtime_executable_aliases(&self) -> &'static [&'static str] {
        &[]
    }

    /// The physical name this harness's PATH shim symlink is created under
    /// (`shims_dir/<shim_name>`) — the name a user actually types. Defaults
    /// to the first alias, else the id. Shared by `ensure_runtime_shim`
    /// (creation) and the shim's own invocation detection (dispatch).
    fn shim_name(&self) -> &'static str {
        self.aliases().first().copied().unwrap_or_else(|| self.id())
    }

    /// How this harness consumes a project's shared `AGENTS.md` context
    /// (see [`ContextDelivery`]). Drives `context inspect|plan|reconcile`;
    /// the default `None` keeps an integration that has not declared a
    /// delivery unreported rather than inheriting another harness's.
    fn context_delivery(&self) -> ContextDelivery {
        ContextDelivery::None
    }

    /// Whether this harness's own binary natively discovers Agent Skills
    /// from a project-local `.agents/skills/` directory, walking up from
    /// cwd, entirely on its own — a vendor convention some harnesses
    /// converged on independently, requiring no UZE involvement at all
    /// (distinct from any UZE-managed global `~/.agents/skills` delivery,
    /// which is a `CapabilityKind::AgentSkill` route, not this). Default
    /// `false`: an integration overrides this only against its own vendor's
    /// documented behavior.
    fn discovers_project_agents_directory(&self) -> bool {
        false
    }

    /// The prefix a human types to explicitly invoke an exposed capability
    /// on this harness (e.g. `/` for slash commands, `$` for Codex's
    /// explicit skill invocation, nothing for a bare-name harness).
    /// Presentation-only, rendered by docs/matrix tooling; never used for
    /// lookup or matching.
    fn invocation_prefix(&self) -> &'static str {
        ""
    }

    /// Public icon path (under the docs site's `public/`) for this harness's
    /// logo, or `None` where no distinct mark exists yet. Presentation-only,
    /// rendered by docs/matrix tooling; never used for lookup or matching.
    fn icon_path(&self) -> Option<&'static str> {
        None
    }

    /// The integration, not the resource representation, selects how the
    /// harness receives a capability from a composed UZE environment.
    fn exposure_plan(&self, resource: &crate::project::Resource) -> ExposurePlan;

    /// Ordered, harness-appropriate candidates for `resource`'s physical
    /// exposure name — most preferred first. This method only *proposes*:
    /// it reads no filesystem, reads no ledger, decides no ownership, and
    /// mutates nothing. Resolving a candidate against what's already
    /// claimed by other UZE-managed resources (via the ledger) and against
    /// what already exists on disk (via `attach`'s own structural checks)
    /// happens entirely outside this method — see
    /// `UzeApplication`'s naming resolution and `ExposureMechanism::attach`.
    ///
    /// The default suits every integration that has no naming policy of
    /// its own: one candidate, fully package-qualified, with **no**
    /// collision-avoidance prefix — the prefix was never part of ownership
    /// (see `AttachmentReceipt`'s doc comment) and this default drops it.
    /// An integration overrides this only when its own harness's UX
    /// genuinely depends on the physical name (Claude Code's decomposed
    /// Skill delivery is the one case today).
    fn exposure_name_candidates(&self, resource: &crate::project::Resource) -> Vec<String> {
        default_exposure_name_candidates(resource)
    }

    /// The physical directory this integration's harness reads Agent Skills
    /// from, when — and only when — that directory is durably shared with
    /// one or more *other* integrations rather than owned exclusively by
    /// this one. OpenCode and Codex both discover Skills from
    /// the same `~/.agents/skills` root, so naming resolution must treat a
    /// name already claimed there by any one of them as claimed for all;
    /// otherwise each independently-computed candidate list produces its
    /// own physical entry in what is, on disk, a single shared folder —
    /// visible to a harness (OpenCode's V2 slash commands) as duplicate
    /// listings of the identical skill. `None` is the correct default for
    /// every integration with an exclusive skills directory (Claude Code)
    /// or no Skill delivery at all: it opts out of this cross-integration
    /// awareness entirely, matching prior behavior.
    fn shared_agent_skill_root(&self) -> Option<std::path::PathBuf> {
        None
    }

    /// Optional preferred delivery of an external package as a whole. The
    /// returned plan owns only the listed resources; remaining resources are
    /// still routed capability-by-capability.
    fn package_exposure_plan(
        &self,
        _package: &StoredPackage,
        _resources: &[&crate::project::Resource],
    ) -> Option<PackageExposurePlan> {
        None
    }

    /// Detects whether the harness binary is present and, if cheaply
    /// obtainable, its version. Read-only; performs no filesystem writes.
    ///
    /// Callers on a path that should stay fast (nearly every command —
    /// see `specs/cli-performance/spec.md`) should prefer
    /// `UzeApplication::detect_cached`, which wraps this method in a
    /// cross-invocation cache (`detection_cache::DetectionCache`, ADR
    /// 018) instead of calling this directly and re-paying a live probe's
    /// cost on every command.
    fn detect(&self) -> HarnessDetection {
        HarnessDetection::default()
    }

    /// Program name(s) `detect()` may resolve to on `PATH`, most preferred
    /// first. Used only by `DetectionCache` to compute the cache's
    /// freshness fingerprint — never to decide presence itself, which
    /// remains `detect()`'s job alone. Defaults to `[id()]`, correct for
    /// every integration whose executable name matches its id; an
    /// integration whose id differs from its binary name (or that may
    /// resolve to more than one name) overrides this.
    fn detection_program_candidates(&self) -> Vec<&'static str> {
        vec![self.id()]
    }

    /// Explicitly provisions or updates the vendor executable through a
    /// route owned by this integration. The default is conservative: an
    /// integration that has not documented a route remains blocked rather
    /// than inheriting another harness's installer.
    fn provision(
        &self,
        _runner: &dyn crate::provisioning::ProcessRunner,
    ) -> Result<crate::provisioning::ProvisioningResult> {
        let detection = self.detect();
        if detection.present {
            return Ok(crate::provisioning::ProvisioningResult::verified(
                crate::provisioning::ProvisionAction::None,
                "existing-executable",
                detection,
            ));
        }
        Ok(crate::provisioning::ProvisioningResult::blocked(
            "this harness has no supported official provisioning route",
        ))
    }

    /// Idempotently ensures this integration's machine-level prerequisites
    /// exist (e.g. its user-scope discovery directory) and records setup
    /// state. Safe to call more than once; a second call refreshes recorded
    /// facts rather than duplicating state or artifacts.
    ///
    /// `detection` is the caller's already-obtained result (normally via
    /// `UzeApplication::detect_cached`) — an implementation records it
    /// (e.g. the version, into `state::IntegrationRecord`) rather than
    /// calling `detect()` again itself. `install` runs on nearly every
    /// command (see `specs/cli-performance/spec.md`), so a fresh,
    /// uncached probe here would silently reintroduce the exact cost this
    /// cache exists to remove.
    fn install(&self, home: &UzeHome, detection: &HarnessDetection) -> Result<()> {
        let _ = (home, detection);
        Ok(())
    }

    /// Current installed/managed status, for `uze doctor`. The default reads
    /// whatever `install` recorded through the shared `state` module for
    /// "configured at all", then upgrades that to `InstalledVerified` when
    /// the most recent `uze setup` provisioning attempt actually confirmed
    /// the binary works (`ProvisionStatus::Verified`, recorded separately by
    /// `provision_and_prepare` via `state::record_provisioning`) — without
    /// this, a harness whose setup was genuinely verified would read back as
    /// merely "unverified" forever, since `install`'s own record tracks
    /// nothing beyond a bare installed flag.
    fn status(&self, home: &UzeHome) -> IntegrationStatus {
        let Some(record) = state::get(home, self.id()).ok().flatten() else {
            return IntegrationStatus::NotConfigured;
        };
        if !record.installed {
            return IntegrationStatus::NotConfigured;
        }
        let verified = state::provisioning(home, self.id())
            .ok()
            .flatten()
            .is_some_and(|provisioning| provisioning.status == ProvisionStatus::Verified);
        if verified {
            IntegrationStatus::InstalledVerified
        } else {
            IntegrationStatus::InstalledUnverified
        }
    }

    /// Idempotently creates or refreshes this harness's managed attachment
    /// for one resource. `None` when the currently selected exposure
    /// mechanism does not support persistent attachment (e.g. setup has not
    /// completed yet and the integration is still on a conformance-probe
    /// fallback).
    fn attach(&self, resource: &crate::project::Resource) -> Result<Option<PathBuf>> {
        let plan = self.exposure_plan(resource);
        match &plan.mechanism {
            ExposureMechanism::ManagedUserScopeReference { .. } => {
                Ok(Some(plan.mechanism.attach()?))
            }
            ExposureMechanism::ManagedTextRegion { .. } => {
                Ok(Some(plan.mechanism.attach_text_region()?))
            }
            _ => Ok(None),
        }
    }

    /// Performs a package-level native delivery and returns its own ownership
    /// receipt. Deliberately one method: a native delivery may cover several
    /// resources and must not manufacture one receipt per capability, and
    /// only the integration can describe the artifact it just created. The
    /// Core supplies no default because it has no vocabulary for one.
    fn attach_package(
        &self,
        _package: &StoredPackage,
        _plan: &PackageExposurePlan,
    ) -> Result<Option<AttachmentReceipt>> {
        Ok(None)
    }

    /// Additional names `uze setup <harness>` accepts for this integration.
    /// Kept beside the integration so the Application never holds a manual
    /// catalogue of vendors.
    fn aliases(&self) -> &'static [&'static str] {
        &[]
    }

    /// Rebuilds any derived, harness-owned view of the installed package set
    /// that this integration maintains — for example a catalogue a harness
    /// reads to discover locally installable packages.
    ///
    /// **This is not part of package ownership.** The Store stays the sole
    /// authority for which packages exist; whatever this writes must be
    /// reconstructible from `packages` alone, must hold nothing that exists
    /// only there, and must be safe to delete and regenerate at any moment.
    /// No receipt is produced and nothing here is reconciled: a derived view
    /// cannot drift, it can only be stale, and staleness is repaired by
    /// calling this again.
    ///
    /// A failure here never invalidates an installation. The package stays
    /// installed and the integration reports itself unpublished through
    /// [`IntegrationPort::publication`].
    fn republish_packages(&self, packages: &[StoredPackage]) -> Result<()> {
        let _ = packages;
        Ok(())
    }

    /// Observed health of the derived view [`republish_packages`] maintains.
    ///
    /// Read at diagnosis time rather than recorded at write time, precisely
    /// because the view is derived: remembering its state would create the
    /// second source of truth the derivation exists to avoid.
    fn publication(&self, packages: &[StoredPackage]) -> PublicationStatus {
        let _ = packages;
        PublicationStatus::NotApplicable
    }

    /// Returns a typed ownership receipt after a successful resource attach.
    fn attach_receipt(
        &self,
        resource: &crate::project::Resource,
    ) -> Result<Option<AttachmentReceipt>> {
        let Some(location) = self.attach(resource)? else {
            return Ok(None);
        };
        let package_id = match &resource.origin {
            crate::project::ResourceOrigin::Package { id, .. } => id.as_str().to_owned(),
            crate::project::ResourceOrigin::Project { .. } => return Ok(None),
        };
        let plan = self.exposure_plan(resource);
        let strategy = format!("{:?}", plan.mechanism);
        let artifact = match plan.mechanism {
            ExposureMechanism::ManagedUserScopeReference { source, .. } => {
                ManagedArtifact::SymlinkReference {
                    path: location,
                    target: source,
                }
            }
            ExposureMechanism::ManagedVendorConfig {
                entry_name,
                transport,
                command,
                args,
                cwd,
                environment,
                enabled,
            } => ManagedArtifact::VendorConfigEntry {
                entry_name,
                transport,
                command,
                args,
                cwd,
                environment,
                enabled,
            },
            ExposureMechanism::ManagedTextRegion {
                target_file,
                region_identity,
                expected_content,
            } => ManagedArtifact::ManagedTextRegion {
                target_file,
                region_identity,
                expected_content,
            },
            ExposureMechanism::ManagedHookConfig {
                config_file,
                entry_name,
                event,
                expected,
            } => ManagedArtifact::HookConfigEntry {
                config_file,
                entry_name,
                event,
                expected,
            },
            ExposureMechanism::ManagedHookFile { path } => {
                ManagedArtifact::ManagedHookFile { path }
            }
            _ => return Ok(None),
        };
        Ok(Some(AttachmentReceipt {
            package_id,
            resource_identity: Some(resource.identity()),
            integration: self.id().to_owned(),
            strategy,
            artifact,
        }))
    }

    fn inspect_receipt(&self, receipt: &AttachmentReceipt) -> AttachmentInspection {
        inspect_standard_receipt(receipt)
    }

    fn detach_receipt(&self, receipt: &AttachmentReceipt) -> Result<AttachmentInspection> {
        detach_standard_receipt(receipt)
    }

    /// Restores a receipt that has been inspected as `Missing`, but only for
    /// standard artifacts whose full desired state is carried by the receipt
    /// itself. Integrations must opt in explicitly for every other artifact:
    /// a missing vendor registration is not proof that re-running a vendor
    /// command cannot replace user-owned state.
    ///
    /// Callers must inspect again after this method returns. A race that puts
    /// a foreign entry at the target is rejected by the normal attachment
    /// primitives rather than overwritten.
    fn repair_missing_receipt(&self, receipt: &AttachmentReceipt) -> Result<bool> {
        match &receipt.artifact {
            ManagedArtifact::SymlinkReference { path, target } => {
                let Some(parent) = path.parent() else {
                    return Ok(false);
                };
                let Some(entry_name) = path.file_name().and_then(|name| name.to_str()) else {
                    return Ok(false);
                };
                ExposureMechanism::ManagedUserScopeReference {
                    discovery_root: parent.to_path_buf(),
                    entry_name: entry_name.to_owned(),
                    source: target.clone(),
                }
                .attach()?;
                Ok(true)
            }
            ManagedArtifact::ManagedTextRegion {
                target_file,
                region_identity,
                expected_content,
            } => {
                ExposureMechanism::ManagedTextRegion {
                    target_file: target_file.clone(),
                    region_identity: region_identity.clone(),
                    expected_content: expected_content.clone(),
                }
                .attach_text_region()?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }
}

/// The naming default every integration inherits unless it overrides
/// `exposure_name_candidates`: one candidate, fully package-qualified,
/// with no collision-avoidance prefix. A free function (not inlined into
/// the trait default) so an integration that overrides the method for one
/// capability kind can still fall through to this exact same computation
/// for another (e.g. MCP, which deliberately stays on this policy while
/// Skills/Commands move to stable namespaced labels — capability naming
/// policies are never mixed just because all are `Resource`s).
pub fn default_exposure_name_candidates(resource: &crate::project::Resource) -> Vec<String> {
    let crate::project::ResourceOrigin::Package { id, .. } = &resource.origin else {
        return Vec::new();
    };
    let Some(logical) = resource.logical_capability_name() else {
        return Vec::new();
    };
    vec![format!("{}-{}", id.as_str(), logical)]
}

/// The stable, plugin-qualified invocation label (ADR-026):
/// `<plugin>:<capability>`. This is a **presentation** label — it never
/// replaces the canonical resource identity, the package layout, or the
/// capability body. `plugin` is the package's *active* local name
/// (ADR-038) — its own bare name unless an install-time alias resolved a
/// collision — never the marketplace-qualified identity. Deterministic and
/// independent of which other plugins are installed.
pub fn qualified_capability_name(active_plugin_name: &str, logical_name: &str) -> String {
    format!("{active_plugin_name}:{logical_name}")
}

/// Resolves the resource's package to the local invocation name it is
/// currently active under (`UzeStore::active_name_for`) — its own bare
/// plugin name unless an install-time alias resolved a collision with
/// another marketplace's same-named plugin (ADR-038). `None` for a
/// Project-origin resource, which has no package identity to resolve.
pub fn active_plugin_name(
    home: &crate::home::UzeHome,
    resource: &crate::project::Resource,
) -> Option<String> {
    let crate::project::ResourceOrigin::Package { id, .. } = &resource.origin else {
        return None;
    };
    Some(crate::store::UzeStore::new(home.clone()).active_name_for(id))
}

/// The single candidate for every UZE-projected Skill: its own stable
/// namespaced invocation label (`flow:review`), never a bare alias and
/// never a collision-dependent qualification (ADR-026). One candidate by
/// construction, so installation order and the presence of other plugins
/// cannot change it. Other capabilities (MCP) deliberately stay on
/// [`default_exposure_name_candidates`]. `active_plugin_name` is the
/// resolved local name from [`active_plugin_name()`] — callers with `&self`
/// access to a `UzeHome` resolve it once and pass it in, rather than this
/// pure label-formatting function doing its own state read.
pub fn qualified_exposure_name_candidates(
    resource: &crate::project::Resource,
    active_plugin_name: &str,
) -> Vec<String> {
    if !matches!(
        resource.origin,
        crate::project::ResourceOrigin::Package { .. }
    ) {
        return Vec::new();
    }
    if resource.capability.kind != CapabilityKind::AgentSkill {
        return Vec::new();
    }
    let Some(logical) = resource.logical_capability_name() else {
        return Vec::new();
    };
    vec![qualified_capability_name(active_plugin_name, &logical)]
}

/// Extracts the physical exposure name a receipt's artifact already claims,
/// generically — no artifact variant here knows or cares what kind of
/// capability it represents. `None` for an artifact shape with no single
/// physical name of its own (a text region spans a *portion* of a shared
/// file, not a dedicated entry; an integration-owned artifact's naming is
/// opaque to the Core by design).
pub fn managed_artifact_exposure_name(artifact: &ManagedArtifact) -> Option<String> {
    match artifact {
        ManagedArtifact::SymlinkReference { path, .. } => {
            path.file_name()?.to_str().map(str::to_owned)
        }
        ManagedArtifact::VendorConfigEntry { entry_name, .. } => Some(entry_name.clone()),
        ManagedArtifact::HookConfigEntry { entry_name, .. } => Some(entry_name.clone()),
        ManagedArtifact::ManagedHookFile { path } => path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned),
        // A text region spans a *portion* of a shared file, not a dedicated
        // entry; an integration-owned artifact's naming is opaque to the
        // Core by design.
        ManagedArtifact::ManagedTextRegion { .. } | ManagedArtifact::IntegrationOwned { .. } => {
            None
        }
    }
}

/// Shared inspection for artifacts whose ownership proof does not depend on a
/// harness schema. Vendor integrations call this explicitly rather than
/// redispatching through `IntegrationPort` from an override.
pub fn inspect_standard_receipt(receipt: &AttachmentReceipt) -> AttachmentInspection {
    match &receipt.artifact {
        ManagedArtifact::SymlinkReference { path, target } => match fs::read_link(path) {
            Ok(actual) if &actual == target => AttachmentInspection {
                state: AttachmentState::Matched,
                reason: "managed symlink target matches receipt".to_owned(),
            },
            Ok(_) => AttachmentInspection {
                state: AttachmentState::Drifted,
                reason: "symlink target differs from receipt".to_owned(),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => AttachmentInspection {
                state: AttachmentState::Missing,
                reason: "managed symlink is missing".to_owned(),
            },
            Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => {
                AttachmentInspection {
                    state: AttachmentState::Conflict,
                    reason: "managed path is occupied by a non-symlink".to_owned(),
                }
            }
            Err(error) => AttachmentInspection {
                state: AttachmentState::Blocked,
                reason: error.to_string(),
            },
        },
        ManagedArtifact::ManagedTextRegion {
            target_file,
            region_identity,
            expected_content,
        } => crate::text_region::inspect(target_file, region_identity, expected_content),
        _ => AttachmentInspection {
            state: AttachmentState::Blocked,
            reason: "integration must inspect this vendor artifact".to_owned(),
        },
    }
}

/// Removes only a currently matched standard artifact. Callers receive any
/// non-matched inspection unchanged, so drift never turns into a destructive
/// operation.
pub fn detach_standard_receipt(receipt: &AttachmentReceipt) -> Result<AttachmentInspection> {
    if let ManagedArtifact::ManagedTextRegion {
        target_file,
        region_identity,
        expected_content,
    } = &receipt.artifact
    {
        // `text_region::detach` already re-inspects immediately before its
        // own destructive write, per the same ADR-009 discipline the rest of
        // this function applies below for a symlink.
        return crate::text_region::detach(target_file, region_identity, expected_content);
    }
    let inspection = inspect_standard_receipt(receipt);
    if inspection.state != AttachmentState::Matched {
        return Ok(inspection);
    }
    // Re-read immediately before unlinking. This protects the normal
    // non-concurrent case where a user changes the reference after a prior
    // doctor/reconcile pass but before detach begins.
    let fresh = inspect_standard_receipt(receipt);
    if fresh.state != AttachmentState::Matched {
        return Ok(fresh);
    }
    if let ManagedArtifact::SymlinkReference { path, .. } = &receipt.artifact {
        fs::remove_file(path).map_err(|source| crate::UzeError::Write {
            path: path.clone(),
            source,
        })?;
    }
    Ok(AttachmentInspection {
        state: AttachmentState::Missing,
        reason: "managed artifact detached".to_owned(),
    })
}

/// A human-readable artifact locator for the existing CLI. The typed receipt
/// remains the source of truth; this intentionally loses no ownership data
/// because it is display-only.
pub fn receipt_location(receipt: &AttachmentReceipt) -> PathBuf {
    match &receipt.artifact {
        ManagedArtifact::SymlinkReference { path, .. } => path.clone(),
        ManagedArtifact::VendorConfigEntry { entry_name, .. } => {
            PathBuf::from(format!("mcp:{entry_name}"))
        }
        ManagedArtifact::HookConfigEntry {
            config_file,
            entry_name,
            ..
        } => PathBuf::from(format!("{}#{entry_name}", config_file.display())),
        ManagedArtifact::ManagedHookFile { path } => path.clone(),
        ManagedArtifact::ManagedTextRegion {
            target_file,
            region_identity,
            ..
        } => PathBuf::from(format!("{}#{region_identity}", target_file.display())),
        ManagedArtifact::IntegrationOwned { kind, selector, .. } => {
            PathBuf::from(format!("{kind}:{selector}"))
        }
    }
}

#[cfg(test)]
mod artifact_compatibility_tests {
    use super::*;

    /// A ledger written before `IntegrationOwned` existed must stay readable,
    /// because an unreadable receipt blocks removal — safe, but it strands the
    /// user with external state UZE can no longer identify.
    #[test]
    fn legacy_marketplace_receipt_still_deserializes() {
        let legacy = r#"{
            "package_id": "plugin-a",
            "resource_identity": null,
            "integration": "codex",
            "strategy": "native-plugin-marketplace",
            "artifact": {
                "MARKETPLACE_PLUGIN": {
                    "selector": "plugin-a@uze-local",
                    "marketplace_root": "/uze/store",
                    "package_root": "/uze/store/packages/plugin-a"
                }
            }
        }"#;
        let receipt: AttachmentReceipt =
            serde_json::from_str(legacy).expect("legacy receipt reads");
        let ManagedArtifact::IntegrationOwned {
            kind,
            selector,
            detail,
        } = &receipt.artifact
        else {
            panic!("legacy artifact did not map onto the integration-owned variant");
        };
        assert_eq!(kind, "marketplace-plugin");
        assert_eq!(selector, "plugin-a@uze-local");
        // The superseded fields survive as opaque detail, so the owning
        // integration can still prove ownership and detach safely.
        assert_eq!(detail["marketplace_root"], "/uze/store");
        assert_eq!(detail["package_root"], "/uze/store/packages/plugin-a");
    }

    /// Reading a legacy receipt must not silently rewrite the ledger, but any
    /// genuinely new write emits only the current representation.
    #[test]
    fn a_new_write_uses_only_the_current_representation() {
        let receipt = AttachmentReceipt {
            package_id: "plugin-a".to_owned(),
            resource_identity: None,
            integration: "codex".to_owned(),
            strategy: "native-plugin-marketplace".to_owned(),
            artifact: ManagedArtifact::IntegrationOwned {
                kind: "marketplace-plugin".to_owned(),
                selector: "plugin-a@uze-local".to_owned(),
                detail: BTreeMap::new(),
            },
        };
        let encoded = serde_json::to_string(&receipt).unwrap();
        assert!(encoded.contains("INTEGRATION_OWNED"));
        assert!(!encoded.contains("MARKETPLACE_PLUGIN"));
    }

    /// The Core routes an integration-owned artifact by `receipt.integration`
    /// and must never guess at its ownership itself.
    #[test]
    fn an_integration_owned_receipt_is_never_inspected_generically() {
        let receipt = AttachmentReceipt {
            package_id: "plugin-a".to_owned(),
            resource_identity: None,
            integration: "codex".to_owned(),
            strategy: "native-plugin-marketplace".to_owned(),
            artifact: ManagedArtifact::IntegrationOwned {
                kind: "marketplace-plugin".to_owned(),
                selector: "plugin-a@uze-local".to_owned(),
                detail: BTreeMap::new(),
            },
        };
        assert_eq!(
            inspect_standard_receipt(&receipt).state,
            AttachmentState::Blocked
        );
        // And a blocked inspection can never become a destructive operation.
        assert_eq!(
            detach_standard_receipt(&receipt).unwrap().state,
            AttachmentState::Blocked
        );
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    fn receipt(path: PathBuf, target: PathBuf) -> AttachmentReceipt {
        AttachmentReceipt {
            package_id: "plugin".to_owned(),
            resource_identity: Some("skill:example".to_owned()),
            integration: "test".to_owned(),
            strategy: "managed-user-scope-reference".to_owned(),
            artifact: ManagedArtifact::SymlinkReference { path, target },
        }
    }

    #[cfg(unix)]
    #[test]
    fn symlink_receipt_is_safe_only_when_ownership_still_matches() {
        use std::os::unix::fs::symlink;

        let root = uze_testkit::temp::scratch("symlink");
        fs::create_dir_all(&root).unwrap();
        let expected = root.join("expected");
        let other = root.join("other");
        fs::create_dir_all(&expected).unwrap();
        fs::create_dir_all(&other).unwrap();
        let path = root.join("managed");
        let receipt = receipt(path.clone(), expected.clone());

        assert_eq!(
            inspect_standard_receipt(&receipt).state,
            AttachmentState::Missing
        );
        symlink(&expected, &path).unwrap();
        assert_eq!(
            inspect_standard_receipt(&receipt).state,
            AttachmentState::Matched
        );
        assert_eq!(
            detach_standard_receipt(&receipt).unwrap().state,
            AttachmentState::Missing
        );
        assert!(!path.exists());

        symlink(&other, &path).unwrap();
        assert_eq!(
            inspect_standard_receipt(&receipt).state,
            AttachmentState::Drifted
        );
        assert_eq!(
            detach_standard_receipt(&receipt).unwrap().state,
            AttachmentState::Drifted
        );
        assert_eq!(fs::read_link(&path).unwrap(), other);

        fs::remove_file(&path).unwrap();
        fs::write(&path, "foreign").unwrap();
        assert_eq!(
            inspect_standard_receipt(&receipt).state,
            AttachmentState::Conflict
        );
        assert_eq!(
            detach_standard_receipt(&receipt).unwrap().state,
            AttachmentState::Conflict
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "foreign");
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_symlink_state_is_blocked() {
        use std::os::unix::fs::PermissionsExt;

        let root = uze_testkit::temp::scratch("unreadable");
        let locked = root.join("locked");
        fs::create_dir_all(&locked).unwrap();
        let receipt = receipt(locked.join("managed"), root.join("expected"));
        let mut permissions = fs::metadata(&locked).unwrap().permissions();
        permissions.set_mode(0o000);
        fs::set_permissions(&locked, permissions).unwrap();
        assert_eq!(
            inspect_standard_receipt(&receipt).state,
            AttachmentState::Blocked
        );
        let mut permissions = fs::metadata(&locked).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&locked, permissions).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}

#[derive(Clone, Debug)]
pub struct IntegrationAssessment {
    pub integration_id: String,
    pub capability_path: String,
    pub decision: RouteDecision,
    pub exposure_plan: ExposurePlan,
}

pub fn assess_environment(
    environment: &EffectiveEnvironment,
    integration: &dyn IntegrationPort,
) -> Vec<IntegrationAssessment> {
    let capabilities = integration.capabilities();
    environment
        .resources
        .iter()
        .map(|resource| {
            let exposure_plan = integration.exposure_plan(resource);
            let mut decision = route(&resource.capability, &capabilities);
            decision.route = exposure_plan.route;
            decision.verification = exposure_plan.verification.clone();
            decision.rationale.clone_from(&exposure_plan.evidence);
            decision.evidence.clone_from(&exposure_plan.evidence);
            IntegrationAssessment {
                integration_id: integration.id().to_owned(),
                capability_path: resource.display_path(&environment.root),
                decision,
                exposure_plan,
            }
        })
        .collect()
}

/// A cheap, vendor-neutral fingerprint of the filesystem surface an
/// attachment lives on — the freshness half of the inspection cache
/// (ADR 018), mirroring the detection cache's own fingerprint rule
/// (ADR 018).
///
/// Only artifacts with a directly stat-able presence produce one:
///
/// - `SymlinkReference` → the managed link's own state (its mtime and the
///   path it currently points at), always `Some` — a missing link is a
///   real, checkable state (`"absent"`), not the absence of a fingerprint;
/// - everything else (vendor config entries, integration-owned native
///   catalogues, text regions) → `None`, because the state lives inside
///   vendor files whose locations this layer deliberately does not know;
///   those verdicts are bounded by TTL + mutation invalidation alone.
pub fn managed_artifact_fingerprint(artifact: &ManagedArtifact) -> Option<String> {
    if let ManagedArtifact::SymlinkReference { path, target } = artifact {
        let state = fs::symlink_metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos())
            .unwrap_or(u128::MAX); // absent/untimed: a state, never "no info"
        let link = fs::read_link(path)
            .map(|resolved| resolved.to_string_lossy().into_owned())
            .unwrap_or_default();
        return Some(format!("{state}:{link}:{}", target.display()));
    }
    None
}

/// Whether a managed artifact is still physically in place. The cheap,
/// vendor-neutral half of "is this attachment still effective": only a
/// directly stat-able artifact can answer without touching vendor state
/// (`SymlinkReference` — the link must exist and still point where the
/// receipt says). Everything else is the owning integration's verdict, so
/// the answer is `false` here and callers fall back to receipt existence.
pub fn managed_artifact_present(artifact: &ManagedArtifact) -> bool {
    if let ManagedArtifact::SymlinkReference { path, target } = artifact {
        return fs::read_link(path).is_ok_and(|resolved| resolved == *target);
    }
    false
}
