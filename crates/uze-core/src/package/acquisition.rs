//! Core-owned source acquisition: where a package came from and how its bytes reach a local directory the
//! Store can ingest.
//!
//! This module owns every source mechanism. The Store owns bytes and persists
//! provenance without ever interpreting it — it never learns what a source
//! *is*, only that two packages either share an origin or do not. That split
//! is the M2 counterpart of the M1 one: the Store already does not know which
//! harness will consume a package, and it should not know where the package
//! was acquired either.
//!
//! ```text
//! PackageSource → acquire() → MaterializedPackage → Store::ingest()
//!   intention                  local bytes +          persists
//!                              provenance             provenance
//! ```

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

pub mod git;
pub mod marketplace;

use crate::error::{Result, UzeError};

/// What the caller asked for. Persisted so a later reinstall or update can
/// start from the same request rather than from whatever happens to be on
/// disk now.
///
/// The axis is **mechanism**, never host: a Git repository is a Git
/// repository whether it lives on GitHub, GitLab or a filesystem path, so a
/// host is data inside a variant rather than a variant of its own. Remote
/// mechanisms arrive in 9C; until then this deliberately declares only what
/// something can actually produce.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PackageSource {
    Local {
        path: PathBuf,
    },
    /// A Git repository, identified only by URL. There is deliberately no
    /// GitHub, GitLab or Bitbucket variant: those are hosts reachable by the
    /// same mechanism, and a host is data inside this variant rather than a
    /// concept the model needs.
    Git {
        url: String,
        /// Branch, tag or commit. `None` means the repository's own default
        /// branch, whatever the remote says it is — never a hardcoded name.
        reference: Option<String>,
        /// Package root within the repository. Validated to stay inside the
        /// materialized checkout.
        subdirectory: Option<PathBuf>,
    },
    /// A snapshot compiled into the running binary — the official default
    /// marketplace's offline bootstrap mechanism. `id` names which embedded
    /// snapshot, nothing more: the bytes live one layer up, in whichever
    /// composition root embedded them, so this crate never learns what
    /// `id` actually contains. `acquire` therefore cannot resolve this
    /// variant itself (see its doc comment) — callers that construct or
    /// re-resolve an `Embedded` source own that resolution.
    Embedded {
        id: String,
    },
}

impl PackageSource {
    pub fn local(path: impl Into<PathBuf>) -> Self {
        Self::Local { path: path.into() }
    }

    pub fn git(url: impl Into<String>) -> Self {
        Self::Git {
            url: url.into(),
            reference: None,
            subdirectory: None,
        }
    }

    /// Whether installing from this source crosses the trust boundary
    /// acquisition introduced.
    ///
    /// A local path was typed by the operator, who has the directory in front
    /// of them — that is the posture UZE has always had and M2 does not
    /// change it. A remote source removes exactly that: nobody read the
    /// contents, so a capability that will execute has to be authorized.
    /// "Local" is about where the bytes are, not how the source is
    /// spelled: a Git URL that names a directory on this machine is a
    /// local path with a different syntax.
    ///
    /// This is a deliberate, narrow scope. It also leaves an honest gap: an
    /// operator can clone a repository by hand and install the result as a
    /// local path, bypassing the question entirely. Closing that would mean
    /// prompting on every local install with an MCP server, which changes an
    /// existing workflow — a product decision, not one to make in passing.
    ///
    /// `Embedded` crosses the boundary too, deliberately not treated like
    /// `Local`: the operator trusted the binary, not necessarily every
    /// capability a future revision of an embedded snapshot might declare.
    /// Today's one embedded package is Skill-only, so this is dormant —
    /// nothing currently exercises it — but it means an embedded snapshot
    /// that later gains an executable capability asks, the same as any
    /// other package would.
    pub fn crosses_trust_boundary(&self) -> bool {
        match self {
            Self::Local { .. } => false,
            // A marketplace is a repository even when it is a clone on
            // this disk, so a local source can arrive spelled as a Git
            // one. The question is where the bytes are, not how the
            // source is written down: a repository the operator has in
            // front of them is the same posture as a directory they
            // typed.
            Self::Git { url, .. } => !names_a_local_path(url),
            Self::Embedded { .. } => true,
        }
    }

