//! Harness-agnostic integration and managed-attachment contracts.

use std::{ffi::OsString, path::Path};

use serde::{Deserialize, Serialize};

use crate::{
    capability::CapabilityKind,
    conversation::SessionId,
    error::Result,
    exposure::{ExposureMechanism, ExposurePlan, PackageExposurePlan},
    harness_runtime::{HarnessRuntimeContribution, RuntimeContext},
    home::UzeHome,
    provisioning::ProvisionStatus,
    router::HarnessCapabilities,
    state,
    store::StoredPackage,
};

pub use crate::exposure::ManagedArtifact;

/// Stable receipt for one harness-owned side effect. The ledger persists this
/// intent; every destructive operation still asks the integration to inspect
/// the real harness state first.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AttachmentReceipt {
    pub package_id: String,
    pub resource_identity: Option<String>,
    pub integration: String,
    pub artifact: ManagedArtifact,
}

impl AttachmentReceipt {
    /// A name for this attachment, for something that has to key on one.
    ///
    /// Derived rather than stored. The ledger used to key its receipts by
    /// this very string, which made it a second copy of three fields — and
    /// an unsplittable one, since a resource identity carries colons of its
    /// own. As a cache key none of that matters: nothing parses it, and it
    /// only has to differ when the attachment does.
    pub fn cache_key(&self) -> String {
        match &self.resource_identity {
            Some(identity) => format!("{}:{}:{identity}", self.package_id, self.integration),
            None => format!("{}:{}:package", self.package_id, self.integration),
        }
    }
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
/// (`agent context inspect|plan|reconcile`). The Core only defines the
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
    /// `agent context inspect|plan|reconcile` maintain.
    Bridge { file_name: &'static str },
    /// No project-context delivery is modeled for this harness; `context`
    /// commands never report it.
    None,
}

/// How a harness lets a conversation be picked up again.
///
/// Declared, never assumed: a harness UZE has no mechanism for says so, and
/// its agents start fresh with the reason stated, rather than quietly
/// looking like they carried something over.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionContinuity {
    /// UZE names the conversation at launch and resumes by the same name.
    /// The identifier is known before the process starts, so nothing has to
    /// be read back and no vendor record is ever parsed.
    Assigned,
    /// The harness names its own conversation; UZE reads the name back from
    /// that harness's records afterwards.
    Observed,
    /// No mechanism UZE can drive.
    Unsupported,
}

/// What a read-back is allowed to accept, gathered at the launch it answers
/// for.
pub struct ObservationContext<'a> {
    /// The checkout the agent was launched in — the only directory whose
    /// conversations belong to this task.
    pub cwd: &'a Path,
    /// Nothing older than this launch is a candidate.
    pub since_unix: u64,
    /// What this harness's records already pointed at for `cwd` when the
    /// launch started, for a harness that keeps one entry per directory
    /// rather than timestamping conversations: reading that same value back
    /// means nothing new was started.
    pub preceded_by: Option<&'a SessionId>,
}

pub trait IntegrationPort: Send + Sync {
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

    /// How this harness lets an agent's conversation be picked up again.
    /// The vendor-neutral default is that it does not: an integration that
    /// says nothing here contributes no argument and reports no
    /// conversation, which is what makes a fifth harness safe before anyone
    /// has looked into its mechanism.
    fn session_continuity(&self) -> SessionContinuity {
        SessionContinuity::Unsupported
    }

    /// Arguments that start a conversation UZE has named. Only
    /// [`SessionContinuity::Assigned`] answers this.
    fn start_session_args(&self, _session: &SessionId) -> Vec<OsString> {
        Vec::new()
    }

    /// Arguments that continue a conversation already recorded.
    fn resume_session_args(&self, _session: &SessionId) -> Vec<OsString> {
        Vec::new()
    }

    /// What this harness's own records already point at for `cwd`, read at
    /// launch so a later read-back can tell a new conversation from the one
    /// that was already there. `None` where the launch time is guard enough.
    fn session_recorded_for(&self, _cwd: &Path) -> Option<SessionId> {
        None
    }

    /// The conversation this harness started for `ctx`, read from its own
    /// records. Only [`SessionContinuity::Observed`] answers this, and
    /// `None` — nothing started yet, records unreadable, nothing new since
    /// the launch — is always a valid answer: the caller waits rather than
    /// guesses.
    fn observe_session(&self, _ctx: &ObservationContext) -> Option<SessionId> {
        None
    }

