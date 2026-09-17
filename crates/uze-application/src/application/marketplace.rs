//! Marketplaces: registering one, reading what it offers, and installing
//! from it.

use std::path::{Path, PathBuf};

use uze_core::{
    PackageSource, Result, UzeError,
    acquisition::{self, MaterializedPackage, Provenance, ResolvedSource, marketplace},
    manifest::BUILT_IN_MARKETPLACE,
    naming::NameCollisionAuthority,
    store::StoredPackage,
    trust::TrustAuthority,
};

use crate::bootstrap;

use super::marketplace_catalogue::read_in_place;
use super::services::Marketplace;
use super::*;

/// A marketplace resolved far enough to read from: the repository behind
/// it, and the narrowing the declaration asked for.
pub(crate) struct MarketplaceRequest {
    pub(crate) repository: marketplace::MarketplaceRepository,
    pub(crate) reference: Option<String>,
    pub(crate) subdirectory: Option<PathBuf>,
}

impl MarketplaceRequest {
    /// What a machine-registered or declared source resolves to. A source
    /// that is not a repository is refused here.
    pub(crate) fn of(source: &PackageSource) -> Result<Self> {
        let repository = marketplace::repository_of(source)?;
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

    /// One plugin's bytes, taken from a *clone* of the marketplace at a
    /// commit — for a local marketplace exactly as for a remote one.
    ///
    /// Reading a local checkout in place was the alternative, and it is
    /// what made a local marketplace unpinnable: the bytes on disk are
    /// whatever their author last saved, so nothing could say they are the
    /// bytes that were installed, and nothing could say whether something
    /// newer exists. A clone at a commit answers both, and a local clone
    /// is cheap (Git hardlinks it).
    ///
    /// The provenance records the repository's `identity` — the URL
    /// another machine resolves it by — which is not always where these
    /// bytes were fetched from. The returned package is the checkout
    /// narrowed to the plugin's directory, so cleanup still owns the whole
    /// checkout (`MaterializedPackage::retarget`) and the bytes live until
    /// the Store has ingested them.
    pub(crate) fn materialize_plugin(&self, plugin: &str) -> Result<MaterializedPackage> {
        let fetch = PackageSource::Git {
            url: self.repository.fetch.clone(),
            reference: self.reference.clone(),
            subdirectory: self.subdirectory.clone(),
        };
        let mut checkout = acquisition::acquire(&fetch)?;
        let ResolvedSource::Git { commit, .. } = checkout.provenance().resolved.clone() else {
            return Err(UzeError::AcquisitionFailed(
                "a marketplace clone must resolve to a commit".to_owned(),
            ));
        };
        let catalogue = read_in_place(checkout.root())?;
        let plugin_root =
            marketplace::resolve_plugin_source(&catalogue.manifest, plugin, checkout.root())?;
        let within_marketplace = plugin_root
            .strip_prefix(checkout.root())
            .map(Path::to_path_buf)
            .ok();
        let identity = self.repository.identity.clone();
        checkout.retarget(
            plugin_root,
            Provenance {
                requested: PackageSource::Git {
                    url: identity.clone(),
                    reference: self.reference.clone(),
                    subdirectory: within_marketplace.clone(),
                },
                resolved: ResolvedSource::Git {
                    url: identity,
                    commit,
                    subdirectory: within_marketplace,
                },
            },
        );
        Ok(checkout)
    }
}

/// A marketplace source as the operator spelled it: a URL (optionally
/// `@reference` and `#subdirectory`), or a local directory holding a
/// marketplace manifest.
fn parse_marketplace_source(source_str: &str) -> Result<PackageSource> {
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

impl Marketplace<'_> {
    /// `Ok(true)` when the marketplace was newly registered, `Ok(false)`
    /// when it was already registered from the exact same source
    /// (idempotent no-op — see `state::marketplace_add`). A different
    /// source under the same name is a `MarketplaceConflict` error.
    ///
    /// Registering a Git source again is also how its cached catalogue is
    /// refreshed on demand: the clone made here to learn the marketplace's
    /// name is what the catalogue cache keeps, so the listing that follows
    /// does not pay it a second time.
    #[tracing::instrument(name = "marketplace.add", skip_all, fields(source_str = %source_str), err)]
    pub fn add(&self, source_str: &str) -> Result<bool> {
        let source = parse_marketplace_source(source_str)?;
        // A Git checkout is scratch its own `Drop` removes: held here until
        // the catalogue cache has copied what it needs.
        let checkout = acquisition::acquire(&source)?;
        let name = read_in_place(checkout.root())?.manifest.name;
        if name == BUILT_IN_MARKETPLACE {
            return Err(UzeError::ReservedMarketplace(name));
        }
        let added = uze_core::state::marketplace_add(&self.0.home, &name, source.clone())?;
        if matches!(source, PackageSource::Git { .. }) {
            self.0
                .marketplace_catalogues
                .store_from(&name, &source, checkout.root())?;
        }
        Ok(added)
    }

    #[tracing::instrument(name = "marketplace.remove", skip_all, fields(name = %name), err)]
    pub fn remove(&self, name: &str) -> Result<()> {
        if name == BUILT_IN_MARKETPLACE {
            return Err(UzeError::ReservedMarketplace(name.to_owned()));
        }
        uze_core::state::marketplace_remove(&self.0.home, name)?;
        self.0.marketplace_catalogues.invalidate(name);
        Ok(())
    }

    #[tracing::instrument(name = "marketplace.list", skip_all, err)]
    pub fn list(&self) -> Result<Vec<MarketplaceSummary>> {
        let mut out = Vec::new();
        let official = bootstrap::entries()?;
        out.push(MarketplaceSummary {
            name: "uze-official".to_owned(),
            source: "embedded:uze-official".to_owned(),
            homepage: official.homepage,
            plugin_count: official.plugins.len(),
        });
        for (name, record) in uze_core::state::marketplace_list(&self.0.home)? {
            let manifest = self
                .0
                .catalogue(&name, &record.source)
                .ok()
                .map(|catalogue| catalogue.manifest);
            let plugin_count = manifest
                .as_ref()
                .map_or(0, |manifest| manifest.plugins.len());
            // What the marketplace says about itself first; its registered
            // source only when that is a URL a browser can open. A local
            // path is where the manifest was read from, not somewhere to
            // send a reader.
            let source = record.source.display();
            let homepage = manifest
                .and_then(|manifest| manifest.owner.and_then(|owner| owner.url))
                .or_else(|| source.starts_with("http").then(|| source.clone()));
            out.push(MarketplaceSummary {
                name: name.clone(),
                source,
                homepage,
                plugin_count,
            });
        }
        Ok(out)
    }

    /// One marketplace's own detail (source, plugin count) — distinct from
    /// inspecting one plugin *within* a marketplace
    /// (`inspect_marketplace_plugin`). Filters the same per-entry
    /// computation `marketplace_list` already does down to one named entry;
    /// no new state or invariant.
    #[tracing::instrument(name = "marketplace.inspect", skip_all, fields(name = %name), err)]
    pub fn inspect(&self, name: &str) -> Result<MarketplaceSummary> {
        self.list()?
            .into_iter()
            .find(|entry| entry.name == name)
            .ok_or_else(|| UzeError::UnknownMarketplace(name.to_owned()))
    }

    #[tracing::instrument(name = "marketplace.install_plugin", skip_all, fields(spec = %spec), err)]
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
    #[tracing::instrument(name = "marketplace.install_plugin_resolving", skip_all, fields(spec = %spec), err)]
    pub fn install_plugin_resolving(
        &self,
        spec: &str,
        authority: &dyn TrustAuthority,
        name_authority: &dyn NameCollisionAuthority,
    ) -> Result<AddPluginReport> {
        let (plugin_name, marketplace_name) = uze_core::store::parse_plugin_marketplace_spec(spec)?;
        let source = if marketplace_name == BUILT_IN_MARKETPLACE {
            None
        } else {
            let record = uze_core::state::marketplace_get(&self.0.home, &marketplace_name)?
                .ok_or_else(|| UzeError::UnknownMarketplace(marketplace_name.to_owned()))?;
            Some(record.source)
        };
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
        let materialized = match source {
            None => bootstrap::materialize(&plugin_name)?,
            Some(source) => MarketplaceRequest::of(&source)?.materialize_plugin(&plugin_name)?,
        };
        self.0.plugins().install_materialized(
            materialized,
            &marketplace_name,
            None,
            authority,
            name_authority,
        )
    }

    /// Every plugin from every marketplace this Store knows about — the
    /// embedded `uze-official` snapshot plus every marketplace registered
    /// via `marketplace add` (`uze_core::state::marketplace_list`). A
    /// marketplace whose manifest can no longer be read (moved/deleted
    /// source) is skipped rather than failing the whole listing, mirroring
    /// `marketplace_list`'s own `plugin_count: 0` fallback.
    #[tracing::instrument(name = "marketplace.plugins", skip_all, err)]
    pub fn plugins(&self) -> Result<Vec<MarketplacePluginSummary>> {
        let installed_packages = self.0.installed_packages();
        let installed: std::collections::BTreeMap<&str, &StoredPackage> = installed_packages
            .iter()
            .map(|package| (package.id.as_str(), package))
            .collect();

        let mut out = Vec::new();

        out.extend(bootstrap::entries()?.plugins.into_iter().map(|entry| {
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
            let Ok(catalogue) = self.0.catalogue(&name, &record.source) else {
                continue;
            };
            out.extend(catalogue.manifest.plugins.into_iter().map(|entry| {
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

    #[tracing::instrument(name = "marketplace.inspect_plugin", skip_all, fields(marketplace = %marketplace, name = %name), err)]
    pub fn inspect_plugin(&self, marketplace: &str, name: &str) -> Result<MarketplacePluginDetail> {
        let summary = self
            .plugins()?
            .into_iter()
            .find(|plugin| plugin.marketplace == marketplace && plugin.name == name)
            .ok_or_else(|| UzeError::UnknownPackage(name.to_owned()))?;
        let materialized = if marketplace == "uze-official" {
            bootstrap::materialize(name)?
        } else {
            // Read from the catalogue's own checkout: what is on offer is a
            // question about the catalogue, and it is answered without a
            // clone, the way the listing above was. Installing is what
            // clones at a commit.
            let record = uze_core::state::marketplace_get(&self.0.home, marketplace)?
                .ok_or_else(|| UzeError::UnknownMarketplace(marketplace.to_owned()))?;
            let catalogue = self.0.catalogue(marketplace, &record.source)?;
            let plugin_root = uze_core::acquisition::marketplace::resolve_plugin_source(
                &catalogue.manifest,
                name,
                &catalogue.root,
            )?;
            uze_core::MaterializedPackage::borrowed(
                plugin_root.clone(),
                uze_core::Provenance {
                    requested: record.source,
                    resolved: uze_core::ResolvedSource::Local { path: plugin_root },
                },
            )
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
}
