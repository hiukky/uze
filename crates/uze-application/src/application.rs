//! Product-facing application boundary.
//!
//! CLI, TUI, and future presentation layers call this facade rather than
//! reaching into Store, integrations, vendor files, or lifecycle mechanics.

#![allow(clippy::empty_line_after_doc_comments)]

use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use serde::Serialize;

use uze_core::{
    PackageSource, Result, UzeEngine, UzeError, UzeHome, UzeStore,
    capability::CapabilityKind,
    context::{self as instruction_context},
    detection_cache::DetectionCache,
    exposure::{ExposureMechanism, ExposurePlan, PackageExposurePlan},
    integration::{
        AttachmentInspection, AttachmentState, HarnessDetection, IntegrationPort,
        IntegrationStatus, PublicationStatus,
    },
    preference::PreferencePort,
    provisioning::{ProcessRunner, ProvisionStatus, ProvisioningResult, SystemProcessRunner},
    reconciliation::{
        PackageRemovalPlan, ReconciledReceipt, ReconciliationReport, reconcile_package,
    },
    router::{CompatibilityRoute, HarnessCapabilities},
    state,
    store::StoredPackage,
    trust::{self, TrustAuthority, TrustOutcome, TrustRequest},
};
use uze_integrations::registry::IntegrationRegistry;

use crate::bootstrap;

mod agent_context;
mod context;
mod doctor;
mod inspection_cache;
mod lifecycle;
mod maintenance;
mod marketplace;
mod overview;
mod profile;
mod project_environment;
mod read_models;

pub use agent_context::{AgentContextStatus, ResourceDelivery, UndeliveredReason};
pub use profile::{ProfileApplyResult, ProfileSummary};
pub use read_models::*;

// Re-export project environment types for CLI access.
pub use project_environment::{
    InstallReport, ProjectEnvironment, ProjectEnvironmentPlan, ProjectLockStatus,
    ProjectPluginHealth, RemoveProjectPluginReport,
};

// Re-export overview read models for TUI/CLI access.
pub use maintenance::{MaintenanceOutcome, MaintenanceReport};
pub use overview::{
    MarketplaceState, MemoryState, OverviewMarketplace, OverviewWorkspaceSummary,
    ProjectEnvironmentState, ProjectOverview,
};
pub use uze_core::workspace::WorkspaceKind;

/// `PERSISTENT CONTEXT DELIVERY STRATEGY`. Harnesses that read a
/// project's shared `AGENTS.md` only through an explicit bridge region
/// written into their own native file *inside the project's working tree*,
/// rather than natively — see `docs/capabilities/instructions-design.md`
/// Fase 4. Which harness needs a bridge is now each integration's own
/// `context_delivery()` declaration; this Application holds only the
/// bridge protocol itself (region identity + content), shared by every
/// bridge-needing harness.
///
/// Kept, unchanged, alongside the newer `EXPERIMENTAL RUNTIME DELIVERY
/// STRATEGY` (`ClaudeIntegration::runtime_contribution`, driven through the
/// PATH shim rather than through `context reconcile`). Whether runtime
/// projection ever replaces this bridge for Claude is a separate, later
/// decision pending an empirical interactive comparison. Do not remove
/// or fold this into the experimental
/// path without that comparison.
///
/// Fixed, package-independent region identity: the bridge is shared
/// infrastructure for however many packages currently contribute to
/// `AGENTS.md`, never owned by one of them (see Fase C.5 of the design).
const INSTRUCTION_BRIDGE_IDENTITY: &str = "instruction-bridge";

/// The vendor-documented import syntax a bridge-needing harness uses for
/// pulling another Markdown file's content into its own native
/// instructions file (`@AGENTS.md`).
const INSTRUCTION_BRIDGE_CONTENT: &str = "@AGENTS.md";

pub struct UzeApplication {
    home: UzeHome,
    store: UzeStore,
    integrations: Vec<Box<dyn IntegrationPort>>,
    /// Preference translation/apply adapters (Profiles feature). Empty by
    /// default from `new`/`new_with_runner` so the many existing call sites
    /// that construct fake `IntegrationPort`-only fixtures keep compiling
    /// unchanged; `from_env`/`from_env_with_runner` populate it from the
    /// same `IntegrationRegistry` that supplies `integrations`.
    preference_adapters: Vec<Box<dyn PreferencePort>>,
    runner: Box<dyn ProcessRunner>,
    detection_cache: DetectionCache,
    inspection_cache: crate::application::inspection_cache::InspectionCache,
}

impl UzeApplication {
    /// Production composition. The integration set comes from
    /// `IntegrationRegistry::builtin` — the one place that knows which
    /// harnesses exist; this layer only knows there are integrations.
    pub fn from_env(home: UzeHome) -> Result<Self> {
        let registry = IntegrationRegistry::builtin(&home)?;
        let (integrations, preference_adapters) = registry.into_parts();
        Ok(Self::new_with_runner_and_preferences(
            home,
            integrations,
            preference_adapters,
            Box::new(SystemProcessRunner),
        ))
    }

