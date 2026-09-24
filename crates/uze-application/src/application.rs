//! Product-facing application boundary.
//!
//! CLI, TUI, and future presentation layers call this facade rather than
//! reaching into Store, integrations, vendor files, or lifecycle mechanics.

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Serialize;

use uze_core::{
    PackageSource, Result, UzeError, UzeHome, UzeStore,
    capability::CapabilityKind,
    context::{self as instruction_context},
    detection_cache::DetectionCache,
    exposure::{ExposureMechanism, ExposurePlan, PackageExposurePlan},
    integration::{
        AttachmentState, HarnessDetection, IntegrationPort, IntegrationStatus, PublicationStatus,
    },
    manifest::BUILT_IN_MARKETPLACE,
    preference::PreferencePort,
    provisioning::{ProcessRunner, ProvisionStatus, SystemProcessRunner},
    reconciliation::{
        PackageRemovalPlan, ReconciliationReport, reconcile_package, reconcile_package_with,
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
mod marketplace_catalogue;
mod notifications;
pub mod offers;
mod overview;
mod profile;
mod project_environment;
mod read_models;
pub mod services;
mod theme;

pub use agent_context::{AgentContextStatus, ResourceDelivery, UndeliveredReason};
pub use profile::{HarnessPreview, ProfileApplyResult, ProfilePreview, ProfileSummary};
pub use read_models::*;
pub use theme::{GlyphSetSummary, ThemeSummary};

pub use maintenance::{MaintenanceOutcome, MaintenanceReport};
pub use overview::{
    MarketplaceState, MemoryState, OverviewMarketplace, OverviewWorkspaceSummary,
    ProjectEnvironmentState, ProjectOverview,
};
use project_environment::ProjectEnvironmentPlan;
pub use project_environment::{
    InstallReport, ProjectLockStatus, RemoveProjectPluginReport, UpdateOutcome, UpdateReport,
};
pub use uze_core::workspace::WorkspaceKind;

/// `PERSISTENT CONTEXT DELIVERY STRATEGY`. Harnesses that read a
/// project's shared `AGENTS.md` only through an explicit bridge region
/// written into their own native file *inside the project's working tree*,
/// rather than natively — see `docs/capabilities/context-manager.md`.
/// Which harness needs a bridge is now each integration's own
/// `context_delivery()` declaration; this Application holds only the
/// bridge protocol itself (region identity + content), shared by every
/// bridge-needing harness.
///
/// Kept, unchanged, alongside the newer `EXPERIMENTAL RUNTIME DELIVERY
/// STRATEGY` (`ClaudeIntegration::runtime_contribution`, driven through the
/// PATH shim rather than through `agent context reconcile`). Whether runtime
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
    marketplace_catalogues: marketplace_catalogue::MarketplaceCatalogues,
    /// Mirrors this command already brought up to date. One command asks a
    /// remote one question once: updating three plugins from one
    /// marketplace fetched it three times, a second apart.
    mirrors_fetched: std::sync::Mutex<Vec<PathBuf>>,
}

impl UzeApplication {
    /// Starts one operation: what the last one fetched is not fresh for
    /// this one, whose own fetches are.
    pub(crate) fn begin_operation(&self) {
        if let Ok(mut fetched) = self.mirrors_fetched.lock() {
            fetched.clear();
        }
    }

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
    /// the Profiles feature's `Profiles::apply`.
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
            marketplace_catalogues: marketplace_catalogue::MarketplaceCatalogues::new(&home),
            mirrors_fetched: std::sync::Mutex::new(Vec::new()),
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
        let _span = tracing::info_span!("integration.detect", integration = id).entered();
        let live = integration.detect();
        self.detection_cache.put(id, &candidates, live.clone());
        live
    }

    /// Ensures every plugin `bootstrap::DEFAULT_PLUGIN_IDS` names is present
    /// in the Store. Each default plugin's *first install* goes through the
    /// exact same lifecycle any other install does
    /// (`Plugins::install_materialized`), so Store, Engine, Router and every
    /// `IntegrationPort` stay unaware any of this is a "default" rather than
    /// an ordinary installed plugin.
    ///
    /// This is BOOTSTRAP, not UPDATE: an already-installed default plugin is
    /// never touched here, no matter how its content compares to the
    /// embedded marketplace snapshot — this runs on every CLI invocation
    /// (including read-only ones like `doctor`/`list`), and an observational
    /// command must not mutate installed plugin content. A newer snapshot is
    /// surfaced as `PluginSummary::update_available` (a pure read) for an
    /// explicit `Plugins::update` to act on later, not applied silently. See
    /// `docs/architecture/invariants.md`'s "Official marketplace" section.
    ///
    /// Idempotent. Returns `true` if it installed at least one Store entry.
    ///
    /// This is deliberately not called from `from_env`/`new` so contract
    /// tests can construct isolated worlds with no default plugins. The CLI
    /// (`src/main.rs`) and `setup` call this explicitly.
    pub fn ensure_default_plugins(&self) -> Result<bool> {
        let _span = tracing::info_span!("bootstrap.ensure_default_plugins").entered();
        let mut installed_any = false;
        for &id in bootstrap::DEFAULT_PLUGIN_IDS {
            installed_any |= self.ensure_default_plugin_installed(id)?;
        }
        // Prepares detected harnesses (creating `~/.claude/skills` etc.) so a
        // harness detected since the last run does not wait for an explicit
        // `uze setup`. Best-effort: `setup`/`doctor` surface a failure.
        let _ = self.prepare_detected_integrations();
        // A Generated Native Package's own catalogue is written by
        // republishing, and a vendor CLI reading a missing one fails
        // outright. Only a view that no longer matches the installed set is
        // rewritten: this runs before every command, and rewriting every
        // catalogue (each a synced atomic write) to say what it already said
        // was most of what a read-only command cost.
        let _ = self.republish_unpublished(&self.installed_packages());
        Ok(installed_any)
    }

    /// Installs default plugin `id` if it is not already in the Store.
    /// Never touches an already-installed copy — see `ensure_default_plugins`.
    pub(crate) fn ensure_default_plugin_installed(&self, id: &str) -> Result<bool> {
        let already_installed = self
            .store
            .package_ids()?
            .iter()
            .any(|package_id| package_id.as_str() == format!("{id}@{BUILT_IN_MARKETPLACE}"));
        if already_installed {
            return Ok(false);
        }
        let materialized = bootstrap::materialize(id)?;
        match self.plugins().install_materialized(
            materialized,
            BUILT_IN_MARKETPLACE,
            None,
            &trust::NoTrustAuthority,
            &uze_core::naming::NoNameCollisionAuthority,
        ) {
            Ok(_) => Ok(true),
            Err(error) => {
                // Production resilience: a foreign-state failure on one
                // harness must not abort bootstrap for the others nor fail
                // the whole `setup` on a user's real machine. But "installed"
                // is a fact about the Store, not a consolation: trust can be
                // refused, a harness can refuse to be prepared, and the ingest
                // itself can fail, all of them before a byte is written. Ask
                // the Store instead of assuming.
                let installed = self.store.package_ids().is_ok_and(|ids| {
                    ids.iter().any(|package_id| {
                        package_id.as_str() == format!("{id}@{BUILT_IN_MARKETPLACE}")
                    })
                });
                tracing::warn!(
                    plugin = id,
                    installed,
                    error = %error,
                    "a default plugin could not be installed completely"
                );
                Ok(installed)
            }
        }
    }

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
                let provisioning = {
                    let _span = tracing::info_span!(
                        "integration.provision",
                        integration = integration.id()
                    )
                    .entered();
                    integration.provision(self.runner.as_ref())?
                };
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
    /// preparation during an install; neither presentation layer needs to
    /// know which directories/configuration an integration owns.
    pub(crate) fn prepare_detected_integrations(&self) -> Result<()> {
        for integration in &self.integrations {
            let detection = self.detect_cached(integration.as_ref());
            if detection.present {
                let _span =
                    tracing::debug_span!("integration.install", integration = integration.id())
                        .entered();
                integration.install(&self.home, &detection)?;
            }
        }
        Ok(())
    }

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
        let resources = uze_core::engine::package_resources(package)?;
        Ok(PluginSummary {
            id: package.id.as_str().to_owned(),
            active_name: package.active_name.clone(),
            source: package.provenance.requested.display(),
            store_path: package.root.clone(),
            capability_count: resources.len(),
            freshness: self.freshness_of(package),
        })
    }

    /// Now, in seconds since the epoch — the date an offline comparison
    /// against the binary's own snapshot is about, since that snapshot is
    /// this binary and nothing older can be meant.
    fn now_unix_seconds() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_secs())
            .unwrap_or_default()
    }

    /// When `package` was last written, named so a person can place it in
    /// time.
    ///
    /// One Git call against a mirror already on this disk, and only on an
    /// explicit selection — the detail view, never a listing. That is the
    /// division the machine snapshot's budget forced: "is there something
    /// newer" has to be a JSON read because every listing pays it, and
    /// "how old is this" can afford to ask.
    pub(crate) fn installed_revision(&self, package: &StoredPackage) -> Option<Revision> {
        if matches!(
            package.provenance.requested,
            uze_core::PackageSource::Embedded { .. }
        ) {
            return Some(Revision::Bundled {
                version: env!("CARGO_PKG_VERSION").to_owned(),
            });
        }
        let marketplace = package.id.marketplace();
        let record = uze_core::state::marketplace_get(&self.home, marketplace).ok()??;
        if let Some(checkout) = record.link {
            return Some(Revision::Checkout { path: checkout });
        }
        let uze_core::ResolvedSource::Git {
            commit,
            subdirectory,
            ..
        } = &package.provenance.resolved
        else {
            return None;
        };
        let repository = marketplace_catalogue::mirror_dir(&self.home, marketplace);
        // The plugin's own directory at the revision installed: what is in
        // front of the reader, not when its marketplace last moved.
        let within = subdirectory.as_ref().map(|path| path.to_string_lossy());
        let described =
            uze_core::acquisition::mirror::describe_path(&repository, commit, within.as_deref())?;
        Some(Revision::Commit {
            short: described.short,
            age: described.age,
            subject: described.subject,
        })
    }

    /// When the plugin `marketplace` offers as `plugin` was last written —
    /// asked of a plugin that is not installed, which has no provenance of
    /// its own to read.
    pub(crate) fn offered_revision(&self, marketplace: &str, plugin: &str) -> Option<Revision> {
        let record = uze_core::state::marketplace_get(&self.home, marketplace).ok()??;
        if let Some(checkout) = record.link {
            return Some(Revision::Checkout { path: checkout });
        }
        let mirrored = marketplace_catalogue::mirrored_head(&self.home, marketplace)?;
        let catalogue = self
            .catalogue_as_it_stands(marketplace, &record.source)
            .ok()?;
        let within =
            uze_core::acquisition::marketplace::plugin_subdirectory(&catalogue.manifest, plugin)
                .ok()?;
        let repository = marketplace_catalogue::mirror_dir(&self.home, marketplace);
        let described = uze_core::acquisition::mirror::describe_path(
            &repository,
            &mirrored.commit,
            Some(&within),
        )?;
        Some(Revision::Commit {
            short: described.short,
            age: described.age,
            subject: described.subject,
        })
    }

    /// Whether `package` is the one that exists.
    ///
    /// A local read in every case. The comparison a marketplace package
    /// needs is between the commit it was installed at and the commit its
    /// declared ref points at, and both are in the mirror already on this
    /// disk — so no read path reaches the network to answer it, and the
    /// answer carries the date the mirror was last brought up to date
    /// rather than pretending to be about this instant.
    ///
    /// A package built into the binary is compared against the snapshot the
    /// binary carries, which is the offline comparison that has always
    /// worked and is the only one it has. A package installed straight from
    /// a path or a URL belongs to no catalogue and reports `Unpinned`:
    /// there is no question to answer, which is a different thing from an
    /// answer UZE does not have.
    pub(crate) fn freshness_of(&self, package: &StoredPackage) -> Freshness {
        if let PackageSource::Embedded { id } = &package.provenance.requested {
            return match bootstrap::has_update(id, &package.root) {
                Ok(true) => Freshness {
                    state: FreshnessState::Behind { commits: None },
                    established_at_unix: Some(Self::now_unix_seconds()),
                },
                Ok(false) => Freshness {
                    state: FreshnessState::UpToDate,
                    established_at_unix: Some(Self::now_unix_seconds()),
                },
                Err(_) => Freshness::not_checked(),
            };
        }

        let marketplace = package.id.marketplace();
        // No catalogue knows this package: `uze add <path|git>` installs
        // under a marketplace nothing registered, so there is no ref for it
        // to be behind.
        let Ok(Some(record)) = uze_core::state::marketplace_get(&self.home, marketplace) else {
            return Freshness::unpinned();
        };
        // A marketplace read from a checkout this machine develops has no
        // meaningful "newer": the working tree is what exists, and it
        // changes whenever its author saves.
        if let Some(checkout) = record.link {
            return Freshness {
                state: FreshnessState::Linked { checkout },
                established_at_unix: None,
            };
        }
        let uze_core::ResolvedSource::Git { commit, .. } = &package.provenance.resolved else {
            return Freshness::unpinned();
        };
        // Read from the entry the mirror wrote when it was last brought up
        // to date — a JSON read, not a subprocess. Asking Git here instead
        // was measurably wrong: one `rev-parse` per installed package put
        // the machine snapshot over its budget, and this is a read path
        // that every listing and every refresh pays.
        //
        // The distance is not computed here for the same reason. "There is
        // something newer" is what a listing needs; how far is a question
        // the detail view can afford to ask.
        let Some(mirrored) = marketplace_catalogue::mirrored_head(&self.home, marketplace) else {
            return Freshness::not_checked();
        };
        let state = if mirrored.commit == *commit {
            FreshnessState::UpToDate
        } else {
            FreshnessState::Behind { commits: None }
        };
        Freshness {
            state,
            established_at_unix: Some(mirrored.at_unix),
        }
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
                error: {
                    let _span = tracing::info_span!(
                        "integration.republish",
                        integration = integration.id()
                    )
                    .entered();
                    integration
                        .republish_packages(&packages)
                        .err()
                        .map(|error| error.to_string())
                },
            })
            .collect()
    }

    /// `republish_all`, for the integrations whose derived view no longer
    /// matches `packages`: one outcome per view it rebuilt.
    pub(crate) fn republish_unpublished(
        &self,
        packages: &[StoredPackage],
    ) -> Vec<PublicationOutcome> {
        self.integrations
            .iter()
            .filter(|integration| {
                matches!(
                    integration.publication(packages),
                    PublicationStatus::Unpublished(_)
                )
            })
            .map(|integration| {
                let _span =
                    tracing::info_span!("integration.republish", integration = integration.id())
                        .entered();
                PublicationOutcome {
                    integration: integration.id().to_owned(),
                    error: integration
                        .republish_packages(packages)
                        .err()
                        .map(|error| error.to_string()),
                }
            })
            .collect()
    }

    /// What the marketplace registered as `name` at `source` offers, from
    /// the catalogue cache (see `marketplace_catalogue`).
    pub(crate) fn catalogue(
        &self,
        name: &str,
        source: &PackageSource,
    ) -> Result<marketplace_catalogue::Catalogue> {
        self.marketplace_catalogues.read(name, source)
    }

    /// `catalogue`, for a reader that must answer without reaching a
    /// remote — every path a keystroke or a click is waiting on.
    pub(crate) fn catalogue_as_it_stands(
        &self,
        name: &str,
        source: &PackageSource,
    ) -> Result<marketplace_catalogue::Catalogue> {
        self.marketplace_catalogues.read_as_it_stands(name, source)
    }

    pub(crate) fn installed_packages(&self) -> Vec<StoredPackage> {
        self.store
            .package_ids()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|id| self.store.package(&id).ok())
            .collect()
    }

    /// The registered integration a person or a record names: by its stable
    /// id (`claude-code`), an alias people type (`claude`), or the label UZE
    /// shows back (`Claude Code`). There is deliberately no central list of
    /// vendors: an integration declares its own names, so registering one is
    /// the only step needed to make it selectable.
    pub(crate) fn integration_named(&self, name: &str) -> Option<&dyn IntegrationPort> {
        self.integrations
            .iter()
            .map(|integration| integration.as_ref())
            .find(|integration| {
                integration.id() == name
                    || integration.aliases().contains(&name)
                    || integration.display_name() == name
            })
    }

    pub(crate) fn resolve_integration_id(&self, requested: &str) -> Result<&'static str> {
        self.integration_named(requested)
            .map(|integration| integration.id())
            .ok_or_else(|| UzeError::UnknownHarness {
                requested: requested.to_owned(),
                known: self
                    .integrations
                    .iter()
                    .map(|integration| integration.id())
                    .collect::<Vec<_>>()
                    .join(", "),
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

    pub(crate) fn reconcile(&self, package_id: &str) -> ReconciliationReport {
        reconcile_package(&self.home, package_id, &self.integration_ports())
    }

    fn integration_ports(&self) -> Vec<&dyn IntegrationPort> {
        self.integrations
            .iter()
            .map(|integration| integration.as_ref() as &dyn IntegrationPort)
            .collect()
    }

    /// The READ-ONLY cousin of `reconcile`: same report shape, but each
    /// receipt's `Matched` verdict may come from the inspection cache
    /// (ADR 018) instead of a live vendor-CLI probe. Anomalies are never
    /// cached, so the report's warnings are always fresh. This is for
    /// report/health surfaces only — removal planning and detach MUST keep
    /// going through [`reconcile`](Self::reconcile), whose live verdict is
    /// what makes ownership checks trustworthy.
    pub(crate) fn reconcile_cached_report(&self, package_id: &str) -> ReconciliationReport {
        reconcile_package_with(
            &self.home,
            package_id,
            &self.integration_ports(),
            |ledger_key, receipt, integration| {
                let fingerprint = receipt.artifact.fingerprint();
                if let Some(cached) = self
                    .inspection_cache
                    .get(ledger_key, fingerprint.as_deref())
                {
                    return cached;
                }
                let _span = tracing::info_span!(
                    "integration.inspect",
                    integration = %receipt.integration,
                    receipt = %ledger_key
                )
                .entered();
                let live = integration.inspect_receipt(receipt);
                self.inspection_cache.put(ledger_key, &live, fingerprint);
                live
            },
        )
    }
}

/// Idempotently points `link` at `target`, refusing to overwrite anything
/// at `link` that is not already a UZE-created symlink to something else —
/// the same conflict-safety shape `ClaudeIntegration`'s own skill symlink
/// helper uses.
fn refresh_shim_symlink(target: &Path, link: &Path) -> Result<()> {
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
    uze_core::persistence::create_symlink(target, link)
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tracing_tests;
