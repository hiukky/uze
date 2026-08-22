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
    exposure::{ExposurePlan, PackageExposurePlan},
    integration::{
        AttachmentState, HarnessDetection, IntegrationPort, IntegrationStatus, PublicationStatus,
    },
    provisioning::{ProcessRunner, ProvisionStatus, ProvisioningResult, SystemProcessRunner},
    reconciliation::{PackageRemovalPlan, ReconciliationReport, reconcile_package},
    router::HarnessCapabilities,
    state,
    store::StoredPackage,
    trust::{self, TrustAuthority, TrustOutcome, TrustRequest},
};
use uze_integrations::{
    claude::ClaudeIntegration, codex::CodexIntegration, gemini::GeminiIntegration,
    opencode::OpenCodeIntegration,
};

use crate::bootstrap;

mod context;
mod doctor;
mod lifecycle;
mod marketplace;
mod project_environment;
mod read_models;

// Re-export project environment types for CLI access.
pub use project_environment::{
    InstallReport, ProjectEnvironment, ProjectEnvironmentPlan, ProjectLockStatus,
    ProjectPluginHealth, RemoveProjectPluginReport,
};

/// `LEGACY/PERSISTENT CONTEXT DELIVERY STRATEGY`. Harnesses that read a
/// project's shared `AGENTS.md` only through an explicit bridge region
/// written into their own native file *inside the project's working tree*,
/// rather than natively — see `docs/capabilities/instructions-design.md`
/// Fase 4. Codex and OpenCode are deliberately absent: both read
/// `AGENTS.md` directly, so `context_reconcile` never needs to write
/// anything into a Codex- or OpenCode-specific file at all. This list is
/// explicit, hardcoded vendor knowledge — appropriate here, in the
/// composition root that already names every concrete integration by type,
/// and not something `uze-core` or `IntegrationPort` needs to know.
///
/// Kept, unchanged, alongside the newer `EXPERIMENTAL RUNTIME DELIVERY
/// STRATEGY` (`ClaudeIntegration::runtime_contribution`, driven through the
/// PATH shim rather than through `context reconcile`). Whether runtime
/// projection ever replaces this bridge for Claude is a separate, later
/// decision pending an empirical interactive comparison — see the
/// Checkpoint 2 report. Do not remove or fold this into the experimental
/// path without that comparison.
const BRIDGE_INTEGRATIONS: &[(&str, &str)] =
    &[("claude-code", "CLAUDE.md"), ("gemini", "GEMINI.md")];

/// Fixed, package-independent region identity: the bridge is shared
/// infrastructure for however many packages currently contribute to
/// `AGENTS.md`, never owned by one of them (see Fase C.5 of the design).
const INSTRUCTION_BRIDGE_IDENTITY: &str = "instruction-bridge";

/// The vendor-documented import syntax both Claude Code and Gemini CLI
/// share for pulling another Markdown file's content into their own native
/// instructions file (`@AGENTS.md`).
const INSTRUCTION_BRIDGE_CONTENT: &str = "@AGENTS.md";

/// Harnesses that read a project's shared `AGENTS.md` directly, with no
/// artifact of their own — reported here purely for `context_inspect`'s
/// benefit (Codex/OpenCode still never appear in `BRIDGE_INTEGRATIONS`,
/// since `context_reconcile` genuinely writes nothing for them).
const NATIVE_INSTRUCTION_INTEGRATIONS: &[&str] = &["codex", "opencode"];

pub struct UzeApplication {
    home: UzeHome,
    store: UzeStore,
    integrations: Vec<Box<dyn IntegrationPort>>,
    runner: Box<dyn ProcessRunner>,
    detection_cache: DetectionCache,
}

