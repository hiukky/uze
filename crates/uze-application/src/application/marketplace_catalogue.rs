//! Cache of what a registered marketplace offers — ADR 018's contract
//! applied to a remote.
//!
//! A marketplace registered by URL is a Git repository somewhere else.
//! Listing what it offers means reading its `marketplace.json`, and doing
//! that by cloning the whole repository into a scratch directory made
//! every listing a network round trip: seconds over SSH, paid by
//! `market list`, by the plugin picker, and twice by every refresh of the
//! management screen. A remote is the one input UZE reads that has no
//! fingerprint at all, so the contract here is TTL plus mutation
//! invalidation, nothing cleverer:
//!
//! - one checkout per Git marketplace under
//!   `UzeHome::marketplace_cache_dir()`, with a small `catalogue.json`
//!   beside it recording the source it was read from and when;
//! - in-process memoization on top, so a marketplace listing and a plugin
//!   listing in one command read the directory once;
//! - a bounded TTL, after which the next read clones again — once per
//!   window, not once per screen;
//! - `market add` fills the entry from the clone it already made to learn
//!   the marketplace's name, so registering the same source again is how
//!   a catalogue is refreshed on demand; `market remove` drops it;
//! - a local marketplace is read in place every time: there is no remote
//!   to spare, and its author is editing it.
//!
//! Fail-open like the other two caches: an unreadable entry is a miss,
//! and a refill that fails while an expired entry still exists answers
//! with what was last seen — a listing must not be worse offline than it
//! was the last time the remote answered.

use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use uze_core::{
    PackageSource, Result, UzeError, UzeHome,
    acquisition::{self, marketplace::MarketplaceManifest},
    workspace::MARKETPLACE_MANIFEST_NAME,
};

/// How long a catalogue stands for before a read clones the remote again.
/// Shorter than the detection cache's day: a marketplace has a person on
/// the other end pushing to it, and nothing here can see that happen.
const MAX_AGE: Duration = Duration::from_secs(60 * 60);

const META_FILE: &str = "catalogue.json";
/// The mirror itself: a bare, blobless clone.
const REPOSITORY_DIR: &str = "repo";
/// Where a plugin's bytes are written when something asks about one.
const MATERIALIZED_DIR: &str = "plugins";

/// A marketplace's manifest, and how to reach the bytes of a plugin it
/// offers.
///
/// The two are separate because a Git marketplace has no directory: its
/// catalogue is a mirror, which answers "what is offered" from history and
/// materializes a plugin's own subdirectory only when something asks for
/// it. A local marketplace is the directory, read where it is.
#[derive(Clone, Debug)]
pub struct Catalogue {
    pub manifest: MarketplaceManifest,
    pub reach: Reach,
}

/// Where a catalogue's plugins are read from.
#[derive(Clone, Debug)]
pub enum Reach {
    /// The marketplace's own directory on this machine — its author is
    /// editing it, so it is read in place and never copied.
    InPlace { root: PathBuf },
    /// A mirror and the commit this catalogue was read at. A plugin's
    /// bytes are written out on demand, under `materialized`, and only
    /// for the plugins something actually asks about.
    Mirrored {
        repository: PathBuf,
        commit: String,
        materialized: PathBuf,
    },
}

impl Catalogue {
    /// The directory holding `plugin`'s bytes, materializing them when the
    /// catalogue is a mirror and they are not out yet.
    ///
    /// Bounded by what is asked for: browsing a marketplace's listing
    /// materializes nothing, and looking at one plugin materializes that
    /// plugin. The whole-tree copy this replaces wrote out every directory
    /// in the repository, installed or not.
    pub fn plugin_root(&self, plugin: &str) -> Result<PathBuf> {
        match &self.reach {
            Reach::InPlace { root } => {
                acquisition::marketplace::resolve_plugin_source(&self.manifest, plugin, root)
            }
            Reach::Mirrored {
                repository,
                commit,
                materialized,
            } => {
                let out = materialized.join(directory_name(plugin));
                if !out.exists() {
                    let within =
                        acquisition::marketplace::plugin_subdirectory(&self.manifest, plugin)?;
                    acquisition::mirror::materialize_subdirectory(
                        repository,
                        commit,
                        Some(&within),
                        &out,
                    )?;
                }
                acquisition::marketplace::resolve_plugin_source(&self.manifest, plugin, &out)
            }
        }
    }
}

