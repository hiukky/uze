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

/// An installed plugin identity, qualified by its marketplace.
///
/// A plugin name is only unique inside a marketplace. The qualified form is
/// deliberately the state/receipt identity so `git@one` and `git@two` can
/// coexist without sharing bytes or lifecycle records.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct PackageId(String);

impl PackageId {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn from_plugin_name(name: &str, manifest: &Path) -> Result<Self> {
        Self::from_marketplace_plugin("local", name, manifest)
    }

    pub fn from_marketplace_plugin(marketplace: &str, name: &str, manifest: &Path) -> Result<Self> {
        // The id is later used as a bare CLI argument to vendor tooling
        // (e.g. `codex plugin remove <id>@marketplace`, with no `--`
        // separator available before it). A leading `-` would let a
        // maliciously or carelessly named plugin be parsed as a flag by
        // that vendor CLI rather than as the id itself, so it is rejected
        // here at the one chokepoint every package id is constructed
        // through — not just re-checked at each call site.
        let valid = is_valid_name_component(name);
        let valid_marketplace = is_valid_name_component(marketplace);
        if !valid_marketplace {
            return Err(UzeError::InvalidPackageName {
                path: manifest.to_path_buf(),
                name: marketplace.to_owned(),
            });
        }
        if valid {
            Ok(Self(format!("{name}@{marketplace}")))
        } else {
            Err(UzeError::InvalidPackageName {
                path: manifest.to_path_buf(),
                name: name.to_owned(),
            })
        }
    }

    pub fn from_qualified(value: &str, manifest: &Path) -> Result<Self> {
        let (name, marketplace) =
            value
                .rsplit_once('@')
                .ok_or_else(|| UzeError::InvalidPackageName {
                    path: manifest.to_path_buf(),
                    name: value.to_owned(),
                })?;
        Self::from_marketplace_plugin(marketplace, name, manifest)
    }

    pub fn plugin_name(&self) -> &str {
        self.0.rsplit_once('@').map_or(&self.0, |(name, _)| name)
    }

    pub fn marketplace(&self) -> &str {
        self.0
            .rsplit_once('@')
            .map_or("local", |(_, marketplace)| marketplace)
    }
}

