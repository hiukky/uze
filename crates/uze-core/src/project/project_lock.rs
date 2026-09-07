//! Project agent environment lock — `agents.lock`.
//!
//! The derived half of the pair [`manifest`] authors. Two rules decide
//! whether a key belongs in this file:
//!
//! 1. **It says something no other line says.** The plugin's name is the
//!    map key, so it is not also a field; a revision belongs to the
//!    marketplace that has one, not repeated onto every plugin inside it;
//!    a field that is permanently empty is not a field.
//! 2. **It is spelled the way `agents.yaml` spells it.** A marketplace is
//!    a `git:` or a `path:` in both files. A tagged union rendered into
//!    YAML (`source: {type: git, url: …}`) is JSON wearing a costume, and
//!    it makes a reader moving between the two files learn the same thing
//!    twice.
//!
//! What resolution adds to a declaration is exactly two things:
//! `revision:` — the immutable commit a Git marketplace was read at — and
//! `integrity:` — a digest of the bytes that landed, recorded only when
//! those bytes cannot change under it. Everything else in the file was
//! already in the manifest, and is repeated here only so the lock stands
//! alone on a machine that has not read one.
//!
//! Vendor-neutral, reproducible, Git-versionable. Store/Engine/Integration
//! never parse this file; only Core's serializer and Application's
//! project-environment use cases do.
//!
//! [`manifest`]: crate::manifest

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use noyalib::compat::serde_yaml;
use serde::{Deserialize, Serialize};

use crate::{Result, UzeError};

pub const SUPPORTED_LOCK_VERSION: u32 = 1;
pub const LOCK_FILE_NAME: &str = "agents.lock";

/// Top-level lock file.
///
/// `version` is the one key here that records nothing about the project.
/// It earns its place by being what lets a file written by another UZE
/// fail with a sentence a person can act on instead of a serde message
/// about an unknown field. It stays at 1 while UZE is pre-release: the
/// number exists to describe a shape somebody else's UZE might have
/// written, and until there are released versions to differ, a shape
/// change is a shape change with nobody downstream to tell.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
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

/// A marketplace: the repository it is, and the commit this project was
/// resolved at.
///
/// Both are required, so "a marketplace with no pin" cannot be written
/// down. `git` is an identity another machine can resolve — never the
/// local checkout somebody happened to read it from, which is a fact about
/// one machine and belongs in that machine's own registry.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LockedMarketplace {
    pub git: String,
    /// The declared branch or tag, kept because it is what a later resolve
    /// follows. `revision` is where it pointed when this was written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subdirectory: Option<PathBuf>,
    pub revision: String,
}

impl LockedMarketplace {
    /// Whether this entry still answers the declaration it was resolved
    /// from. `revision` is not part of the question — that is the answer.
    ///
    /// A `path:` declaration is compared on everything except the URL: the
    /// identity of a local checkout is a question for that checkout's Git
    /// remote, and this comparison is deliberately a pure read of two
    /// files. Repointing a `path:` at a different repository is therefore
    /// not staleness anything sees here; `uze add` re-resolves it.
    pub fn answers(&self, declared: &crate::manifest::DeclaredMarketplace) -> bool {
        if let Some(url) = &declared.git
            && &self.git != url
        {
            return false;
        }
        self.r#ref == declared.r#ref && self.subdirectory == declared.subdirectory
    }

    pub fn display(&self) -> String {
        let mut spelled = self.git.clone();
        if let Some(reference) = &self.r#ref {
            spelled.push('@');
            spelled.push_str(reference);
        }
        if let Some(subdirectory) = &self.subdirectory {
            spelled.push('#');
            spelled.push_str(&subdirectory.display().to_string());
        }
        spelled
    }
}