    /// Human-facing description for `uze list`/`inspect`. Never parsed back.
    pub fn display(&self) -> String {
        match self {
            Self::Local { path } => path.display().to_string(),
            Self::Git {
                url,
                reference,
                subdirectory,
            } => {
                let mut text = url.clone();
                if let Some(reference) = reference {
                    text.push('@');
                    text.push_str(reference);
                }
                if let Some(subdirectory) = subdirectory {
                    text.push_str(&format!(" ({})", subdirectory.display()));
                }
                text
            }
            Self::Embedded { id } => format!("embedded:{id}"),
        }
    }
}

/// What the request resolved to at acquisition time.
///
/// Kept separate from [`PackageSource`] because the two answer different
/// questions, and collapsing them loses the one that matters for
/// reproducibility: `repo@main` is a stable *request* whose *result* changes.
/// A local path has no immutable revision, and this type says so by carrying
/// none rather than inventing one.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ResolvedSource {
    /// Canonicalized at acquisition time. Deliberately carries no revision:
    /// a directory is mutable, so reinstalling from one is not reproducible
    /// and the model must not pretend otherwise.
    Local { path: PathBuf },
    /// Always an immutable commit, whatever the request named. A branch is a
    /// stable *request* whose result moves; recording only the branch would
    /// make reinstall unreproducible.
    Git {
        url: String,
        commit: String,
        subdirectory: Option<PathBuf>,
    },
    /// Identity, not a revision: an embedded snapshot's content changes only
    /// when the binary does, and the binary is already what a reinstall or
    /// update re-reads — there is nothing else to pin.
    Embedded { id: String },
}

impl ResolvedSource {
    pub fn display(&self) -> String {
        match self {
            Self::Local { path } => path.display().to_string(),
            Self::Git { url, commit, .. } => format!("{url}@{commit}"),
            Self::Embedded { id } => format!("embedded:{id}"),
        }
    }

    /// The value a lock file records to make reproduction possible: an
    /// immutable commit for Git, a fixed identity marker for the embedded
    /// snapshot, or nothing for a local path — a directory is mutable, so
    /// pinning a "revision" for it would claim a reproducibility this
    /// source can't back (see the `Local` variant's own doc). Shared by
    /// `project_lock`'s marketplace and plugin lock entries so both use
    /// the same rule.
    pub fn lock_revision(&self) -> Option<String> {
        match self {
            Self::Local { .. } => None,
            Self::Git { commit, .. } => Some(commit.clone()),
            Self::Embedded { .. } => Some("embedded".to_owned()),
        }
    }
}

/// Whether a Git URL is a directory on this machine rather than something
/// fetched.
///
/// A bare absolute path is local: it is what an operator types with the
/// directory in front of them, and Git clones it by hardlinking rather
/// than over a transport. Everything with a scheme is not — `file://`
/// included, which Git itself routes through the transport layer and which
/// is the spelling that means "treat this as a remote". `scp`-style
/// `host:path` is remote too.
fn names_a_local_path(url: &str) -> bool {
    if url.contains("://") {
        return false;
    }
    // A colon before the first slash is a host (`git@example.com:org/repo`);
    // one after it is just a directory with a colon in its name.
    if url.split('/').next().unwrap_or_default().contains(':') {
        return false;
    }
    Path::new(url).is_absolute()
}

/// Everything the Store persists about a package's origin, and nothing it
/// interprets.
///
/// The Store holds this, writes it and compares it through
/// [`Provenance::same_origin`] — it never reads a field or matches a variant.
/// That is what keeps source mechanisms out of `store.rs`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Provenance {
    pub requested: PackageSource,
    pub resolved: ResolvedSource,
}

