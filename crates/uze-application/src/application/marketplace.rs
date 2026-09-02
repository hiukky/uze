//! Marketplace — extracted from application.rs without semantic change.

#![allow(clippy::empty_line_after_doc_comments)]

use uze_core::{PackageSource, Result, UzeError, store::StoredPackage, trust::TrustAuthority};

use crate::bootstrap;

use super::services::Marketplace;
use super::*;

impl Marketplace<'_> {
    /// `Ok(true)` when the marketplace was newly registered, `Ok(false)`
    /// when it was already registered from the exact same source
    /// (idempotent no-op — see `state::marketplace_add`). A different
    /// source under the same name is a `MarketplaceConflict` error.
    pub fn add(&self, source_str: &str) -> Result<bool> {
        let source = UzeApplication::parse_marketplace_source(source_str)?;
        let (marketplace_root, manifest) = UzeApplication::load_marketplace_manifest(&source)?;
        let name = manifest.name.clone();
        if name == "uze-official" {
            return Err(UzeError::ReservedMarketplace(name));
        }
        let added = uze_core::state::marketplace_add(&self.0.home, &name, source)?;
        let _ = (marketplace_root, manifest);
        Ok(added)
    }

    pub fn remove(&self, name: &str) -> Result<()> {
        if name == "uze-official" {
            return Err(UzeError::ReservedMarketplace(name.to_owned()));
        }
        uze_core::state::marketplace_remove(&self.0.home, name)
    }

    pub fn list(&self) -> Result<Vec<MarketplaceSummary>> {
        let mut out = Vec::new();
        let (official_name, official_entries) = bootstrap::entries()?;
        out.push(MarketplaceSummary {
            name: "uze-official".to_owned(),
            source: "embedded:uze-official".to_owned(),
            plugin_count: official_entries.len(),
        });
        for (name, record) in uze_core::state::marketplace_list(&self.0.home)? {
            let plugin_count = match UzeApplication::load_marketplace_manifest(&record.source) {
                Ok((_, manifest)) => manifest.plugins.len(),
                Err(_) => 0,
            };
            out.push(MarketplaceSummary {
                name: name.clone(),
                source: record.source.display(),
                plugin_count,
            });
        }
        let _ = official_name;
        Ok(out)
    }

    /// One marketplace's own detail (source, plugin count) — distinct from
    /// inspecting one plugin *within* a marketplace
    /// (`inspect_marketplace_plugin`). Filters the same per-entry
    /// computation `marketplace_list` already does down to one named entry;
    /// no new state or invariant.
    pub fn inspect(&self, name: &str) -> Result<MarketplaceSummary> {
        self.list()?
            .into_iter()
            .find(|entry| entry.name == name)
            .ok_or_else(|| UzeError::UnknownPackage(format!("marketplace `{name}` not found")))
    }

    pub fn install_plugin(
        &self,
        spec: &str,
        authority: &dyn TrustAuthority,
    ) -> Result<AddPluginReport> {
        self.install_plugin_resolving(spec, authority, &uze_core::naming::NoNameCollisionAuthority)
    }

    /// `plugin_install`, with an explicit answer for a bare-plugin-name
    /// collision with an already-active, differently-marketplaced package
    /// (ADR-038) — see `add_plugin_resolving`.
    pub fn install_plugin_resolving(
        &self,
        spec: &str,
        authority: &dyn TrustAuthority,
        name_authority: &dyn uze_core::naming::NameCollisionAuthority,
    ) -> Result<AddPluginReport> {
        let (plugin_name, marketplace_name) =
            uze_core::project_lock::parse_plugin_marketplace_spec(spec)?;
        if marketplace_name == "uze-official" {
            return self.install_from_resolving(&plugin_name, authority, name_authority);
        }
        let record = uze_core::state::marketplace_get(&self.0.home, &marketplace_name)?
            .ok_or_else(|| {
                UzeError::UnknownPackage(format!("marketplace `{marketplace_name}` not found"))
            })?;
        let (marketplace_root, manifest) =
            UzeApplication::load_marketplace_manifest(&record.source)?;
        let plugin_source = uze_core::acquisition::marketplace::resolve_plugin_source(
            &manifest,
            &plugin_name,
            &marketplace_root,
        )?;
        let source = PackageSource::Local {
            path: plugin_source,
        };
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
        let materialized = self.0.acquire(&source)?;
        let report = self.0.install_materialized_from_marketplace(
            materialized,
            &marketplace_name,
            authority,
            &[],
            false,
            name_authority,
        )?;
        uze_core::state::plugin_marketplace_record(
            &self.0.home,
            &report.plugin.id,
            &marketplace_name,
        )?;
        Ok(report)
    }

    /// Every plugin from every marketplace this Store knows about — the
    /// embedded `uze-official` snapshot plus every marketplace registered
    /// via `marketplace add` (`uze_core::state::marketplace_list`). A
    /// marketplace whose manifest can no longer be read (moved/deleted
    /// source) is skipped rather than failing the whole listing, mirroring
    /// `marketplace_list`'s own `plugin_count: 0` fallback.
    pub fn plugins(&self) -> Result<Vec<MarketplacePluginSummary>> {
        let installed_packages = self.0.installed_packages();
        let installed: std::collections::BTreeMap<&str, &StoredPackage> = installed_packages
            .iter()
            .map(|package| (package.id.as_str(), package))
            .collect();

        let mut out = Vec::new();

        let (_name, official_entries) = bootstrap::entries()?;
        out.extend(official_entries.into_iter().map(|entry| {
            // `installed` is keyed by the full `plugin@marketplace` identity
            // (ADR-036); a catalog entry's own `name` is bare, scoped to
            // *this* marketplace listing, so the lookup must reconstruct the
            // qualified id it would have installed under — matching by bare
            // name alone would (and did) also match a same-named plugin
            // installed from an entirely different marketplace.
            let installed_package = installed.get(format!("{}@uze-official", entry.name).as_str());
            let update_available = installed_package
                .and_then(|package| bootstrap::has_update(&entry.name, &package.root).ok());
            MarketplacePluginSummary {
                marketplace: "uze-official".to_owned(),
                name: entry.name.clone(),
                description: entry.description,
                keywords: entry.keywords,
                installed: installed_package.is_some(),
                update_available,
                is_default: bootstrap::DEFAULT_PLUGIN_IDS.contains(&entry.name.as_str()),
            }
        }));

        for (name, record) in uze_core::state::marketplace_list(&self.0.home)? {
            let Ok((_, manifest)) = UzeApplication::load_marketplace_manifest(&record.source)
            else {
                continue;
            };
            out.extend(manifest.plugins.into_iter().map(|entry| {
                let installed_package = installed.get(format!("{}@{name}", entry.name).as_str());
                MarketplacePluginSummary {
                    marketplace: name.clone(),
                    name: entry.name.clone(),
                    description: entry.description,
                    keywords: entry.keywords,
                    installed: installed_package.is_some(),
                    // Update-comparison only exists for the embedded
                    // snapshot's own offline directory-tree diff.
                    update_available: None,
                    is_default: false,
                }
            }));
        }

        Ok(out)
    }

    pub fn inspect_plugin(&self, marketplace: &str, name: &str) -> Result<MarketplacePluginDetail> {
        let summary = self
            .plugins()?
            .into_iter()
            .find(|plugin| plugin.marketplace == marketplace && plugin.name == name)
            .ok_or_else(|| UzeError::UnknownPackage(name.to_owned()))?;
        let materialized = if marketplace == "uze-official" {
            bootstrap::materialize(name)?
        } else {
            let record =
                uze_core::state::marketplace_get(&self.0.home, marketplace)?.ok_or_else(|| {
                    UzeError::UnknownPackage(format!("marketplace `{marketplace}` not found"))
                })?;
            let (marketplace_root, manifest) =
                UzeApplication::load_marketplace_manifest(&record.source)?;
            let plugin_source = uze_core::acquisition::marketplace::resolve_plugin_source(
                &manifest,
                name,
                &marketplace_root,
            )?;
            uze_core::acquisition::acquire(&PackageSource::Local {
                path: plugin_source,
            })?
        };
        let inspected = uze_core::acquisition::inspect_capabilities(&materialized)?;
        Ok(MarketplacePluginDetail {
            capabilities: inspected
                .resources
                .iter()
                .map(|resource| PluginCapability {
                    identity: resource.identity(),
                    name: capability_display_name(resource),
                    kind: resource.capability.kind,
                })
                .collect(),
            summary,
        })
    }

    pub fn install_from(
        &self,
        name: &str,
        authority: &dyn TrustAuthority,
    ) -> Result<AddPluginReport> {
        self.install_from_resolving(name, authority, &uze_core::naming::NoNameCollisionAuthority)
    }

    /// `install_from_marketplace`, with an explicit answer for a
    /// bare-plugin-name collision (ADR-038) — see `add_plugin_resolving`.
    pub fn install_from_resolving(
        &self,
        name: &str,
        authority: &dyn TrustAuthority,
        name_authority: &dyn uze_core::naming::NameCollisionAuthority,
    ) -> Result<AddPluginReport> {
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
        let materialized = self.0.acquire(&PackageSource::Embedded {
            id: name.to_owned(),
        })?;
        self.0.install_materialized_from_marketplace(
            materialized,
            "uze-official",
            authority,
            &[],
            false,
            name_authority,
        )
    }
}
