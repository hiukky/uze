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
    ///
    /// This is a deliberate, narrow scope. It also leaves an honest gap: an
    /// operator can clone a repository by hand and install the result as a
    /// local path, bypassing the question entirely. Closing that would mean
    /// prompting on every local install with an MCP server, which changes an
    /// existing workflow — a product decision, not one to make in passing.
    pub fn crosses_trust_boundary(&self) -> bool {
        match self {
            Self::Local { .. } => false,
            Self::Git { .. } => true,
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
}

impl ResolvedSource {
    pub fn display(&self) -> String {
        match self {
            Self::Local { path } => path.display().to_string(),
            Self::Git { url, commit, .. } => format!("{url}@{commit}"),
        }
    }
}

/// Everything the Store persists about a package's origin, and nothing it
/// interprets.
///
/// The Store holds this, writes it and compares it through
/// [`Provenance::same_origin`] — it never reads a field or matches a variant.
/// That is what keeps source mechanisms out of `store.rs`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Provenance {
    pub requested: PackageSource,
    pub resolved: ResolvedSource,
}

/// Accepts both the current shape and the one a registry written before
/// provenance existed used: a bare path string.
///
/// That legacy value maps without losing anything — a path string *is* a
/// local source, requested and resolved alike. Compatibility is read-only:
/// nothing here rewrites the registry, and every new write goes through the
/// derived `Serialize`, which only ever emits the current shape. There is
/// deliberately no schema version: this is one fallback for one superseded
/// representation, not a migration framework.
impl<'de> Deserialize<'de> for Provenance {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Current {
                requested: PackageSource,
                resolved: ResolvedSource,
            },
            LegacyLocalPath(PathBuf),
        }
        Ok(match Wire::deserialize(deserializer)? {
            Wire::Current {
                requested,
                resolved,
            } => Self {
                requested,
                resolved,
            },
            Wire::LegacyLocalPath(path) => Self {
                requested: PackageSource::Local { path: path.clone() },
                resolved: ResolvedSource::Local { path },
            },
        })
    }
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
    fn retarget(&mut self, root: PathBuf, provenance: Provenance) {
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
pub fn acquire(source: &PackageSource) -> Result<MaterializedPackage> {
    match source {
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
                None => checkout.clone(),
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
fn scratch_directory() -> Result<PathBuf> {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock is after the epoch")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("uze-acquire-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&path).map_err(|source| UzeError::Write {
        path: path.clone(),
        source,
    })?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary(label: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "uze-acquisition-{label}-{}-{nonce}",
            std::process::id()
        ))
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
        assert!(matches!(
            acquire(&PackageSource::local(&root)),
            Err(UzeError::MissingPath(_))
        ));
        fs::create_dir_all(&root).unwrap();
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

    /// A registry written before provenance existed must still load, because
    /// an unreadable registry blocks removal — safe, but it strands the user
    /// with external state UZE can no longer identify.
    #[test]
    fn a_legacy_path_string_still_deserializes_as_a_local_source() {
        let provenance: Provenance = serde_json::from_str("\"/legacy/plugin\"").unwrap();
        assert_eq!(
            provenance.requested,
            PackageSource::Local {
                path: PathBuf::from("/legacy/plugin")
            }
        );
        assert_eq!(
            provenance.resolved,
            ResolvedSource::Local {
                path: PathBuf::from("/legacy/plugin")
            }
        );
    }

    /// Reading a legacy value never rewrites it, but a new write emits only
    /// the current shape.
    #[test]
    fn a_new_write_uses_only_the_current_shape() {
        let provenance: Provenance = serde_json::from_str("\"/legacy/plugin\"").unwrap();
        let encoded = serde_json::to_string(&provenance).unwrap();
        assert!(encoded.starts_with('{'), "legacy shape was written back");
        assert!(encoded.contains("requested"));
        assert!(encoded.contains("resolved"));
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
