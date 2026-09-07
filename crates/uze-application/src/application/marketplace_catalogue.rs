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
const CHECKOUT_DIR: &str = "checkout";

/// A marketplace's manifest, and the directory its plugin entries resolve
/// against — the cached checkout for a Git source, the directory itself
/// for a local one.
#[derive(Clone, Debug)]
pub struct Catalogue {
    pub root: PathBuf,
    pub manifest: MarketplaceManifest,
}

#[derive(Deserialize, Serialize)]
struct Meta {
    source: PackageSource,
    cached_at_unix_nanos: u128,
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

    /// Records `checkout_root` — a marketplace just cloned from `source`
    /// for some other reason — as `name`'s catalogue, so the clone is not
    /// paid a second time by the listing that follows.
    pub fn store_from(
        &self,
        name: &str,
        source: &PackageSource,
        checkout_root: &Path,
    ) -> Result<Catalogue> {
        let entry = self.entry_dir(name);
        let staging = self.root.join(format!(
            "{}.staging-{}-{}",
            directory_name(name),
            std::process::id(),
            now_unix_nanos()
        ));
        let stored = (|| {
            copy_tree(checkout_root, &staging.join(CHECKOUT_DIR))?;
            let meta = Meta {
                source: source.clone(),
                cached_at_unix_nanos: now_unix_nanos(),
            };
            let payload = serde_json::to_vec_pretty(&meta).expect("catalogue meta is serializable");
            fs::write(staging.join(META_FILE), payload).map_err(|source| UzeError::Write {
                path: staging.join(META_FILE),
                source,
            })?;
            if entry.exists() {
                fs::remove_dir_all(&entry).map_err(|source| UzeError::Write {
                    path: entry.clone(),
                    source,
                })?;
            }
            fs::rename(&staging, &entry).map_err(|source| UzeError::Write {
                path: entry.clone(),
                source,
            })?;
            read_in_place(&entry.join(CHECKOUT_DIR))
        })();
        if stored.is_err() {
            let _ = fs::remove_dir_all(&staging);
        }
        let catalogue = stored?;
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

    fn refill(&self, name: &str, source: &PackageSource) -> Result<Catalogue> {
        let _span = tracing::info_span!("marketplace.clone", marketplace = name).entered();
        let checkout = acquisition::acquire(source)?;
        self.store_from(name, source, checkout.root())
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
        read_in_place(&entry.join(CHECKOUT_DIR)).ok()
    }

    fn entry_dir(&self, name: &str) -> PathBuf {
        self.root.join(directory_name(name))
    }
}

fn read_in_place(root: &Path) -> Result<Catalogue> {
    let path = root.join(MARKETPLACE_MANIFEST_NAME);
    let bytes = fs::read(&path).map_err(|source| UzeError::Read {
        path: path.clone(),
        source,
    })?;
    Ok(Catalogue {
        root: root.to_path_buf(),
        manifest: acquisition::marketplace::parse_manifest(&bytes)?,
    })
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

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination).map_err(|error| UzeError::Write {
        path: destination.to_path_buf(),
        source: error,
    })?;
    let entries = fs::read_dir(source).map_err(|error| UzeError::Read {
        path: source.to_path_buf(),
        source: error,
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| UzeError::Read {
            path: source.to_path_buf(),
            source: error,
        })?;
        let from = entry.path();
        let to = destination.join(entry.file_name());
        if from.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            fs::copy(&from, &to).map_err(|error| UzeError::Write {
                path: to.clone(),
                source: error,
            })?;
        }
    }
    Ok(())
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

    fn git_source(url: &str) -> PackageSource {
        PackageSource::Git {
            url: url.to_owned(),
            reference: None,
            subdirectory: None,
        }
    }

    #[test]
    fn a_stored_catalogue_answers_without_the_source_being_reachable() {
        let root = uze_testkit::temp::scratch("catalogue-stored");
        let source_dir = root.join("remote");
        marketplace_at(&source_dir, "remote", &["flow", "review"]);
        let cache = MarketplaceCatalogues::new(&UzeHome::at(root.join("uze")));
        let source = git_source("ssh://nowhere.invalid/remote.git");

        cache.store_from("remote", &source, &source_dir).unwrap();
        fs::remove_dir_all(&source_dir).unwrap();

        let fresh = MarketplaceCatalogues::new(&UzeHome::at(root.join("uze")));
        let catalogue = fresh.read("remote", &source).unwrap();
        assert_eq!(catalogue.manifest.plugins.len(), 2);
        assert!(catalogue.root.join("plugins/flow/plugin.json").is_file());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_entry_read_from_another_source_is_a_miss() {
        let root = uze_testkit::temp::scratch("catalogue-source-mismatch");
        let source_dir = root.join("remote");
        marketplace_at(&source_dir, "remote", &["flow"]);
        let cache = MarketplaceCatalogues::new(&UzeHome::at(root.join("uze")));
        cache
            .store_from("remote", &git_source("ssh://a.invalid/r.git"), &source_dir)
            .unwrap();

        let fresh = MarketplaceCatalogues::new(&UzeHome::at(root.join("uze")));
        assert!(
            fresh
                .on_disk("remote", &git_source("ssh://b.invalid/r.git"), true)
                .is_none()
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn an_expired_entry_still_answers_when_the_refill_fails() {
        let root = uze_testkit::temp::scratch("catalogue-expired-offline");
        let source_dir = root.join("remote");
        marketplace_at(&source_dir, "remote", &["flow"]);
        let home = UzeHome::at(root.join("uze"));
        // A `file://` URL that resolves nowhere: Git refuses it at once, so
        // the refill fails without a network round trip.
        let source = git_source("file:///nonexistent/uze-catalogue-test/remote.git");
        MarketplaceCatalogues::new(&home)
            .store_from("remote", &source, &source_dir)
            .unwrap();
        let meta_path = home.marketplace_cache_dir().join("remote").join(META_FILE);
        let expired = Meta {
            source: source.clone(),
            cached_at_unix_nanos: now_unix_nanos() - (MAX_AGE.as_nanos() + 1),
        };
        fs::write(&meta_path, serde_json::to_vec(&expired).unwrap()).unwrap();

        let cache = MarketplaceCatalogues::new(&home);
        assert!(cache.on_disk("remote", &source, false).is_none());
        // `read` tries to clone the source, which fails, and falls back to
        // the expired entry.
        let catalogue = cache.read("remote", &source).unwrap();
        assert_eq!(catalogue.manifest.plugins[0].name, "flow");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_local_marketplace_is_read_where_it_is() {
        let root = uze_testkit::temp::scratch("catalogue-local");
        let source_dir = root.join("local");
        marketplace_at(&source_dir, "local", &["flow"]);
        let cache = MarketplaceCatalogues::new(&UzeHome::at(root.join("uze")));
        let catalogue = cache
            .read("local", &PackageSource::local(&source_dir))
            .unwrap();
        assert_eq!(catalogue.root, source_dir);
        assert!(!cache.root.exists(), "a local source leaves no cache entry");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn invalidating_drops_both_tiers() {
        let root = uze_testkit::temp::scratch("catalogue-invalidate");
        let source_dir = root.join("remote");
        marketplace_at(&source_dir, "remote", &["flow"]);
        let cache = MarketplaceCatalogues::new(&UzeHome::at(root.join("uze")));
        let source = git_source("ssh://nowhere.invalid/remote.git");
        cache.store_from("remote", &source, &source_dir).unwrap();
        cache.invalidate("remote");
        assert!(cache.memo.borrow().is_empty());
        assert!(cache.on_disk("remote", &source, true).is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_name_cannot_leave_the_cache_directory() {
        assert_eq!(directory_name("../../etc"), "______etc");
        assert_eq!(directory_name("team/ai"), "team_ai");
        assert_eq!(directory_name(""), "_");
    }
}
