//! Project environment use cases — project-scoped desired state.
//!
//! Provides `project_environment()`, `plan_project_environment()`,
//! `add_project_plugin()`, `remove_project_plugin()`, `install_project_environment()`.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use serde::Serialize;

use uze_core::{
    Result, UzeError,
    manifest::{self, DeclaredMarketplace},
    project_lock::{self, LockedMarketplace, LockedPlugin, ProjectLock},
    project_root,
    trust::{self, TrustAuthority},
};

use super::services::Project;
use super::*;

/// The manifest's spelling of a marketplace the lock recorded. Both files
/// name a source the same way, so this carries the keys across and adds
/// the one thing only the manifest has: the list of plugins taken from it,
/// left empty because `declare_plugin` pushes into whatever is already
/// declared rather than replacing it.
fn declared_marketplace_for(lock: &ProjectLock, marketplace: &str) -> Option<DeclaredMarketplace> {
    let locked = lock.marketplaces.get(marketplace)?;
    Some(DeclaredMarketplace {
        git: Some(locked.git.clone()),
        path: None,
        r#ref: locked.r#ref.clone(),
        subdirectory: locked.subdirectory.clone(),
        plugins: Vec::new(),
    })
}

/// A marketplace resolved far enough to read from: the repository behind
/// it, and the narrowing the declaration asked for.
struct MarketplaceRequest {
    repository: uze_core::acquisition::marketplace::MarketplaceRepository,
    reference: Option<String>,
    subdirectory: Option<PathBuf>,
}

impl MarketplaceRequest {
    /// What a machine-registered or declared source resolves to.
    fn of(source: &PackageSource) -> Result<Self> {
        let repository = uze_core::acquisition::marketplace::repository_of(source)?;
        let (reference, subdirectory) = match source {
            PackageSource::Git {
                reference,
                subdirectory,
                ..
            } => (reference.clone(), subdirectory.clone()),
            _ => (None, None),
        };
        Ok(Self {
            repository,
            reference,
            subdirectory,
        })
    }
}