/// A plugin: the marketplace that carries it, and a digest of the bytes
/// that landed. Its own name is the map key, so it is not repeated as a
/// field, and the revision that reproduces it belongs to the marketplace
/// above — a directory inside a repository has no revision of its own.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LockedPlugin {
    pub marketplace: String,
    /// Recorded only when the bytes cannot change underneath it. A plugin
    /// from a `path:` marketplace has none — a digest of a directory
    /// somebody is editing would be wrong by the next command, and a pin
    /// that is routinely wrong teaches people to ignore pins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integrity: Option<String>,
}

impl LockedPlugin {
    /// The entry for a plugin just resolved. `reproducible` says whether
    /// the bytes came from an immutable source and may therefore be
    /// pinned; `root` is where they landed.
    pub fn resolved(marketplace: &str, root: &Path, reproducible: bool) -> Self {
        Self {
            marketplace: marketplace.to_owned(),
            integrity: reproducible
                .then(|| crate::digest::tree_sha256(root).ok())
                .flatten(),
        }
    }
}

/// One declaration the lock no longer answers for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StaleEntry {
    pub plugin: String,
    /// The marketplace `agents.yaml` declares it under.
    pub marketplace: String,
    /// The marketplace the lock resolved it from, when it resolved it at
    /// all — so a message can name both sides of a move.
    pub locked: Option<String>,
}

/// Which of the manifest's declarations the lock no longer answers for.
///
/// Deliberately offline and deliberately cheap: it compares the two files
/// and nothing else. Re-resolving to find out whether a lock is current
/// would need the network for a question the files already answer, and
/// would make `status` fail in a tunnel.
///
/// A declaration is stale when the lock has never resolved it, when it was
/// resolved from a different marketplace, or when the marketplace it comes
/// from is declared differently now — a new source, or a `ref:` pointing
/// somewhere else. A lock entry the manifest no longer declares is not
/// staleness: it is surplus, which [`surplus_against`] answers for.
pub fn stale_against(
    manifest: &crate::manifest::ProjectManifest,
    lock: &ProjectLock,
) -> Vec<StaleEntry> {
    let mut stale = Vec::new();
    for (plugin, marketplace) in manifest.declared_plugins() {
        let entry = |locked: Option<&str>| StaleEntry {
            plugin: plugin.to_owned(),
            marketplace: marketplace.to_owned(),
            locked: locked.map(str::to_owned),
        };
        let Some(locked) = lock.plugins.get(plugin) else {
            stale.push(entry(None));
            continue;
        };
        if locked.marketplace != marketplace {
            stale.push(entry(Some(&locked.marketplace)));
            continue;
        }
        let declared = manifest.marketplaces.get(marketplace);
        let recorded = lock.marketplaces.get(marketplace);
        let agrees = match (declared, recorded) {
            (Some(declared), Some(recorded)) => recorded.answers(declared),
            _ => false,
        };
        if !agrees {
            stale.push(entry(Some(&locked.marketplace)));
        }
    }
    stale
}