impl UzeApplication {
    /// Production composition root. Concrete harness details remain inside
    /// this layer, not in CLI or TUI code.
    pub fn from_env(home: UzeHome) -> Result<Self> {
        Ok(Self::new(
            home.clone(),
            vec![
                Box::new(ClaudeIntegration::from_env(home.clone())?),
                Box::new(CodexIntegration::from_env(home.clone())?),
                Box::new(OpenCodeIntegration::from_env(home.clone())?),
                // EXPERIMENTAL / CONFORMANCE. Registered to exercise the
                // vendor-neutral core against a fourth, differently shaped
                // harness; not a v0 support claim. See integrations/gemini.rs.
                Box::new(GeminiIntegration::from_env(home)?),
            ],
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
        Ok(Self::new_with_runner(
            home.clone(),
            vec![
                Box::new(ClaudeIntegration::from_env(home.clone())?),
                Box::new(CodexIntegration::from_env(home.clone())?),
                Box::new(OpenCodeIntegration::from_env(home.clone())?),
                Box::new(GeminiIntegration::from_env(home)?),
            ],
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
        Self {
            store: UzeStore::new(home.clone()),
            detection_cache: DetectionCache::new(&home),
            home,
            integrations,
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
    /// `docs/architecture/overview.md`'s Marketplace section.
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
        let installed_ids: BTreeSet<&str> = bootstrap::DEFAULT_PLUGIN_IDS.iter().copied().collect();
        for package_id in self.store.package_ids().unwrap_or_default() {
            if !installed_ids.contains(package_id.as_str()) {
                continue;
            }
            let Ok(package) = self.store.package(&package_id) else {
                continue;
            };
            for integration in &self.integrations {
                if self.detect_cached(integration.as_ref()).present {
                    self.attach_package_to(&package, integration.as_ref())?;
                }
            }
        }
        let _ = self.republish_all();
        Ok(installed_any)
    }

    /// Installs default plugin `id` if it is not already in the Store.
    /// Never touches an already-installed copy — see `ensure_default_plugins`.
    pub(crate) fn ensure_default_plugin_installed(&self, id: &str) -> Result<bool> {
        let already_installed = self
            .store
            .package_ids()?
            .iter()
            .any(|package_id| package_id.as_str() == id);
        if already_installed {
            return Ok(false);
        }
        let materialized = bootstrap::materialize(id)?;
        self.install_materialized(materialized, &trust::NoTrustAuthority, &[], false)?;
        Ok(true)
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
            let manifest_path = path.join("marketplace.json");
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
                let manifest_path = path.join("marketplace.json");
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
                let manifest_path = root.join("marketplace.json");
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
                self.attach_stored_packages_to(integration.as_ref())?;
                result.runtime_shim = self.ensure_runtime_shim(integration.as_ref())?;
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
    /// not a `CONTEXT DELIVERY POLICY` decision; see `BRIDGE_INTEGRATIONS`).
    pub(crate) fn ensure_runtime_shim(
        &self,
        integration: &dyn IntegrationPort,
    ) -> Result<Option<RuntimeShimSetup>> {
        if !integration.supports_runtime_integration() {
            return Ok(None);
        }
        let shim_name = integration
            .aliases()
            .first()
            .copied()
            .unwrap_or(integration.id());
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

        let on_path = std::env::var_os("PATH")
            .map(|path| std::env::split_paths(&path).any(|entry| entry == shims_dir))
            .unwrap_or(false);

        let mut rc_file_updated = None;
        let mut path_hint = None;
        if !on_path {
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
                        path_hint = Some(format!(
                            "open a new terminal, or run: source {}",
                            target.rc_file.display()
                        ));
                    }
                    // The rc file has a marker in a shape this function
                    // doesn't recognize (edited by hand, presumably) —
                    // refuse to guess, fall back to the manual instruction.
                    Err(_) => path_hint = Some(manual_export),
                },
                // No detected shell (uncommon shell, `$SHELL`/`$HOME` unset)
                // — nothing to edit, same manual fallback.
                None => path_hint = Some(manual_export),
            }
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
    /// `shared_agent_skill_root` (OpenCode, Codex, and Gemini CLI all read
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
        self.store
            .package_ids()?
            .into_iter()
            .find(|id| id.as_str() == name)
            .map(|id| self.store.package(&id))
            .transpose()?
            .ok_or_else(|| UzeError::UnknownPackage(name.to_owned()))
    }

    pub(crate) fn plugin_summary(&self, package: &StoredPackage) -> Result<PluginSummary> {
        let environment = self.engine().compose(std::slice::from_ref(&package.id))?;
        let update_available = match &package.provenance.requested {
            PackageSource::Embedded { id } => bootstrap::has_update(id, &package.root).ok(),
            _ => None,
        };
        Ok(PluginSummary {
            id: package.id.as_str().to_owned(),
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
}

#[derive(Clone, Debug, Serialize)]
pub struct PluginSummary {
    pub id: String,
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
    /// ordinary part of this `setup` call — see `BRIDGE_INTEGRATIONS`'s
    /// doc comment for how this relates to the existing, still-default,
    /// persistent `CLAUDE.md`/`GEMINI.md` bridge. `None` for every
    /// integration with no runtime-integration story (not an error).
    pub runtime_shim: Option<RuntimeShimSetup>,
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
    /// Whether this harness reads a project's AGENTS.md directly rather
    /// than through UZE's managed bridge region. Instructions are not a
    /// `Resource`/`CapabilityKind::Instruction` routed through `capabilities`
    /// above — they're a distinct delivery mechanism (see `context` module)
    /// — so this is sourced from the same `NATIVE_INSTRUCTION_INTEGRATIONS`
    /// list `context_reconcile` itself uses, not re-derived.
    pub native_instructions: bool,
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
    pub portability: Portability,
    /// Human-readable notices for a state worth surfacing but that is not
    /// itself a gap or an error — e.g. a harness carrying legitimate
    /// vendor-specific content alongside its bridge. Never a suggestion to
    /// consolidate or an automatic action.
    pub warnings: Vec<String>,
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
    pub bridges: Vec<BridgePlan>,
}

impl ContextPlan {
    pub fn has_changes(&self) -> bool {
        self.agents_md_plan.has_changes()
            || self.bridges.iter().any(|bridge| {
                matches!(
                    bridge.action,
                    instruction_context::PlannedAction::Attach
                        | instruction_context::PlannedAction::Remove
                )
            })
    }
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
    pub bridges: Vec<BridgeStatus>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PackageManagedState {
    pub plugin: String,
    pub state: ManagedStateSummary,
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

#[cfg(test)]
mod tests {
    use std::{
        cell::Cell,
        fs,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    use super::*;
    use uze_core::{
        capability::CapabilityKind,
        exposure::{ExposureMechanism, ExposurePlan},
        integration::{AttachmentReceipt, HarnessDetection, ManagedArtifact},
        project::Resource,
        router::{CompatibilityRoute, HarnessCapabilities, VerificationStatus},
    };

    struct SymlinkIntegration;
    impl IntegrationPort for SymlinkIntegration {
        fn id(&self) -> &'static str {
            "test"
        }
        fn capabilities(&self) -> uze_core::router::HarnessCapabilities {
            HarnessCapabilities::default()
        }
        fn exposure_plan(&self, resource: &Resource) -> ExposurePlan {
            ExposurePlan {
                representation: resource.capability.representation,
                route: CompatibilityRoute::Adaptable,
                verification: VerificationStatus::Unverified,
                mechanism: ExposureMechanism::Unsupported {
                    rationale: "test does not attach".to_owned(),
                },
                evidence: "test".to_owned(),
            }
        }
    }

    struct PartialIntegration {
        root: PathBuf,
        attached: Cell<bool>,
    }

    struct AbsentIntegration {
        attach_attempted: Cell<bool>,
    }

    struct AllResourceSymlinkIntegration {
        root: PathBuf,
    }

    impl IntegrationPort for AllResourceSymlinkIntegration {
        fn id(&self) -> &'static str {
            "all-resources"
        }

        fn capabilities(&self) -> HarnessCapabilities {
            HarnessCapabilities::default()
        }

        fn detect(&self) -> HarnessDetection {
            HarnessDetection {
                present: true,
                version: None,
            }
        }

        fn exposure_plan(&self, resource: &Resource) -> ExposurePlan {
            ExposurePlan {
                representation: resource.capability.representation,
                route: CompatibilityRoute::Adaptable,
                verification: VerificationStatus::Unverified,
                mechanism: ExposureMechanism::Unsupported {
                    rationale: "test attachment is implemented directly".to_owned(),
                },
                evidence: "test".to_owned(),
            }
        }

        fn attach_receipt(&self, resource: &Resource) -> Result<Option<AttachmentReceipt>> {
            let path = self.root.join(resource.name());
            #[cfg(unix)]
            {
                let already_correct = fs::read_link(&path)
                    .map(|target| target == resource.capability.path)
                    .unwrap_or(false);
                if !already_correct {
                    if path.symlink_metadata().is_ok() {
                        fs::remove_file(&path).map_err(|source| UzeError::Write {
                            path: path.clone(),
                            source,
                        })?;
                    }
                    std::os::unix::fs::symlink(&resource.capability.path, &path).map_err(
                        |source| UzeError::Write {
                            path: path.clone(),
                            source,
                        },
                    )?;
                }
            }
            Ok(Some(AttachmentReceipt {
                package_id: match &resource.origin {
                    uze_core::ResourceOrigin::Package { id, .. } => id.as_str().to_owned(),
                    _ => unreachable!(),
                },
                resource_identity: Some(resource.identity()),
                integration: self.id().to_owned(),
                strategy: "test".to_owned(),
                artifact: ManagedArtifact::SymlinkReference {
                    path,
                    target: resource.capability.path.clone(),
                },
            }))
        }
    }

    impl IntegrationPort for PartialIntegration {
        fn id(&self) -> &'static str {
            "partial"
        }

        fn capabilities(&self) -> HarnessCapabilities {
            HarnessCapabilities::default()
        }

        fn detect(&self) -> HarnessDetection {
            HarnessDetection {
                present: true,
                version: None,
            }
        }

        fn exposure_plan(&self, resource: &Resource) -> ExposurePlan {
            ExposurePlan {
                representation: resource.capability.representation,
                route: CompatibilityRoute::Adaptable,
                verification: VerificationStatus::Unverified,
                mechanism: ExposureMechanism::Unsupported {
                    rationale: "test attachment is implemented directly".to_owned(),
                },
                evidence: "test".to_owned(),
            }
        }

        fn attach_receipt(&self, resource: &Resource) -> Result<Option<AttachmentReceipt>> {
            if resource.name() == "github" {
                return Err(UzeError::ExposureUnavailable(
                    "simulated second attachment failure".to_owned(),
                ));
            }
            let path = self.root.join("first-managed-resource");
            #[cfg(unix)]
            std::os::unix::fs::symlink(&resource.capability.path, &path).map_err(|source| {
                UzeError::Write {
                    path: path.clone(),
                    source,
                }
            })?;
            self.attached.set(true);
            Ok(Some(AttachmentReceipt {
                package_id: match &resource.origin {
                    uze_core::ResourceOrigin::Package { id, .. } => id.as_str().to_owned(),
                    _ => unreachable!(),
                },
                resource_identity: Some(resource.identity()),
                integration: self.id().to_owned(),
                strategy: "test".to_owned(),
                artifact: ManagedArtifact::SymlinkReference {
                    path,
                    target: resource.capability.path.clone(),
                },
            }))
        }
    }

    impl IntegrationPort for AbsentIntegration {
        fn id(&self) -> &'static str {
            "absent"
        }

        fn capabilities(&self) -> HarnessCapabilities {
            HarnessCapabilities::default()
        }

        fn exposure_plan(&self, resource: &Resource) -> ExposurePlan {
            ExposurePlan {
                representation: resource.capability.representation,
                route: CompatibilityRoute::Adaptable,
                verification: VerificationStatus::Unverified,
                mechanism: ExposureMechanism::Unsupported {
                    rationale: "an absent integration must not attach".to_owned(),
                },
                evidence: "test".to_owned(),
            }
        }

        fn attach_receipt(&self, _resource: &Resource) -> Result<Option<AttachmentReceipt>> {
            self.attach_attempted.set(true);
            Err(UzeError::ExposureUnavailable(
                "absent integration was invoked".to_owned(),
            ))
        }
    }

    pub(crate) fn temp(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "uze-application-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    pub(crate) fn fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/packages/agent-plugin-skill")
    }

    pub(crate) fn multi_mcp_fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/packages/multi-mcp-plugin")
    }

    #[test]
    pub(crate) fn list_and_inspect_are_package_centric() {
        let root = temp("inspect");
        let app = UzeApplication::new(UzeHome::at(&root), vec![Box::new(SymlinkIntegration)]);
        app.add_plugin(
            uze_core::PackageSource::local(fixture()),
            &uze_core::trust::AlwaysTrust,
        )
        .unwrap();
        let listed = app.list_plugins().unwrap();
        assert_eq!(listed.len(), 1);
        let inspection = app.inspect_plugin(&listed[0].id).unwrap();
        assert_eq!(inspection.plugin.id, listed[0].id);
        assert_eq!(inspection.capabilities[0].kind, CapabilityKind::AgentSkill);
        assert_eq!(inspection.deliveries.len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    pub(crate) fn add_installs_portable_package_without_invoking_absent_harnesses() {
        let root = temp("absent-harness");
        let absent = AbsentIntegration {
            attach_attempted: Cell::new(false),
        };
        let app = UzeApplication::new(UzeHome::at(&root), vec![Box::new(absent)]);
        app.add_plugin(
            uze_core::PackageSource::local(fixture()),
            &uze_core::trust::AlwaysTrust,
        )
        .unwrap();
        assert_eq!(app.list_plugins().unwrap().len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    pub(crate) fn removal_uses_reconciliation_and_preserves_drift() {
        use std::os::unix::fs::symlink;
        let root = temp("remove");
        let home = UzeHome::at(&root);
        let app = UzeApplication::new(home.clone(), vec![Box::new(SymlinkIntegration)]);
        let package = app
            .store
            .ingest(
                &uze_core::acquisition::acquire(&uze_core::PackageSource::local(fixture()))
                    .unwrap(),
            )
            .unwrap();
        let expected = package.root.join("skills/uze-e2e");
        let managed = root.join("managed");
        symlink(&expected, &managed).unwrap();
        state::record_receipt(
            &home,
            "receipt".to_owned(),
            AttachmentReceipt {
                package_id: package.id.as_str().to_owned(),
                resource_identity: None,
                integration: "test".to_owned(),
                strategy: "symlink".to_owned(),
                artifact: ManagedArtifact::SymlinkReference {
                    path: managed.clone(),
                    target: expected.clone(),
                },
            },
        )
        .unwrap();
        assert!(matches!(
            app.remove_plugin(package.id.as_str()).unwrap(),
            RemovePluginReport::Removed { .. }
        ));
        assert!(!managed.exists());
        assert!(app.store.package(&package.id).is_err());

        let package = app
            .store
            .ingest(
                &uze_core::acquisition::acquire(&uze_core::PackageSource::local(fixture()))
                    .unwrap(),
            )
            .unwrap();
        let foreign = root.join("foreign");
        fs::create_dir_all(&foreign).unwrap();
        symlink(&foreign, &managed).unwrap();
        state::record_receipt(
            &home,
            "receipt".to_owned(),
            AttachmentReceipt {
                package_id: package.id.as_str().to_owned(),
                resource_identity: None,
                integration: "test".to_owned(),
                strategy: "symlink".to_owned(),
                artifact: ManagedArtifact::SymlinkReference {
                    path: managed.clone(),
                    target: package.root.clone(),
                },
            },
        )
        .unwrap();
        assert!(matches!(
            app.remove_plugin(package.id.as_str()).unwrap(),
            RemovePluginReport::Blocked {
                plan: PackageRemovalPlan::BlockedByDrift,
                ..
            }
        ));
        assert!(app.store.package(&package.id).is_ok());
        assert_eq!(fs::read_link(&managed).unwrap(), foreign);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    pub(crate) fn doctor_reports_corrupt_ledger_without_destructive_work() {
        let root = temp("doctor");
        let home = UzeHome::at(&root);
        home.ensure_layout().unwrap();
        fs::write(home.state_dir().join("attachments.json"), "bad").unwrap();
        fs::write(home.integrations_state_path(), "bad").unwrap();
        let app = UzeApplication::new(home.clone(), vec![Box::new(SymlinkIntegration)]);
        let report = app.doctor();
        assert!(report.ledger_error.is_some());
        assert!(report.integration_state_error.is_some());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    pub(crate) fn add_failure_after_a_confirmed_attachment_leaves_reconcilable_ledger_evidence() {
        let root = temp("partial-add");
        let home = UzeHome::at(&root);
        let integration = PartialIntegration {
            root: root.clone(),
            attached: Cell::new(false),
        };
        let app = UzeApplication::new(home.clone(), vec![Box::new(integration)]);
        assert!(
            app.add_plugin(
                uze_core::PackageSource::local(multi_mcp_fixture()),
                &uze_core::trust::AlwaysTrust
            )
            .is_err()
        );
        assert!(
            app.store
                .package_ids()
                .unwrap()
                .iter()
                .any(|id| id.as_str() == "multi-mcp-plugin")
        );
        let receipts = state::receipts(&home, Some("multi-mcp-plugin")).unwrap();
        assert_eq!(receipts.len(), 1);
        assert_eq!(app.doctor().attachments[0].state.matched, 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    pub(crate) fn remove_is_idempotent_without_claiming_history_for_absent_state() {
        let root = temp("remove-twice");
        let app = UzeApplication::new(UzeHome::at(&root), vec![Box::new(SymlinkIntegration)]);
        let package = app
            .store
            .ingest(
                &uze_core::acquisition::acquire(&uze_core::PackageSource::local(fixture()))
                    .unwrap(),
            )
            .unwrap();
        assert!(matches!(
            app.remove_plugin(package.id.as_str()).unwrap(),
            RemovePluginReport::Removed { .. }
        ));
        assert!(matches!(
            app.remove_plugin(package.id.as_str()).unwrap(),
            RemovePluginReport::AlreadyAbsent { .. }
        ));
        app.add_plugin(
            uze_core::PackageSource::local(fixture()),
            &uze_core::trust::AlwaysTrust,
        )
        .unwrap();
        assert_eq!(app.list_plugins().unwrap().len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    pub(crate) fn multi_mcp_package_has_independent_receipts_through_safe_removal() {
        let root = temp("multi-mcp-lifecycle");
        let home = UzeHome::at(&root);
        let app = UzeApplication::new(
            home.clone(),
            vec![Box::new(AllResourceSymlinkIntegration {
                root: root.clone(),
            })],
        );
        app.add_plugin(
            uze_core::PackageSource::local(multi_mcp_fixture()),
            &uze_core::trust::AlwaysTrust,
        )
        .unwrap();
        let receipts = state::receipts(&home, Some("multi-mcp-plugin")).unwrap();
        assert_eq!(receipts.len(), 2);
        assert_ne!(
            receipts[0].1.resource_identity,
            receipts[1].1.resource_identity
        );
        assert!(matches!(
            app.remove_plugin("multi-mcp-plugin").unwrap(),
            RemovePluginReport::Removed { .. }
        ));
        assert!(
            state::receipts(&home, Some("multi-mcp-plugin"))
                .unwrap()
                .is_empty()
        );
        fs::remove_dir_all(root).unwrap();
    }

    pub(crate) fn mcp_fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/packages/agent-plugin-mcp")
    }

    // --- Marketplace bootstrap: install-only, never silent-update --------

    #[test]
    pub(crate) fn bootstrap_installs_exactly_the_default_policy_and_is_idempotent() {
        let root = temp("bootstrap-default");
        let app = UzeApplication::new(UzeHome::at(&root), Vec::new());

        assert!(app.ensure_default_plugins().unwrap(), "first call installs");
        let installed: Vec<String> = app
            .list_plugins()
            .unwrap()
            .into_iter()
            .map(|p| p.id)
            .collect();
        assert_eq!(installed, bootstrap::DEFAULT_PLUGIN_IDS);

        assert!(
            !app.ensure_default_plugins().unwrap(),
            "second call installs nothing new"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    pub(crate) fn bootstrap_never_mutates_an_already_installed_default_plugin() {
        let root = temp("bootstrap-no-silent-update");
        let app = UzeApplication::new(UzeHome::at(&root), Vec::new());
        app.ensure_default_plugins().unwrap();

        let package = app.package_by_name("uze").unwrap();
        let manifest_path = package.root.join("plugin.json");
        fs::write(&manifest_path, "{\"name\":\"uze\",\"tampered\":true}").unwrap();
        let tampered = fs::read_to_string(&manifest_path).unwrap();

        // A read-only-shaped call (every CLI command runs this) must not
        // touch the tampered content, even though it clearly differs from
        // the embedded snapshot.
        assert!(!app.ensure_default_plugins().unwrap());
        assert_eq!(fs::read_to_string(&manifest_path).unwrap(), tampered);

        // The drift is still visible — read-only, informational.
        let summary = app
            .plugin_summary(&app.package_by_name("uze").unwrap())
            .unwrap();
        assert_eq!(summary.update_available, Some(true));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    pub(crate) fn read_only_bootstrap_leaves_store_state_byte_identical_on_repeat() {
        let root = temp("bootstrap-snapshot");
        let home = UzeHome::at(&root);
        let app = UzeApplication::new(home.clone(), Vec::new());
        app.ensure_default_plugins().unwrap();

        let packages_before = fs::read(home.registry_path()).unwrap();
        let attachments_path = home.state_dir().join("attachments.json");
        let attachments_before = fs::read(&attachments_path).ok();

        app.ensure_default_plugins().unwrap();
        app.list_plugins().unwrap();
        app.doctor();

        assert_eq!(fs::read(home.registry_path()).unwrap(), packages_before);
        assert_eq!(fs::read(&attachments_path).ok(), attachments_before);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    pub(crate) fn existing_receipts_survive_repeated_bootstrap_unchanged() {
        let root = temp("bootstrap-receipts");
        let home = UzeHome::at(&root);
        let app = UzeApplication::new(
            home.clone(),
            vec![Box::new(AllResourceSymlinkIntegration {
                root: root.clone(),
            })],
        );
        app.ensure_default_plugins().unwrap();
        let before = state::receipts(&home, Some("uze")).unwrap();
        assert!(!before.is_empty());

        app.ensure_default_plugins().unwrap();
        let after = state::receipts(&home, Some("uze")).unwrap();
        assert_eq!(before, after);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    pub(crate) fn a_default_plugin_that_would_cross_the_trust_boundary_is_not_installed_silently() {
        let root = temp("bootstrap-trust");
        let app = UzeApplication::new(UzeHome::at(&root), Vec::new());

        // Simulate a hypothetical embedded snapshot revision that declares
        // an MCP server — exactly the scenario Fase I describes. `Embedded`
        // sources cross the trust boundary (see `crosses_trust_boundary`),
        // so a non-interactive authority must refuse, not install.
        let mut materialized =
            uze_core::acquisition::acquire(&uze_core::PackageSource::local(mcp_fixture())).unwrap();
        let fixture_root = materialized.root().to_path_buf();
        materialized.retarget(
            fixture_root,
            uze_core::Provenance {
                requested: uze_core::PackageSource::Embedded {
                    id: "uze-mcp-conformance".to_owned(),
                },
                resolved: uze_core::ResolvedSource::Embedded {
                    id: "uze-mcp-conformance".to_owned(),
                },
            },
        );

        let result =
            app.install_materialized(materialized, &uze_core::trust::NoTrustAuthority, &[], false);
        assert!(matches!(result, Err(UzeError::TrustRequired { .. })));
        assert!(
            app.list_plugins().unwrap().is_empty(),
            "nothing was installed"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    pub(crate) fn a_corrupted_stored_copy_reports_unknown_update_status_without_panicking() {
        let root = temp("bootstrap-corrupt");
        let app = UzeApplication::new(UzeHome::at(&root), Vec::new());
        app.ensure_default_plugins().unwrap();

        let package = app.package_by_name("uze").unwrap();
        fs::remove_file(package.root.join("plugin.json")).unwrap();

        let summary = app
            .plugin_summary(&app.package_by_name("uze").unwrap())
            .unwrap();
        assert_eq!(summary.update_available, Some(true));

        fs::remove_dir_all(&package.root).unwrap();
        let summary = app
            .plugin_summary(&app.package_by_name("uze").unwrap())
            .unwrap();
        assert_eq!(summary.update_available, None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    pub(crate) fn official_embedded_plugin_is_protected_from_remove_but_allows_update() {
        let root = temp("protected-update");
        let home = UzeHome::at(&root);
        let app = UzeApplication::new(home, Vec::new());
        app.add_plugin(
            uze_core::PackageSource::Embedded {
                id: "uze".to_owned(),
            },
            &uze_core::trust::AlwaysTrust,
        )
        .unwrap();

        let err = app.remove_plugin("uze").unwrap_err();
        assert!(
            err.to_string()
                .contains("official marketplace plugin `uze` is protected"),
            "expected protected error, got: {err}"
        );

        let report = app
            .update_plugin("uze", &uze_core::trust::AlwaysTrust)
            .unwrap();
        assert!(
            matches!(report, UpdatePluginReport::Updated { .. }),
            "expected Updated, got: {report:?}"
        );
        assert!(app.package_by_name("uze").is_ok());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    pub(crate) fn local_spoof_named_uze_is_not_protected() {
        let root = temp("spoof-not-protected");
        let spoof_src = temp("spoof-src-uze");
        fs::create_dir_all(spoof_src.join("skills/spoof")).unwrap();
        fs::write(
            spoof_src.join("plugin.json"),
            r#"{"name":"uze","description":"spoof","version":"0.1.0"}"#,
        )
        .unwrap();
        fs::write(spoof_src.join("skills/spoof/SKILL.md"), "# Spoof\n").unwrap();

        let home = UzeHome::at(&root);
        let app = UzeApplication::new(home, Vec::new());
        app.add_plugin(
            uze_core::PackageSource::Local {
                path: spoof_src.clone(),
            },
            &uze_core::trust::AlwaysTrust,
        )
        .unwrap();

        let package = app.package_by_name("uze").unwrap();
        assert!(!UzeApplication::is_protected_package(&package));

        let report = app.remove_plugin("uze").unwrap();
        assert!(matches!(report, RemovePluginReport::Removed { .. }));
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(spoof_src).unwrap();
    }

    // --- cli-performance: detect_cached / DetectionCache integration ---
    // See ADR 018 and specs/cli-performance/spec.md. `FakeIntegration`
    // stands in for a slow harness (like `gemini`, ~2-11s per real
    // `--version` invocation) without spawning a real subprocess, and its
    // shared `Arc<AtomicUsize>` counter is what these tests assert
    // against: the whole point of `detect_cached` is that this counter
    // stays at 1 no matter how many call sites, command executions, or
    // (simulated) CLI invocations ask for the same integration's
    // detection result.

    struct FakeIntegration {
        id: &'static str,
        detection: HarnessDetection,
        delay: Duration,
        calls: Arc<AtomicUsize>,
    }

    impl FakeIntegration {
        fn new(id: &'static str, present: bool, calls: Arc<AtomicUsize>) -> Self {
            Self {
                id,
                detection: HarnessDetection {
                    present,
                    version: present.then(|| "1.0.0".to_owned()),
                },
                delay: Duration::ZERO,
                calls,
            }
        }

        fn with_delay(mut self, delay: Duration) -> Self {
            self.delay = delay;
            self
        }
    }

    impl IntegrationPort for FakeIntegration {
        fn id(&self) -> &'static str {
            self.id
        }

        fn capabilities(&self) -> HarnessCapabilities {
            HarnessCapabilities::default()
        }

        fn detect(&self) -> HarnessDetection {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if !self.delay.is_zero() {
                std::thread::sleep(self.delay);
            }
            self.detection.clone()
        }

        fn exposure_plan(&self, resource: &Resource) -> ExposurePlan {
            ExposurePlan {
                representation: resource.capability.representation,
                route: CompatibilityRoute::Adaptable,
                verification: VerificationStatus::Unverified,
                mechanism: ExposureMechanism::Unsupported {
                    rationale: "test does not attach".to_owned(),
                },
                evidence: "test".to_owned(),
            }
        }
    }

    #[test]
    fn detect_cached_calls_detect_at_most_once_per_command() {
        let root = temp("detect-cached-once-per-command");
        let calls = Arc::new(AtomicUsize::new(0));
        let fake = FakeIntegration::new("fake-a", true, calls.clone());
        let app = UzeApplication::new(UzeHome::at(&root), vec![Box::new(fake)]);
        let integration = app.integrations[0].as_ref();

        let _ = app.detect_cached(integration);
        let _ = app.detect_cached(integration);
        let _ = app.detect_cached(integration);

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "three calls within one UzeApplication (one command) must probe only once"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn detect_cached_reuses_the_on_disk_result_across_separate_uze_application_instances() {
        // A fresh `UzeApplication` instance stands in for a separate CLI
        // invocation: it shares nothing in-process with the first, only
        // the on-disk cache file under the same `UzeHome`.
        let root = temp("detect-cached-cross-invocation");
        let calls = Arc::new(AtomicUsize::new(0));

        let first = UzeApplication::new(
            UzeHome::at(&root),
            vec![Box::new(FakeIntegration::new(
                "fake-b",
                true,
                calls.clone(),
            ))],
        );
        let _ = first.detect_cached(first.integrations[0].as_ref());

        let second = UzeApplication::new(
            UzeHome::at(&root),
            vec![Box::new(FakeIntegration::new(
                "fake-b",
                true,
                calls.clone(),
            ))],
        );
        let _ = second.detect_cached(second.integrations[0].as_ref());

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "the second UzeApplication instance must reuse the first's on-disk result"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn prepare_detected_integrations_probes_each_integration_at_most_once() {
        // Regression test for the bug found while measuring end-to-end
        // timing (design.md decision 7): `install()` used to call
        // `self.detect()` again internally, on top of the one
        // `detect_cached` call `prepare_detected_integrations` already
        // made — two live probes per integration instead of one.
        let root = temp("prepare-detected-once");
        let calls = Arc::new(AtomicUsize::new(0));
        let fake = FakeIntegration::new("fake-c", true, calls.clone());
        let app = UzeApplication::new(UzeHome::at(&root), vec![Box::new(fake)]);

        let results = app.prepare_detected_integrations(None).unwrap();

        assert!(results[0].configured);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "prepare_detected_integrations must probe each integration exactly once, \
             including the install() step it triggers"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn provision_and_prepare_writes_through_the_cache_on_success() {
        let root = temp("write-through-on-provision");
        let calls = Arc::new(AtomicUsize::new(0));
        let fake = FakeIntegration::new("fake-d", true, calls.clone());
        let app = UzeApplication::new(UzeHome::at(&root), vec![Box::new(fake)]);

        let results = app.provision_and_prepare(None).unwrap();
        assert!(results[0].configured);
        let calls_after_provision = calls.load(Ordering::SeqCst);
        assert!(calls_after_provision >= 1);

        // The write-through (ADR 018 decision 3) means a `detect_cached`
        // call right after observes the fresh result without an extra
        // live probe.
        let _ = app.detect_cached(app.integrations[0].as_ref());
        assert_eq!(
            calls.load(Ordering::SeqCst),
            calls_after_provision,
            "detect_cached after a successful provision must not re-probe"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn cache_warm_detect_cached_meets_the_performance_budget() {
        // Stands in for `gemini --version`'s real-world 2-11s cost
        // without spawning a subprocess (see proposal.md's measurements).
        const SLOW_HARNESS_DELAY: Duration = Duration::from_millis(500);
        const BUDGET: Duration = Duration::from_millis(50);

        let root = temp("perf-budget");
        let calls = Arc::new(AtomicUsize::new(0));

        {
            let fake = FakeIntegration::new("slow-harness", true, calls.clone())
                .with_delay(SLOW_HARNESS_DELAY);
            let app = UzeApplication::new(UzeHome::at(&root), vec![Box::new(fake)]);
            let integration = app.integrations[0].as_ref();

            // Cold: pays the simulated delay once, populates both cache
            // tiers.
            let _ = app.detect_cached(integration);

            // Warm, same command: in-process memoization tier.
            let started = Instant::now();
            let _ = app.detect_cached(integration);
            let elapsed = started.elapsed();
            assert!(
                elapsed < BUDGET,
                "warm in-process detect_cached took {elapsed:?}, budget is {BUDGET:?}"
            );
        }

        {
            // A fresh UzeApplication simulates a separate CLI invocation:
            // only the on-disk tier is available, no in-process memo.
            let fake = FakeIntegration::new("slow-harness", true, calls.clone())
                .with_delay(SLOW_HARNESS_DELAY);
            let app = UzeApplication::new(UzeHome::at(&root), vec![Box::new(fake)]);
            let integration = app.integrations[0].as_ref();

            let started = Instant::now();
            let _ = app.detect_cached(integration);
            let elapsed = started.elapsed();
            assert!(
                elapsed < BUDGET,
                "warm cross-invocation detect_cached took {elapsed:?}, budget is {BUDGET:?}"
            );
        }

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "the simulated slow probe must be paid exactly once across the cold call \
             and both warm reads (in-process and cross-invocation)"
        );
        let _ = fs::remove_dir_all(&root);
    }
}
