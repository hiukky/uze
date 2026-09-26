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

/// Names the marketplace and the URL on an access refusal.
///
/// Git says "could not read from remote repository"; only this layer knows
/// *which* repository that was and what the operator calls it. Every other
/// error is passed through untouched — wrapping them all would bury the
/// one that is actually about access.
pub(crate) fn naming_the_marketplace<T>(
    result: Result<T>,
    marketplace: &str,
    url: &str,
) -> Result<T> {
    match result {
        Err(UzeError::RepositoryAccessRefused { detail }) => {
            Err(UzeError::RepositoryAccessRefused {
                detail: format!("marketplace `{marketplace}` at {url}\n{detail}"),
            })
        }
        Err(UzeError::RepositoryOffline { detail }) => Err(UzeError::RepositoryOffline {
            detail: format!("marketplace `{marketplace}` at {url}\n{detail}"),
        }),
        other => other,
    }
}

/// Where a plugin's bytes are fetched from and written to.
///
/// A mirror is per marketplace, so installing needs the name the machine
/// registered it under — the same key its catalogue is filled under, which
/// is what lets one connection serve both.
pub(crate) struct MirrorAt<'a> {
    pub(crate) home: &'a uze_core::UzeHome,
    pub(crate) marketplace: &'a str,
    /// How old an answer about a branch may be: [`RECENT`] for adding and
    /// installing, which ask the remote the same question a `market add`
    /// seconds earlier already did, and `None` for updating, whose whole
    /// point is where the ref points now.
    pub(crate) recent: Option<std::time::Duration>,
    /// What this command already fetched, which is fresh whatever `recent`.
    pub(crate) fetched: &'a std::sync::Mutex<Vec<PathBuf>>,
}

/// Brings every marketplace an operation is about to read up to date at
/// once, so a project drawing from four marketplaces waits for the slowest
/// of them rather than for all four in a row.
///
/// Best effort: one that fails is left for the operation to reach on its
/// own, which is where its failure is reported. One this machine reads from
/// a linked checkout has no mirror to fetch.
pub(crate) fn prefetch_mirrors(
    home: &uze_core::UzeHome,
    fetched: &std::sync::Mutex<Vec<PathBuf>>,
    wanted: &[(String, MarketplaceRequest)],
    recent: Option<std::time::Duration>,
) {
    let mut seen: Vec<&str> = Vec::new();
    let wanted: Vec<&(String, MarketplaceRequest)> = wanted
        .iter()
        .filter(|(name, _)| {
            let first = !seen.contains(&name.as_str());
            seen.push(name);
            first
        })
        .filter(|(name, _)| {
            !uze_core::state::marketplace_get(home, name)
                .ok()
                .flatten()
                .is_some_and(|record| record.link.is_some())
        })
        .collect();
    if wanted.len() < 2 {
        return;
    }
    let parent = tracing::Span::current();
    std::thread::scope(|scope| {
        for (name, request) in wanted {
            let parent = parent.clone();
            scope.spawn(move || {
                let directory = super::marketplace_catalogue::mirror_dir(home, name);
                let reached = parent.in_scope(|| {
                    acquisition::mirror::ensure_for(
                        &request.repository.fetch,
                        &request.repository.identity,
                        &directory,
                        request.reference.as_deref(),
                        recent,
                    )
                });
                if reached.is_ok()
                    && let Ok(mut fetched) = fetched.lock()
                {
                    fetched.push(directory);
                }
            });
        }
    });
}

