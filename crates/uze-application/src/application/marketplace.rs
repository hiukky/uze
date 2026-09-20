//! Marketplaces: registering one, reading what it offers, and installing
//! from it.

use std::path::PathBuf;

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

/// Where a plugin's bytes are fetched from and written to.
///
/// A mirror is per marketplace, so installing needs the name the machine
/// registered it under — the same key its catalogue is filled under, which
/// is what lets one connection serve both.
pub(crate) struct MirrorAt<'a> {
    pub(crate) home: &'a uze_core::UzeHome,
    pub(crate) marketplace: &'a str,
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
    /// newer exists. A commit answers both.
    ///
    /// Read from the marketplace's mirror rather than by cloning the
    /// repository again. The clone this replaces was paid per plugin — a
    /// `market add` plus one install was two of them, and each further
    /// plugin another — and then discarded everything but one directory.
    /// The mirror is filled once and fetched after that, and only the
    /// plugin's own subdirectory is ever written out.
    ///
    /// The provenance records the repository's `identity` — the URL
    /// another machine resolves it by — which is not always where these
    /// bytes were fetched from. The returned package is the checkout
    /// narrowed to the plugin's directory, so cleanup still owns the whole
    /// checkout (`MaterializedPackage::retarget`) and the bytes live until
    /// the Store has ingested them.
    /// A plugin read from a checkout this machine develops.
    ///
    /// Its provenance resolves to a *path*, not a commit — because that is
    /// what it is. Nothing downstream can then mistake it for something
    /// reproducible: `agents.lock` refuses to pin it, which is the whole
    /// point, since a pin taken from unpublished work is one a
    /// collaborator cannot reach.
    fn materialize_from_link(&self, plugin: &str, checkout: &Path) -> Result<MaterializedPackage> {
        let manifest_bytes = std::fs::read(
            checkout.join(uze_core::workspace::MARKETPLACE_MANIFEST_NAME),
        )
        .map_err(|source| UzeError::Read {
            path: checkout.join(uze_core::workspace::MARKETPLACE_MANIFEST_NAME),
            source,
        })?;
        let manifest = marketplace::parse_manifest(&manifest_bytes)?;
        let within = marketplace::plugin_subdirectory(&manifest, plugin)?;

        let scratch = acquisition::scratch_directory()?;
        let within_marketplace = (within != ".").then(|| PathBuf::from(&within));
        let provenance = Provenance {
            requested: PackageSource::Local {
                path: checkout.to_path_buf(),
            },
            resolved: ResolvedSource::Local {
                path: checkout.to_path_buf(),
            },
        };
        let mut package = MaterializedPackage::owned(scratch.clone(), provenance.clone());
        acquisition::mirror::materialize_linked(checkout, Some(&within), &scratch)?;
        let plugin_root = match &within_marketplace {
            Some(within) => scratch.join(within),
            None => scratch,
        };
        package.retarget(plugin_root, provenance);
        Ok(package)
    }

    pub(crate) fn materialize_plugin(
        &self,
        plugin: &str,
        at: MirrorAt<'_>,
    ) -> Result<MaterializedPackage> {
        // A marketplace the operator develops is read from their checkout,
        // working tree and all. The Store still holds the delivered bytes —
        // every harness reads it, and containment is enforced on ingest —
        // so what the link changes is when the Store is refilled, not who
        // is authoritative.
        if let Ok(Some(record)) = uze_core::state::marketplace_get(at.home, at.marketplace)
            && let Some(checkout) = record.link
        {
            return self.materialize_from_link(plugin, &checkout);
        }

        let repository = super::marketplace_catalogue::mirror_dir(at.home, at.marketplace);
        acquisition::mirror::ensure_for(
            &self.repository.fetch,
            &repository,
            self.reference.as_deref(),
        )?;
        let commit = acquisition::mirror::resolve(&repository, self.reference.as_deref())?;

        let manifest_bytes = acquisition::mirror::read_file(
            &repository,
            &commit,
            uze_core::workspace::MARKETPLACE_MANIFEST_NAME,
        )?;
        let manifest = marketplace::parse_manifest(&manifest_bytes)?;
        let within = marketplace::plugin_subdirectory(&manifest, plugin)?;

        // Scratch the package owns: the bytes live until the Store has
        // ingested them and go with it afterwards. Only the plugin's own
        // directory is written out, never the repository.
        let scratch = acquisition::scratch_directory()?;
        let within_marketplace = (within != ".").then(|| PathBuf::from(&within));
        let identity = self.repository.identity.clone();
        let provenance = Provenance {
            requested: PackageSource::Git {
                url: identity.clone(),
                reference: self.reference.clone(),
                subdirectory: within_marketplace.clone(),
            },
            resolved: ResolvedSource::Git {
                url: identity,
                commit: commit.clone(),
                subdirectory: within_marketplace.clone(),
            },
        };
        // Owned before anything is written into it, so a failure below
        // still takes the directory with it.
        let mut package = MaterializedPackage::owned(scratch.clone(), provenance.clone());
        acquisition::mirror::materialize_subdirectory(
            &repository,
            &commit,
            Some(&within),
            &scratch,
        )?;
        let plugin_root = match &within_marketplace {
            Some(within) => scratch.join(within),
            None => scratch,
        };
        package.retarget(plugin_root, provenance);
        Ok(package)
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
        // A Git source is mirrored rather than cloned: the mirror is what
        // answers the name, and it is also what every later read and every
        // install of one of its plugins works from. A local source is read
        // where it is — its author is editing it.
        let name = match &source {
            PackageSource::Git { .. } => self.0.marketplace_catalogues.adopt(&source)?.0,
            _ => {
                let checkout = acquisition::acquire(&source)?;
                read_in_place(checkout.root())?.manifest.name
            }
        };
        if name == BUILT_IN_MARKETPLACE {
            return Err(UzeError::ReservedMarketplace(name));
        }
        uze_core::state::marketplace_add(&self.0.home, &name, source.clone())
    }

    /// Reads `name` from `checkout` on this machine from now on.
    ///
    /// Machine scope by construction: the record lives in the machine's own
    /// registry and no project file is touched. That separation is the
    /// point — inferring a link from a `path:` source is the conflation
    /// that put an operator's home directory into a versioned
    /// `agents.yaml`.
    #[tracing::instrument(name = "marketplace.link", skip_all, fields(name = %name), err)]
    pub fn link(&self, name: &str, checkout: &Path) -> Result<()> {
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
        uze_core::state::marketplace_link(&self.0.home, name, checkout)?;
        // What the catalogue holds was read from the source, not from the
        // checkout now answering for it.
        self.0.marketplace_catalogues.invalidate(name);
        Ok(())
    }

    /// Stops reading `name` from a checkout. `Ok(false)` when it was not
    /// linked, which is an answer rather than a failure.
    #[tracing::instrument(name = "marketplace.unlink", skip_all, fields(name = %name), err)]
    pub fn unlink(&self, name: &str) -> Result<bool> {
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
        let had = uze_core::state::marketplace_unlink(&self.0.home, name)?;
        self.0.marketplace_catalogues.invalidate(name);
        Ok(had)
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
            name: BUILT_IN_MARKETPLACE.to_owned(),
            source: format!("embedded:{BUILT_IN_MARKETPLACE}"),
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
    /// (`Marketplace::inspect_plugin`). Filters the same per-entry
    /// computation `Marketplace::list` already does down to one named entry;
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

    /// `Marketplace::install_plugin`, with an explicit answer for a bare-plugin-name
    /// collision with an already-active, differently-marketplaced package
    /// (ADR-038) — see `Marketplace::install_plugin_resolving`.
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
            Some(source) => MarketplaceRequest::of(&source)?.materialize_plugin(
                &plugin_name,
                MirrorAt {
                    home: &self.0.home,
                    marketplace: &marketplace_name,
                },
            )?,
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
    /// `Marketplace::list`'s own `plugin_count: 0` fallback.
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
            let installed_package =
                installed.get(format!("{}@{BUILT_IN_MARKETPLACE}", entry.name).as_str());
            let freshness = installed_package
                .map(|package| self.0.freshness_of(package))
                .unwrap_or_else(Freshness::not_checked);
            MarketplacePluginSummary {
                marketplace: BUILT_IN_MARKETPLACE.to_owned(),
                name: entry.name.clone(),
                description: entry.description,
                keywords: entry.keywords,
                installed: installed_package.is_some(),
                freshness,
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
                    freshness: installed_package
                        .map(|package| self.0.freshness_of(package))
                        .unwrap_or_else(Freshness::not_checked),
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
        let materialized = if marketplace == BUILT_IN_MARKETPLACE {
            bootstrap::materialize(name)?
        } else {
            // Read from the catalogue's own checkout: what is on offer is a
            // question about the catalogue, and it is answered without a
            // clone, the way the listing above was. Installing is what
            // clones at a commit.
            let record = uze_core::state::marketplace_get(&self.0.home, marketplace)?
                .ok_or_else(|| UzeError::UnknownMarketplace(marketplace.to_owned()))?;
            let catalogue = self.0.catalogue(marketplace, &record.source)?;
            let plugin_root = catalogue.plugin_root(name)?;
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

#[cfg(test)]
mod mirror_tests {
    use super::super::marketplace_catalogue::mirror_dir;
    use crate::UzeApplication;
    use std::fs;
    use uze_core::UzeHome;

    /// A marketplace repository with two plugins, and the commit each of
    /// its two revisions landed on.
    fn marketplace(label: &str) -> (uze_testkit::git::Repository, String) {
        let repository = uze_testkit::git::Repository::empty(label);
        let root = repository.root().to_path_buf();
        for plugin in ["flow", "review"] {
            fs::create_dir_all(root.join("plugins").join(plugin).join("skills/one")).unwrap();
            fs::write(
                root.join("plugins").join(plugin).join("plugin.json"),
                format!(r#"{{"name":"{plugin}","description":"d"}}"#),
            )
            .unwrap();
            fs::write(
                root.join("plugins")
                    .join(plugin)
                    .join("skills/one/SKILL.md"),
                format!("---\nname: one\ndescription: d\n---\n\n{plugin} first.\n"),
            )
            .unwrap();
        }
        fs::write(
            root.join("marketplace.json"),
            r#"{"name":"mkt","plugins":[
                {"name":"flow","source":"./plugins/flow"},
                {"name":"review","source":"./plugins/review"}
            ]}"#,
        )
        .unwrap();
        repository.git(&["add", "-A"]);
        repository.git(&["commit", "-m", "first"]);
        let first = repository.head();
        (repository, first)
    }

    #[test]
    fn a_plugins_bytes_are_materialized_in_exactly_one_place() {
        let home_root = uze_testkit::temp::scratch("mirror-one-place");
        let (repository, _first) = marketplace("mirror-one-place-src");
        let home = UzeHome::at(home_root.join("uze"));
        let application = UzeApplication::new(home.clone(), Vec::new());

        application
            .marketplace()
            .add(&repository.root().to_string_lossy())
            .unwrap();
        let report = application
            .marketplace()
            .install_plugin("flow@mkt", &uze_core::trust::AlwaysTrust)
            .unwrap();

        let stored = report.plugin.store_path;
        assert!(
            stored.join("skills/one/SKILL.md").is_file(),
            "the Store has the plugin"
        );
        assert!(
            !stored.join(".git").exists(),
            "no repository metadata travels into the Store"
        );

        // The cache holds a repository, not a copy: nothing under it is a
        // second materialized copy of what the Store now has.
        let entry = home.marketplace_cache_dir().join("mkt");
        assert!(mirror_dir(&home, "mkt").join("HEAD").is_file());
        assert!(
            !entry.join("checkout").exists(),
            "the cache keeps no working tree"
        );
        assert!(
            !entry.join("plugins/flow").exists(),
            "installing writes the plugin into the Store, not into the cache"
        );

        fs::remove_dir_all(&home_root).unwrap();
    }

    #[test]
    fn two_plugins_from_one_marketplace_share_one_mirror() {
        let home_root = uze_testkit::temp::scratch("mirror-second");
        let (repository, _first) = marketplace("mirror-second-src");
        let home = UzeHome::at(home_root.join("uze"));
        let application = UzeApplication::new(home.clone(), Vec::new());
        application
            .marketplace()
            .add(&format!("file://{}", repository.root().display()))
            .unwrap();

        let first = application
            .marketplace()
            .install_plugin("flow@mkt", &uze_core::trust::AlwaysTrust)
            .unwrap();
        let second = application
            .marketplace()
            .install_plugin("review@mkt", &uze_core::trust::AlwaysTrust)
            .unwrap();

        // One mirror, whatever was installed from it. The clone this
        // replaces was paid per plugin.
        let entries: Vec<_> = fs::read_dir(home.marketplace_cache_dir())
            .unwrap()
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.file_name())
            .collect();
        assert_eq!(
            entries.len(),
            1,
            "one cache entry per marketplace: {entries:?}"
        );
        assert!(mirror_dir(&home, "mkt").join("HEAD").is_file());

        // And the Store stands on its own once the cache is gone: nothing
        // a harness reads depends on the mirror existing.
        fs::remove_dir_all(home.cache_dir()).unwrap();
        assert!(
            first
                .plugin
                .store_path
                .join("skills/one/SKILL.md")
                .is_file(),
            "the first plugin still reads"
        );
        assert!(
            second
                .plugin
                .store_path
                .join("skills/one/SKILL.md")
                .is_file(),
            "the second plugin still reads"
        );

        fs::remove_dir_all(&home_root).unwrap();
    }
}