#[derive(Deserialize, Serialize)]
struct Meta {
    source: PackageSource,
    cached_at_unix_nanos: u128,
    /// The commit the mirror was read at. Recorded because the whole
    /// reason to keep a repository rather than a copied tree is to be able
    /// to say which revision an answer is about.
    #[serde(default)]
    commit: Option<String>,
}

pub struct MarketplaceCatalogues {
    root: PathBuf,
    memo: RefCell<HashMap<String, Catalogue>>,
}

impl MarketplaceCatalogues {
    pub fn new(home: &UzeHome) -> Self {
        Self {
            root: home.marketplace_cache_dir(),
            memo: RefCell::new(HashMap::new()),
        }
    }

    /// The catalogue registered as `name` at `source`. A Git source is
    /// answered from the cache while its entry stands, and refilled by
    /// cloning when it does not; a local source is read where it is.
    pub fn read(&self, name: &str, source: &PackageSource) -> Result<Catalogue> {
        if let Some(catalogue) = self.memo.borrow().get(name) {
            return Ok(catalogue.clone());
        }
        let catalogue = match source {
            PackageSource::Local { path } => read_in_place(path)?,
            PackageSource::Embedded { .. } => {
                return Err(UzeError::ExposureUnavailable(
                    "embedded marketplace cannot be used as marketplace source".to_owned(),
                ));
            }
            PackageSource::Git { .. } => match self.on_disk(name, source, false) {
                Some(catalogue) => catalogue,
                None => match self.refill(name, source) {
                    Ok(catalogue) => catalogue,
                    Err(error) => self.on_disk(name, source, true).ok_or(error)?,
                },
            },
        };
        self.memo
            .borrow_mut()
            .insert(name.to_owned(), catalogue.clone());
        Ok(catalogue)
    }

    /// Forgets `name` in both tiers.
    pub fn invalidate(&self, name: &str) {
        self.memo.borrow_mut().remove(name);
        let _ = fs::remove_dir_all(self.entry_dir(name));
    }

    /// Brings `name`'s mirror up to date and reads the manifest out of it.
    ///
    /// The first call clones; every one after it fetches. Nothing is
    /// checked out: the manifest is read from the commit, which is the
    /// whole reason the mirror has no working tree.
    fn refill(&self, name: &str, source: &PackageSource) -> Result<Catalogue> {
        let _span = tracing::info_span!("marketplace.fetch", marketplace = name).entered();
        let PackageSource::Git { url, reference, .. } = source else {
            return Err(UzeError::ExposureUnavailable(
                "only a Git marketplace is mirrored".to_owned(),
            ));
        };
        let entry = self.entry_dir(name);
        let repository = entry.join(REPOSITORY_DIR);
        acquisition::mirror::ensure(url, &repository)?;
        let commit = acquisition::mirror::resolve(&repository, reference.as_deref())?;
        let manifest = self.manifest_at(&repository, &commit)?;

        let meta = Meta {
            source: source.clone(),
            cached_at_unix_nanos: now_unix_nanos(),
            commit: Some(commit.clone()),
        };
        let payload = serde_json::to_vec_pretty(&meta).expect("catalogue meta is serializable");
        fs::create_dir_all(&entry).map_err(|source| UzeError::Write {
            path: entry.clone(),
            source,
        })?;
        fs::write(entry.join(META_FILE), payload).map_err(|source| UzeError::Write {
            path: entry.join(META_FILE),
            source,
        })?;
        // A plugin materialized from an older commit is not this one's.
        let _ = fs::remove_dir_all(entry.join(MATERIALIZED_DIR));

        Ok(Catalogue {
            manifest,
            reach: Reach::Mirrored {
                repository,
                commit,
                materialized: entry.join(MATERIALIZED_DIR),
            },
        })
    }