    /// Whether the harness still holds `session`, asked before resuming
    /// into it. The default is `true`: `exec` cannot retry, so a harness
    /// that cannot answer cheaply is taken at its word, and a resume that
    /// then fails is the harness's own error rather than a pane UZE killed
    /// by guessing.
    fn session_exists(&self, _session: &SessionId, _cwd: &Path) -> bool {
        true
    }

    /// Whether this harness's [`Self::runtime_contribution`] is the
    /// mechanism carrying a project's portable context (`AGENTS.md`,
    /// `.agents/`) into a launch. A declaration about the harness, not an
    /// observation about any directory — the machine-level answer to "how
    /// would this harness receive project context", where
    /// [`Self::runtime_contribution_would_activate`] answers it for one
    /// cwd. Default `false`: the passthrough contribution projects nothing.
    fn runtime_projects_project_context(&self) -> bool {
        false
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
    /// (see [`ContextDelivery`]). Drives `agent context inspect|plan|reconcile`;
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

    /// The vendor's own page for this harness — where a reader who has just
    /// met the name goes to find out what it is. `None` where the harness
    /// has no public home. Presentation-only, rendered by docs/matrix
    /// tooling; never fetched, and never used for lookup or matching.
    fn homepage(&self) -> Option<&'static str> {
        None
    }