/// How long a mirror's answer about a branch stands for adding a plugin: as
/// long as the catalogue it was chosen from stands. What a listing showed is
/// what adding installs, rather than something newer nobody has seen;
/// `uze update` is what asks the remote where the ref is now.
pub(crate) const RECENT: std::time::Duration = super::marketplace_catalogue::MAX_AGE;

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
        let fetched_already = at
            .fetched
            .lock()
            .is_ok_and(|fetched| fetched.contains(&repository));
        let recent = if fetched_already {
            Some(std::time::Duration::MAX)
        } else {
            at.recent
        };
        naming_the_marketplace(
            acquisition::mirror::ensure_for(
                &self.repository.fetch,
                &self.repository.identity,
                &repository,
                self.reference.as_deref(),
                recent,
            ),
            at.marketplace,
            &self.repository.identity,
        )?;
        if let Ok(mut fetched) = at.fetched.lock()
            && !fetched.contains(&repository)
        {
            fetched.push(repository.clone());
        }
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
        self.register(source_str)
            .map(|registration| registration.added)
    }

    /// [`Self::add`], answering with what the input resolved to.
    ///
    /// A short locator names one host — its prefix, or the machine's
    /// default — and is never tried on another: a forge answers "not
    /// found" for a private repository the caller cannot see, so a
    /// fallback would resolve the operator's own private name to whoever
    /// owns it elsewhere. The failure suggests the other hosts instead.
    #[tracing::instrument(name = "marketplace.register", skip_all, fields(source_str = %source_str), err)]
    pub fn register(&self, source_str: &str) -> Result<MarketplaceRegistration> {
        let hosts = uze_core::hosts::load(&self.0.home)?;
        let (source, short) = self.parse_source(source_str, &hosts)?;
        let registered = self.register_source(&source);
        let added = match registered {
            Err(error) if short => return Err(suggesting_other_hosts(error, &source, &hosts)),
            other => other?,
        };
        // A local directory that is not a repository yet still registers —
        // its installs will say what it lacks — and names only itself.
        let identity = marketplace::repository_of(&source)
            .map(|repository| repository.identity)
            .unwrap_or_else(|_| source.display());
        let resolves_here_only = match &source {
            PackageSource::Local { path } => identity == path.display().to_string(),
            _ => false,
        };
        Ok(MarketplaceRegistration {
            added,
            resolves_here_only,
            identity,
        })
    }

    /// What the operator typed, read by the one locator grammar: a path
    /// spelled as one, a URL, `alias:owner/repo`, or `owner/repo` on the
    /// default host. A remote is recorded in its canonical spelling.
    fn parse_source(
        &self,
        source_str: &str,
        hosts: &uze_core::hosts::Hosts,
    ) -> Result<(PackageSource, bool)> {
        let locator =
            acquisition::forge::parse_locator(source_str, hosts, &|path| Path::new(path).is_dir())
                .map_err(|refusal| UzeError::UnreadableLocator(refusal.to_string()))?;
        match locator {
            acquisition::forge::Locator::Path(typed) => {
                let path = typed
                    .canonicalize()
                    .map_err(|_| UzeError::MissingPath(typed.clone()))?;
                let manifest_path = path.join(uze_core::workspace::MARKETPLACE_MANIFEST_NAME);
                if !manifest_path.is_file() {
                    return Err(UzeError::MissingManifest(manifest_path));
                }
                Ok((PackageSource::Local { path }, false))
            }
            acquisition::forge::Locator::Remote {
                url,
                reference,
                subdirectory,
                short,
            } => Ok((
                PackageSource::Git {
                    url: acquisition::forge::canonical(&url),
                    reference,
                    subdirectory,
                },
                short,
            )),
        }
    }

    fn register_source(&self, source: &PackageSource) -> Result<bool> {
        let source = source.clone();
        // A Git source is mirrored rather than cloned: the mirror is what
        // answers the name, and it is also what every later read and every
        // install of one of its plugins works from. A local source is read
        // where it is — its author is editing it.
        let registered = uze_core::state::marketplace_list(&self.0.home)?
            .into_iter()
            .find(|(_, record)| record.source.same_source(&source))
            .map(|(name, _)| name);
        let name = match (&source, registered) {
            // Already registered: a fetch into its mirror, reached the way it
            // was last reached, rather than a clone from nothing.
            (PackageSource::Git { .. }, Some(name)) => {
                self.0.marketplace_catalogues.refresh(&name, &source)?;
                name
            }
            (PackageSource::Git { .. }, None) => self.0.marketplace_catalogues.adopt(&source)?.0,
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

    /// [`Self::register`], for a caller that already holds a typed source.
    ///
    /// A local directory that is not a repository yet still registers —
    /// its installs will say what it lacks — and names only itself.
    #[tracing::instrument(name = "marketplace.register_source", skip_all, err)]
    pub fn register_typed_source(&self, source: &PackageSource) -> Result<bool> {
        self.register_source(source)
    }

    /// The host aliases this machine resolves, built-ins first.
    #[tracing::instrument(name = "marketplace.hosts", skip_all, err)]
    pub fn hosts(&self) -> Result<Vec<uze_core::hosts::HostEntry>> {
        Ok(uze_core::hosts::load(&self.0.home)?.entries())
    }

    /// Makes `alias` the host a bare `owner/repo` resolves against.
    #[tracing::instrument(name = "marketplace.host_default", skip_all, fields(alias = %alias), err)]
    pub fn set_default_host(&self, alias: &str) -> Result<()> {
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
        uze_core::hosts::set_default(&self.0.home, alias)
    }

    /// Defines `alias` as the forge at `base`.
    #[tracing::instrument(name = "marketplace.host_define", skip_all, fields(alias = %alias), err)]
    pub fn define_host(&self, alias: &str, base: &str) -> Result<()> {
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
        uze_core::hosts::define(&self.0.home, alias, base)
    }

    /// Removes an alias the operator defined; `Ok(true)` when it was the
    /// default, which is `github` again.
    #[tracing::instrument(name = "marketplace.host_remove", skip_all, fields(alias = %alias), err)]
    pub fn remove_host(&self, alias: &str) -> Result<bool> {
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
        uze_core::hosts::remove(&self.0.home, alias)
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
    pub fn remove(&self, name: &str) -> Result<MarketplaceRemovalReport> {
        if name == BUILT_IN_MARKETPLACE {
            return Err(UzeError::ReservedMarketplace(name.to_owned()));
        }
        let _mutation = uze_core::persistence::MutationLock::acquire(&self.0.home)?;
        // Removal changes vendor-visible state; cached inspection verdicts
        // must not outlive it (ADR 018).
        self.0.inspection_cache.invalidate();
        // Taking a marketplace off the machine takes what it delivered with
        // it: every package the Store holds from this marketplace goes
        // through the same teardown a per-package removal runs —
        // inspect-before-detach, receipt-owned, drift safety. The lock is
        // held here, so the inner, lock-free teardown path is what runs;
        // `Plugins::remove` would refuse the re-entrant acquisition.
        let installed: Vec<String> = self
            .0
            .installed_packages()
            .into_iter()
            .filter(|package| package.id.marketplace() == name)
            .map(|package| package.id.as_str().to_owned())
            .collect();
        let mut removed = Vec::new();
        let mut blocked = Vec::new();
        for id in &installed {
            match self.0.plugins().detach_and_remove(id, false) {
                Ok(RemovePluginReport::Removed { plugin, .. })
                | Ok(RemovePluginReport::AlreadyAbsent { plugin }) => removed.push(plugin),
                Ok(RemovePluginReport::Blocked { report, plan }) => {
                    blocked.push(BlockedPackageRemoval {
                        package: id.clone(),
                        reason: format!(
                            "{} was blocked ({plan:?}); its bytes were left in place",
                            report.package_id
                        ),
                    });
                }
                Err(error) => blocked.push(BlockedPackageRemoval {
                    package: id.clone(),
                    reason: error.to_string(),
                }),
            }
        }
        // The registry entry goes last: a marketplace that could not be
        // emptied stays registered, so the leftovers it still holds are
        // reachable — `market remove` again after the block is cleared.
        let record_removed = blocked.is_empty();
        if record_removed {
            uze_core::state::marketplace_remove(&self.0.home, name)?;
            self.0.marketplace_catalogues.invalidate(name);
        }
        Ok(MarketplaceRemovalReport {
            marketplace: name.to_owned(),
            removed,
            blocked,
            record_removed,
        })
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
            linked_to: None,
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
                linked_to: record.link,
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
    /// (ADR-036) — see `Marketplace::install_plugin_resolving`.
    #[tracing::instrument(name = "marketplace.install_plugin_resolving", skip_all, fields(spec = %spec), err)]
    pub fn install_plugin_resolving(
        &self,
        spec: &str,
        authority: &dyn TrustAuthority,
        name_authority: &dyn NameCollisionAuthority,
    ) -> Result<AddPluginReport> {
        self.0.begin_operation();
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
                    recent: Some(RECENT),
                    fetched: &self.0.mirrors_fetched,
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
            // As it stands: this list is drawn on every refresh of the
            // plugins screen, and a refill here is a remote inside a
            // render.
            let Ok(catalogue) = self.0.catalogue_as_it_stands(&name, &record.source) else {
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
            let catalogue = self.0.catalogue_as_it_stands(marketplace, &record.source)?;
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
            // The built-in marketplace has no repository to ask; every
            // other one is asked about this plugin's own directory.
            revision: if marketplace == BUILT_IN_MARKETPLACE {
                Some(Revision::Bundled {
                    version: env!("CARGO_PKG_VERSION").to_owned(),
                })
            } else {
                self.0.offered_revision(marketplace, name)
            },
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

/// A short locator's failure, with the prefixed forms to try on the other
/// hosts this machine knows. Only suggested — never tried.
fn suggesting_other_hosts(
    error: UzeError,
    source: &PackageSource,
    hosts: &uze_core::hosts::Hosts,
) -> UzeError {
    let PackageSource::Git { url, .. } = source else {
        return error;
    };
    // The repository's path from its host's root — `my-org/plugins` even
    // when it was typed as `org:plugins` — is the one name another host
    // could hold it under, and only an alias for a host's root spells it.
    let Some(path) = url
        .split_once("://")
        .and_then(|(_, rest)| rest.split_once('/'))
        .map(|(_, path)| path.trim_matches('/'))
        .filter(|path| !path.is_empty())
    else {
        return error;
    };
    // Host and port: two forges on one machine are two places to look.
    let authority = |url: &str| {
        url.split_once("://")
            .map(|(_, rest)| rest.split('/').next().unwrap_or_default().to_lowercase())
    };
    let asked = authority(url);
    let others: Vec<String> = hosts
        .entries()
        .into_iter()
        .filter(|entry| {
            let at_root = entry
                .base
                .split_once("://")
                .is_some_and(|(_, rest)| !rest.trim_end_matches('/').contains('/'));
            at_root && authority(&entry.base) != asked
        })
        .map(|entry| format!("{}:{path}", entry.alias))
        .collect();
    if others.is_empty() {
        return error;
    }
    let hint = format!("If it lives elsewhere: {}", others.join(", "));
    match error {
        UzeError::RepositoryAccessRefused { detail } => UzeError::RepositoryAccessRefused {
            detail: format!("{detail}\n{hint}"),
        },
        other => other,
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
    /// A marketplace that moved makes the plugin installed from it read as
    /// behind — answered from the entry the mirror wrote, with no
    /// subprocess, because every listing pays this read.
    fn a_plugin_reads_as_behind_once_its_marketplace_has_moved() {
        let home_root = uze_testkit::temp::scratch("freshness-distance");
        let (repository, _first) = marketplace("freshness-distance-src");
        let home = UzeHome::at(home_root.join("uze"));
        let application = UzeApplication::new(home.clone(), Vec::new());

        application
            .marketplace()
            .add(&format!("file://{}", repository.root().display()))
            .unwrap();
        application
            .marketplace()
            .install_plugin("flow@mkt", &uze_core::trust::AlwaysTrust)
            .unwrap();

        // The marketplace moves twice after the install.
        for subject in ["second", "third"] {
            fs::write(
                repository.root().join("plugins/flow/skills/one/SKILL.md"),
                format!("---\nname: one\ndescription: d\n---\n\nflow {subject}.\n"),
            )
            .unwrap();
            repository.git(&["add", "-A"]);
            repository.git(&["commit", "-m", subject]);
        }

        // Age the cached entry so the next catalogue read refills it, which
        // is what brings the mirror — and the head freshness compares
        // against — up to date.
        let entry = home.marketplace_cache_dir().join("mkt");
        let meta_path = entry.join("catalogue.json");
        let mut meta: serde_json::Value =
            serde_json::from_slice(&fs::read(&meta_path).unwrap()).unwrap();
        meta["cached_at_unix_nanos"] = serde_json::json!(0);
        fs::write(&meta_path, serde_json::to_vec_pretty(&meta).unwrap()).unwrap();
        // A fresh application, the way a new invocation is: the catalogue
        // is refilled from disk rather than answered from this one's memo.
        let application = UzeApplication::new(home.clone(), Vec::new());
        // `list` is the read that refills an expired entry; the plugin
        // reads deliberately answer from the entry as it stands, so that no
        // listing pays the network.
        application.marketplace().list().unwrap();

        let listed = application.plugins().list().unwrap();
        let flow = listed
            .iter()
            .find(|plugin| plugin.active_name == "flow")
            .expect("flow is installed");
        assert_eq!(
            flow.freshness.state,
            crate::application::FreshnessState::Behind { commits: None },
            "a listing reports that there is something newer, and counts nothing"
        );
        assert!(
            flow.freshness.established_at_unix.is_some(),
            "carrying when the comparison was made, which is what makes it \
             readable as an answer rather than as a guess"
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

#[cfg(test)]
mod removal_tests {
    use crate::UzeApplication;
    use uze_core::integration::{AttachmentInspection, AttachmentState};
    use uze_core::{PackageSource, UzeHome};

    #[test]
    fn removing_a_marketplace_invalidates_the_inspection_cache() {
        let base = uze_testkit::temp::scratch("market-remove-invalidates");
        let home = UzeHome::at(base.join("home"));
        let application = UzeApplication::new(home.clone(), Vec::new());
        uze_core::state::marketplace_add(
            &home,
            "tools",
            PackageSource::Local {
                path: base.join("tools"),
            },
        )
        .unwrap();
        let matched = AttachmentInspection {
            state: AttachmentState::Matched,
            reason: String::new(),
        };
        application.inspection_cache.put("ledger", &matched, None);

        application.marketplace().remove("tools").unwrap();

        assert!(
            application.inspection_cache.get("ledger", None).is_none(),
            "a verdict cached before the removal must not outlive it"
        );
        std::fs::remove_dir_all(&base).ok();
    }
}