    /// Dependency-injected constructor for deterministic contract tests or
    /// embedded clients. It has the same application behavior as `from_env`.
    pub fn new(home: UzeHome, integrations: Vec<Box<dyn IntegrationPort>>) -> Self {
        Self::new_with_runner(home, integrations, Box::new(SystemProcessRunner))
    }

    /// Same production integration set as `from_env`, with an explicit
    /// process runner instead of the default `SystemProcessRunner`. For a
    /// caller that owns the terminal itself (the TUI's alternate screen), a
    /// vendor installer's inherited-output progress would otherwise print
    /// straight onto the real terminal and corrupt whatever is rendered
    /// there.
    pub fn from_env_with_runner(home: UzeHome, runner: Box<dyn ProcessRunner>) -> Result<Self> {
        let registry = IntegrationRegistry::builtin(&home)?;
        let (integrations, preference_adapters) = registry.into_parts();
        Ok(Self::new_with_runner_and_preferences(
            home,
            integrations,
            preference_adapters,
            runner,
        ))
    }

    /// Test and embedding composition point for the process runner used only
    /// by explicit harness provisioning. Package lifecycle remains entirely
    /// independent of process execution.
    pub fn new_with_runner(
        home: UzeHome,
        integrations: Vec<Box<dyn IntegrationPort>>,
        runner: Box<dyn ProcessRunner>,
    ) -> Self {
        Self::new_with_runner_and_preferences(home, integrations, Vec::new(), runner)
    }

    /// Like `new_with_runner`, additionally wiring preference adapters for
    /// the Profiles feature's `apply_profile`.
    pub fn new_with_runner_and_preferences(
        home: UzeHome,
        integrations: Vec<Box<dyn IntegrationPort>>,
        preference_adapters: Vec<Box<dyn PreferencePort>>,
        runner: Box<dyn ProcessRunner>,
    ) -> Self {
        Self {
            store: UzeStore::new(home.clone()),
            detection_cache: DetectionCache::new(&home),
            inspection_cache: inspection_cache::InspectionCache::new(&home),
            home,
            integrations,
            preference_adapters,
            runner,
        }
    }

    /// The cached path for `IntegrationPort::detect()`: an in-process hit
    /// or a still-fresh on-disk entry (see `detection_cache::
    /// DetectionCache`) is returned with no subprocess spawned; only a
    /// genuine cache miss falls through to a live probe, whose result is
    /// then written through both cache tiers for the next caller — in
    /// this run and in the next CLI invocation alike. Every internal
    /// caller on a path that should stay fast (see
    /// `specs/cli-performance/spec.md`) must go through this rather than
    /// `integration.detect()` directly.
    pub(crate) fn detect_cached(&self, integration: &dyn IntegrationPort) -> HarnessDetection {
        let id = integration.id();
        let candidates = integration.detection_program_candidates();
        if let Some(cached) = self.detection_cache.get(id, &candidates) {
            return cached;
        }
        let live = integration.detect();
        self.detection_cache.put(id, &candidates, live.clone());
        live
    }