    fn manifest_at(&self, repository: &Path, commit: &str) -> Result<MarketplaceManifest> {
        let bytes = acquisition::mirror::read_file(repository, commit, MARKETPLACE_MANIFEST_NAME)?;
        acquisition::marketplace::parse_manifest(&bytes)
    }

    /// Registers a Git source whose marketplace name is not known yet:
    /// mirrors it, reads the name out of the manifest, and keeps the mirror
    /// under that name. `market add` used to clone the whole repository for
    /// this and then hand the copy to the cache.
    pub fn adopt(&self, source: &PackageSource) -> Result<(String, Catalogue)> {
        let PackageSource::Git { url, reference, .. } = source else {
            return Err(UzeError::ExposureUnavailable(
                "only a Git marketplace is mirrored".to_owned(),
            ));
        };
        let staging = self.root.join(format!(
            ".adopting-{}-{}",
            std::process::id(),
            now_unix_nanos()
        ));
        let adopted = (|| {
            acquisition::mirror::ensure(url, &staging)?;
            let commit = acquisition::mirror::resolve(&staging, reference.as_deref())?;
            let manifest = self.manifest_at(&staging, &commit)?;
            Ok((manifest.name.clone(), manifest, commit))
        })();
        let (name, manifest, commit) = match adopted {
            Ok(answer) => answer,
            Err(error) => {
                let _ = fs::remove_dir_all(&staging);
                return Err(error);
            }
        };

        let entry = self.entry_dir(&name);
        let repository = entry.join(REPOSITORY_DIR);
        let moved = (|| {
            if entry.exists() {
                fs::remove_dir_all(&entry).map_err(|source| UzeError::Write {
                    path: entry.clone(),
                    source,
                })?;
            }
            fs::create_dir_all(&entry).map_err(|source| UzeError::Write {
                path: entry.clone(),
                source,
            })?;
            fs::rename(&staging, &repository).map_err(|source| UzeError::Write {
                path: repository.clone(),
                source,
            })?;
            let meta = Meta {
                source: source.clone(),
                cached_at_unix_nanos: now_unix_nanos(),
                commit: Some(commit.clone()),
            };
            let payload = serde_json::to_vec_pretty(&meta).expect("catalogue meta is serializable");
            fs::write(entry.join(META_FILE), payload).map_err(|source| UzeError::Write {
                path: entry.join(META_FILE),
                source,
            })
        })();
        if moved.is_err() {
            let _ = fs::remove_dir_all(&staging);
        }
        moved?;

        let catalogue = Catalogue {
            manifest,
            reach: Reach::Mirrored {
                repository,
                commit,
                materialized: entry.join(MATERIALIZED_DIR),
            },
        };
        self.memo
            .borrow_mut()
            .insert(name.clone(), catalogue.clone());
        Ok((name, catalogue))
    }

    /// The on-disk entry for `name`, if it was read from `source` and,
    /// unless `accept_expired`, inside the TTL.
    fn on_disk(
        &self,
        name: &str,
        source: &PackageSource,
        accept_expired: bool,
    ) -> Option<Catalogue> {
        let entry = self.entry_dir(name);
        let meta: Meta = serde_json::from_slice(&fs::read(entry.join(META_FILE)).ok()?).ok()?;
        if &meta.source != source {
            return None;
        }
        let age_nanos = now_unix_nanos().saturating_sub(meta.cached_at_unix_nanos);
        if !accept_expired && age_nanos >= MAX_AGE.as_nanos() {
            return None;
        }
        // An entry written by a build that kept a copied tree has no
        // commit; there is nothing to carry across in the cache tier, so it
        // is a miss and the mirror is made.
        let commit = meta.commit?;
        let repository = entry.join(REPOSITORY_DIR);
        let manifest = self.manifest_at(&repository, &commit).ok()?;
        Some(Catalogue {
            manifest,
            reach: Reach::Mirrored {
                repository,
                commit,
                materialized: entry.join(MATERIALIZED_DIR),
            },
        })
    }

    fn entry_dir(&self, name: &str) -> PathBuf {
        self.root.join(directory_name(name))
    }
}

