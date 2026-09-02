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
    worktree::WorktreePolicy,
};

pub const SUPPORTED_LOCK_VERSION: u32 = 1;
pub const LOCK_FILE_NAME: &str = "agents.lock";

/// Top-level lock file.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ProjectLock {
    pub version: u32,
    /// What this project does with an isolated agent's finished work. The
    /// layout of isolated checkouts is fixed infrastructure, not declared
    /// here — see `crate::worktree`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktrees: Option<WorktreePolicy>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub marketplaces: BTreeMap<String, LockedMarketplace>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub plugins: BTreeMap<String, LockedPlugin>,
}

impl Default for ProjectLock {
    fn default() -> Self {
        Self {
            version: SUPPORTED_LOCK_VERSION,
            worktrees: None,
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
    let lock = parse_lock_str(&text, &path)?;
    if let Some(policy) = &lock.worktrees {
        reject_unignored_links(root, &path, policy)?;
    }
    Ok(Some(lock))
}

/// A linked file must be ignored by the repository: a tracked file linked
/// into a checkout would land in the agent's commits as a symlink. Asked
/// of Git only when a lock declares links, so a lock that declares none
/// costs no subprocess to read.
fn reject_unignored_links(
    root: &Path,
    path: &Path,
    policy: &crate::worktree::WorktreePolicy,
) -> Result<()> {
    for link in &policy.link {
        let spelled = link.to_string_lossy();
        let answer =
            uze_git::read(root, &["check-ignore", "--quiet", "--", &spelled]).map_err(|error| {
                UzeError::MalformedLock {
                    path: path.to_path_buf(),
                    reason: format!("`worktrees.link` names `{spelled}`, but {error}"),
                }
            })?;
        match answer.code {
            Some(0) => {}
            Some(1) => {
                return Err(UzeError::MalformedLock {
                    path: path.to_path_buf(),
                    reason: format!(
                        "`worktrees.link` names `{spelled}`, which the repository does not \
                         ignore; a linked file must be ignored, or it would be committed as a \
                         symlink from an agent's checkout"
                    ),
                });
            }
            _ => {
                return Err(UzeError::MalformedLock {
                    path: path.to_path_buf(),
                    reason: format!(
                        "`worktrees.link` names `{spelled}`, but this directory is not a Git \
                         repository that could ignore it"
                    ),
                });
            }
        }
    }
    Ok(())
}

/// The superseded spelling of the isolation policy: a bare directory, with
/// no trigger, naming, base-ref, or integration semantics, and no projection
/// to any harness. Rejected loudly rather than ignored — `ProjectLock` does
/// not deny unknown fields, so silently dropping it would turn a declared
/// policy into no policy at all with nothing said.
const REPLACED_DIRECTORY_KEY: &str = "worktrees_dir";

fn parse_lock_str(text: &str, path: &Path) -> Result<ProjectLock> {
    let raw: serde_yaml::Value =
        serde_yaml::from_str(text).map_err(|e| UzeError::MalformedLock {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;
    if raw
        .as_mapping()
        .is_some_and(|mapping| mapping.contains_key(REPLACED_DIRECTORY_KEY))
    {
        return Err(UzeError::MalformedLock {
            path: path.to_path_buf(),
            reason: format!(
                "`{REPLACED_DIRECTORY_KEY}` was replaced by the `worktrees` policy block; write a \
                 `worktrees:` block with a `completion:` entry instead, as the checkout layout is \
                 now fixed infrastructure rather than something a project declares"
            ),
        });
    }

    let lock: ProjectLock = serde_yaml::from_value(raw).map_err(|e| UzeError::MalformedLock {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })?;
    if let Some(policy) = &lock.worktrees
        && let Some((link, why)) = policy.misplaced_links().into_iter().next()
    {
        return Err(UzeError::MalformedLock {
            path: path.to_path_buf(),
            reason: format!(
                "`worktrees.link` names `{}`, which is {why}; a link is a relative path inside \
                 the repository",
                link.display()
            ),
        });
    }
    if lock.version != SUPPORTED_LOCK_VERSION {
        return Err(UzeError::UnsupportedLockVersion {
            found: lock.version,
            expected: SUPPORTED_LOCK_VERSION,
        });
    }
    Ok(lock)
}

pub fn save_lock(root: &Path, lock: &ProjectLock) -> Result<()> {
    if let Some(policy) = &lock.worktrees
        && let Some((link, why)) = policy.misplaced_links().into_iter().next()
    {
        return Err(UzeError::MalformedLock {
            path: lock_path_for(root),
            reason: format!(
                "`worktrees.link` names `{}`, which is {why}; a link is a relative path inside \
                 the repository",
                link.display()
            ),
        });
    }
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
    fn a_policy_block_defaults_every_axis_it_does_not_state() {
        let lock =
            parse_lock_str("version: 1\nworktrees: {}\n", &PathBuf::from("agents.lock")).unwrap();
        let policy = lock.worktrees.expect("an empty block is still a policy");
        assert_eq!(policy, crate::worktree::WorktreePolicy::default());
    }

    #[test]
    fn the_declared_completion_behavior_round_trips() {
        let lock = parse_lock_str(
            "version: 1\nworktrees:\n  completion: merge\n",
            &PathBuf::from("agents.lock"),
        )
        .unwrap();
        assert_eq!(
            lock.worktrees.unwrap().completion,
            crate::worktree::CompletionBehavior::Merge
        );
    }

    #[test]
    fn the_replaced_directory_key_is_rejected_rather_than_silently_dropped() {
        let err = parse_lock_str(
            "version: 1\nworktrees_dir: ./.worktrees\n",
            &PathBuf::from("agents.lock"),
        )
        .unwrap_err();
        let UzeError::MalformedLock { reason, .. } = err else {
            panic!("a replaced key must be reported as a malformed lock");
        };
        assert!(reason.contains(REPLACED_DIRECTORY_KEY), "{reason}");
        assert!(
            reason.contains("completion:"),
            "the operator must be told what to write instead: {reason}"
        );
    }

    /// The policy block is a closed vocabulary, unlike the lock around it: a
    /// key nobody recognizes there is a mistake, and reading past it would
    /// report a policy as honored that was never applied.
    #[test]
    fn an_unknown_key_inside_the_policy_block_is_refused_by_name() {
        let err = parse_lock_str(
            "version: 1\nworktrees:\n  completion: merge\n  directory: ./.worktrees\n",
            &PathBuf::from("agents.lock"),
        )
        .unwrap_err();
        let UzeError::MalformedLock { reason, .. } = err else {
            panic!("an unknown policy key must be reported as a malformed lock");
        };
        assert!(reason.contains("directory"), "{reason}");
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

    #[test]
    fn the_policy_round_trips_with_every_field() {
        let lock = parse_lock_str(
            "version: 1\nworktrees:\n  target: develop\n  completion: pr\n  link: [.env, .env.local]\n  setup: pnpm install\n  gate: cargo test\n  slots: 3\n",
            &PathBuf::from("agents.lock"),
        )
        .unwrap();
        let policy = lock.worktrees.clone().unwrap();
        assert_eq!(policy.target.as_deref(), Some("develop"));
        assert_eq!(policy.completion, crate::worktree::CompletionBehavior::Pr);
        assert_eq!(
            policy.link,
            vec![PathBuf::from(".env"), PathBuf::from(".env.local")]
        );
        assert_eq!(policy.setup.as_deref(), Some("pnpm install"));
        assert_eq!(policy.gate.as_deref(), Some("cargo test"));
        assert_eq!(policy.slots, Some(3));
        let text = serde_yaml::to_string(&lock).unwrap();
        let again = parse_lock_str(&text, &PathBuf::from("agents.lock")).unwrap();
        assert_eq!(again, lock);
    }

    #[test]
    fn a_policy_block_declaring_nothing_has_safe_defaults() {
        let lock =
            parse_lock_str("version: 1\nworktrees: {}\n", &PathBuf::from("agents.lock")).unwrap();
        let policy = lock.worktrees.unwrap();
        assert_eq!(policy.target, None);
        assert_eq!(
            policy.completion,
            crate::worktree::CompletionBehavior::Handoff
        );
        assert!(policy.link.is_empty() && policy.setup.is_none() && policy.gate.is_none());
        assert_eq!(policy.slots, None);
    }

    #[test]
    fn a_link_escaping_the_repository_is_rejected_at_read_time() {
        for link in ["/etc/passwd", "../sibling/.env", "config/../../.env"] {
            let err = parse_lock_str(
                &format!("version: 1\nworktrees:\n  link: ['{link}']\n"),
                &PathBuf::from("agents.lock"),
            )
            .unwrap_err();
            let UzeError::MalformedLock { reason, .. } = err else {
                panic!("{link}: a misplaced link must be a malformed lock");
            };
            assert!(reason.contains(link), "{reason}");
        }
    }

    /// Against a real repository: an ignored link loads, a tracked one is
    /// refused by name and reason.
    #[test]
    fn a_link_to_a_tracked_file_is_rejected_and_an_ignored_one_loads() {
        let repository = uze_testkit::git::Repository::new("lock-links");
        repository.commit_file(".gitignore", ".env\n");
        let root = repository.root();

        fs::write(
            root.join(LOCK_FILE_NAME),
            "version: 1\nworktrees:\n  link: [.env]\n",
        )
        .unwrap();
        let lock = load_lock(root).unwrap().unwrap();
        assert_eq!(lock.worktrees.unwrap().link, vec![PathBuf::from(".env")]);

        fs::write(
            root.join(LOCK_FILE_NAME),
            "version: 1\nworktrees:\n  link: [README.md]\n",
        )
        .unwrap();
        let err = load_lock(root).unwrap_err();
        let UzeError::MalformedLock { reason, .. } = err else {
            panic!("a tracked link must be a malformed lock");
        };
        assert!(
            reason.contains("README.md") && reason.contains("ignore"),
            "{reason}"
        );
    }
}