/// The one charset/shape rule every plugin name, marketplace name, and
/// local active-name alias is held to (ADR-038): a leading `-` would let a
/// carelessly named entry be parsed as a flag by a vendor CLI that takes it
/// as a bare positional argument, so it is rejected at every chokepoint
/// that turns operator/manifest text into one of these tokens.
fn is_valid_name_component(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredPackage {
    pub id: PackageId,
    pub root: PathBuf,
    pub manifest: PathBuf,
    /// Where this package came from. Carried for reporting and for a later
    /// reinstall; the Store itself never reads inside it.
    pub provenance: Provenance,
    /// The local token this plugin currently invokes under — `id.plugin_name()`
    /// unless an install-time alias resolved a collision with another
    /// marketplace's same-named plugin (ADR-038). This is what a harness's
    /// generated manifest/catalog and every Skill/Command label use; `id`
    /// remains the real, marketplace-qualified identity everywhere else
    /// (Store paths, receipts, removal, update).
    pub active_name: String,
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
    /// `None` means "no alias was ever chosen" — the local name defaults to
    /// `id.plugin_name()`. A registration written before this field existed
    /// deserializes as `None` here too (`#[serde(default)]`), which is
    /// exactly the correct meaning for it: every pre-existing install was
    /// implicitly active under its own bare plugin name, no migration
    /// needed. `Some(alias)` is only ever written by an explicit `alias`
    /// collision resolution at install time.
    #[serde(default)]
    active_name: Option<String>,
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
        self.ingest_from_marketplace(package, "local")
    }

    /// Ingests a materialized plugin under the marketplace that resolved it,
    /// active under its own bare plugin name. Fails with
    /// `PluginNameCollision` when that name is already active under a
    /// different marketplace-qualified identity — see
    /// `ingest_with_active_name` for the `alias`/`replace` resolutions.
    pub fn ingest_from_marketplace(
        &self,
        package: &MaterializedPackage,
        marketplace: &str,
    ) -> Result<StoredPackage> {
        self.ingest_with_active_name(package, marketplace, None)
    }

    /// Ingests a materialized plugin, optionally under an explicit local
    /// `active_name` alias rather than its own bare plugin name — the
    /// `alias` collision resolution (ADR-038). `None` behaves exactly like
    /// [`ingest_from_marketplace`]: the bare plugin name is both the
    /// collision check and the name recorded.
    pub fn ingest_with_active_name(
        &self,
        package: &MaterializedPackage,
        marketplace: &str,
        active_name: Option<&str>,
    ) -> Result<StoredPackage> {
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
        let id = PackageId::from_marketplace_plugin(marketplace, name, &manifest)?;
        let requested_active = active_name.unwrap_or(name);
        if let Some(alias) = active_name
            && !is_valid_name_component(alias)
        {
            return Err(UzeError::InvalidPackageName {
                path: manifest.clone(),
                name: alias.to_owned(),
            });
        }

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
        // A plugin name is only reserved once actively claimed: two
        // packages coexist fine in the Store (ADR-036, bytes never share
        // state), but only one of them may answer to a given invocation
        // name at a time — the other would silently shadow it in every
        // harness (verified against real Claude Code: whichever loads
        // first wins `/name:capability`, with zero indication the other
        // exists). This is the one place every install path passes
        // through, so the check cannot be bypassed by a different entry
        // point (ADR-038).
        if let Some(holder) = Self::active_name_holder(&registry, requested_active)
            && holder != &id
        {
            return Err(UzeError::PluginNameCollision {
                name: requested_active.to_owned(),
                existing: holder.as_str().to_owned(),
                requested: id.as_str().to_owned(),
            });
        }

        let destination = self.home.plugin_dir(&id);
        fs::create_dir_all(
            destination
                .parent()
                .expect("plugin directory has a marketplace parent"),
        )
        .map_err(|source_error| UzeError::Write {
            path: destination
                .parent()
                .expect("plugin directory has a marketplace parent")
                .to_path_buf(),
            source: source_error,
        })?;
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
                active_name: active_name
                    .filter(|alias| *alias != name)
                    .map(str::to_owned),
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
        let root = self.home.plugin_dir(id);
        let active_name = registration
            .active_name
            .clone()
            .unwrap_or_else(|| id.plugin_name().to_owned());
        Ok(StoredPackage {
            id: id.clone(),
            manifest: root.join("plugin.json"),
            root,
            provenance: registration.provenance.clone(),
            active_name,
        })
    }

    /// The local invocation name `id` currently answers to, without paying
    /// for a full [`package`] resolution (no root/manifest path building).
    /// Falls back to the bare plugin name for an id this Store does not
    /// recognize — permissive, since this is a naming convenience read, not
    /// an existence check; callers that need existence use [`package`].
    pub fn active_name_for(&self, id: &PackageId) -> String {
        self.load_registry()
            .ok()
            .and_then(|registry| {
                registry
                    .packages
                    .get(id)
                    .and_then(|r| r.active_name.clone())
            })
            .unwrap_or_else(|| id.plugin_name().to_owned())
    }

    /// The installed package currently active under local name `name`, if
    /// any — either because it is its own bare plugin name and holds no
    /// alias, or because an `alias` resolution explicitly claimed `name`
    /// for it. At most one package ever holds a given active name (enforced
    /// at ingest time), so this never needs to report ambiguity.
    pub fn find_by_active_name(&self, name: &str) -> Result<Option<PackageId>> {
        let registry = self.load_registry()?;
        Ok(Self::active_name_holder(&registry, name).cloned())
    }

    fn active_name_holder<'a>(registry: &'a PackageRegistry, name: &str) -> Option<&'a PackageId> {
        registry.packages.iter().find_map(|(id, registration)| {
            let active = registration
                .active_name
                .as_deref()
                .unwrap_or(id.plugin_name());
            (active == name).then_some(id)
        })
    }

    /// Re-points `id`'s local invocation name — the `alias` resolution
    /// applied after the fact (e.g. freeing up a bare name a since-removed
    /// package used to hold). `None` clears any alias, reverting to the
    /// bare plugin name. Does not check for a collision against the new
    /// name: a caller choosing to repoint an existing registration has
    /// already made that decision (ingest-time collision checking is what
    /// protects a *new* install from silently shadowing one already active).
    pub fn set_active_name(&self, id: &PackageId, name: Option<&str>) -> Result<()> {
        if let Some(alias) = name
            && !is_valid_name_component(alias)
        {
            return Err(UzeError::InvalidPackageName {
                path: self.home.plugin_dir(id).join("plugin.json"),
                name: alias.to_owned(),
            });
        }
        let mut registry = self.load_registry()?;
        let registration = registry
            .packages
            .get_mut(id)
            .ok_or_else(|| UzeError::UnknownPackage(id.as_str().to_owned()))?;
        registration.active_name = name
            .filter(|alias| *alias != id.plugin_name())
            .map(str::to_owned);
        self.save_registry(&registry)
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

    /// Removes registry entries whose backing directory is gone — a
    /// registration that survives whatever stopped writing its bytes
    /// (an interrupted install, manual cleanup, or an id-format change
    /// leaving an old entry's directory unreachable under the current
    /// `plugin_dir` formula — the exact fallout of this project's own
    /// marketplace-qualification, which computes `plugin_dir` from
    /// `id.marketplace()`/`id.plugin_name()` and left every
    /// pre-qualification id's directory unreachable under it).
    ///
    /// A registry entry is the Store's sole claim that a package is
    /// installed; once its directory is gone, that claim is simply false,
    /// not merely unhealthy — so this prunes rather than reports it, the
    /// same way a `Missing` receipt is either repaired or forgotten, never
    /// left to keep asserting something false. Returns the ids pruned, so
    /// a caller can also clean up anything that still references them
    /// (receipts, project locks).
    pub fn prune_ghost_registrations(&self) -> Result<Vec<PackageId>> {
        let mut registry = self.load_registry()?;
        let ghosts: Vec<PackageId> = registry
            .packages
            .keys()
            .filter(|id| !self.home.plugin_dir(id).is_dir())
            .cloned()
            .collect();
        if ghosts.is_empty() {
            return Ok(ghosts);
        }
        for id in &ghosts {
            registry.packages.remove(id);
        }
        self.save_registry(&registry)?;
        Ok(ghosts)
    }

    /// Removes only UZE-owned package bytes and its registry entry. Callers
    /// must complete attachment reconciliation first; the Store deliberately
    /// knows nothing about harness artifacts or their ownership.
    pub fn remove_package(&self, id: &PackageId) -> Result<()> {
        let mut registry = self.load_registry()?;
        if registry.packages.remove(id).is_none() {
            return Err(UzeError::UnknownPackage(id.as_str().to_owned()));
        }
        let root = self.home.plugin_dir(id);
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
        link.parent().unwrap_or_else(|| Path::new("")).to_path_buf()
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
    entries.sort_by_key(std::fs::DirEntry::file_name);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UzeHome;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("uze-store-{label}-{}-{nonce}", std::process::id()))
    }

    #[test]
    fn package_id_rejects_invalid_names() {
        let manifest = PathBuf::from("/tmp/plugin.json");
        assert!(PackageId::from_plugin_name("valid-name_123", &manifest).is_ok());
        assert!(PackageId::from_plugin_name("", &manifest).is_err());
        assert!(PackageId::from_plugin_name("has space", &manifest).is_err());
        assert!(PackageId::from_plugin_name("has/slash", &manifest).is_err());
        assert!(PackageId::from_plugin_name("has.dot", &manifest).is_err());
        assert!(PackageId::from_plugin_name("has:colon", &manifest).is_err());
    }

    #[test]
    fn package_id_rejects_a_leading_dash() {
        // Package ids are used as bare positional/selector arguments to
        // vendor CLIs (e.g. `codex plugin remove <id>@marketplace`); a
        // leading `-` would let the vendor CLI parse the id as a flag
        // instead, so it must be rejected before it ever becomes an id.
        let manifest = PathBuf::from("/tmp/plugin.json");
        assert!(PackageId::from_plugin_name("-force", &manifest).is_err());
        assert!(PackageId::from_plugin_name("--force", &manifest).is_err());
        // A dash elsewhere in the name remains fine.
        assert!(PackageId::from_plugin_name("my-plugin", &manifest).is_ok());
    }

    #[test]
    fn load_registry_returns_empty_when_missing_and_survives_corrupt_json() {
        let root = temp("registry-missing");
        let home = UzeHome::at(&root);
        let store = UzeStore::new(home.clone());
        // No registry yet — should be empty, not error.
        assert_eq!(store.package_ids().unwrap().len(), 0);
        // Corrupt JSON should surface as error, not panic.
        home.ensure_layout().unwrap();
        fs::write(home.registry_path(), "bad json").unwrap();
        assert!(store.package_ids().is_err());
        let _ = fs::remove_dir_all(root);
    }
}