/// The marketplace manifest at `root`, read where it is.
pub(crate) fn read_in_place(root: &Path) -> Result<Catalogue> {
    let path = root.join(MARKETPLACE_MANIFEST_NAME);
    let bytes = fs::read(&path).map_err(|source| UzeError::Read {
        path: path.clone(),
        source,
    })?;
    Ok(Catalogue {
        manifest: acquisition::marketplace::parse_manifest(&bytes)?,
        reach: Reach::InPlace {
            root: root.to_path_buf(),
        },
    })
}

/// Where `name`'s mirror lives under `home`.
///
/// Public so acquisition can install a plugin from the same mirror a
/// listing already filled — which is the whole point of keeping one: the
/// second plugin from a marketplace costs no second connection.
pub(crate) fn mirror_dir(home: &UzeHome, name: &str) -> PathBuf {
    home.marketplace_cache_dir()
        .join(directory_name(name))
        .join(REPOSITORY_DIR)
}

/// A marketplace name is whatever its manifest declared; as a directory
/// name it must not be able to leave the cache. Two names that collapse
/// to one directory cannot corrupt each other: the entry records the
/// source it was read from, and a mismatch is a miss.
fn directory_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|character| match character {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' => character,
            _ => '_',
        })
        .collect();
    if sanitized.is_empty() {
        "_".to_owned()
    } else {
        sanitized
    }
}

