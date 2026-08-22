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

use super::*;

impl UzeApplication {
    /// Read-only: observes the project's current state (lock + diagnostics).
    pub fn project_environment(&self, root: &Path) -> Result<ProjectEnvironment> {
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
                        let global = uze_core::state::marketplace_get(&self.home, name)?;
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
    pub fn plan_project_environment(&self, root: &Path) -> Result<ProjectEnvironmentPlan> {
        let env = self.project_environment(root)?;
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

        let installed_packages = self.installed_packages();
        let installed_ids: BTreeSet<String> = installed_packages
            .iter()
            .map(|p| p.id.as_str().to_owned())
            .collect();

        let mut dependencies = Vec::new();
        let mut installed = Vec::new();
        let mut missing = Vec::new();
        let conflicts = Vec::new();

        for (plugin_name, locked_plugin) in &lock.plugins {
            dependencies.push(locked_plugin.clone());
            if installed_ids.contains(plugin_name) {
                // Check if installed package matches lock resolution.
                if let Some(stored) = installed_packages
                    .iter()
                    .find(|p| p.id.as_str() == plugin_name)
                {
                    installed.push(stored.id.as_str().to_owned());
                    // TODO: Check if resolved.revision matches stored.provenance.resolved
                }
            } else {
                missing.push(locked_plugin.clone());
            }
        }

        // TODO: Compute trust_required by inspecting missing packages for executable capabilities.
        let trust_required = Vec::new();

        // TODO: Compute delivery_changes by checking which integrations need republish/attach.
        let delivery_changes = Vec::new();

        // TODO: Compute offline_unavailable by checking if missing packages can be acquired offline.
        let offline_unavailable = Vec::new();

        let has_changes = !missing.is_empty() || !conflicts.is_empty();

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

    /// Adds a plugin to the project lock and ensures it's in the Store.
    pub fn add_project_plugin(
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
                uze_core::state::marketplace_get(&self.home, marketplace)?.ok_or_else(|| {
                    UzeError::UnknownPackage(format!("marketplace `{marketplace}` not found"))
                })?;
            let source = MarketplaceSource::from(global.source.clone());
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
                source: mp_source.clone(),
                resolved: mp_resolved,
            },
        );

        // Resolve plugin source via marketplace.
        let plugin_source = PackageSource::from(mp_source.clone());
        let (marketplace_root, manifest) = Self::load_marketplace_manifest(&plugin_source)?;
        let plugin_path = uze_core::acquisition::marketplace::resolve_plugin_source(
            &manifest,
            plugin,
            &marketplace_root,
        )?;
        let package_source = PackageSource::Local { path: plugin_path };

        // Acquire and ingest (reuses existing lifecycle).
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.home)?;
        let materialized = self.acquire(&package_source)?;
        let report = self.install_materialized(materialized, authority, &[], false)?;

        // Add plugin to lock.
        lock.plugins.insert(
            plugin.to_owned(),
            LockedPlugin {
                source: PluginSource::Marketplace {
                    marketplace: marketplace.to_owned(),
                    plugin: plugin.to_owned(),
                },
                resolved: ResolvedPlugin {
                    revision: None, // TODO: Populate from materialized.provenance.resolved
                    version: None,  // TODO: Read from plugin.json
                    integrity: None,
                },
            },
        );

        // Persist lock.
        project_lock::save_lock(&canonical, &lock)?;

        Ok(report)
    }

    /// Removes a plugin from the project lock (does NOT remove from Store).
    pub fn remove_project_plugin(
        &self,
        plugin: &str,
        root: &Path,
    ) -> Result<RemoveProjectPluginReport> {
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

        // Persist lock.
        project_lock::save_lock(&canonical, &lock)?;

        Ok(RemoveProjectPluginReport::Removed {
            plugin: plugin.to_owned(),
        })
    }

    /// Applies the project environment plan (acquires missing, persists lock).
    pub fn install_project_environment(
        &self,
        root: &Path,
        _authority: &dyn TrustAuthority,
    ) -> Result<InstallReport> {
        let plan = self.plan_project_environment(root)?;
        if !plan.has_changes {
            return Ok(InstallReport::NoChanges);
        }

        // TODO: Implement full install logic (acquire missing, authorize, ingest, attach, persist lock).
        // For now, return a placeholder.
        Ok(InstallReport::NotImplemented)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectEnvironment {
    pub root: PathBuf,
    pub canonical: PathBuf,
    pub lock: Option<ProjectLock>,
    pub diagnostics: Vec<String>,
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
    NoChanges,
    NotImplemented,
    // TODO: Add Installed { packages: Vec<String> } etc.
}