/// Which of the lock's plugins the manifest no longer declares.
///
/// The inverse of [`stale_against`], and the half it cannot express: that
/// function iterates the manifest's declarations, so a plugin deleted from
/// `agents.yaml` is invisible to it by construction. Same cost and same
/// offline promise — two documents compared, nothing else asked.
///
/// Scoped to the marketplaces the manifest actually declares, and this is
/// the load-bearing part: silence is not a claim. A lock inherited from
/// before the manifest existed, or one whose marketplace nobody has
/// declared, is a project that has said nothing about those plugins — not
/// a project asking for all of them to be taken away. Only a marketplace
/// the manifest names can make one of its plugins surplus, which is
/// exactly the edit a person makes when they mean it.
pub fn surplus_against(
    manifest: &crate::manifest::ProjectManifest,
    lock: &ProjectLock,
) -> Vec<String> {
    let declared: Vec<&str> = manifest
        .declared_plugins()
        .map(|(plugin, _)| plugin)
        .collect();
    lock.plugins
        .iter()
        .filter(|(plugin, locked)| {
            manifest.marketplaces.contains_key(&locked.marketplace)
                && !declared.contains(&plugin.as_str())
        })
        .map(|(plugin, _)| plugin.clone())
        .collect()
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
    // A key written twice is a mistake, not a precedence question — the
    // same rule the manifest holds. YAML's default is to keep the last,
    // which would let a second `integrity:` quietly replace the pin.
    let config = noyalib::ParserConfig::serde_yaml_compat()
        .duplicate_key_policy(noyalib::DuplicateKeyPolicy::Error);
    let raw: serde_yaml::Value =
        noyalib::from_str_with_config(text, &config).map_err(|e| UzeError::MalformedLock {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;
    // Asked before the schema sees the document, and before the keys that
    // moved out are looked for: a lock written by another UZE must be
    // reported as exactly that, not as whichever of its fields this
    // version happens to notice first.
    let version = raw
        .as_mapping()
        .and_then(|mapping| mapping.get("version"))
        .and_then(serde_yaml::Value::as_u64);
    match version {
        Some(version) if version == u64::from(SUPPORTED_LOCK_VERSION) => {}
        Some(found) => {
            return Err(UzeError::UnsupportedLockVersion {
                found: u32::try_from(found).unwrap_or(u32::MAX),
                expected: SUPPORTED_LOCK_VERSION,
            });
        }
        None => {
            return Err(UzeError::MalformedLock {
                path: path.to_path_buf(),
                reason: format!(
                    "no `version:` — every lock carries one, and this one is expected to be \
                     {SUPPORTED_LOCK_VERSION}"
                ),
            });
        }
    }
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

    serde_yaml::from_value(raw).map_err(|e| UzeError::MalformedLock {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })
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
    let mut yaml = serde_yaml::to_string(lock).map_err(|e| UzeError::MalformedLock {
        path: path.clone(),
        reason: e.to_string(),
    })?;
    // This file is generated and committed: it ends with a newline, like
    // every other text file in a repository, so appending to it or reading
    // it in a terminal does not start mid-line.
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn locked(marketplace: &str) -> LockedPlugin {
        LockedPlugin {
            marketplace: marketplace.to_owned(),
            integrity: None,
        }
    }

    mod staleness {
        use super::*;
        use crate::manifest::{DeclaredMarketplace, ProjectManifest};

        fn declared(marketplace: &str) -> DeclaredMarketplace {
            DeclaredMarketplace {
                git: Some(format!("https://example.invalid/{marketplace}")),
                path: None,
                r#ref: None,
                subdirectory: None,
                plugins: Vec::new(),
            }
        }

        /// The declarations, grouped the way the manifest groups them: the
        /// marketplace carries the plugins, so what a declaration can differ
        /// in is which marketplace it is under.
        fn manifest(entries: &[(&str, &str)]) -> ProjectManifest {
            let mut manifest = ProjectManifest::default();
            for (name, marketplace) in entries {
                manifest
                    .marketplaces
                    .entry((*marketplace).to_owned())
                    .or_insert_with(|| declared(marketplace))
                    .plugins
                    .push((*name).to_owned());
            }
            manifest
        }

        /// The lock the manifest above resolves to, so a test changes one
        /// thing at a time rather than starting from disagreement.
        fn lock(entries: &[(&str, &str)]) -> ProjectLock {
            let mut lock = ProjectLock::default();
            for (name, marketplace) in entries {
                lock.marketplaces.insert(
                    (*marketplace).to_owned(),
                    LockedMarketplace {
                        git: format!("https://example.invalid/{marketplace}"),
                        r#ref: None,
                        subdirectory: None,
                        revision: "abc123".to_owned(),
                    },
                );
                lock.plugins.insert((*name).to_owned(), locked(marketplace));
            }
            lock
        }

        #[test]
        fn a_lock_answering_the_manifest_is_current() {
            let stale = stale_against(&manifest(&[("flow", "ai")]), &lock(&[("flow", "ai")]));
            assert!(stale.is_empty(), "{stale:?}");
        }

        #[test]
        fn a_plugin_taken_from_a_different_marketplace_is_stale_and_names_both() {
            let stale = stale_against(&manifest(&[("flow", "mirror")]), &lock(&[("flow", "ai")]));
            assert_eq!(stale.len(), 1);
            assert_eq!(stale[0].plugin, "flow");
            assert_eq!(stale[0].marketplace, "mirror");
            assert_eq!(stale[0].locked.as_deref(), Some("ai"));
        }

        #[test]
        fn a_plugin_the_lock_has_never_seen_is_stale() {
            let stale = stale_against(&manifest(&[("flow", "ai")]), &lock(&[]));
            assert_eq!(stale.len(), 1);
            assert!(stale[0].locked.is_none());
        }

        /// The declaration moving is the whole question: a `ref:` pointing
        /// somewhere else resolves to different bytes, so the entry that
        /// answered the old one no longer answers this.
        #[test]
        fn a_marketplace_declared_differently_makes_its_plugins_stale() {
            let mut manifest = manifest(&[("flow", "ai")]);
            manifest.marketplaces.get_mut("ai").unwrap().r#ref = Some("v2".to_owned());
            let stale = stale_against(&manifest, &lock(&[("flow", "ai")]));
            assert_eq!(stale.len(), 1, "{stale:?}");
            assert_eq!(stale[0].plugin, "flow");
        }

        /// A plugin in the lock the manifest no longer declares is not
        /// staleness: it is a removal `remove` resolves.
        #[test]
        fn a_lock_entry_the_manifest_dropped_is_not_reported_here() {
            let stale = stale_against(&manifest(&[]), &lock(&[("flow", "ai")]));
            assert!(stale.is_empty(), "{stale:?}");
        }
    }

    #[test]
    fn a_saved_lock_ends_with_a_newline() {
        let root = std::env::temp_dir().join(format!("uze-lock-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let mut lock = ProjectLock::default();
        lock.plugins.insert("flow".to_owned(), locked("ai"));
        save_lock(&root, &lock).unwrap();
        let written = fs::read_to_string(lock_path_for(&root)).unwrap();
        fs::remove_dir_all(&root).ok();
        assert!(written.ends_with('\n'), "{written:?}");
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

    /// The whole file, spelled out. This is the contract, so it is asserted
    /// as text rather than as a round trip: a reader must be able to see
    /// that nothing here is said twice, and a change to the shape must
    /// change this test. (The final newline is `save_lock`'s, not the
    /// serializer's — `a_saved_lock_ends_with_a_newline` covers it.)
    #[test]
    fn the_lock_says_each_thing_once() {
        let mut lock = ProjectLock::default();
        lock.marketplaces.insert(
            "ai".to_owned(),
            LockedMarketplace {
                git: "https://github.com/hiukky/ai".to_owned(),
                r#ref: Some("main".to_owned()),
                subdirectory: None,
                revision: "abc123".to_owned(),
            },
        );
        lock.plugins.insert(
            "flow".to_owned(),
            LockedPlugin {
                marketplace: "ai".to_owned(),
                integrity: Some("sha256:def456".to_owned()),
            },
        );
        assert_eq!(
            serde_yaml::to_string(&lock).unwrap(),
            "version: 1\n\
             marketplaces:\n\
             \x20 ai:\n\
             \x20   git: https://github.com/hiukky/ai\n\
             \x20   ref: main\n\
             \x20   revision: abc123\n\
             plugins:\n\
             \x20 flow:\n\
             \x20   marketplace: ai\n\
             \x20   integrity: sha256:def456"
        );
    }

    #[test]
    fn lock_round_trips_deterministically() {
        let mut lock = ProjectLock::default();
        lock.marketplaces.insert(
            "ai".to_owned(),
            LockedMarketplace {
                git: "https://github.com/hiukky/ai.git".to_owned(),
                r#ref: None,
                subdirectory: None,
                revision: "abc123".to_owned(),
            },
        );
        lock.plugins.insert("flow".to_owned(), locked("ai"));
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

    /// Pre-release, a shape change does not move the version — there is
    /// no released UZE downstream to tell apart — so a lock in the earlier
    /// shape is refused by its fields rather than by its number.
    #[test]
    fn a_lock_in_an_earlier_shape_is_refused() {
        let older = "version: 1
plugins:
  flow:
    source:
      type: marketplace
      marketplace: ai
      plugin: flow
    resolved: {}
";
        let error = parse_lock_str(older, &PathBuf::from("agents.lock")).unwrap_err();
        assert!(matches!(error, UzeError::MalformedLock { .. }), "{error:?}");
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

    /// A key nothing here understands is refused, at every level. The
    /// alternative — tolerating it for forward compatibility — cannot be
    /// right for this file: the version already says whether UZE can read
    /// it, and a lock is regenerated rather than preserved, so quietly
    /// ignoring a key that might have mattered buys nothing.
    #[test]
    fn a_key_this_uze_does_not_understand_is_refused() {
        for spelled in [
            "version: 1\nsomething_from_a_newer_uze: true\n",
            "version: 1\nplugins:\n  flow:\n    marketplace: ai\n    signature: nope\n",
        ] {
            let err = parse_lock_str(spelled, &PathBuf::from("agents.lock")).unwrap_err();
            assert!(matches!(err, UzeError::MalformedLock { .. }), "{spelled}");
        }
    }
}

#[cfg(test)]
mod surplus_tests {
    use super::*;

    fn manifest(declared: &[&str]) -> crate::manifest::ProjectManifest {
        let mut text = String::from("marketplaces:\n  ai:\n    path: /tmp/ai\n");
        if !declared.is_empty() {
            text.push_str("    plugins:\n");
            for plugin in declared {
                text.push_str(&format!("      - {plugin}\n"));
            }
        }
        crate::manifest::parse(&text, Path::new("agents.yaml")).unwrap()
    }

    fn lock(plugins: &[&str]) -> ProjectLock {
        let mut lock = ProjectLock::default();
        for plugin in plugins {
            lock.plugins.insert(
                (*plugin).to_owned(),
                LockedPlugin {
                    marketplace: "ai".to_owned(),
                    integrity: None,
                },
            );
        }
        lock
    }

    /// The half `stale_against` cannot express: it iterates the manifest's
    /// declarations, so a plugin deleted from `agents.yaml` is invisible to
    /// it by construction.
    #[test]
    fn a_plugin_the_manifest_no_longer_declares_is_surplus() {
        let surplus = surplus_against(&manifest(&["git"]), &lock(&["git", "flow"]));
        assert_eq!(surplus, vec!["flow".to_owned()]);
    }

    /// Silence is not a claim: a lock whose marketplace the manifest never
    /// mentions is a project that has said nothing about those plugins.
    #[test]
    fn a_marketplace_the_manifest_does_not_declare_makes_nothing_surplus() {
        let manifest = crate::manifest::parse("", Path::new("agents.yaml")).unwrap();
        assert!(surplus_against(&manifest, &lock(&["git", "flow"])).is_empty());
    }

    #[test]
    fn a_declared_plugin_is_never_surplus() {
        assert!(surplus_against(&manifest(&["git", "flow"]), &lock(&["git", "flow"])).is_empty());
    }

    /// The two answers are complements, never overlapping: a plugin is
    /// either something to resolve or something to take away.
    #[test]
    fn nothing_is_both_stale_and_surplus() {
        let (manifest, lock) = (manifest(&["git"]), lock(&["flow"]));
        let stale: Vec<String> = stale_against(&manifest, &lock)
            .into_iter()
            .map(|entry| entry.plugin)
            .collect();
        let surplus = surplus_against(&manifest, &lock);
        assert_eq!(stale, vec!["git".to_owned()]);
        assert_eq!(surplus, vec!["flow".to_owned()]);
        assert!(stale.iter().all(|plugin| !surplus.contains(plugin)));
    }
}
