//! Project desired agent environment lock — `agents.lock`.
//!
//! Vendor-neutral, reproducible, Git-versionable. Store/Engine/Integration
//! never parse this file; only Core's serializer and Application's
//! project-environment use cases do.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use noyalib::compat::serde_yaml;
use serde::{Deserialize, Serialize};

use crate::{
    Result, UzeError,
    acquisition::{PackageSource, ResolvedSource},
};

pub const SUPPORTED_LOCK_VERSION: u32 = 1;
pub const LOCK_FILE_NAME: &str = "agents.lock";

/// Top-level lock file.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ProjectLock {
    pub version: u32,
    /// Directory containing this project's linked Git worktrees. Kept
    /// relative to the project root so the lock remains portable across
    /// clones and machines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktrees_dir: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub marketplaces: BTreeMap<String, LockedMarketplace>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub plugins: BTreeMap<String, LockedPlugin>,
}

impl Default for ProjectLock {
    fn default() -> Self {
        Self {
            version: SUPPORTED_LOCK_VERSION,
            worktrees_dir: None,
            marketplaces: BTreeMap::new(),
            plugins: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LockedMarketplace {
    pub source: MarketplaceSource,
    #[serde(default, skip_serializing_if = "ResolvedMarketplace::is_empty")]
    pub resolved: ResolvedMarketplace,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MarketplaceSource {
    Git {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reference: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subdirectory: Option<PathBuf>,
    },
    Path {
        path: PathBuf,
    },
    Embedded {
        id: String,
    },
}

impl MarketplaceSource {
    pub fn display(&self) -> String {
        match self {
            Self::Git {
                url,
                reference,
                subdirectory,
            } => {
                let mut s = url.clone();
                if let Some(r) = reference {
                    s.push('@');
                    s.push_str(r);
                }
                if let Some(sub) = subdirectory {
                    s.push('#');
                    s.push_str(&sub.display().to_string());
                }
                s
            }
            Self::Path { path } => path.display().to_string(),
            Self::Embedded { id } => format!("embedded:{id}"),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct ResolvedMarketplace {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
}

impl ResolvedMarketplace {
    pub fn is_empty(&self) -> bool {
        self.revision.is_none()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct LockedPlugin {
    pub source: PluginSource,
    pub resolved: ResolvedPlugin,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginSource {
    Marketplace {
        marketplace: String,
        plugin: String,
    },
    Git {
        url: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reference: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        subdirectory: Option<PathBuf>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ResolvedPlugin {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrity: Option<String>,
}

pub fn lock_path_for(root: &Path) -> PathBuf {
    root.join(LOCK_FILE_NAME)
}

pub fn load_lock(root: &Path) -> Result<Option<ProjectLock>> {
    let path = lock_path_for(root);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&path).map_err(|source| UzeError::Read {
        path: path.clone(),
        source,
    })?;
    let text = String::from_utf8(bytes).map_err(|_| UzeError::MalformedLock {
        path: path.clone(),
        reason: "agents.lock is not valid UTF-8".to_owned(),
    })?;
    parse_lock_str(&text, &path).map(Some)
}

fn parse_lock_str(text: &str, path: &Path) -> Result<ProjectLock> {
    let lock: ProjectLock = serde_yaml::from_str(text).map_err(|e| UzeError::MalformedLock {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;
    if lock.version != SUPPORTED_LOCK_VERSION {
        return Err(UzeError::UnsupportedLockVersion {
            found: lock.version,
            expected: SUPPORTED_LOCK_VERSION,
        });
    }
    if let Some(directory) = &lock.worktrees_dir
        && (directory.as_os_str().is_empty() || directory.is_absolute())
    {
        return Err(UzeError::MalformedLock {
            path: path.to_path_buf(),
            reason: "worktrees_dir must be a non-empty path relative to the project root"
                .to_owned(),
        });
    }
    Ok(lock)
}

pub fn parse_lock_bytes(bytes: &[u8], path: &Path) -> Result<ProjectLock> {
    let text = std::str::from_utf8(bytes).map_err(|_| UzeError::MalformedLock {
        path: path.to_path_buf(),
        reason: "agents.lock is not valid UTF-8".to_owned(),
    })?;
    parse_lock_str(text, path)
}

pub fn save_lock(root: &Path, lock: &ProjectLock) -> Result<()> {
    if lock.version != SUPPORTED_LOCK_VERSION {
        return Err(UzeError::UnsupportedLockVersion {
            found: lock.version,
            expected: SUPPORTED_LOCK_VERSION,
        });
    }
    let path = lock_path_for(root);
    // Deterministic YAML: BTreeMap ensures sorted keys, serde_yaml preserves order.
    let yaml = serde_yaml::to_string(lock).map_err(|e| UzeError::MalformedLock {
        path: path.clone(),
        reason: e.to_string(),
    })?;
    crate::persistence::write_atomic(&path, yaml.as_bytes())
}

/// Parses `plugin@marketplace` shorthand. Marketplace is required.
pub fn parse_plugin_marketplace_spec(spec: &str) -> Result<(String, String)> {
    let (plugin, marketplace) = spec.split_once('@').ok_or_else(|| {
        UzeError::InvalidPluginSpec(format!("`{spec}` must be `name@marketplace`"))
    })?;
    if plugin.is_empty() || marketplace.is_empty() {
        return Err(UzeError::InvalidPluginSpec(format!(
            "`{spec}` must be `name@marketplace` with non-empty parts"
        )));
    }
    // Validate charset similar to PackageId but allow same set.
    for c in plugin.chars() {
        if !(c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err(UzeError::InvalidPackageName {
                path: PathBuf::from("agents.lock"),
                name: plugin.to_owned(),
            });
        }
    }
    for c in marketplace.chars() {
        if !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.') {
            return Err(UzeError::InvalidPluginSpec(format!(
                "invalid marketplace name `{marketplace}`"
            )));
        }
    }
    Ok((plugin.to_owned(), marketplace.to_owned()))
}

impl From<PackageSource> for MarketplaceSource {
    fn from(value: PackageSource) -> Self {
        match value {
            PackageSource::Git {
                url,
                reference,
                subdirectory,
            } => Self::Git {
                url,
                reference,
                subdirectory,
            },
            PackageSource::Local { path } => Self::Path { path },
            PackageSource::Embedded { id } => Self::Embedded { id },
        }
    }
}

impl From<MarketplaceSource> for PackageSource {
    fn from(value: MarketplaceSource) -> Self {
        match value {
            MarketplaceSource::Git {
                url,
                reference,
                subdirectory,
            } => Self::Git {
                url,
                reference,
                subdirectory,
            },
            MarketplaceSource::Path { path } => Self::Local { path },
            MarketplaceSource::Embedded { id } => Self::Embedded { id },
        }
    }
}

impl From<ResolvedSource> for ResolvedMarketplace {
    fn from(value: ResolvedSource) -> Self {
        Self {
            revision: value.lock_revision(),
        }
    }
}

impl ResolvedPlugin {
    /// Builds a lock entry's resolved facts from what acquisition actually
    /// observed. `version` stays `None` here: nothing in this crate parses
    /// a plugin manifest's `version` field yet (unlike `revision`, which
    /// `ResolvedSource` already carries) — a real gap, not silently
    /// papered over with a fabricated value.
    pub fn from_resolved_source(resolved: &ResolvedSource) -> Self {
        Self {
            revision: resolved.lock_revision(),
            version: None,
            integrity: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn parse_plugin_marketplace_requires_at() {
        assert!(parse_plugin_marketplace_spec("flow").is_err());
        assert!(parse_plugin_marketplace_spec("flow@").is_err());
        assert!(parse_plugin_marketplace_spec("@ai").is_err());
        let (p, m) = parse_plugin_marketplace_spec("flow@ai").unwrap();
        assert_eq!(p, "flow");
        assert_eq!(m, "ai");
    }

    #[test]
    fn lock_round_trips_deterministically() {
        let mut lock = ProjectLock::default();
        lock.marketplaces.insert(
            "ai".to_owned(),
            LockedMarketplace {
                source: MarketplaceSource::Git {
                    url: "https://github.com/hiukky/ai.git".to_owned(),
                    reference: None,
                    subdirectory: None,
                },
                resolved: ResolvedMarketplace {
                    revision: Some("abc123".to_owned()),
                },
            },
        );
        lock.plugins.insert(
            "flow".to_owned(),
            LockedPlugin {
                source: PluginSource::Marketplace {
                    marketplace: "ai".to_owned(),
                    plugin: "flow".to_owned(),
                },
                resolved: ResolvedPlugin {
                    revision: Some("abc123".to_owned()),
                    version: Some("0.3.1".to_owned()),
                    integrity: None,
                },
            },
        );
        let yaml = serde_yaml::to_string(&lock).unwrap();
        let parsed: ProjectLock = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed, lock);
        // Second serialization must be byte-identical (deterministic via BTreeMap)
        let yaml2 = serde_yaml::to_string(&parsed).unwrap();
        assert_eq!(yaml, yaml2);
    }

    #[test]
    fn unsupported_version_is_rejected() {
        let yaml = "version: 99\n";
        let err = parse_lock_str(yaml, &PathBuf::from("agents.lock")).unwrap_err();
        assert!(matches!(
            err,
            UzeError::UnsupportedLockVersion { found: 99, .. }
        ));
    }

    #[test]
    fn malformed_yaml_is_rejected() {
        let yaml = "version: 1\nmarketplaces: [";
        let err = parse_lock_str(yaml, &PathBuf::from("agents.lock")).unwrap_err();
        assert!(matches!(err, UzeError::MalformedLock { .. }));
    }

    #[test]
    fn worktrees_dir_is_portable_and_must_be_relative() {
        let path = PathBuf::from("agents.lock");
        let lock = parse_lock_str("version: 1\nworktrees_dir: ../.worktrees\n", &path).unwrap();
        assert_eq!(lock.worktrees_dir, Some(PathBuf::from("../.worktrees")));
        assert!(parse_lock_str("version: 1\nworktrees_dir: /tmp/worktrees\n", &path).is_err());
    }
}
