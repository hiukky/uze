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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub marketplaces: BTreeMap<String, LockedMarketplace>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub plugins: BTreeMap<String, LockedPlugin>,
}

impl Default for ProjectLock {
    fn default() -> Self {
        Self {
            version: SUPPORTED_LOCK_VERSION,
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
    /// What resolution found. Flattened rather than nested under
    /// `resolved:` — in a file that holds nothing but resolution, that key
    /// names the obvious.
    #[serde(flatten)]
    pub resolved: ResolvedPlugin,
    /// What the manifest asked for, echoed so staleness is decidable by
    /// comparing the two files: no network, no re-resolution. `uv.lock`'s
    /// `[package.metadata] requires-dist` serves the same purpose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested: Option<RequestedPlugin>,
}

/// The declaration an entry was resolved from. Only what can change the
/// resolution lives here — a comment or a key's position in the manifest
/// cannot, so neither belongs.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct RequestedPlugin {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marketplace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<String>,
}

impl RequestedPlugin {
    /// The manifest's declaration, reduced to what resolution depends on.
    /// A declared plugin is a name under a marketplace; `git` and `ref`
    /// stay for what the lock itself can carry, which is more.
    pub fn from_marketplace(marketplace: &str) -> Self {
        Self {
            marketplace: Some(marketplace.to_owned()),
            git: None,
            r#ref: None,
        }
    }
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

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct ResolvedPlugin {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrity: Option<String>,
}

/// One entry's disagreement between what the manifest asks for and what the
/// lock recorded — the whole of staleness, decided by reading two files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaleEntry {
    pub plugin: String,
    pub requested: RequestedPlugin,
    pub locked: Option<RequestedPlugin>,
}

/// Which of the manifest's declarations the lock no longer answers for.
///
/// Deliberately offline and deliberately cheap: it compares `requested`
/// against the manifest and nothing else. Re-resolving to find out whether a
/// lock is current would need the network for a question the two files
/// already answer, and would make `status` fail in a tunnel.
///
/// A plugin the lock has never seen is stale. A plugin the lock carries with
/// no `requested` at all is *not* reported: it predates the echo, and calling
/// it stale would tell every project it is out of date for a reason nobody
/// can act on.
pub fn stale_against(
    manifest: &crate::manifest::ProjectManifest,
    lock: &ProjectLock,
) -> Vec<StaleEntry> {
    let mut stale = Vec::new();
    for (plugin, marketplace) in manifest.declared_plugins() {
        let requested = RequestedPlugin::from_marketplace(marketplace);
        match lock.plugins.get(plugin) {
            None => stale.push(StaleEntry {
                plugin: plugin.to_owned(),
                requested,
                locked: None,
            }),
            Some(locked) => {
                if let Some(recorded) = &locked.requested
                    && recorded != &requested
                {
                    stale.push(StaleEntry {
                        plugin: plugin.to_owned(),
                        requested,
                        locked: Some(recorded.clone()),
                    });
                }
            }
        }
    }
    stale
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

/// Keys a lock once carried and no longer may. `worktrees_dir` was the
/// bare-directory spelling of the isolation policy; `worktrees` was the
/// policy itself, which now lives in `agents.yaml` because every field of
/// it is a decision rather than a resolution. Both are rejected loudly:
/// `ProjectLock` does not deny unknown fields, so dropping them silently
/// would turn a declared policy into no policy at all with nothing said.
const REPLACED_KEYS: [(&str, &str); 2] = [
    (
        "worktrees_dir",
        "the checkout layout is fixed infrastructure rather than something a project declares",
    ),
    (
        "worktrees",
        "the isolation policy is a declaration, so it belongs in agents.yaml; agents.lock records \
         only what resolution produced",
    ),
];

fn parse_lock_str(text: &str, path: &Path) -> Result<ProjectLock> {
    let raw: serde_yaml::Value =
        serde_yaml::from_str(text).map_err(|e| UzeError::MalformedLock {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;
    for (key, why) in REPLACED_KEYS {
        if raw
            .as_mapping()
            .is_some_and(|mapping| mapping.contains_key(key))
        {
            return Err(UzeError::MalformedLock {
                path: path.to_path_buf(),
                reason: format!(
                    "`{key}` no longer belongs in agents.lock: {why}. Move it to agents.yaml and \
                     let UZE regenerate the lock."
                ),
            });
        }
    }

    let lock: ProjectLock = serde_yaml::from_value(raw).map_err(|e| UzeError::MalformedLock {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;
    if lock.version != SUPPORTED_LOCK_VERSION {
        return Err(UzeError::UnsupportedLockVersion {
            found: lock.version,
            expected: SUPPORTED_LOCK_VERSION,
        });
    }
    Ok(lock)
}

/// Deletes the lock. A lock with nothing left to reproduce is not an empty
/// lock, it is no lock — leaving the file behind declaring nothing invites
/// the belief that resolution happened.
pub fn remove_lock(root: &Path) -> Result<()> {
    let path = lock_path_for(root);
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(UzeError::Write { path, source }),
    }
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

    /// Records the digest of the bytes actually ingested. A source with no
    /// stable bytes — a local path someone is editing — records none rather
    /// than a value that would be wrong by the next command.
    pub fn with_integrity_of(mut self, root: &Path, reproducible: bool) -> Self {
        if reproducible {
            self.integrity = crate::digest::tree_sha256(root).ok();
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    mod staleness {
        use super::*;
        use crate::manifest::{DeclaredMarketplace, ProjectManifest};

        /// The declarations, grouped the way the manifest groups them: the
        /// marketplace carries the plugins, so what a declaration can differ
        /// in is which marketplace it is under.
        fn manifest(entries: &[(&str, &str)]) -> ProjectManifest {
            let mut manifest = ProjectManifest::default();
            for (name, marketplace) in entries {
                manifest
                    .marketplaces
                    .entry((*marketplace).to_owned())
                    .or_insert_with(|| DeclaredMarketplace {
                        git: Some(format!("https://example.invalid/{marketplace}")),
                        path: None,
                        r#ref: None,
                        subdirectory: None,
                        plugins: Vec::new(),
                    })
                    .plugins
                    .push((*name).to_owned());
            }
            manifest
        }

        fn lock(entries: &[(&str, &str, bool)]) -> ProjectLock {
            let mut lock = ProjectLock::default();
            for (name, marketplace, echoed) in entries {
                lock.plugins.insert(
                    (*name).to_owned(),
                    LockedPlugin {
                        source: PluginSource::Marketplace {
                            marketplace: (*marketplace).to_owned(),
                            plugin: (*name).to_owned(),
                        },
                        resolved: ResolvedPlugin::default(),
                        requested: echoed.then(|| RequestedPlugin::from_marketplace(marketplace)),
                    },
                );
            }
            lock
        }

        #[test]
        fn a_lock_answering_the_manifest_is_current() {
            let stale = stale_against(&manifest(&[("flow", "ai")]), &lock(&[("flow", "ai", true)]));
            assert!(stale.is_empty(), "{stale:?}");
        }

        #[test]
        fn a_plugin_taken_from_a_different_marketplace_is_stale_and_names_both() {
            let stale = stale_against(
                &manifest(&[("flow", "mirror")]),
                &lock(&[("flow", "ai", true)]),
            );
            assert_eq!(stale.len(), 1);
            assert_eq!(stale[0].plugin, "flow");
            assert_eq!(stale[0].requested.marketplace.as_deref(), Some("mirror"));
            assert_eq!(
                stale[0].locked.as_ref().unwrap().marketplace.as_deref(),
                Some("ai")
            );
        }

        #[test]
        fn a_plugin_the_lock_has_never_seen_is_stale() {
            let stale = stale_against(&manifest(&[("flow", "ai")]), &lock(&[]));
            assert_eq!(stale.len(), 1);
            assert!(stale[0].locked.is_none());
        }

        /// An entry written before the echo existed must not report every
        /// project as out of date for a reason nobody can act on.
        #[test]
        fn an_entry_with_no_echo_is_not_called_stale() {
            let stale = stale_against(
                &manifest(&[("flow", "mirror")]),
                &lock(&[("flow", "ai", false)]),
            );
            assert!(stale.is_empty(), "{stale:?}");
        }

        /// A plugin in the lock the manifest no longer declares is not
        /// staleness: it is a removal the next write resolves.
        #[test]
        fn a_lock_entry_the_manifest_dropped_is_not_reported_here() {
            let stale = stale_against(&manifest(&[]), &lock(&[("flow", "ai", true)]));
            assert!(stale.is_empty(), "{stale:?}");
        }
    }

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
                requested: None,
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
    fn a_key_the_lock_no_longer_carries_is_rejected_rather_than_silently_dropped() {
        for (key, spelled) in [
            (
                "worktrees_dir",
                "version: 1
worktrees_dir: ./.worktrees
",
            ),
            (
                "worktrees",
                "version: 1
worktrees:
  completion: pr
",
            ),
        ] {
            let err = parse_lock_str(spelled, &PathBuf::from("agents.lock")).unwrap_err();
            let UzeError::MalformedLock { reason, .. } = err else {
                panic!("a retired key must be reported as a malformed lock");
            };
            assert!(reason.contains(key), "{reason}");
            assert!(
                reason.contains("agents.yaml"),
                "the operator must be told where the declaration lives now: {reason}"
            );
        }
    }

    /// The complement, and the reason the check above is scoped to the
    /// policy: a lock written by a newer UZE must still load on an older
    /// one, so the top level tolerates keys it does not know.
    #[test]
    fn an_unknown_key_at_the_top_level_is_tolerated() {
        let lock = parse_lock_str(
            "version: 1\nsomething_from_a_newer_uze: true\n",
            &PathBuf::from("agents.lock"),
        )
        .expect("the lock stays forward-compatible");
        assert_eq!(lock.version, SUPPORTED_LOCK_VERSION);
    }
}