    /// Ensures every plugin `bootstrap::DEFAULT_PLUGIN_IDS` names is present
    /// in the Store, then attached to every detected harness. Each default
    /// plugin's *first install* goes through the exact same lifecycle a
    /// normal `uze add` would (`install_materialized`), so Store, Engine,
    /// Router and every `IntegrationPort` stay unaware any of this is a
    /// "default" rather than an ordinary installed plugin.
    ///
    /// This is BOOTSTRAP, not UPDATE: an already-installed default plugin is
    /// never touched here, no matter how its content compares to the
    /// embedded marketplace snapshot — this runs on every CLI invocation
    /// (including read-only ones like `doctor`/`list`), and an observational
    /// command must not mutate installed plugin content. A newer snapshot is
    /// surfaced as `PluginSummary::update_available` (a pure read) for an
    /// explicit `update_plugin` to act on later, not applied silently. See
    /// `docs/architecture/invariants.md`'s "Official marketplace" section.
    ///
    /// Idempotent: nothing changes on a repeat call once every default
    /// plugin is installed and attached. Returns `true` if it installed at
    /// least one Store entry.
    ///
    /// This is deliberately not called from `from_env`/`new` so contract
    /// tests can construct isolated worlds with no default plugins. The CLI
    /// (`src/main.rs`) and `setup` call this explicitly; `add`/`remove` do
    /// not need to because `setup` already covers the attach path.
    pub fn ensure_default_plugins(&self) -> Result<bool> {
        let mut installed_any = false;
        for &id in bootstrap::DEFAULT_PLUGIN_IDS {
            installed_any |= self.ensure_default_plugin_installed(id)?;
        }
        // Attach every default plugin — freshly installed or already
        // present — to every currently detected harness. Kept as its own
        // pass, unconditional, because a harness detected since the last
        // run should not wait for an explicit `uze setup` before seeing the
        // fallback delivery; also prepares detected harnesses (creating
        // `~/.claude/skills` etc.) so a fresh `UZE_HOME` gets the plugin
        // without a prior `uze setup`. Idempotent via ledger receipt keys,
        // so re-attaching a plugin `install_materialized` just attached is
        // harmless. This does not touch plugin *content*, only exposure —
        // distinct from the update question above.
        let _ = self.prepare_detected_integrations(None);
        // Derived views refresh before attachment, same ordering `add_plugin`
        // already relies on (`install_materialized`): a Generated Native
        // Package's own catalogue (e.g. Claude's `generated/.claude-plugin/
        // marketplace.json`) is written by `republish_all`, and native
        // delivery below reads that view. Attaching first on a fresh/
        // catalogue-less `UZE_HOME` made the vendor CLI's own `marketplace
        // add` fail outright (`Marketplace file not found at .../
        // marketplace.json`) — real-host dogfood caught this.
        let _ = self.republish_all();
        let installed_ids: BTreeSet<&str> = bootstrap::DEFAULT_PLUGIN_IDS.iter().copied().collect();
        for package_id in self.store.package_ids().unwrap_or_default() {
            if !installed_ids.contains(package_id.as_str()) {
                continue;
            }
            let Ok(package) = self.store.package(&package_id) else {
                continue;
            };
            for integration in &self.integrations {
                let effective = self
                    .attachment_effective(package.id.as_str(), integration.as_ref())
                    .unwrap_or(false);
                if self.detect_cached(integration.as_ref()).present && !effective {
                    // Production resilience: a single harness's foreign state
                    // (e.g. Antigravity `uze` already imported outside UZE)
                    // must not abort bootstrap for other harnesses. Attach
                    // failures are best-effort here; `setup`/`doctor` will
                    // surface them as warnings. This intentionally swallows
                    // the error — the method's contract is "best-effort attach",
                    // not "all harnesses must succeed".
                    if let Err(err) = self.attach_package_to(&package, integration.as_ref()) {
                        // Swallow foreign-state errors silently in bootstrap;
                        // explicit `setup` will surface them per-harness.
                        let _ = err;
                    }
                }
            }
        }
        Ok(installed_any)
    }

