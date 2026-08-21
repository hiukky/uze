//! Install-once package Store.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    acquisition::{MaterializedPackage, Provenance},
    error::{Result, UzeError},
    home::UzeHome,
    importer::{AgentPluginImporter, ForeignImporter},
};

/// An opaque package identity sourced from the external Agent Plugin name.
/// It is store state, not a UZE package manifest or a replacement standard.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct PackageId(String);

impl PackageId {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn from_plugin_name(name: &str, manifest: &Path) -> Result<Self> {
        let valid = !name.is_empty()
            && name.chars().all(|character| {
                character.is_ascii_alphanumeric() || character == '-' || character == '_'
            });
        if valid {
            Ok(Self(name.to_owned()))
        } else {
            Err(UzeError::InvalidPackageName {
                path: manifest.to_path_buf(),
                name: name.to_owned(),
            })
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredPackage {
    pub id: PackageId,
    pub root: PathBuf,
    pub manifest: PathBuf,
    /// Where this package came from. Carried for reporting and for a later
    /// reinstall; the Store itself never reads inside it.
    pub provenance: Provenance,
}

#[derive(Clone, Debug)]
pub struct UzeStore {
    home: UzeHome,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PackageRegistry {
    packages: BTreeMap<PackageId, Registration>,
}

/// One registry entry.
///
/// `source` is the historical field name, kept so a ledger written before
/// provenance existed still loads: a bare JSON string deserializes as a local
/// source (see `Provenance`'s deserializer). Reading a legacy entry never
/// rewrites it, and every new write emits the current shape.
#[derive(Clone, Debug, Deserialize, Serialize)]
struct Registration {
    #[serde(rename = "source")]
    provenance: Provenance,
}

impl UzeStore {
    pub fn new(home: UzeHome) -> Self {
        Self { home }
    }

    pub fn home(&self) -> &UzeHome {
        &self.home
    }

    /// Ingests an already-materialized Agent Plugins 1.0 package once.
    ///
    /// The Store preserves the complete external package tree (including any
    /// vendor-native envelope) and never creates a UZE plugin manifest or
    /// rewrites payloads. It writes nothing a harness reads: a harness-owned
    /// view of the installed set belongs to that harness's integration.
    ///
    /// It also knows nothing about where the bytes came from. Provenance
    /// arrives attached to the materialized package, is persisted verbatim,
    /// and is compared only through `Provenance::same_origin` — this module
    /// never reads a field of it or matches a source mechanism.
    pub fn ingest(&self, package: &MaterializedPackage) -> Result<StoredPackage> {
        let source = package.root();
        let manifest = source.join("plugin.json");
        let imported = AgentPluginImporter
            .import(source)?
            .ok_or_else(|| UzeError::MissingManifest(source.to_path_buf()))?;
        let manifest_value: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest).map_err(|source_error| {
                UzeError::Read {
                    path: manifest.clone(),
                    source: source_error,
                }
            })?)
            .map_err(|source_error| UzeError::Json {
                path: manifest.clone(),
                source: source_error,
            })?;
        let name = manifest_value
            .get("name")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| UzeError::MissingPackageName(manifest.clone()))?;
        let id = PackageId::from_plugin_name(name, &manifest)?;

        // Every source passes through this one check, so a local package and
        // a remote one are held to the same rule. It runs before any byte is
        // written, so a rejected package leaves nothing behind.
        assert_self_contained(source)?;

        self.home.ensure_layout()?;
        let mut registry = self.load_registry()?;
        if let Some(existing) = registry.packages.get(&id) {
            if existing.provenance.same_origin(package.provenance()) {
                return self.package(&id);
            }
            return Err(UzeError::PackageConflict {
                id: id.as_str().to_owned(),
                existing: existing.provenance.requested.display(),
                requested: package.provenance().requested.display(),
            });
        }

        let destination = self.home.package_dir(&id);
        fs::create_dir(&destination).map_err(|source_error| UzeError::Write {
            path: destination.clone(),
            source: source_error,
        })?;
        copy_tree(source, &destination)?;

        // The importer has already performed external-manifest safety checks.
        // Keeping this value live makes that boundary explicit and prevents an
        // accidental installation of an empty, non-Agent-Plugin directory.
        let _ = imported;
        registry.packages.insert(
            id.clone(),
            Registration {
                provenance: package.provenance().clone(),
            },
        );
        self.save_registry(&registry)?;
        self.package(&id)
    }

    pub fn package(&self, id: &PackageId) -> Result<StoredPackage> {
        let registry = self.load_registry()?;
        let registration = registry
            .packages
            .get(id)
            .ok_or_else(|| UzeError::UnknownPackage(id.as_str().to_owned()))?;
        let root = self.home.package_dir(id);
        Ok(StoredPackage {
            id: id.clone(),
            manifest: root.join("plugin.json"),
            root,
            provenance: registration.provenance.clone(),
        })
    }

    pub fn registration_count(&self) -> Result<usize> {
        Ok(self.load_registry()?.packages.len())
    }

    /// Lists installed package identities in deterministic order. Package
    /// selection and dependency resolution are intentionally out of scope for
    /// this PoC; the composed local environment currently includes every
    /// locally installed package.
    pub fn package_ids(&self) -> Result<Vec<PackageId>> {
        Ok(self.load_registry()?.packages.into_keys().collect())
    }