impl Provenance {
    /// Whether two installations came from the same origin.
    ///
    /// The rule lives here rather than in the Store because only this module
    /// knows what makes two origins the same. It compares the *request*, not
    /// the resolution: a branch that moved is the same origin at a new
    /// revision, which is an update — not a conflicting package.
    pub fn same_origin(&self, other: &Self) -> bool {
        self.requested == other.requested
    }
}

/// A local directory holding a package's bytes, ready for the Store, plus the
/// provenance that produced it.
///
/// It owns cleanup, and that is the reason this is a type rather than a plain
/// path: a directory UZE created for a remote acquisition must be removed
/// afterwards, while a caller's own directory must never be. Handing back a
/// bare `PathBuf` would make every caller responsible for remembering which
/// it holds.
#[derive(Debug)]
pub struct MaterializedPackage {
    root: PathBuf,
    provenance: Provenance,
    /// Set when UZE created the scratch directory and must remove it. Held
    /// separately from `root` because `root` may be narrowed to a
    /// subdirectory while cleanup still owns the whole checkout.
    owned_scratch: Option<PathBuf>,
}

impl MaterializedPackage {
    /// A directory UZE created and must remove once the Store has ingested it.
    pub fn owned(root: PathBuf, provenance: Provenance) -> Self {
        Self {
            owned_scratch: Some(root.clone()),
            root,
            provenance,
        }
    }

    /// A directory the caller already owns. UZE reads it and never deletes it.
    pub fn borrowed(root: PathBuf, provenance: Provenance) -> Self {
        Self {
            root,
            provenance,
            owned_scratch: None,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    /// Narrows the package root to a subdirectory of the checkout and records
    /// the provenance that was only knowable after resolution.
    ///
    /// Ownership stays with the *checkout*, not the narrowed root: cleanup
    /// must still remove everything UZE created, not just the package.
    /// Narrows `root` to a subdirectory of what's already materialized,
    /// without touching cleanup: `owned_scratch` still points at the whole
    /// checkout, so it (not just the narrowed root) is what gets removed on
    /// drop. Used wherever a source resolves to a subtree of a larger
    /// acquired root — a Git checkout's `subdirectory`, or a marketplace
    /// root's resolved plugin entry.
    pub fn retarget(&mut self, root: PathBuf, provenance: Provenance) {
        self.root = root;
        self.provenance = provenance;
    }
}

impl Drop for MaterializedPackage {
    fn drop(&mut self) {
        if let Some(scratch) = &self.owned_scratch {
            let _ = fs::remove_dir_all(scratch);
        }
    }
}

/// Brings a package's bytes to a local directory.
///
/// Acquisition is the only step that knows source mechanisms. It performs no
/// package validation beyond reaching the bytes — containment and manifest
/// checks belong to the single ingestion boundary every source passes
/// through, so a local package and a remote one are held to the same rule.
///
/// It never executes package code.
/// Turns a [`PackageSource`] into local bytes the Store can ingest.
///
/// `Embedded` is deliberately not handled here: this crate has no vendor or
/// product knowledge (see the module doc), and an embedded snapshot's bytes
/// live in whatever composition root `include_str!`-ed them one layer up.
/// Resolving one is that caller's job — see
/// `uze_application::bootstrap::materialize`.
pub fn acquire(source: &PackageSource) -> Result<MaterializedPackage> {
    match source {
        PackageSource::Embedded { id } => Err(UzeError::AcquisitionFailed(format!(
            "embedded source `{id}` cannot be resolved by generic acquisition; \
             the composition root that embedded it must resolve it directly"
        ))),
        PackageSource::Local { path } => {
            let resolved = checked_directory(path)?;
            Ok(MaterializedPackage::borrowed(
                resolved.clone(),
                Provenance {
                    requested: source.clone(),
                    resolved: ResolvedSource::Local { path: resolved },
                },
            ))
        }
        PackageSource::Git {
            url,
            reference,
            subdirectory,
        } => {
            // The checkout is owned by the returned value from the moment it
            // exists, so every failure path below still cleans it up.
            let checkout = scratch_directory()?;
            let mut materialized = MaterializedPackage::owned(
                checkout.clone(),
                Provenance {
                    requested: source.clone(),
                    resolved: ResolvedSource::Local {
                        path: checkout.clone(),
                    },
                },
            );
            let commit = git::materialize(url, reference.as_deref(), &checkout)?;
            let root = match subdirectory {
                Some(subdirectory) => git::resolve_subdirectory(&checkout, subdirectory)?,
                None => checkout,
            };
            materialized.retarget(
                root,
                Provenance {
                    requested: source.clone(),
                    resolved: ResolvedSource::Git {
                        url: url.clone(),
                        commit,
                        subdirectory: subdirectory.clone(),
                    },
                },
            );
            Ok(materialized)
        }
    }
}

/// A fresh directory UZE owns for one acquisition. Deliberately not
/// `UzeHome::cache_dir()`: this is scratch that must not survive the
/// operation, and a cache would be a second place packages live.
/// A directory no other acquisition is using.
///
/// The clock alone does not say that. `as_nanos` reports whatever
/// resolution the platform's clock has, and macOS's is coarse enough that
/// two acquisitions starting together in one process read the same instant
/// — which handed both the same directory, and `git clone` then failed
/// mid-clone with `cannot copy … File exists`. A counter is what makes two
/// calls differ; the clock only makes two *runs* differ.
///
/// `create_dir`, not `create_dir_all`: an existing directory here means the
/// name was not unique after all, and that has to be an error rather than a
/// silent share. It also refuses a directory an attacker pre-created in a
/// world-writable temp dir, and `0o700` keeps package bytes unreadable
/// while they are being checked.
fn scratch_directory() -> Result<PathBuf> {
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock is after the epoch")
        .as_nanos();
    let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "uze-acquire-{}-{nonce}-{sequence}",
        std::process::id()
    ));
    fs::create_dir(&path).map_err(|source| UzeError::Write {
        path: path.clone(),
        source,
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o700));
    }
    Ok(path)
}