    /// Whether the bootstrap can consider `integration`'s delivery of
    /// `package_id` already effective:
    ///
    /// - no receipt for this integration → not effective (attach);
    /// - stat-able artifacts (skill symlinks) must still be physically in
    ///   place — a vanished link is healed by a cheap re-attach (no
    ///   vendor CLI involved);
    /// - non-stat-able artifacts (vendor-native catalogues recorded
    ///   through the vendor CLIs) are effective by receipt: re-running
    ///   `codex/claude plugin add` on every invocation was the
    ///   steady-state cost this guard removes, and a vendor-side loss is
    ///   surfaced by the read-time inspection (anomalies are always
    ///   re-inspected live) and healed by the explicit setup path.
    fn attachment_effective(
        &self,
        package_id: &str,
        integration: &dyn IntegrationPort,
    ) -> Result<bool> {
        let receipts = state::receipts(&self.home, Some(package_id))?;
        let for_integration: Vec<_> = receipts
            .into_iter()
            .filter(|(_, receipt)| receipt.integration == integration.id())
            .collect();
        if for_integration.is_empty() {
            return Ok(false);
        }
        for (_, receipt) in &for_integration {
            if uze_core::integration::managed_artifact_fingerprint(&receipt.artifact).is_some()
                && !uze_core::integration::managed_artifact_present(&receipt.artifact)
            {
                // A stat-able artifact that is not in place (or
                // re-pointed): not effective, re-attach to heal.
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Installs default plugin `id` if it is not already in the Store.
    /// Never touches an already-installed copy — see `ensure_default_plugins`.
    pub(crate) fn ensure_default_plugin_installed(&self, id: &str) -> Result<bool> {
        let already_installed = self
            .store
            .package_ids()?
            .iter()
            .any(|package_id| package_id.as_str() == format!("{id}@uze-official"));
        if already_installed {
            return Ok(false);
        }
        let materialized = bootstrap::materialize(id)?;
        match self.install_materialized_from_marketplace(
            materialized,
            "uze-official",
            &trust::NoTrustAuthority,
            &[],
            false,
            &uze_core::naming::NoNameCollisionAuthority,
        ) {
            Ok(_) => Ok(true),
            Err(_) => {
                // Production resilience: the Store entry is already persisted
                // before harness attachment, and a foreign-state failure on
                // one harness must not abort bootstrap for other harnesses nor
                // fail the whole `setup` on a user's real machine. The package
                // remains installed; `setup`/`doctor` will surface the
                // per-harness warning.
                Ok(true)
            }
        }
    }

    /// Resolves a `PackageSource` into local bytes. `Embedded` sources are
    /// resolved through the embedded default marketplace snapshot this
    /// composition root carries (`uze-core`'s generic acquisition cannot
    /// reach bytes `include_str!` compiled a layer up); every other source
    /// goes through the normal acquisition mechanism.

    /// Every marketplace this composition root knows how to read from.
    /// Exactly one today — the official embedded snapshot — but the return
    /// shape does not assume that stays true. Read-only; parses no product
    /// content beyond `marketplace.json`'s own name and plugin count.

    pub(crate) fn parse_marketplace_source(source_str: &str) -> Result<PackageSource> {
        let looks_remote = source_str.starts_with("https://")
            || source_str.starts_with("http://")
            || source_str.starts_with("git://")
            || source_str.starts_with("ssh://")
            || source_str.starts_with("file://");
        if !looks_remote {
            let path = PathBuf::from(source_str)
                .canonicalize()
                .map_err(|_| UzeError::MissingPath(PathBuf::from(source_str)))?;
            let manifest_path = path.join(uze_core::workspace::MARKETPLACE_MANIFEST_NAME);
            if !manifest_path.is_file() {
                return Err(UzeError::MissingManifest(manifest_path));
            }
            return Ok(PackageSource::Local { path });
        }
        let (locator, subdirectory) = match source_str.split_once('#') {
            Some((locator, sub)) => (locator, Some(PathBuf::from(sub))),
            None => (source_str, None),
        };
        let scheme_end = locator.find("://").map(|at| at + 3).unwrap_or(0);
        let (url, reference) = match locator[scheme_end..].rfind('@') {
            Some(at) => {
                let at = scheme_end + at;
                (&locator[..at], Some(locator[at + 1..].to_owned()))
            }
            None => (locator, None),
        };
        Ok(PackageSource::Git {
            url: url.to_owned(),
            reference,
            subdirectory,
        })
    }

    pub(crate) fn load_marketplace_manifest(
        source: &PackageSource,
    ) -> Result<(
        PathBuf,
        uze_core::acquisition::marketplace::MarketplaceManifest,
    )> {
        match source {
            PackageSource::Local { path } => {
                let manifest_path = path.join(uze_core::workspace::MARKETPLACE_MANIFEST_NAME);
                let bytes = std::fs::read(&manifest_path).map_err(|e| UzeError::Read {
                    path: manifest_path.clone(),
                    source: e,
                })?;
                let manifest = uze_core::acquisition::marketplace::parse_manifest(&bytes)?;
                Ok((path.clone(), manifest))
            }
            PackageSource::Git {
                url,
                reference,
                subdirectory,
            } => {
                let git_source = PackageSource::Git {
                    url: url.clone(),
                    reference: reference.clone(),
                    subdirectory: subdirectory.clone(),
                };
                let materialized = uze_core::acquisition::acquire(&git_source)?;
                let root = materialized.root().to_path_buf();
                let manifest_path = root.join(uze_core::workspace::MARKETPLACE_MANIFEST_NAME);
                let bytes = std::fs::read(&manifest_path).map_err(|e| UzeError::Read {
                    path: manifest_path.clone(),
                    source: e,
                })?;
                let manifest = uze_core::acquisition::marketplace::parse_manifest(&bytes)?;
                Ok((root, manifest))
            }
            PackageSource::Embedded { .. } => Err(UzeError::ExposureUnavailable(
                "embedded marketplace cannot be used as marketplace source".to_owned(),
            )),
        }
    }

    /// Every plugin the official marketplace lists, cross-referenced against
    /// what's actually installed. `update_available` is computed the same
    /// pure, offline way `PluginSummary`'s is — never re-applied here.

    /// One marketplace plugin's full detail, including capabilities read
    /// straight off the manifest snapshot — no install required. Capability
    /// inspection materializes a scratch copy that is discarded before this
    /// returns; nothing is written to the Store.

    /// Installs a marketplace plugin by name through the exact same
    /// lifecycle any other `add` uses — trust included. A thin, named
    /// convenience over `add_plugin(PackageSource::Embedded { .. })` so
    /// callers never need to know `Embedded` is the mechanism.

    /// Installs once, chooses package-native delivery first, attaches only
    /// remaining resources, and records every persistent side effect.

    /// The half of installation that runs once bytes exist locally.
    ///
    /// Deliberately takes no lock: both public entry points hold one already,
    /// and `MutationLock` is not reentrant. Sharing this body is what lets
    /// `update_plugin` reuse installation without re-entering it.

    /// Re-resolves a package's original request and replaces the installed
    /// copy with the result.
    ///
    /// The order is the whole safety story. Everything that can fail without
    /// consequence happens first — re-resolve, materialize, validate, and ask
    /// about any execution the installed revision did not already have — and
    /// only then is the current package detached. A network failure, an
    /// invalid package or a refused trust question therefore mutates nothing
    /// at all.
    ///
    /// There is deliberately no rollback across integrations. If one fails to
    /// re-attach, the Store stays consistent, the others keep what they got,
    /// and the partial state is reported rather than papered over — `doctor`
    /// reconciles it and a repeat `update` finishes the job. Blind rollback
    /// would mean detaching artifacts UZE had just proven it owns, on the
    /// word of an unrelated failure.

    /// Runs only selected, detected setup routines. No integration knowledge
    /// leaks to the caller beyond stable ids and reported facts.
    ///
    /// Resilience contract (production environments): a single harness's
    /// attach or shim failure never aborts the whole `setup` run. Failures
    /// are collected per-harness into `SetupResult::attach_error` /
    /// `shim_error` and surfaced as warnings — the caller still gets a
    /// `Vec<SetupResult>` with one entry per harness, and `doctor` shows the
    /// same facts via reconciliation.
    pub fn setup(&self, requested: Option<&str>) -> Result<Vec<SetupResult>> {
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.home)?;
        self.home.ensure_layout()?;
        // Seed the default marketplace plugins before any provisioning, so a
        // fresh `UZE_HOME` gets the Skill without a manual `uze add` and so
        // an updated binary heals its attachment on next `setup`.
        let _ = self.ensure_default_plugins();
        let wanted = requested
            .map(|name| self.resolve_integration_id(name))
            .transpose()?;
        let mut results = self.provision_and_prepare(wanted)?;
        // `setup` is the documented way to repair a derived view that
        // failed to publish, so it always rebuilds them.
        let _ = self.republish_all();
        for result in results.iter_mut().filter(|result| result.configured) {
            if let Some(integration) = self
                .integrations
                .iter()
                .find(|integration| integration.id() == result.integration)
            {
                match self.attach_stored_packages_to(integration.as_ref()) {
                    Ok(()) => {}
                    Err(error) => {
                        result.attach_error = Some(error.to_string());
                    }
                }
                match self.ensure_runtime_shim(integration.as_ref()) {
                    Ok(shim) => result.runtime_shim = shim,
                    Err(error) => {
                        result.shim_error = Some(error.to_string());
                    }
                }
            }
        }
        Ok(results)
    }

    /// Idempotently creates/refreshes the PATH shim for `integration` when
    /// it opts in via `IntegrationPort::supports_runtime_integration` — see
    /// that method's doc comment for why there is no separate enabled/
    /// disabled flag: the shim symlink's own presence at `shims_dir/<name>`
    /// is the only state this tracks. `Ok(None)` (not an error) when the
    /// integration has no runtime-integration story. Called automatically
    /// by `setup()` — running `uze setup <harness>` is the entire opt-in,
    /// no separate flag.
    ///
    /// `EXPERIMENTAL RUNTIME DELIVERY STRATEGY` (`RUNTIME INFRASTRUCTURE`,
    /// not a `CONTEXT DELIVERY POLICY` decision; see this module's
    /// `INSTRUCTION_BRIDGE_IDENTITY` doc for how the two relate).
    pub(crate) fn ensure_runtime_shim(
        &self,
        integration: &dyn IntegrationPort,
    ) -> Result<Option<RuntimeShimSetup>> {
        if !integration.supports_runtime_integration() {
            return Ok(None);
        }
        let shim_name = integration.shim_name();
        let shims_dir = self.home.shims_dir();

        // Refuse to shim a harness with no real binary anywhere — that
        // would silently create a symlink that can never resolve. Includes
        // the integration's own `runtime_executable_aliases` (e.g. OpenCode's
        // `opencode2`) so a harness whose installer names the binary
        // differently from `shim_name` is still found.
        let mut candidates = vec![shim_name];
        candidates.extend(integration.runtime_executable_aliases());
        let resolved = uze_core::harness_runtime::resolve_real_executable(&candidates, &shims_dir)
            .ok_or_else(|| {
                UzeError::ExposureUnavailable(format!(
                    "no real `{shim_name}` executable found on PATH outside {} — install it \
                         first",
                    shims_dir.display()
                ))
            })?;

        fs::create_dir_all(&shims_dir).map_err(|source| UzeError::Write {
            path: shims_dir.clone(),
            source,
        })?;
        let uze_binary = std::env::current_exe().map_err(|source| UzeError::Process {
            program: "uze".to_owned(),
            source,
        })?;
        let shim_path = shims_dir.join(shim_name);
        refresh_shim_symlink(&uze_binary, &shim_path)?;

        let shim_precedes_real_executable = std::env::var_os("PATH")
            .map(|path| {
                let entries: Vec<_> = std::env::split_paths(&path).collect();
                let shim_position = entries.iter().position(|entry| entry == &shims_dir);
                let executable_position = resolved
                    .parent()
                    .and_then(|parent| entries.iter().position(|entry| entry == parent));
                matches!(
                    (shim_position, executable_position),
                    (Some(shim), Some(executable)) if shim < executable
                )
            })
            .unwrap_or(false);

        let mut rc_file_updated = None;
        let mut path_hint = None;
        let manual_export = format!("export PATH=\"{}:$PATH\"", shims_dir.display());
        match std::env::var_os("HOME")
            .map(PathBuf::from)
            .and_then(|home_dir| uze_core::shell_path::detect_shell_rc(&home_dir))
        {
            Some(target) => match uze_core::shell_path::ensure_path_line(&target, &shims_dir) {
                Ok(changed) => {
                    if changed {
                        rc_file_updated = Some(target.rc_file.clone());
                    }
                    if !shim_precedes_real_executable {
                        path_hint = Some(format!(
                            "open a new terminal, or run: source {}",
                            target.rc_file.display()
                        ));
                    }
                }
                // The rc file has a marker in a shape this function doesn't
                // recognize (edited by hand, presumably) — refuse to guess,
                // fall back to the manual instruction when the current shell
                // does not resolve the shim first.
                Err(_) if !shim_precedes_real_executable => path_hint = Some(manual_export),
                Err(_) => {}
            },
            // No detected shell (uncommon shell, `$SHELL`/`$HOME` unset) —
            // nothing to edit, same manual fallback when needed.
            None if !shim_precedes_real_executable => path_hint = Some(manual_export),
            None => {}
        }

        Ok(Some(RuntimeShimSetup {
            shim_path,
            resolved_executable: resolved,
            rc_file_updated,
            path_hint,
        }))
    }

    /// Explicit setup is the only path allowed to provision or update an
    /// executable. `add` deliberately calls only `prepare_detected_*`.
    pub(crate) fn provision_and_prepare(
        &self,
        requested: Option<&str>,
    ) -> Result<Vec<SetupResult>> {
        self.integrations
            .iter()
            .filter(|integration| requested.is_none_or(|id| integration.id() == id))
            .map(|integration| {
                let provisioning = integration.provision(self.runner.as_ref())?;
                state::record_provisioning(&self.home, integration.id(), &provisioning)?;
                let configured = provisioning.status == ProvisionStatus::Verified;
                if configured {
                    integration.install(&self.home, &provisioning.detection)?;
                    // Write-through (ADR 018 decision 3): `provision()`
                    // already verified this result, so record it in the
                    // cache directly instead of leaving the pre-action
                    // entry to be caught later by a read-time fingerprint
                    // check — a UZE-driven install/update has no stale
                    // window, and no separate probe is spent to get that.
                    self.detection_cache.put(
                        integration.id(),
                        &integration.detection_program_candidates(),
                        provisioning.detection.clone(),
                    );
                }
                Ok(SetupResult {
                    integration: integration.id().to_owned(),
                    detection: provisioning.detection.clone(),
                    configured,
                    provisioning,
                    // Only explicit `setup()` wires up `ensure_runtime_shim` —
                    // this helper also backs `add`'s implicit preparation,
                    // which must never silently create a PATH shim.
                    runtime_shim: None,
                    attach_error: None,
                    shim_error: None,
                })
            })
            .collect()
    }

    /// Prepares integrations only when their real executable is present.
    /// This is the shared bridge between explicit `setup` and implicit
    /// preparation during `add`; neither presentation layer needs to know
    /// which directories/configuration an integration owns.
    pub(crate) fn prepare_detected_integrations(
        &self,
        requested: Option<&str>,
    ) -> Result<Vec<SetupResult>> {
        self.integrations
            .iter()
            .filter(|integration| requested.is_none_or(|id| integration.id() == id))
            .map(|integration| {
                let detection = self.detect_cached(integration.as_ref());
                let configured = detection.present;
                if detection.present {
                    integration.install(&self.home, &detection)?;
                }
                Ok(SetupResult {
                    integration: integration.id().to_owned(),
                    detection: detection.clone(),
                    configured,
                    provisioning: ProvisioningResult::verified(
                        uze_core::provisioning::ProvisionAction::None,
                        "implicit-existing-executable",
                        detection,
                    ),
                    runtime_shim: None,
                    attach_error: None,
                    shim_error: None,
                })
            })
            .collect()
    }

    /// Delivers packages which were installed before an explicit setup made
    /// this integration available. This repeats the same package-first plan
    /// as `add`, scoped to one integration, and ledger keys make it
    /// idempotent without inventing a sync subsystem.

    /// Attaches one already-stored `package` to `integration`: a package-level
    /// native delivery when the integration offers one, then per-resource
    /// attachment for whatever it doesn't cover. Idempotent via the ledger's
    /// receipt keys. Shared by `attach_stored_packages_to` (every package) and
    /// `ensure_default_plugins` (only the default marketplace plugins).
    ///
    /// When a package gains a native envelope, previously decomposed
    /// capability receipts that are now covered by `provided` are migrated
    /// safely: only `Matched` receipts are detached, `Drifted`/`Conflict`/
    /// `Blocked` block migration per ADR-009.

    /// Applies the approved lifecycle contract: reconcile, plan, detach only
    /// matched receipts, re-reconcile, forget resolved ledger records, then
    /// delete UZE-owned package bytes.

    /// Removal without taking the lock; see `install_materialized`.

    /// Deterministic environment diagnostics. Attachment facts are always
    /// obtained through the same receipt reconciliation used by removal.

    /// A short, project-scoped health summary — the single high-level
    /// question most callers actually want answered: "is everything UZE
    /// touches, here, in order?"
    ///
    /// Deliberately **not** a merge with `doctor`: `doctor` has no
    /// `project_root` concept at all and never will — it answers "is my
    /// UZE *installation* healthy," global, independent of any project.
    /// `status` answers "is *this project's* context healthy," and is
    /// built almost entirely by composing `context_inspect` (already
    /// read-only) with the Store's own package count. It duplicates no
    /// health logic doctor already owns; a genuine installation problem
    /// (corrupt ledger, missing executable) stays doctor's to report.

    /// Resolves `resource`'s physical exposure name for `integration`,
    /// immediately before an attach call — the one place a naming decision
    /// happens. Returns a clone of `resource` with `resolved_exposure_name`
    /// set; `resource` itself is never mutated.
    ///
    /// "Existing receipt wins": if a receipt for this exact
    /// `resource.identity()` already exists for this integration — on any
    /// naming scheme, including the legacy `uze-<package>-<skill>` shape —
    /// its already-recorded physical name is reused verbatim. No naming
    /// policy ever recomputes, moves, or renames an already-attached
    /// resource; this is what makes re-add/setup idempotent and legacy
    /// installs safe without any migration step.
    ///
    /// The same reuse extends to a *different* integration's receipt for
    /// this identical resource when the two integrations report the same
    /// `shared_agent_skill_root` (OpenCode and Codex all read
    /// `~/.agents/skills`): reusing that name means the second integration's
    /// attach writes the very same symlink rather than a second one next to
    /// it, so a directory one harness scans in full never ends up listing
    /// the identical skill twice.
    ///
    /// Only for a brand new resource with no reusable receipt anywhere does
    /// this ask the integration for ordered candidates
    /// (`exposure_name_candidates`) and pick the first one not already
    /// claimed — by this integration, or by another integration sharing its
    /// skill root. This resolves purely from the ledger — no filesystem
    /// access — so it can never itself decide a foreign-artifact conflict;
    /// `attach`'s own structural check (unchanged) remains the last word on
    /// that.

    pub(crate) fn package_by_name(&self, name: &str) -> Result<StoredPackage> {
        // A plugin is addressable by its active local name (ADR-038) first —
        // its own bare plugin name unless an install-time alias resolved a
        // collision, in which case only one installed package ever answers
        // to a given name at all, so this can never be ambiguous. Falls
        // through to the qualified-id/bare-plugin-name lookup only for a
        // name nothing is currently active under (defensive: normal install
        // flows never leave two packages sharing a bare `plugin_name()`
        // with neither of them active under it).
        if let Some(id) = self.store.find_by_active_name(name)? {
            return self.store.package(&id);
        }
        let matches: Vec<_> = self
            .store
            .package_ids()?
            .into_iter()
            .filter(|id| id.as_str() == name || id.plugin_name() == name)
            .collect();
        match matches.as_slice() {
            [id] => self.store.package(id),
            [] => Err(UzeError::UnknownPackage(name.to_owned())),
            _ => Err(UzeError::ExposureUnavailable(format!(
                "plugin `{name}` is installed from multiple marketplaces; use `plugin@marketplace`"
            ))),
        }
    }

    pub(crate) fn plugin_summary(&self, package: &StoredPackage) -> Result<PluginSummary> {
        let environment = self.engine().compose(std::slice::from_ref(&package.id))?;
        let update_available = match &package.provenance.requested {
            PackageSource::Embedded { id } => bootstrap::has_update(id, &package.root).ok(),
            _ => None,
        };
        Ok(PluginSummary {
            id: package.id.as_str().to_owned(),
            active_name: package.active_name.clone(),
            source: package.provenance.requested.display(),
            store_path: package.root.clone(),
            capability_count: environment.resources.len(),
            update_available,
        })
    }

    /// Refreshes every integration's derived view of the installed package
    /// set. Collects failures instead of propagating them: publication is not
    /// part of package ownership, so one harness failing to rebuild its view
    /// leaves the package installed and the other harnesses unaffected.
    pub(crate) fn republish_all(&self) -> Vec<PublicationOutcome> {
        let packages = self.installed_packages();
        self.integrations
            .iter()
            .map(|integration| PublicationOutcome {
                integration: integration.id().to_owned(),
                error: integration
                    .republish_packages(&packages)
                    .err()
                    .map(|error| error.to_string()),
            })
            .collect()
    }

    pub(crate) fn installed_packages(&self) -> Vec<StoredPackage> {
        self.store
            .package_ids()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|id| self.store.package(&id).ok())
            .collect()
    }

    /// Resolves a requested harness name against the integrations actually
    /// registered in this composition root. There is deliberately no central
    /// list of vendors: an integration declares its own id and aliases, so
    /// registering one is the only step needed to make it selectable.
    pub(crate) fn resolve_integration_id(&self, requested: &str) -> Result<&'static str> {
        self.integrations
            .iter()
            .find(|integration| {
                integration.id() == requested || integration.aliases().contains(&requested)
            })
            .map(|integration| integration.id())
            .ok_or_else(|| {
                let known = self
                    .integrations
                    .iter()
                    .map(|integration| integration.id())
                    .collect::<Vec<_>>()
                    .join(", ");
                UzeError::ExposureUnavailable(format!(
                    "unknown harness `{requested}` (registered: {known})"
                ))
            })
    }