impl Project<'_> {
    /// Read-only: observes the project's current state (lock + diagnostics).
    #[tracing::instrument(name = "project.environment", skip_all, fields(root = %root.display()), err)]
    pub fn environment(&self, root: &Path) -> Result<ProjectEnvironment> {
        let canonical = project_root::resolve_project_root(root)?;
        let _lock_path = project_lock::lock_path_for(&canonical);
        let lock = project_lock::load_lock(&canonical)?;
        let mut diagnostics = Vec::new();

        if let Some(lock) = &lock {
            for name in lock.marketplaces.keys() {
                if uze_core::state::marketplace_get(&self.0.home, name)?.is_none() {
                    diagnostics.push(format!(
                        "marketplace `{name}` in lock but not in global registry (will be \
                         resolved from lock source on install)"
                    ));
                }
            }
            for (plugin, locked) in &lock.plugins {
                if !lock.marketplaces.contains_key(&locked.marketplace) {
                    diagnostics.push(format!(
                        "plugin `{plugin}` references marketplace `{}` not declared in lock",
                        locked.marketplace
                    ));
                }
            }
        }

        Ok(ProjectEnvironment {
            root: root.to_path_buf(),
            canonical,
            lock,
            diagnostics,
        })
    }

    /// Read-only: computes what `install_project_environment` would do.
    ///
    /// `trust_required`, `delivery_changes`, and `offline_unavailable` are
    /// deliberately left empty rather than faked: each would require
    /// materializing a missing package just to *inspect* it (executable
    /// capabilities, delivery routes, offline availability) without
    /// installing it — a real feature this pass does not implement. Left
    /// as future work rather than reported as done; see
    /// `openspec/changes/project-agent-environment/tasks.md`.
    #[tracing::instrument(name = "project.plan", skip_all, fields(root = %root.display()), err)]
    pub fn plan(&self, root: &Path) -> Result<ProjectEnvironmentPlan> {
        let env = self.environment(root)?;
        let canonical = env.canonical.clone();
        // The manifest is the head of the chain, not the lock. A plan
        // founded on `agents.lock` cannot see the edit a person just made
        // to `agents.yaml`, which is the most common reason to ask for one
        // at all. Both documents are read; nothing is resolved.
        let manifest = manifest::load(&canonical)?.unwrap_or_default();
        let lock = env.lock.unwrap_or_default();
        let unresolved: Vec<String> = project_lock::stale_against(&manifest, &lock)
            .into_iter()
            .map(|stale| stale.plugin)
            .collect();
        let surplus = project_lock::surplus_against(&manifest, &lock);
        let stale_projection = self.stale_projection(&canonical);

        let installed_ids = self.installed_plugin_ids();
        let dependencies: Vec<LockedPlugin> = lock.plugins.values().cloned().collect();
        let installed: Vec<String> = lock
            .plugins
            .iter()
            .filter(|(name, locked)| {
                installed_ids.contains(&UzeApplication::locked_plugin_id(name, locked))
            })
            .map(|(name, _)| name.clone())
            .collect();
        let missing: Vec<LockedPlugin> = Self::missing_locked_plugins(&lock, &installed_ids)
            .into_iter()
            .map(|(_, locked)| locked.clone())
            .collect();

        // Deliberately deferred (see doc comment above): none of these
        // compare an *installed* package's provenance against what the
        // lock expects, which is what a real `conflicts` check would
        // need. `project_environment()`'s own `diagnostics` already
        // surfaces the one related, weaker signal available today (a
        // plugin referencing a marketplace not declared in the lock) —
        // not duplicated here as a false "conflict".
        let trust_required = Vec::new();
        let delivery_changes = Vec::new();
        let offline_unavailable = Vec::new();
        let conflicts = Vec::new();

        let has_changes = !missing.is_empty()
            || !unresolved.is_empty()
            || !surplus.is_empty()
            || stale_projection.is_some();

        Ok(ProjectEnvironmentPlan {
            dependencies,
            installed,
            missing,
            unresolved,
            surplus,
            stale_projection,
            trust_required,
            delivery_changes,
            conflicts,
            offline_unavailable,
            has_changes,
        })
    }

    /// Whether the projected worktree-policy region has fallen behind the
    /// policy the manifest declares, and what the two say.
    ///
    /// One string comparison, and no harness is asked anything: the region
    /// carries `WorktreePolicy::region_identity()`, a digest of the exact
    /// bytes it should hold, so "has the projection caught up" is answered
    /// by the identity already written into `AGENTS.md`.
    fn stale_projection(&self, canonical: &Path) -> Option<StaleProjection> {
        // Only a *declared* policy is owed a projection: an undeclared one
        // projects nothing, so it can never be behind. Same gate the
        // context service uses to decide whether the region exists at all.
        let policy = manifest::load(canonical).ok()??.worktrees?;
        let wanted = policy.region_identity();
        let agents_md = canonical.join(uze_core::project_context::AGENTS_MD_FILE_NAME);
        let found = uze_core::text_region::region_identities_present(&agents_md)
            .into_iter()
            .find(|identity| uze_core::worktree::WorktreePolicy::owns_region(identity));
        // A missing region is as behind as a stale one: the policy is
        // declared and the agents are reading nothing at all.
        (found.as_deref() != Some(wanted.as_str())).then(|| StaleProjection {
            declared: policy.completion.abi_name().to_owned(),
            projected_identity: found.unwrap_or_else(|| "none".to_owned()),
        })
    }

    fn installed_plugin_ids(&self) -> BTreeSet<String> {
        self.0
            .installed_packages()
            .into_iter()
            .map(|p| p.id.as_str().to_owned())
            .collect()
    }

    /// Locked plugins not yet present in the Store, paired with their
    /// name — `LockedPlugin` itself carries no name (it's the `BTreeMap`
    /// key), and both `plan_project_environment` (reporting) and
    /// `install_project_environment` (acting) need it, so this is the one
    /// place that walks the lock and keeps the two in sync.
    fn missing_locked_plugins<'lock>(
        lock: &'lock ProjectLock,
        installed_ids: &BTreeSet<String>,
    ) -> Vec<(&'lock str, &'lock LockedPlugin)> {
        lock.plugins
            .iter()
            .filter(|(name, locked)| {
                !installed_ids.contains(&UzeApplication::locked_plugin_id(name, locked))
            })
            .map(|(name, locked)| (name.as_str(), locked))
            .collect()
    }

    /// Reproduces one locked plugin: the recorded commit, never the
    /// declared `ref:` — a lock that re-resolved `main` would install
    /// whatever was pushed since, which is what it exists to prevent.
    ///
    /// The commit is fetched from the local checkout when this machine has
    /// one registered for the same repository, and from the recorded URL
    /// otherwise. Same bytes either way — a commit is a commit — but a
    /// person who set up a local marketplace should not need the network
    /// to reinstall from it.
    fn reproduce_locked_plugin(
        &self,
        locked: &LockedMarketplace,
        marketplace: &str,
        plugin: &str,
    ) -> Result<uze_core::MaterializedPackage> {
        let mut repository = uze_core::acquisition::marketplace::MarketplaceRepository {
            fetch: locked.git.clone(),
            identity: locked.git.clone(),
        };
        if let Ok(Some(registered)) = uze_core::state::marketplace_get(&self.0.home, marketplace)
            && let Ok(local) = uze_core::acquisition::marketplace::repository_of(&registered.source)
            && local.identity == locked.git
        {
            repository.fetch = local.fetch;
        }
        UzeApplication::materialize_marketplace_plugin_at(
            &repository,
            Some(&locked.revision),
            locked.subdirectory.as_deref(),
            plugin,
        )
    }

    /// Adds a plugin to the project lock and ensures it's in the Store.
    #[tracing::instrument(name = "project.add", skip_all, fields(plugin = %plugin, marketplace = %marketplace, root = %root.display()), err)]
    pub fn add(
        &self,
        plugin: &str,
        marketplace: &str,
        root: &Path,
        authority: &dyn TrustAuthority,
    ) -> Result<AddPluginReport> {
        let canonical = project_root::resolve_project_root(root)?;
        // The marketplace built into UZE is not a project's to declare:
        // its plugins are installed for every project by the machine's own
        // bootstrap. `declare_plugin` already refuses to write it into
        // `agents.yaml`, and the lock refuses it for the same reason — an
        // entry recording something nobody declared is a line that cannot
        // be acted on.
        if marketplace == uze_core::manifest::BUILT_IN_MARKETPLACE {
            // No mutation lock taken here: the call below takes it, and it
            // is not re-entrant.
            return self.0.marketplace().install_plugin_resolving(
                &format!("{plugin}@{marketplace}"),
                authority,
                &uze_core::naming::NoNameCollisionAuthority,
            );
        }

        let mut lock = project_lock::load_lock(&canonical)?.unwrap_or_default();
        let global =
            uze_core::state::marketplace_get(&self.0.home, marketplace)?.ok_or_else(|| {
                UzeError::UnknownPackage(format!("marketplace `{marketplace}` not found"))
            })?;
        let request = MarketplaceRequest::of(&global.source)?;

        // A marketplace name means one repository. The lock naming one and
        // the machine registry another is a question only a person can
        // settle.
        if let Some(recorded) = lock.marketplaces.get(marketplace)
            && recorded.git != request.repository.identity
        {
            return Err(UzeError::MarketplaceSourceConflict {
                marketplace: marketplace.to_owned(),
                lock_source: recorded.display(),
                global_source: request.repository.identity.clone(),
            });
        }

        if let Some(existing) = lock.plugins.get(plugin)
            && existing.marketplace != marketplace
        {
            return Err(UzeError::MarketplaceMismatch {
                plugin: plugin.to_owned(),
                expected: existing.marketplace.clone(),
                found: marketplace.to_owned(),
            });
        }

        // Acquire and ingest (reuses existing lifecycle).
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
        let report = self.resolve_into_lock(&mut lock, plugin, marketplace, &request, authority)?;

        // The declaration comes first and the lock second: `agents.yaml` is
        // what the project meant, and the lock is what that meant resolved
        // to. Writing the lock alone would leave the manifest — the file a
        // person reads and edits — silently out of date.
        manifest::declare_plugin(
            &canonical,
            plugin,
            marketplace,
            declared_marketplace_for(&lock, marketplace).as_ref(),
        )?;
        project_lock::save_lock(&canonical, &lock)?;

        Ok(report)
    }

    /// Removes a plugin's declaration and the entry it resolved to. The
    /// Store keeps the bytes: another project may want them, and this
    /// command is about what *this* project declares.
    #[tracing::instrument(name = "project.remove", skip_all, fields(plugin = %plugin, root = %root.display()), err)]
    pub fn remove(&self, plugin: &str, root: &Path) -> Result<RemoveProjectPluginReport> {
        let canonical = project_root::resolve_project_root(root)?;
        let undeclared = manifest::undeclare_plugin(&canonical, plugin)?;
        let mut lock = match project_lock::load_lock(&canonical)? {
            Some(lock) => lock,
            None if undeclared => ProjectLock::default(),
            None => return Ok(RemoveProjectPluginReport::NoLock),
        };

        if lock.plugins.remove(plugin).is_none() && !undeclared {
            return Ok(RemoveProjectPluginReport::NotInLock {
                plugin: plugin.to_owned(),
            });
        }

        // A lock with nothing left to reproduce is not an empty lock, it is
        // no lock: leaving `agents.lock` behind declaring nothing invites
        // the belief that resolution happened.
        if lock.plugins.is_empty() && lock.marketplaces.is_empty() {
            project_lock::remove_lock(&canonical)?;
        } else {
            project_lock::save_lock(&canonical, &lock)?;
        }

        Ok(RemoveProjectPluginReport::Removed {
            plugin: plugin.to_owned(),
        })
    }

    /// Brings the project's declared environment about, in two passes over
    /// the same lifecycle an ordinary add uses
    /// (`authorize → prepare → ingest → republish → attach`):
    ///
    /// 1. **Resolution** — every plugin `agents.yaml` declares that the
    ///    lock does not answer for is acquired from the marketplace the
    ///    manifest declares it under, and what that produced is written to
    ///    `agents.lock`. The manifest is the authority; the lock is only
    ///    what asking it produced, so a project that has declared but
    ///    never resolved is exactly the case this exists for.
    /// 2. **Reproduction** — every locked plugin not yet in the Store is
    ///    installed from the source the lock itself carries, so a machine
    ///    that cloned the repository and never ran `market add` reaches
    ///    the same environment.
    ///
    /// `authority` is honored exactly as it is in `add`:
    /// `install_materialized`'s own `authorize()` call is what enforces
    /// the trust boundary, per plugin, so this function does no trust
    /// reasoning of its own.
    ///
    /// Stops at the first plugin that fails to acquire or install and
    /// returns that error — an install is either fully applied or (for
    /// whichever plugins came before the failure) partially applied with
    /// the failure surfaced, never silently partial. Plugins already
    /// installed are left untouched; already-successful ones are not
    /// rolled back on a later failure, matching `add_project_plugin`'s own
    /// no-transaction model (the Store has no all-or-nothing multi-package
    /// primitive to build one on).
    #[tracing::instrument(name = "project.install", skip_all, fields(root = %root.display()), err)]
    pub fn install(&self, root: &Path, authority: &dyn TrustAuthority) -> Result<InstallReport> {
        let canonical = project_root::resolve_project_root(root)?;
        // `install` is an explicit act of setting this project up, so it is
        // the right moment to create the file a person edits — unlike
        // opening the client, which must write nothing into a repository
        // somebody is only looking at.
        manifest::ensure_exists(&canonical)?;
        let manifest = manifest::load(&canonical)?.unwrap_or_default();
        let mut lock = project_lock::load_lock(&canonical)?.unwrap_or_default();

        let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
        let mut installed_plugins = Vec::new();

        // Resolution comes first: `agents.yaml` is what the project asked
        // for and the lock is only what asking produced, so a declaration
        // the lock does not answer for is resolved now rather than
        // reported as nothing to do. A clone carrying only the manifest
        // must reach the same environment as one carrying both.
        for stale in project_lock::stale_against(&manifest, &lock) {
            let marketplace = stale.marketplace.as_str();
            let declared = manifest.marketplaces.get(marketplace).ok_or_else(|| {
                UzeError::MarketplaceMismatch {
                    plugin: stale.plugin.clone(),
                    expected: marketplace.to_owned(),
                    found: "not declared in agents.yaml".to_owned(),
                }
            })?;
            // The declaration is the authority over its own source: a lock
            // recording where the plugin used to come from is the stale
            // half, so it is overwritten rather than defended.
            let fetch_source = Self::declared_fetch_source(&canonical, marketplace, declared)?;
            let request = MarketplaceRequest::of(&fetch_source)?;
            self.register_marketplace(marketplace, fetch_source, &request.repository.identity)?;
            self.resolve_into_lock(&mut lock, &stale.plugin, marketplace, &request, authority)?;
            // Saved per entry, not once at the end: bytes are already in
            // the Store, and a later failure must not leave the lock
            // denying what this machine now holds.
            project_lock::save_lock(&canonical, &lock)?;
            installed_plugins.push(stale.plugin);
        }

        // Reproduction second: what the lock records and the Store does
        // not hold yet — the fresh machine cloning a project.
        let installed_ids = self.installed_plugin_ids();
        let missing: Vec<(String, LockedPlugin)> =
            Self::missing_locked_plugins(&lock, &installed_ids)
                .into_iter()
                .map(|(name, locked)| (name.to_owned(), locked.clone()))
                .collect();
        for (name, locked) in missing {
            // A fresh machine reproducing a cloned `agents.lock` has never
            // run `market add` — `project_environment()`'s own diagnostics
            // already promise "(will be resolved from lock source on
            // install)" for exactly this case, so this registers the
            // locked marketplace globally (from the source the lock itself
            // carries) before ingesting from it. Idempotent for a
            // same-source re-run; a genuinely different source already
            // registered under this name surfaces as `MarketplaceConflict`
            // rather than silently mis-attributing the package.
            let marketplace = locked.marketplace.as_str();
            let recorded = lock.marketplaces.get(marketplace).ok_or_else(|| {
                UzeError::MarketplaceMismatch {
                    plugin: name.clone(),
                    expected: marketplace.to_owned(),
                    found: "not declared in lock".to_owned(),
                }
            })?;
            self.register_marketplace(
                marketplace,
                PackageSource::Git {
                    url: recorded.git.clone(),
                    reference: recorded.r#ref.clone(),
                    subdirectory: recorded.subdirectory.clone(),
                },
                &recorded.git,
            )?;
            let materialized = self.reproduce_locked_plugin(recorded, marketplace, &name)?;
            // The pin is checked before the bytes are ingested, let alone
            // delivered to a harness: a moved tag, a rewritten history or a
            // substituted remote must stop here, not be discovered later by
            // reading what an agent was told to do.
            Self::verify_integrity_of(&name, &locked, materialized.root())?;
            self.0.plugins().install_materialized_from_marketplace(
                materialized,
                marketplace,
                authority,
                &[],
                false,
                &uze_core::naming::NoNameCollisionAuthority,
            )?;
            installed_plugins.push(name);
        }

        // Convergence, third: what the manifest no longer declares.
        //
        // Nothing is asked and nothing on this machine is touched. The lock
        // is derived — regenerable from the manifest, and losing an entry
        // loses nothing a person did not just delete themselves — so making
        // it agree with what the project declared is the least destructive
        // thing this command does, not the most. What *would* be
        // destructive is taking the package off the machine or out of a
        // harness, and that is `uze plugin remove`'s to do: other projects
        // share the Store, and ADR-019 keeps project scope out of it.
        let mut removed_plugins = Vec::new();
        let surplus = project_lock::surplus_against(&manifest, &lock);
        if !surplus.is_empty() {
            drop(_mutation);
            for plugin in &surplus {
                self.remove(plugin, &canonical)?;
                removed_plugins.push(plugin.clone());
            }
        }

        // Installing changes what this project's packages contribute to
        // `AGENTS.md`, and a policy edit changes what the projected region
        // should say. Leaving either to a second command is how a policy
        // stayed in force for UZE and not for the agents reading the file.
        let nothing_moved = installed_plugins.is_empty() && removed_plugins.is_empty();
        let attempted = (!nothing_moved || self.stale_projection(&canonical).is_some())
            .then(|| self.0.context().reconcile(&canonical));
        // Declaring an environment and projecting it are one command, so a
        // projection that failed fails the command. Swallowed, it reported
        // `NoChanges` — "everything already agrees" — over a read-only
        // `AGENTS.md` or a bridge that could not be written, and the half of
        // the environment the agents actually read never moved. Re-running
        // `install` converges the rest again and retries this.
        let reconciled = match attempted {
            Some(outcome) => {
                outcome?;
                true
            }
            None => false,
        };

        if nothing_moved && !reconciled {
            return Ok(InstallReport::NoChanges);
        }
        Ok(InstallReport::Installed {
            plugins: installed_plugins,
            removed: removed_plugins,
            reconciled,
        })
    }

    /// Makes sure this machine knows the marketplace, without overruling
    /// what it already knows.
    ///
    /// Identities are compared, not sources: a local clone and the remote
    /// it came from are the same marketplace, and which of the two this
    /// machine reads is its own business. A name already pointing at a
    /// different repository is a conflict only a person can settle.
    fn register_marketplace(
        &self,
        marketplace: &str,
        source: PackageSource,
        identity: &str,
    ) -> Result<()> {
        if let Some(registered) = uze_core::state::marketplace_get(&self.0.home, marketplace)? {
            let known = uze_core::acquisition::marketplace::repository_of(&registered.source)?;
            if known.identity == identity {
                return Ok(());
            }
            return Err(UzeError::MarketplaceConflict {
                name: marketplace.to_owned(),
                existing: known.identity,
                requested: identity.to_owned(),
            });
        }
        uze_core::state::marketplace_add(&self.0.home, marketplace, source)?;
        Ok(())
    }

    /// Where a declaration says to read the marketplace from. A relative
    /// `path:` is resolved against the project root and canonicalized, so
    /// the source `market add` would have registered and the one a
    /// manifest produces are the same source — two spellings of one
    /// directory would otherwise collide as a conflict under one name.
    fn declared_fetch_source(
        root: &Path,
        marketplace: &str,
        declared: &DeclaredMarketplace,
    ) -> Result<PackageSource> {
        if let Some(url) = &declared.git {
            return Ok(PackageSource::Git {
                url: url.clone(),
                reference: declared.r#ref.clone(),
                subdirectory: declared.subdirectory.clone(),
            });
        }
        let declared_path = declared.path.as_ref().ok_or_else(|| {
            UzeError::UnknownPackage(format!("marketplace `{marketplace}` declares no source"))
        })?;
        let joined = root.join(declared_path);
        let path = joined
            .canonicalize()
            .map_err(|_| UzeError::MissingPath(joined.clone()))?;
        Ok(PackageSource::Local { path })
    }

    /// Acquires one plugin from a marketplace already recorded in `lock`,
    /// installs it, and records what resolution produced. `add` and
    /// `install` differ in where the declaration came from — a command
    /// argument or `agents.yaml` — and must not differ in how it resolves.
    fn resolve_into_lock(
        &self,
        lock: &mut ProjectLock,
        plugin: &str,
        marketplace: &str,
        request: &MarketplaceRequest,
        authority: &dyn TrustAuthority,
    ) -> Result<AddPluginReport> {
        let materialized = UzeApplication::materialize_marketplace_plugin_at(
            &request.repository,
            request.reference.as_deref(),
            request.subdirectory.as_deref(),
            plugin,
        )?;
        let report = self.0.plugins().install_materialized_from_marketplace(
            materialized,
            marketplace,
            authority,
            &[],
            false,
            &uze_core::naming::NoNameCollisionAuthority,
        )?;

        // What actually landed is the source of truth, so both facts are
        // read back from the Store rather than from the request — the
        // discipline `Provenance` exists to enforce.
        let stored = self.0.package_by_name(&report.plugin.id)?;
        let reproducible = stored.provenance.resolved.lock_revision().is_some();
        lock.plugins.insert(
            plugin.to_owned(),
            LockedPlugin::resolved(marketplace, &stored.root, reproducible),
        );
        // The revision belongs to the marketplace, which is the thing that
        // has one: a plugin is a directory inside it. The entry is written
        // from what the clone reported, so it cannot exist without the
        // commit it was read at.
        let uze_core::acquisition::ResolvedSource::Git { commit, .. } = &stored.provenance.resolved
        else {
            return Err(UzeError::AcquisitionFailed(format!(
                "`{marketplace}` did not resolve to a commit"
            )));
        };
        lock.marketplaces.insert(
            marketplace.to_owned(),
            LockedMarketplace {
                git: request.repository.identity.clone(),
                r#ref: request.reference.clone(),
                subdirectory: request.subdirectory.clone(),
                revision: commit.clone(),
            },
        );
        Ok(report)
    }

    /// Refuses bytes that are not the bytes the lock pinned. An entry with
    /// no `integrity` is not checked — a local path has none to record, and
    /// so has nothing to contradict.
    fn verify_integrity_of(plugin: &str, locked: &LockedPlugin, acquired: &Path) -> Result<()> {
        let Some(expected) = &locked.integrity else {
            return Ok(());
        };
        let found = uze_core::digest::tree_sha256(acquired).map_err(|source| UzeError::Read {
            path: acquired.to_path_buf(),
            source,
        })?;
        if &found == expected {
            return Ok(());
        }
        Err(UzeError::IntegrityMismatch {
            plugin: plugin.to_owned(),
            expected: expected.clone(),
            found,
        })
    }

    /// A summary of this project's `agents.lock` for `uze status` — never
    /// errors: a missing lock is `Absent`, and a lock that fails to parse
    /// is `Malformed` rather than failing `status` itself, since `status`
    /// is meant to diagnose exactly this kind of problem, not refuse to
    /// run because of it.
    #[tracing::instrument(name = "project.lock_status", skip_all, fields(root = %root.display()))]
    pub fn lock_status(&self, root: &Path) -> ProjectLockStatus {
        let canonical = match project_root::resolve_project_root(root) {
            Ok(canonical) => canonical,
            Err(_) => return ProjectLockStatus::Absent,
        };
        let lock = match project_lock::load_lock(&canonical) {
            Ok(Some(lock)) => lock,
            Ok(None) => return ProjectLockStatus::Absent,
            Err(error) => {
                return ProjectLockStatus::Malformed {
                    reason: error.to_string(),
                };
            }
        };
        let installed_ids = self.installed_plugin_ids();
        let plugins = lock
            .plugins
            .iter()
            .map(|(name, locked)| ProjectPluginHealth {
                plugin: name.clone(),
                installed: installed_ids.contains(&UzeApplication::locked_plugin_id(name, locked)),
            })
            .collect();
        ProjectLockStatus::Present { plugins }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectEnvironment {
    pub root: PathBuf,
    pub canonical: PathBuf,
    pub lock: Option<ProjectLock>,
    pub diagnostics: Vec<String>,
}

/// `uze status`'s view of this project's `agents.lock` — deliberately
/// smaller than `ProjectEnvironment`/`ProjectEnvironmentPlan` (which
/// `context inspect`-equivalent commands already cover in full): just
/// enough to answer "is there a lock, and does it match what's installed."
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "state", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProjectLockStatus {
    Absent,
    Malformed { reason: String },
    Present { plugins: Vec<ProjectPluginHealth> },
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectPluginHealth {
    pub plugin: String,
    pub installed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectEnvironmentPlan {
    pub dependencies: Vec<LockedPlugin>,
    pub installed: Vec<String>, // Package IDs
    pub missing: Vec<LockedPlugin>,
    /// Declared in `agents.yaml`, and the lock does not answer for it.
    pub unresolved: Vec<String>,
    /// In the lock, and the manifest no longer declares it.
    pub surplus: Vec<String>,
    /// The projected instruction region has fallen behind the policy.
    pub stale_projection: Option<StaleProjection>,
    pub trust_required: Vec<trust::TrustRequest>,
    pub delivery_changes: Vec<PublicationOutcome>,
    pub conflicts: Vec<String>,
    pub offline_unavailable: Vec<String>,
    pub has_changes: bool,
}

/// The projected policy region is behind what the manifest declares:
/// UZE itself acts on the live policy, but the agents that must honor it
/// read the projected text, so this is a policy only half in force.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StaleProjection {
    /// The completion behavior `agents.yaml` declares today.
    pub declared: String,
    /// The identity the region in `AGENTS.md` still carries.
    pub projected_identity: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "outcome", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RemoveProjectPluginReport {
    NoLock,
    NotInLock { plugin: String },
    Removed { plugin: String },
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "outcome", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InstallReport {
    /// The declared environment, the lock and the machine already agreed,
    /// and the projection was current; nothing to do.
    NoChanges,
    /// The project's environment moved: plugins resolved or reproduced,
    /// plugins the manifest no longer declares removed, and the project
    /// context left reconciled.
    Installed {
        plugins: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        removed: Vec<String>,
        #[serde(default)]
        reconciled: bool,
    },
}

/// Read by both the project environment and the workspace overview, so it
/// belongs to the type that owns the state rather than to either view.
impl UzeApplication {
    pub(crate) fn locked_plugin_id(name: &str, locked: &LockedPlugin) -> String {
        format!("{name}@{}", locked.marketplace)
    }
}