fn checked_directory(root: &Path) -> Result<PathBuf> {
    if !root.exists() {
        return Err(UzeError::MissingPath(root.to_path_buf()));
    }
    if !root.is_dir() {
        return Err(UzeError::NotDirectory(root.to_path_buf()));
    }
    root.canonicalize().map_err(|source| UzeError::Read {
        path: root.to_path_buf(),
        source,
    })
}

/// What a materialized package declares, read **before** the Store has
/// accepted it.
///
/// Trust has to be decided on a package that is not installed yet: asking
/// after ingestion would mean the answer "no" still left bytes behind. This
/// reads the same manifests the Engine reads later, from the materialized
/// directory rather than the store.
///
/// It parses declarations. It never executes anything the package contains.
pub struct InspectedPackage {
    pub package_id: String,
    pub resources: Vec<crate::Resource>,
}

pub fn inspect_capabilities(package: &MaterializedPackage) -> Result<InspectedPackage> {
    let manifest = package.root().join("plugin.json");
    let bytes = fs::read(&manifest).map_err(|source| UzeError::Read {
        path: manifest.clone(),
        source,
    })?;
    let parsed: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|source| UzeError::Json {
            path: manifest.clone(),
            source,
        })?;
    let package_id = parsed
        .get("name")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| UzeError::MissingPackageName(manifest.clone()))?
        .to_owned();
    let id = crate::store::PackageId::from_plugin_name(&package_id, &manifest)?;
    Ok(InspectedPackage {
        package_id,
        resources: crate::engine::package_resources_at(&id, package.root())?,
    })
}

#[cfg(test)]
mod local_path_urls {
    use super::*;

