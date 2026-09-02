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
    PackageSource, Result, UzeError,
    project_lock::{
        self, LockedMarketplace, LockedPlugin, MarketplaceSource, PluginSource, ProjectLock,
        ResolvedMarketplace, ResolvedPlugin,
    },
    project_root,
    trust::{self, TrustAuthority},
};

use super::services::Project;
use super::*;

impl Project<'_> {
    /// Read-only: observes the project's current state (lock + diagnostics).
    pub fn environment(&self, root: &Path) -> Result<ProjectEnvironment> {
        let canonical = project_root::resolve_project_root(root)?;
        let _lock_path = project_lock::lock_path_for(&canonical);
        let lock = project_lock::load_lock(&canonical)?;
        let mut diagnostics = Vec::new();

        if let Some(lock) = &lock {
            // Validate marketplace sources exist in global registry or are embedded.
            for (name, locked_mp) in &lock.marketplaces {
                match &locked_mp.source {
                    MarketplaceSource::Embedded { .. } => {
                        // Embedded is always valid (no global registry needed).
                    }
                    MarketplaceSource::Git { .. } | MarketplaceSource::Path { .. } => {
                        // Check if global registry has this marketplace.
                        let global = uze_core::state::marketplace_get(&self.0.home, name)?;
                        if global.is_none() {
                            diagnostics.push(format!(
                                "marketplace `{name}` in lock but not in global registry (will be resolved from lock source on install)"
                            ));
                        }
                    }
                }
            }

            // Validate plugins reference valid marketplaces.
            for (plugin_name, locked_plugin) in &lock.plugins {
                if let PluginSource::Marketplace { marketplace, .. } = &locked_plugin.source
                    && !lock.marketplaces.contains_key(marketplace)
                {
                    diagnostics.push(format!(
                        "plugin `{plugin_name}` references marketplace `{marketplace}` not declared in lock"
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
    pub fn plan(&self, root: &Path) -> Result<ProjectEnvironmentPlan> {
        let env = self.environment(root)?;
        let lock = match env.lock {
            Some(lock) => lock,
            None => {
                return Ok(ProjectEnvironmentPlan {
                    dependencies: Vec::new(),
                    installed: Vec::new(),
                    missing: Vec::new(),
                    trust_required: Vec::new(),
                    delivery_changes: Vec::new(),
                    conflicts: Vec::new(),
                    offline_unavailable: Vec::new(),
                    has_changes: false,
                });
            }
        };

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

        let has_changes = !missing.is_empty();

        Ok(ProjectEnvironmentPlan {
            dependencies,
            installed,
            missing,
            trust_required,
            delivery_changes,
            conflicts,
            offline_unavailable,
            has_changes,
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

    /// Resolves a locked plugin's source into an acquirable `PackageSource`.
    /// Shared by `add_project_plugin` (adding a new entry) and
    /// `install_project_environment` (reproducing existing entries), so
    /// the two can never resolve the same kind of source differently.
    fn resolve_locked_plugin_source(
        lock: &ProjectLock,
        plugin: &str,
        source: &PluginSource,
    ) -> Result<PackageSource> {
        match source {
            PluginSource::Marketplace {
                marketplace,
                plugin: marketplace_plugin,
            } => {
                let locked_mp = lock.marketplaces.get(marketplace).ok_or_else(|| {
                    UzeError::MarketplaceMismatch {
                        plugin: plugin.to_owned(),
                        expected: marketplace.clone(),
                        found: "not declared in lock".to_owned(),
                    }
                })?;
                let marketplace_source = PackageSource::from(locked_mp.source.clone());
                let (marketplace_root, manifest) =
                    UzeApplication::load_marketplace_manifest(&marketplace_source)?;
                let plugin_path = uze_core::acquisition::marketplace::resolve_plugin_source(
                    &manifest,
                    marketplace_plugin,
                    &marketplace_root,
                )?;
                Ok(PackageSource::Local { path: plugin_path })
            }
            PluginSource::Git {
                url,
                reference,
                subdirectory,
            } => Ok(PackageSource::Git {
                url: url.clone(),
                reference: reference.clone(),
                subdirectory: subdirectory.clone(),
            }),
        }
    }

    /// Adds a plugin to the project lock and ensures it's in the Store.
    pub fn add_plugin(
        &self,
        plugin: &str,
        marketplace: &str,
        root: &Path,
        authority: &dyn TrustAuthority,
    ) -> Result<AddPluginReport> {
        let canonical = project_root::resolve_project_root(root)?;
        let mut lock = project_lock::load_lock(&canonical)?.unwrap_or_default();

        // Resolve marketplace source.
        let (mp_source, mp_resolved) = if marketplace == "uze-official" {
            // Embedded official marketplace.
            (
                MarketplaceSource::Embedded {
                    id: "uze-official".to_owned(),
                },
                ResolvedMarketplace {
                    revision: Some("embedded".to_owned()),
                },
            )
        } else {
            // Check global registry.
            let global =
                uze_core::state::marketplace_get(&self.0.home, marketplace)?.ok_or_else(|| {
                    UzeError::UnknownPackage(format!("marketplace `{marketplace}` not found"))
                })?;
            let source = MarketplaceSource::from(global.source);
            // TODO: Resolve marketplace to get commit SHA for resolved.revision.
            // For now, use empty resolved (will be populated on install).
            (source, ResolvedMarketplace { revision: None })
        };

        // Check for marketplace source conflict.
        if let Some(existing_mp) = lock.marketplaces.get(marketplace)
            && existing_mp.source != mp_source
        {
            return Err(UzeError::MarketplaceSourceConflict {
                marketplace: marketplace.to_owned(),
                lock_source: existing_mp.source.display(),
                global_source: mp_source.display(),
            });
        }

        // Check for plugin marketplace mismatch.
        if let Some(existing_plugin) = lock.plugins.get(plugin)
            && let PluginSource::Marketplace {
                marketplace: existing_mp,
                ..
            } = &existing_plugin.source
            && existing_mp != marketplace
        {
            return Err(UzeError::MarketplaceMismatch {
                plugin: plugin.to_owned(),
                expected: existing_mp.clone(),
                found: marketplace.to_owned(),
            });
        }

        // Add marketplace to lock if not present.
        lock.marketplaces.insert(
            marketplace.to_owned(),
            LockedMarketplace {
                source: mp_source,
                resolved: mp_resolved,
            },
        );

        let plugin_source = PluginSource::Marketplace {
            marketplace: marketplace.to_owned(),
            plugin: plugin.to_owned(),
        };
        let package_source = Self::resolve_locked_plugin_source(&lock, plugin, &plugin_source)?;

        // Acquire and ingest (reuses existing lifecycle).
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
        let materialized = self.0.acquire(&package_source)?;
        let report = self.0.install_materialized_from_marketplace(
            materialized,
            marketplace,
            authority,
            &[],
            false,
            &uze_core::naming::NoNameCollisionAuthority,
        )?;

        // What was actually acquired is the source of truth for `resolved`
        // — read back from the Store rather than trusting the request,
        // the same discipline `Provenance` itself exists to enforce.
        let resolved = ResolvedPlugin::from_resolved_source(
            &self
                .0
                .package_by_name(&report.plugin.id)?
                .provenance
                .resolved,
        );

        lock.plugins.insert(
            plugin.to_owned(),
            LockedPlugin {
                source: plugin_source,
                resolved,
            },
        );

        project_lock::save_lock(&canonical, &lock)?;

        Ok(report)
    }

    /// Removes a plugin from the project lock (does NOT remove from Store).
    pub fn remove_plugin(&self, plugin: &str, root: &Path) -> Result<RemoveProjectPluginReport> {
        let canonical = project_root::resolve_project_root(root)?;
        let mut lock = match project_lock::load_lock(&canonical)? {
            Some(lock) => lock,
            None => {
                return Ok(RemoveProjectPluginReport::NoLock);
            }
        };

        if lock.plugins.remove(plugin).is_none() {
            return Ok(RemoveProjectPluginReport::NotInLock {
                plugin: plugin.to_owned(),
            });
        }

        project_lock::save_lock(&canonical, &lock)?;

        Ok(RemoveProjectPluginReport::Removed {
            plugin: plugin.to_owned(),
        })
    }

    /// Reproduces the project's desired environment: acquires and installs
    /// every locked plugin not yet in the Store, through the same
    /// `authorize → prepare → ingest → republish → attach` lifecycle
    /// `add_project_plugin`/`add_plugin` use — a fresh machine running
    /// `uze install` against a cloned `agents.lock` is not a different
    /// code path from an ordinary add, just a batch of them driven by the
    /// lock instead of a marketplace argument. `authority` is honored
    /// exactly as it is there: `install_materialized`'s own `authorize()`
    /// call is what actually enforces the trust boundary, per plugin, so
    /// this function does no trust reasoning of its own.
    ///
    /// Stops at the first plugin that fails to acquire or install and
    /// returns that error — an install is either fully applied or (for
    /// whichever plugins came before the failure) partially applied with
    /// the failure surfaced, never silently partial. Plugins already
    /// installed are left untouched; already-successful ones are not
    /// rolled back on a later failure, matching `add_project_plugin`'s own
    /// no-transaction model (the Store has no all-or-nothing multi-package
    /// primitive to build one on).
    pub fn install(&self, root: &Path, authority: &dyn TrustAuthority) -> Result<InstallReport> {
        let canonical = project_root::resolve_project_root(root)?;
        let lock = match project_lock::load_lock(&canonical)? {
            Some(lock) => lock,
            None => return Ok(InstallReport::NoChanges),
        };

        let installed_ids = self.installed_plugin_ids();
        let missing: Vec<(String, LockedPlugin)> =
            Self::missing_locked_plugins(&lock, &installed_ids)
                .into_iter()
                .map(|(name, locked)| (name.to_owned(), locked.clone()))
                .collect();
        if missing.is_empty() {
            return Ok(InstallReport::NoChanges);
        }

        let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
        let mut installed_plugins = Vec::new();
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
            if let PluginSource::Marketplace { marketplace, .. } = &locked.source
                && marketplace != "uze-official"
                && let Some(locked_mp) = lock.marketplaces.get(marketplace)
            {
                uze_core::state::marketplace_add(
                    &self.0.home,
                    marketplace,
                    PackageSource::from(locked_mp.source.clone()),
                )?;
            }
            let package_source = Self::resolve_locked_plugin_source(&lock, &name, &locked.source)?;
            let materialized = self.0.acquire(&package_source)?;
            let marketplace = match &locked.source {
                PluginSource::Marketplace { marketplace, .. } => marketplace.as_str(),
                PluginSource::Git { .. } => "local",
            };
            self.0.install_materialized_from_marketplace(
                materialized,
                marketplace,
                authority,
                &[],
                false,
                &uze_core::naming::NoNameCollisionAuthority,
            )?;
            installed_plugins.push(name);
        }

        Ok(InstallReport::Installed {
            plugins: installed_plugins,
        })
    }

    /// A summary of this project's `agents.lock` for `uze status` — never
    /// errors: a missing lock is `Absent`, and a lock that fails to parse
    /// is `Malformed` rather than failing `status` itself, since `status`
    /// is meant to diagnose exactly this kind of problem, not refuse to
    /// run because of it.
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
    pub trust_required: Vec<trust::TrustRequest>,
    pub delivery_changes: Vec<PublicationOutcome>,
    pub conflicts: Vec<String>,
    pub offline_unavailable: Vec<String>,
    pub has_changes: bool,
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
    /// Every locked plugin was already installed; nothing to do.
    NoChanges,
    /// At least one previously-missing locked plugin was acquired and
    /// installed.
    Installed { plugins: Vec<String> },
}

/// Read by both the project environment and the workspace overview, so it
/// belongs to the type that owns the state rather than to either view.
impl UzeApplication {
    pub(crate) fn locked_plugin_id(name: &str, locked: &LockedPlugin) -> String {
        match &locked.source {
            PluginSource::Marketplace { marketplace, .. } => format!("{name}@{marketplace}"),
            PluginSource::Git { .. } => format!("{name}@local"),
        }
    }
}
