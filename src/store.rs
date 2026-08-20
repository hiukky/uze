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

    fn from_plugin_name(name: &str, manifest: &Path) -> Result<Self> {
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

    /// Installs an Agent Plugins 1.0 package once. The store copies only the
    /// external `plugin.json` and `skills/` tree; it never creates a UZE
    /// manifest or rewrites SKILL.md payloads.
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
        copy_file(&manifest, &destination.join("plugin.json"))?;
        let source_skills = source.join("skills");
        if source_skills.is_dir() {
            copy_tree(&source_skills, &destination.join("skills"))?;
        }

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
        fs::write(&path, payload).map_err(|source| UzeError::Write { path, source })
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
        if source_path.is_dir() {
            copy_tree(&source_path, &destination_path)?;
        } else if source_path.is_file() {
            copy_file(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

fn copy_file(source: &Path, destination: &Path) -> Result<()> {
    fs::copy(source, destination).map_err(|source_error| UzeError::Write {
        path: destination.to_path_buf(),
        source: source_error,
    })?;
    Ok(())
}