    /// The integration, not the resource representation, selects how the
    /// harness receives a capability from a composed UZE environment.
    fn exposure_plan(&self, resource: &crate::capability::Resource) -> ExposurePlan;

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
    fn exposure_name_candidates(&self, resource: &crate::capability::Resource) -> Vec<String> {
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
        _resources: &[&crate::capability::Resource],
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
    /// merely "unverified" forever, since `install`'s own record says only
    /// that it ran.
    fn status(&self, home: &UzeHome) -> IntegrationStatus {
        if !state::is_installed(home, self.id()) {
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
    /// for one resource and returns the artifact it now owns. `None` when
    /// the plan has nothing to attach (e.g. setup has not completed).
    fn attach(&self, resource: &crate::capability::Resource) -> Result<Option<ManagedArtifact>> {
        let ExposureMechanism::Managed(artifact) = self.exposure_plan(resource).mechanism else {
            return Ok(None);
        };
        if !matches!(
            artifact,
            ManagedArtifact::SymlinkReference { .. } | ManagedArtifact::ManagedTextRegion { .. }
        ) {
            return Ok(None);
        }
        artifact.attach_standard()?;
        Ok(Some(artifact))
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
        resource: &crate::capability::Resource,
    ) -> Result<Option<AttachmentReceipt>> {
        let Some(artifact) = self.attach(resource)? else {
            return Ok(None);
        };
        Ok(Some(AttachmentReceipt {
            package_id: resource.package_id.as_str().to_owned(),
            resource_identity: Some(resource.identity()),
            integration: self.id().to_owned(),
            artifact,
        }))
    }

    fn inspect_receipt(&self, receipt: &AttachmentReceipt) -> AttachmentInspection {
        receipt.artifact.inspect_standard()
    }

    fn detach_receipt(&self, receipt: &AttachmentReceipt) -> Result<AttachmentInspection> {
        receipt.artifact.detach_standard()
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
            ManagedArtifact::SymlinkReference { .. }
            | ManagedArtifact::ManagedTextRegion { .. } => {
                receipt.artifact.attach_standard()?;
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
pub fn default_exposure_name_candidates(resource: &crate::capability::Resource) -> Vec<String> {
    let Some(logical) = resource.logical_capability_name() else {
        return Vec::new();
    };
    vec![format!("{}-{}", resource.package_id.as_str(), logical)]
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
/// another marketplace's same-named plugin (ADR-038).
pub fn active_plugin_name(
    home: &crate::home::UzeHome,
    resource: &crate::capability::Resource,
) -> String {
    crate::store::UzeStore::new(home.clone()).active_name_for(&resource.package_id)
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
    resource: &crate::capability::Resource,
    active_plugin_name: &str,
) -> Vec<String> {
    if resource.capability.kind != CapabilityKind::AgentSkill {
        return Vec::new();
    }
    let Some(logical) = resource.logical_capability_name() else {
        return Vec::new();
    };
    vec![qualified_capability_name(active_plugin_name, &logical)]
}

#[cfg(test)]
mod artifact_representation_tests {
    use super::*;
    use std::collections::BTreeMap;

    /// The ledger carries one representation for an integration-owned
    /// artifact, and a write emits exactly it.
    #[test]
    fn a_write_uses_only_the_current_representation() {
        let receipt = AttachmentReceipt {
            package_id: "plugin-a".to_owned(),
            resource_identity: None,
            integration: "codex".to_owned(),
            artifact: ManagedArtifact::IntegrationOwned {
                kind: "marketplace-plugin".to_owned(),
                selector: "plugin-a@uze-local".to_owned(),
                detail: BTreeMap::new(),
            },
        };
        let encoded = serde_json::to_string(&receipt).unwrap();
        assert!(encoded.contains("INTEGRATION_OWNED"));
    }

    /// The Core routes an integration-owned artifact by `receipt.integration`
    /// and must never guess at its ownership itself.
    #[test]
    fn an_integration_owned_receipt_is_never_inspected_generically() {
        let receipt = AttachmentReceipt {
            package_id: "plugin-a".to_owned(),
            resource_identity: None,
            integration: "codex".to_owned(),
            artifact: ManagedArtifact::IntegrationOwned {
                kind: "marketplace-plugin".to_owned(),
                selector: "plugin-a@uze-local".to_owned(),
                detail: BTreeMap::new(),
            },
        };
        assert_eq!(
            receipt.artifact.inspect_standard().state,
            AttachmentState::Blocked
        );
        // And a blocked inspection can never become a destructive operation.
        assert_eq!(
            receipt.artifact.detach_standard().unwrap().state,
            AttachmentState::Blocked
        );
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use std::{fs, path::PathBuf};

    fn receipt(path: PathBuf, target: PathBuf) -> AttachmentReceipt {
        AttachmentReceipt {
            package_id: "plugin".to_owned(),
            resource_identity: Some("skill:example".to_owned()),
            integration: "test".to_owned(),
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
            receipt.artifact.inspect_standard().state,
            AttachmentState::Missing
        );
        symlink(&expected, &path).unwrap();
        assert_eq!(
            receipt.artifact.inspect_standard().state,
            AttachmentState::Matched
        );
        assert_eq!(
            receipt.artifact.detach_standard().unwrap().state,
            AttachmentState::Missing
        );
        assert!(!path.exists());

        symlink(&other, &path).unwrap();
        assert_eq!(
            receipt.artifact.inspect_standard().state,
            AttachmentState::Drifted
        );
        assert_eq!(
            receipt.artifact.detach_standard().unwrap().state,
            AttachmentState::Drifted
        );
        assert_eq!(fs::read_link(&path).unwrap(), other);

        fs::remove_file(&path).unwrap();
        fs::write(&path, "foreign").unwrap();
        assert_eq!(
            receipt.artifact.inspect_standard().state,
            AttachmentState::Conflict
        );
        assert_eq!(
            receipt.artifact.detach_standard().unwrap().state,
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
            receipt.artifact.inspect_standard().state,
            AttachmentState::Blocked
        );
        let mut permissions = fs::metadata(&locked).unwrap().permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(&locked, permissions).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::HarnessCapabilities;

    /// A harness nobody has looked into yet.
    struct UndeclaredIntegration;
    impl IntegrationPort for UndeclaredIntegration {
        fn id(&self) -> &'static str {
            "undeclared"
        }
        fn capabilities(&self) -> HarnessCapabilities {
            HarnessCapabilities::default()
        }
        fn exposure_plan(&self, _resource: &crate::Resource) -> ExposurePlan {
            panic!("not used")
        }
        fn detect(&self) -> HarnessDetection {
            HarnessDetection::default()
        }
    }

    /// The whole point of the default: a fifth harness is safe before
    /// anyone has answered the continuity questions for it.
    #[test]
    fn an_integration_that_declares_nothing_contributes_no_session_argument() {
        let integration = UndeclaredIntegration;
        let session = SessionId::new("whatever");
        let cwd = std::path::Path::new("/tmp");

        assert_eq!(
            integration.session_continuity(),
            SessionContinuity::Unsupported
        );
        assert!(integration.start_session_args(&session).is_empty());
        assert!(integration.resume_session_args(&session).is_empty());
        assert_eq!(integration.session_recorded_for(cwd), None);
        assert_eq!(
            integration.observe_session(&ObservationContext {
                cwd,
                since_unix: 0,
                preceded_by: None,
            }),
            None
        );
    }

    /// `exec` cannot retry, so "I cannot tell you cheaply" must not read as
    /// "it is gone" — that would start a fresh conversation over a resume
    /// that would have worked.
    #[test]
    fn a_harness_that_cannot_answer_cheaply_is_taken_at_its_word() {
        assert!(
            UndeclaredIntegration
                .session_exists(&SessionId::new("s"), std::path::Path::new("/tmp"))
        );
    }
}