    /// Asks the supplied authority about any capability that would introduce
    /// process execution, and refuses to proceed without a grant.
    ///
    /// Returns `Ok(())` immediately when the package declares nothing
    /// executable: a purely declarative package needs no consent beyond the
    /// decision to install it.
    pub(crate) fn authorize(
        &self,
        materialized: &uze_core::MaterializedPackage,
        authority: &dyn TrustAuthority,
        already_trusted: &[trust::ExecutableCapability],
        replacing_installed: bool,
    ) -> Result<()> {
        let provenance = materialized.provenance();
        if !provenance.requested.crosses_trust_boundary() {
            return Ok(());
        }
        let inspected = uze_core::acquisition::inspect_capabilities(materialized)?;
        let resources: Vec<&uze_core::Resource> = inspected.resources.iter().collect();
        let executable = trust::executable_capabilities(&resources);
        if executable.is_empty() || !trust::introduces_new_execution(already_trusted, &executable) {
            return Ok(());
        }
        let request = TrustRequest {
            package_id: inspected.package_id.clone(),
            requested_source: provenance.requested.display(),
            resolved_source: provenance.resolved.display(),
            executable,
            // The operator is being asked about a *change* to something they
            // already have, not about a first install. Derived from the fact
            // of an existing installation rather than from whether the
            // previous revision happened to execute anything — a declarative
            // package gaining an MCP server is exactly the case that must
            // read as a change.
            previously_trusted: replacing_installed,
        };
        match authority.authorize(&request) {
            TrustOutcome::Granted => Ok(()),
            TrustOutcome::Denied => Err(UzeError::TrustDenied(request.package_id)),
            TrustOutcome::Unavailable => Err(UzeError::TrustRequired {
                package: request.package_id.clone(),
                detail: request
                    .executable
                    .iter()
                    .map(|capability| {
                        format!(
                            "{} -> {} {}",
                            capability.name,
                            capability.command,
                            capability.arguments.join(" ")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; "),
            }),
        }
    }

    pub(crate) fn engine(&self) -> UzeEngine {
        UzeEngine::new(self.store.clone())
    }

    pub(crate) fn reconcile(&self, package_id: &str) -> ReconciliationReport {
        let integrations = self
            .integrations
            .iter()
            .map(|integration| integration.as_ref() as &dyn IntegrationPort)
            .collect::<Vec<_>>();
        reconcile_package(&self.home, package_id, &integrations)
    }

    /// The READ-ONLY cousin of `reconcile`: same report shape, but each
    /// receipt's `Matched` verdict may come from the inspection cache
    /// (ADR 018) instead of a live vendor-CLI probe. Anomalies are never
    /// cached, so the report's warnings are always fresh. This is for
    /// report/health surfaces only — removal planning and detach MUST keep
    /// going through [`reconcile`](Self::reconcile), whose live verdict is
    /// what makes ownership checks trustworthy.
    pub(crate) fn reconcile_cached_report(&self, package_id: &str) -> ReconciliationReport {
        let entries = match state::receipts(&self.home, Some(package_id)) {
            Ok(entries) => entries,
            Err(error) => {
                return ReconciliationReport {
                    package_id: package_id.to_owned(),
                    receipts: Vec::new(),
                    ledger_error: Some(error.to_string()),
                };
            }
        };
        let receipts = entries
            .into_iter()
            .map(|(ledger_key, receipt)| {
                let fingerprint =
                    uze_core::integration::managed_artifact_fingerprint(&receipt.artifact);
                let inspection = match self
                    .inspection_cache
                    .get(&ledger_key, fingerprint.as_deref())
                {
                    Some(cached) => cached,
                    None => {
                        let live = self
                            .integrations
                            .iter()
                            .find(|integration| integration.id() == receipt.integration)
                            .map(|integration| integration.inspect_receipt(&receipt))
                            .unwrap_or_else(|| AttachmentInspection {
                                state: AttachmentState::Blocked,
                                reason: format!(
                                    "integration `{}` is unavailable",
                                    receipt.integration
                                ),
                            });
                        self.inspection_cache.put(&ledger_key, &live, fingerprint);
                        live
                    }
                };
                ReconciledReceipt {
                    ledger_key,
                    receipt,
                    inspection,
                }
            })
            .collect();
        ReconciliationReport {
            package_id: package_id.to_owned(),
            receipts,
            ledger_error: None,
        }
    }
}
#[cfg(test)]
mod tests;