    #[test]
    fn a_repository_on_this_machine_is_not_a_trust_boundary() {
        for url in ["/home/someone/ai", "/tmp/a:b/market"] {
            assert!(
                !PackageSource::git(url).crosses_trust_boundary(),
                "{url} was treated as remote"
            );
        }
    }

    #[test]
    fn a_remote_repository_is_one_however_it_is_spelled() {
        for url in [
            "https://github.com/acme/plugins",
            "ssh://git@github.com/acme/plugins",
            "git@github.com:acme/plugins.git",
            // Git routes this through its transport layer rather than
            // hardlinking, and it is what "pretend this is a remote"
            // is spelled as.
            "file:///tmp/origin.git",
        ] {
            assert!(
                PackageSource::git(url).crosses_trust_boundary(),
                "{url} was treated as local"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary(label: &str) -> PathBuf {
        uze_testkit::temp::scratch(label)
    }

    #[test]
    fn a_local_source_resolves_to_a_canonical_path() {
        let root = temporary("local");
        fs::create_dir_all(root.join("inner")).unwrap();
        let source = PackageSource::local(root.join("inner/../inner"));

        let materialized = acquire(&source).unwrap();

        assert_eq!(
            materialized.root(),
            root.join("inner").canonicalize().unwrap()
        );
        assert_eq!(&materialized.provenance().requested, &source);
        assert_eq!(
            materialized.provenance().resolved,
            ResolvedSource::Local {
                path: root.join("inner").canonicalize().unwrap()
            }
        );
        let _ = fs::remove_dir_all(root);
    }

    /// A caller's own directory must survive acquisition — only a directory
    /// UZE created may be cleaned up.
    #[test]
    fn acquiring_a_local_source_never_deletes_the_callers_directory() {
        let root = temporary("borrowed");
        fs::create_dir_all(&root).unwrap();
        {
            let materialized = acquire(&PackageSource::local(&root)).unwrap();
            assert!(materialized.root().is_dir());
        }
        assert!(
            root.is_dir(),
            "acquisition removed a caller-owned directory"
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_missing_or_non_directory_source_is_rejected() {
        let root = temporary("missing");
        // `temporary` creates the directory it hands back, so the absent
        // path has to be one below it — asking about the scratch root
        // itself only ever proves that it exists.
        assert!(matches!(
            acquire(&PackageSource::local(root.join("absent"))),
            Err(UzeError::MissingPath(_))
        ));
        let file = root.join("file");
        fs::write(&file, b"x").unwrap();
        assert!(matches!(
            acquire(&PackageSource::local(&file)),
            Err(UzeError::NotDirectory(_))
        ));
        let _ = fs::remove_dir_all(root);
    }

    /// Same request is the same origin even when the resolution differs —
    /// that is an update, not a conflicting package.
    #[test]
    fn origin_identity_compares_the_request_not_the_resolution() {
        let requested = PackageSource::local("/a");
        let first = Provenance {
            requested: requested.clone(),
            resolved: ResolvedSource::Local {
                path: PathBuf::from("/a"),
            },
        };
        let second = Provenance {
            requested,
            resolved: ResolvedSource::Local {
                path: PathBuf::from("/a/moved"),
            },
        };
        assert!(first.same_origin(&second));

        let other = Provenance {
            requested: PackageSource::local("/b"),
            resolved: ResolvedSource::Local {
                path: PathBuf::from("/b"),
            },
        };
        assert!(!first.same_origin(&other));
    }

    #[test]
    fn provenance_round_trips_through_json() {
        let provenance = Provenance {
            requested: PackageSource::local("/requested"),
            resolved: ResolvedSource::Local {
                path: PathBuf::from("/resolved"),
            },
        };
        let encoded = serde_json::to_string(&provenance).unwrap();
        assert_eq!(
            serde_json::from_str::<Provenance>(&encoded).unwrap(),
            provenance
        );
    }
}