fn now_unix_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_nanos()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marketplace_at(root: &Path, name: &str, plugins: &[&str]) {
        fs::create_dir_all(root).unwrap();
        let entries: Vec<serde_json::Value> = plugins
            .iter()
            .map(|plugin| {
                fs::create_dir_all(root.join("plugins").join(plugin)).unwrap();
                fs::write(
                    root.join("plugins").join(plugin).join("plugin.json"),
                    format!(r#"{{"name":"{plugin}"}}"#),
                )
                .unwrap();
                serde_json::json!({ "name": plugin, "source": format!("./plugins/{plugin}") })
            })
            .collect();
        fs::write(
            root.join(MARKETPLACE_MANIFEST_NAME),
            serde_json::json!({ "name": name, "plugins": entries }).to_string(),
        )
        .unwrap();
    }

    /// A marketplace that is a real repository, which is what one is.
    /// The `Repository` is returned because dropping it releases the
    /// isolated Git configuration the fixture runs under.
    fn marketplace_repository(
        label: &str,
        name: &str,
        plugins: &[&str],
    ) -> (uze_testkit::git::Repository, PackageSource) {
        let repository = uze_testkit::git::Repository::empty(label);
        marketplace_at(repository.root(), name, plugins);
        repository.git(&["add", "-A"]);
        repository.git(&["commit", "-m", "marketplace"]);
        let source = PackageSource::Git {
            url: repository.root().to_string_lossy().into_owned(),
            reference: None,
            subdirectory: None,
        };
        (repository, source)
    }

    fn git_source(url: &str) -> PackageSource {
        PackageSource::Git {
            url: url.to_owned(),
            reference: None,
            subdirectory: None,
        }
    }

    #[test]
    fn a_mirrored_catalogue_answers_without_the_source_being_reachable() {
        let root = uze_testkit::temp::scratch("catalogue-mirrored");
        let (repository, source) =
            marketplace_repository("cat-mirrored", "remote", &["flow", "review"]);
        let home = UzeHome::at(root.join("uze"));

        MarketplaceCatalogues::new(&home).adopt(&source).unwrap();
        fs::remove_dir_all(repository.root()).unwrap();

        let fresh = MarketplaceCatalogues::new(&home);
        let catalogue = fresh.read("remote", &source).unwrap();
        assert_eq!(catalogue.manifest.plugins.len(), 2);
        assert!(
            matches!(catalogue.reach, Reach::Mirrored { .. }),
            "the entry is answered from its mirror, at a recorded commit"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn nothing_is_materialized_until_a_plugin_is_asked_about() {
        let root = uze_testkit::temp::scratch("catalogue-lazy");
        let (_repository, source) =
            marketplace_repository("cat-lazy", "remote", &["flow", "review"]);
        let home = UzeHome::at(root.join("uze"));
        let cache = MarketplaceCatalogues::new(&home);
        let (_, catalogue) = cache.adopt(&source).unwrap();

        let entry = home.marketplace_cache_dir().join("remote");
        assert!(
            !entry.join(MATERIALIZED_DIR).exists(),
            "listing a marketplace writes no plugin out"
        );
        assert!(
            !entry
                .join(REPOSITORY_DIR)
                .join(MARKETPLACE_MANIFEST_NAME)
                .exists(),
            "the mirror has no working tree"
        );

        let flow = catalogue.plugin_root("flow").unwrap();
        assert!(flow.join("plugin.json").is_file());
        assert!(
            !entry.join(MATERIALIZED_DIR).join("review").exists(),
            "only the plugin asked about is written out"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_entry_read_from_another_source_is_a_miss() {
        let root = uze_testkit::temp::scratch("catalogue-other-source");
        let (_repository, source) = marketplace_repository("cat-other", "remote", &["flow"]);
        let home = UzeHome::at(root.join("uze"));
        MarketplaceCatalogues::new(&home).adopt(&source).unwrap();

        let fresh = MarketplaceCatalogues::new(&home);
        let other = git_source("ssh://elsewhere.invalid/remote.git");
        assert!(
            fresh.read("remote", &other).is_err(),
            "a different source under the same name must not be answered from this entry"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_expired_entry_still_answers_when_the_refill_fails() {
        let root = uze_testkit::temp::scratch("catalogue-expired");
        let (repository, source) = marketplace_repository("cat-expired", "remote", &["flow"]);
        let home = UzeHome::at(root.join("uze"));
        MarketplaceCatalogues::new(&home).adopt(&source).unwrap();

        // Age the entry past its window, then take the source away.
        let entry = home.marketplace_cache_dir().join("remote");
        let mut meta: Meta =
            serde_json::from_slice(&fs::read(entry.join(META_FILE)).unwrap()).unwrap();
        meta.cached_at_unix_nanos = 0;
        fs::write(
            entry.join(META_FILE),
            serde_json::to_vec_pretty(&meta).unwrap(),
        )
        .unwrap();
        fs::remove_dir_all(repository.root()).unwrap();

        let fresh = MarketplaceCatalogues::new(&home);
        let catalogue = fresh.read("remote", &source).unwrap();
        assert_eq!(
            catalogue.manifest.plugins.len(),
            1,
            "an expired entry answers rather than nothing when the refill fails"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_local_marketplace_is_read_where_it_is() {
        let root = uze_testkit::temp::scratch("catalogue-local");
        let source_dir = root.join("local");
        marketplace_at(&source_dir, "local", &["flow"]);
        let home = UzeHome::at(root.join("uze"));
        let cache = MarketplaceCatalogues::new(&home);

        let catalogue = cache
            .read(
                "local",
                &PackageSource::Local {
                    path: source_dir.clone(),
                },
            )
            .unwrap();

        match &catalogue.reach {
            Reach::InPlace { root } => assert_eq!(root, &source_dir),
            other => panic!("a local marketplace is read in place, got {other:?}"),
        }
        assert!(
            !home.marketplace_cache_dir().join("local").exists(),
            "a local source leaves no cache entry"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn invalidating_drops_both_tiers() {
        let root = uze_testkit::temp::scratch("catalogue-invalidate");
        let (repository, source) = marketplace_repository("cat-invalidate", "remote", &["flow"]);
        let home = UzeHome::at(root.join("uze"));
        let cache = MarketplaceCatalogues::new(&home);
        cache.adopt(&source).unwrap();

        cache.invalidate("remote");

        assert!(!home.marketplace_cache_dir().join("remote").exists());
        fs::remove_dir_all(repository.root()).unwrap();
        assert!(
            cache.read("remote", &source).is_err(),
            "the memo went with it"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_name_cannot_leave_the_cache_directory() {
        assert_eq!(directory_name("../../etc"), "______etc");
        assert_eq!(directory_name("a/b"), "a_b");
        assert_eq!(directory_name(""), "_");
    }
}