    /// Removes only UZE-owned package bytes and its registry entry. Callers
    /// must complete attachment reconciliation first; the Store deliberately
    /// knows nothing about harness artifacts or their ownership.
    pub fn remove_package(&self, id: &PackageId) -> Result<()> {
        let mut registry = self.load_registry()?;
        if registry.packages.remove(id).is_none() {
            return Err(UzeError::UnknownPackage(id.as_str().to_owned()));
        }
        let root = self.home.package_dir(id);
        if let Err(source) = fs::remove_dir_all(&root)
            && source.kind() != std::io::ErrorKind::NotFound
        {
            return Err(UzeError::Write { path: root, source });
        }
        self.save_registry(&registry)
    }

    fn load_registry(&self) -> Result<PackageRegistry> {
        let path = self.home.registry_path();
        if !path.exists() {
            return Ok(PackageRegistry {
                packages: BTreeMap::new(),
            });
        }
        let bytes = fs::read(&path).map_err(|source| UzeError::Read {
            path: path.clone(),
            source,
        })?;
        serde_json::from_slice(&bytes).map_err(|source| UzeError::Json { path, source })
    }

    fn save_registry(&self, registry: &PackageRegistry) -> Result<()> {
        let path = self.home.registry_path();
        let payload =
            serde_json::to_vec_pretty(registry).expect("registry serialization is infallible");
        crate::persistence::write_atomic(&path, &payload)
    }
}

/// Enforces the invariant that an installed package is **self-contained**:
/// no symlink the Store persists may resolve outside the package root.
///
/// This is not a rule about where a package came from. A local directory and
/// a cloned repository are held to it identically, because it protects what
/// happens *after* installation: an integration later points a harness at a
/// path inside the store, and the harness follows whatever it finds there.
///
/// Every symlink is checked on its own and none is followed. That is
/// deliberate and it is also what makes chains and cycles harmless: a chain
/// can only leave the root if some individual link leaves it, and that link
/// is checked like any other. Nothing here traverses a link, so there is no
/// cycle to guard against.
fn assert_self_contained(root: &Path) -> Result<()> {
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory).map_err(|source| UzeError::Read {
            path: directory.clone(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| UzeError::Read {
                path: directory.clone(),
                source,
            })?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|source| UzeError::Read {
                path: path.clone(),
                source,
            })?;
            if metadata.file_type().is_symlink() {
                let target = fs::read_link(&path).map_err(|source| UzeError::Read {
                    path: path.clone(),
                    source,
                })?;
                let resolved = resolve_lexically(&path, &target);
                if !resolved.starts_with(root) {
                    return Err(UzeError::PackageEscapesRoot {
                        link: path,
                        target: resolved,
                    });
                }
            } else if metadata.is_dir() {
                // A real directory only. Symlinked directories were rejected
                // or accepted above and are never descended into, so this
                // walk cannot be led outside the root either.
                pending.push(path);
            }
        }
    }
    Ok(())
}

/// Resolves a symlink target against its own location **without touching the
/// filesystem**, so `..` is normalized textually rather than by following
/// whatever it currently points at. An absolute target resolves to itself and
/// therefore fails the containment check unless it is already inside.
fn resolve_lexically(link: &Path, target: &Path) -> PathBuf {
    let base = if target.is_absolute() {
        PathBuf::new()
    } else {
        link.parent().unwrap_or(Path::new("")).to_path_buf()
    };
    let mut resolved = base;
    for component in target.components() {
        match component {
            std::path::Component::ParentDir => {
                resolved.pop();
            }
            std::path::Component::CurDir => {}
            other => resolved.push(other.as_os_str()),
        }
    }
    resolved
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination).map_err(|source_error| UzeError::Write {
        path: destination.to_path_buf(),
        source: source_error,
    })?;
    let mut entries = fs::read_dir(source)
        .map_err(|source_error| UzeError::Read {
            path: source.to_path_buf(),
            source: source_error,
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|source_error| UzeError::Read {
            path: source.to_path_buf(),
            source: source_error,
        })?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata =
            fs::symlink_metadata(&source_path).map_err(|source_error| UzeError::Read {
                path: source_path.clone(),
                source: source_error,
            })?;
        if metadata.file_type().is_symlink() {
            copy_symlink(&source_path, &destination_path)?;
        } else if metadata.is_dir() {
            copy_tree(&source_path, &destination_path)?;
        } else if metadata.is_file() {
            copy_file(&source_path, &destination_path)?;
        } else {
            return Err(UzeError::ExposureUnavailable(format!(
                "plugin store cannot preserve special filesystem entry `{}`",
                source_path.display()
            )));
        }
    }
    Ok(())
}

fn copy_file(source: &Path, destination: &Path) -> Result<()> {
    fs::copy(source, destination).map_err(|source_error| UzeError::Write {
        path: destination.to_path_buf(),
        source: source_error,
    })?;
    let permissions = fs::metadata(source)
        .map_err(|source_error| UzeError::Read {
            path: source.to_path_buf(),
            source: source_error,
        })?
        .permissions();
    fs::set_permissions(destination, permissions).map_err(|source_error| UzeError::Write {
        path: destination.to_path_buf(),
        source: source_error,
    })?;
    Ok(())
}

#[cfg(unix)]
fn copy_symlink(source: &Path, destination: &Path) -> Result<()> {
    let target = fs::read_link(source).map_err(|source_error| UzeError::Read {
        path: source.to_path_buf(),
        source: source_error,
    })?;
    std::os::unix::fs::symlink(target, destination).map_err(|source_error| UzeError::Write {
        path: destination.to_path_buf(),
        source: source_error,
    })
}

#[cfg(not(unix))]
fn copy_symlink(source: &Path, _destination: &Path) -> Result<()> {
    Err(UzeError::ExposureUnavailable(format!(
        "plugin contains symlink `{}` which this platform cannot preserve",
        source.display()
    )))
}
