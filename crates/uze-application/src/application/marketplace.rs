//! Marketplace — extracted from application.rs without semantic change.

#![allow(clippy::empty_line_after_doc_comments)]

use uze_core::{PackageSource, Result, UzeError, store::StoredPackage, trust::TrustAuthority};

use crate::bootstrap;

use super::*;

impl UzeApplication {
    pub fn list_marketplaces(&self) -> Result<Vec<MarketplaceSummary>> {
        let (name, entries) = bootstrap::entries()?;
        Ok(vec![MarketplaceSummary {
            name: name.clone(),
            source: "embedded:uze-official".to_owned(),
            plugin_count: entries.len(),
        }])
    }

    pub fn marketplace_add(&self, source_str: &str) -> Result<()> {
        let source = Self::parse_marketplace_source(source_str)?;
        let (marketplace_root, manifest) = Self::load_marketplace_manifest(&source)?;
        let name = manifest.name.clone();
        if name == "uze-official" {
            return Err(UzeError::ExposureUnavailable(
                "marketplace `uze-official` is reserved".to_owned(),
            ));
        }
        uze_core::state::marketplace_add(&self.home, &name, source)?;
        let _ = (marketplace_root, manifest);
        Ok(())
    }

    pub fn marketplace_remove(&self, name: &str) -> Result<()> {
        if name == "uze-official" {
            return Err(UzeError::ExposureUnavailable(
                "marketplace `uze-official` cannot be removed".to_owned(),
            ));
        }
        uze_core::state::marketplace_remove(&self.home, name)
    }

    pub fn marketplace_list(&self) -> Result<Vec<MarketplaceSummary>> {
        let mut out = Vec::new();
        let (official_name, official_entries) = bootstrap::entries()?;
        out.push(MarketplaceSummary {
            name: "uze-official".to_owned(),
            source: "embedded:uze-official".to_owned(),
            plugin_count: official_entries.len(),
        });
        for (name, record) in uze_core::state::marketplace_list(&self.home)? {
            let plugin_count = match Self::load_marketplace_manifest(&record.source) {
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

    pub fn plugin_install(
        &self,
        spec: &str,
        authority: &dyn TrustAuthority,
    ) -> Result<AddPluginReport> {
        let (plugin_name, marketplace_name) = spec.split_once('@').ok_or_else(|| {
            UzeError::ExposureUnavailable(format!(
                "plugin spec `{spec}` must be `name@marketplace`"
            ))
        })?;
        if marketplace_name == "uze-official" {
            return self.install_from_marketplace(plugin_name, authority);
        }
        let record =
            uze_core::state::marketplace_get(&self.home, marketplace_name)?.ok_or_else(|| {
                UzeError::UnknownPackage(format!("marketplace `{marketplace_name}` not found"))
            })?;
        let (marketplace_root, manifest) = Self::load_marketplace_manifest(&record.source)?;
        let plugin_source = uze_core::acquisition::marketplace::resolve_plugin_source(
            &manifest,
            plugin_name,
            &marketplace_root,
        )?;
        let source = PackageSource::Local {
            path: plugin_source,
        };
        let report = self.add_plugin(source, authority)?;
        uze_core::state::plugin_marketplace_record(
            &self.home,
            &report.plugin.id,
            marketplace_name,
        )?;
        Ok(report)
    }

    pub fn list_marketplace_plugins(&self) -> Result<Vec<MarketplacePluginSummary>> {
        let (_name, entries) = bootstrap::entries()?;
        let installed_packages = self.installed_packages();
        let installed: std::collections::BTreeMap<&str, &StoredPackage> = installed_packages
            .iter()
            .map(|package| (package.id.as_str(), package))
            .collect();
        Ok(entries
            .into_iter()
            .map(|entry| {
                let installed_package = installed.get(entry.name.as_str());
                let update_available = installed_package
                    .and_then(|package| bootstrap::has_update(&entry.name, &package.root).ok());
                MarketplacePluginSummary {
                    name: entry.name.clone(),
                    description: entry.description,
                    keywords: entry.keywords,
                    installed: installed_package.is_some(),
                    update_available,
                    is_default: bootstrap::DEFAULT_PLUGIN_IDS.contains(&entry.name.as_str()),
                }
            })
            .collect())
    }

    pub fn inspect_marketplace_plugin(&self, name: &str) -> Result<MarketplacePluginDetail> {
        let summary = self
            .list_marketplace_plugins()?
            .into_iter()
            .find(|plugin| plugin.name == name)
            .ok_or_else(|| UzeError::UnknownPackage(name.to_owned()))?;
        let materialized = bootstrap::materialize(name)?;
        let inspected = uze_core::acquisition::inspect_capabilities(&materialized)?;
        Ok(MarketplacePluginDetail {
            capabilities: inspected
                .resources
                .iter()
                .map(|resource| PluginCapability {
                    identity: resource.identity(),
                    name: resource.name(),
                    kind: resource.capability.kind,
                })
                .collect(),
            summary,
        })
    }

    pub fn install_from_marketplace(
        &self,
        name: &str,
        authority: &dyn TrustAuthority,
    ) -> Result<AddPluginReport> {
        self.add_plugin(
            PackageSource::Embedded {
                id: name.to_owned(),
            },
            authority,
        )
    }
}
