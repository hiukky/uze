use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
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

    pub(crate) fn from_plugin_name(name: &str, manifest: &Path) -> Result<Self> {
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
    pub source: PathBuf,
}

#[derive(Clone, Debug)]
pub struct UzeStore {
    home: UzeHome,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct PackageRegistry {
    packages: BTreeMap<PackageId, Registration>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Registration {
    source: PathBuf,
}

impl UzeStore {
    pub fn new(home: UzeHome) -> Self {
        Self { home }
    }

    pub fn home(&self) -> &UzeHome {
        &self.home
    }

    /// Installs an Agent Plugins 1.0 package once. The store preserves the
    /// complete external package tree (including any vendor-native envelope)
    /// and never creates a UZE plugin manifest or rewrites payloads.
    pub fn install_agent_plugin(&self, source: impl AsRef<Path>) -> Result<StoredPackage> {
        let source = checked_root(source.as_ref())?;
        let manifest = source.join("plugin.json");
        let imported = AgentPluginImporter
            .import(&source)?
            .ok_or_else(|| UzeError::MissingManifest(source.clone()))?;
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

        self.home.ensure_layout()?;
        let mut registry = self.load_registry()?;
        if let Some(existing) = registry.packages.get(&id) {
            if existing.source == source {
                self.refresh_codex_marketplace(&registry)?;
                return self.package(&id);
            }
            return Err(UzeError::PackageConflict {
                id: id.as_str().to_owned(),
                existing: existing.source.clone(),
                requested: source,
            });
        }

        let destination = self.home.package_dir(&id);
        fs::create_dir(&destination).map_err(|source_error| UzeError::Write {
            path: destination.clone(),
            source: source_error,
        })?;
        copy_tree(&source, &destination)?;

        // The importer has already performed external-manifest safety checks.
        // Keeping this value live makes that boundary explicit and prevents an
        // accidental installation of an empty, non-Agent-Plugin directory.
        let _ = imported;
        registry.packages.insert(
            id.clone(),
            Registration {
                source: source.clone(),
            },
        );
        self.save_registry(&registry)?;
        self.refresh_codex_marketplace(&registry)?;
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
            source: registration.source.clone(),
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
        if root.exists() {
            fs::remove_dir_all(&root).map_err(|source| UzeError::Write { path: root, source })?;
        }
        self.save_registry(&registry)?;
        self.refresh_codex_marketplace(&registry)
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

    /// Materializes only Codex's documented marketplace catalog, pointing at
    /// already-preserved package directories. It carries no UZE semantics and
    /// is regenerated from the installed-package registry.
    fn refresh_codex_marketplace(&self, registry: &PackageRegistry) -> Result<()> {
        let path = self.home.codex_marketplace_path();
        let parent = path.parent().expect("marketplace path has a parent");
        fs::create_dir_all(parent).map_err(|source| UzeError::Write {
            path: parent.to_path_buf(),
            source,
        })?;
        let plugins: Vec<serde_json::Value> = registry
            .packages
            .keys()
            .filter(|id| {
                self.home
                    .package_dir(id)
                    .join(".codex-plugin/plugin.json")
                    .is_file()
            })
            .map(|id| {
                serde_json::json!({
                    "name": id.as_str(),
                    "source": { "source": "local", "path": format!("./packages/{}", id.as_str()) },
                    "policy": { "installation": "AVAILABLE", "authentication": "ON_INSTALL" },
                    "category": "Developer tools"
                })
            })
            .collect();
        let catalog = serde_json::json!({
            "name": "uze-local",
            "interface": { "displayName": "UZE Local" },
            "plugins": plugins,
        });
        crate::persistence::write_atomic(
            &path,
            &serde_json::to_vec_pretty(&catalog).expect("catalog is serializable"),
        )
    }
}

fn checked_root(root: &Path) -> Result<PathBuf> {
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
