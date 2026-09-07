//! `agents.yaml` — what a project declares about its agent environment.
//!
//! This is the authored half of the pair. Everything a person decides
//! lives here: which marketplaces the project draws from, which plugins it
//! wants, and how isolated work is delivered. [`project_lock`] holds the
//! other half — what resolving these declarations produced — and carries
//! no intent, so deleting it loses nothing.
//!
//! Reading is serde and rejects unknown fields: a typo in a file a person
//! wrote must be an error, never silence. Writing goes through
//! [`edit`], which patches the document in place so comments and
//! formatting survive.
//!
//! [`project_lock`]: crate::project_lock

pub mod edit;

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use noyalib::{DuplicateKeyPolicy, ParserConfig, compat::serde_yaml, from_str_with_config};
use serde::{Deserialize, Serialize};

use crate::{
    Result, UzeError,
    worktree::{CompletionBehavior, WorktreePolicy},
};

pub const MANIFEST_FILE_NAME: &str = "agents.yaml";

/// The marketplace UZE carries inside its own binary. A project never
/// declares it: it is part of how UZE works rather than something the
/// project chose, so a plugin may name it with no `marketplaces:` entry
/// behind it — and an entry claiming the name is refused, since the only
/// thing it could express is shadowing the built-in one.
pub const BUILT_IN_MARKETPLACE: &str = "uze-official";

/// The manifest as declared. Every field is optional: a project that
/// declares only an isolation policy is as valid as one that declares only
/// plugins.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProjectManifest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktrees: Option<WorktreePolicy>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub marketplaces: BTreeMap<String, DeclaredMarketplace>,
}

impl ProjectManifest {
    /// Every plugin the project declares, with the marketplace it comes
    /// from — the `plugin@marketplace` pair ADR-036 makes the identity,
    /// which this file spells structurally rather than by repeating the
    /// marketplace on each entry.
    pub fn declared_plugins(&self) -> impl Iterator<Item = (&str, &str)> {
        self.marketplaces
            .iter()
            .flat_map(|(marketplace, declared)| {
                declared
                    .plugins
                    .iter()
                    .map(move |plugin| (plugin.as_str(), marketplace.as_str()))
            })
    }

    /// The marketplace a plugin was declared under, if any.
    pub fn marketplace_of(&self, plugin: &str) -> Option<&str> {
        self.declared_plugins()
            .find(|(declared, _)| *declared == plugin)
            .map(|(_, marketplace)| marketplace)
    }
}

/// Where a marketplace comes from. The name a project refers to it by is
/// UX; this is its identity.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DeclaredMarketplace {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subdirectory: Option<PathBuf>,
    /// What the project installs from it. Bare names: the source above is
    /// what says where they come from, and the `ref:` above is what moves
    /// them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plugins: Vec<String>,
}

pub fn manifest_path_for(root: &Path) -> PathBuf {
    root.join(MANIFEST_FILE_NAME)
}

/// Reads the project's manifest, or `None` when the project has never
/// declared one. A project with no manifest behaves as one declaring
/// nothing — never as an error, and never as a prompt to create it.
pub fn load(root: &Path) -> Result<Option<ProjectManifest>> {
    let path = manifest_path_for(root);
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(&path).map_err(|source| UzeError::Read {
        path: path.clone(),
        source,
    })?;
    let text = String::from_utf8(bytes).map_err(|_| UzeError::MalformedManifest {
        path: path.clone(),
        reason: format!("{MANIFEST_FILE_NAME} is not valid UTF-8"),
    })?;
    let manifest = parse(&text, &path)?;
    if let Some(policy) = &manifest.worktrees {
        reject_unignored_links(root, &path, policy)?;
    }
    Ok(Some(manifest))
}

/// A linked file must be ignored by the repository: a tracked file linked
/// into a checkout would land in the agent's commits as a symlink. Asked of
/// Git only when a manifest declares links, so one that declares none costs
/// no subprocess to read.
fn reject_unignored_links(root: &Path, path: &Path, policy: &WorktreePolicy) -> Result<()> {
    for link in &policy.link {
        let spelled = link.to_string_lossy();
        let answer =
            uze_git::read(root, &["check-ignore", "--quiet", "--", &spelled]).map_err(|error| {
                UzeError::MalformedManifest {
                    path: path.to_path_buf(),
                    reason: format!("`worktrees.link` names `{spelled}`, but {error}"),
                }
            })?;
        match answer.code {
            Some(0) => {}
            Some(1) => {
                return Err(UzeError::MalformedManifest {
                    path: path.to_path_buf(),
                    reason: format!(
                        "`worktrees.link` names `{spelled}`, which the repository does not \
                         ignore; a linked file must be ignored, or it would be committed as a \
                         symlink from an agent's checkout"
                    ),
                });
            }
            _ => {
                return Err(UzeError::MalformedManifest {
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

/// The policy in force for a project: what the manifest declares, or the
/// built-in default. No machine-scoped setting participates — an
/// undeclared policy resolves the same way on every machine, so the text
/// projected into `AGENTS.md` does not depend on who ran the command.
pub fn worktree_policy(root: &Path) -> Result<WorktreePolicy> {
    Ok(load(root)?
        .and_then(|manifest| manifest.worktrees)
        .unwrap_or_default())
}

/// The file UZE writes when it creates the manifest itself: every key the
/// schema understands, with the default already in force spelled out beside
/// it, so the vocabulary is discoverable by opening the file rather than by
/// reading documentation.
///
/// Only `completion` is live. Everything else is commented, because a value
/// written here would be a decision UZE made on the project's behalf —
/// uncommenting a line is what changes behavior, never the file appearing.
const SCAFFOLD: &str = r"# This project's agent environment. UZE reads this file and writes
# agents.lock from it — edit this one; the lock regenerates.
#
# Every key UZE understands is below. A commented line carries the default
# already in force: uncomment it to make the choice the project's own.

worktrees:
  # handoff | merge | pr — what UZE does with an agent's finished branch.
  # `handoff` leaves it for you to integrate.
  completion: handoff

  # The branch finished work targets. Undeclared, it is the branch the
  # primary checkout is on when the task is created.
  # target: main

  # Ignored files a fresh checkout links from the primary one. Relative,
  # inside the repository, and ignored by it — a symlink the agent writes
  # through reaches the primary.
  # link: [.env, .env.local]

  # Prepares a fresh checkout, run in it after linking. Its failure warns
  # and never blocks a launch.
  # setup: pnpm install

  # Run in the checkout on the rebased commits; the first non-zero exit
  # refuses delivery and is named. One command, or a list run in order —
  # a list is what makes a failure say which step failed.
  # gate:
  #   - pnpm test
  #   - pnpm lint

  # The most checkouts that may exist at once. Undeclared, peak
  # concurrency is the only bound.
  # slots: 3

# The marketplaces this project draws from, and what it takes from each.
# Exactly one source per marketplace — `git:` or `path:` — and a Git one may
# be narrowed by `ref:` and `subdirectory:`. The `ref:` is also the pin: it
# is what moves the plugins listed under it.
#
# UZE's own marketplace is not here. It is built into the binary, its
# plugins are installed for every project, and neither is yours to declare
# — `uze market` shows both, this file carries only what the project chose.
#
# `uze <plugin>@<market>` writes these entries and records what they
# resolved to in agents.lock; `uze install` reproduces that lock on
# another machine.
# marketplaces:
#   ours:
#     git: https://github.com/acme/plugins
#     ref: main
#     subdirectory: market
#     plugins:
#       - review
#       - changelog
#   local:
#     path: ../marketplace
#     plugins:
#       - bench-runner

";

/// Creates the manifest with the built-in defaults written out, when the
/// project has none. Idempotent: an existing manifest is left exactly as it
/// is, comments and all.
///
/// Called only from an explicit act of setting a project up. Nothing that
/// merely *reads* a project — opening the client, inspecting, planning —
/// may call this: a repository somebody is only trying UZE against must
/// come back unchanged.
pub fn ensure_exists(root: &Path) -> Result<bool> {
    let path = manifest_path_for(root);
    if path.exists() {
        return Ok(false);
    }
    let mut document = edit::ManifestDocument::empty(&path)?;
    document.append_block(SCAFFOLD)?;
    document.save()?;
    Ok(true)
}

/// Declares the completion behavior, creating the manifest when the project
/// has none. This is the other act that declares something — the client's
/// own — so it creates the file for the same reason `install` does, and
/// reports whether it had to, since a caller showing a person the
/// consequence of their click needs to say "this creates a tracked file"
/// before it happens rather than after.
pub fn set_completion(root: &Path, behavior: CompletionBehavior) -> Result<bool> {
    let path = manifest_path_for(root);
    let created = ensure_exists(root)?;
    let mut document = edit::ManifestDocument::open(&path)?;
    document.upsert(
        "worktrees",
        "completion",
        &serde_yaml::Value::String(behavior.abi_name().to_owned()),
    )?;
    document.save()?;
    // Re-read through the typed path, the same guard `declare_plugin` has:
    // a write the schema would reject is a bug here, not on a later command.
    load(root)?;
    Ok(created)
}

/// Declares a plugin under the marketplace it comes from, creating the
/// manifest when the project has none. `source` is `None` for the built-in
/// marketplace: its plugins are installed for every project by the
/// machine's own bootstrap, so a project declaring one would be declaring
/// something it never chose — nothing is written, and the caller is told
/// so. The document is patched in place, so a comment a person wrote
/// beside an unrelated entry survives.
pub fn declare_plugin(
    root: &Path,
    plugin: &str,
    marketplace: &str,
    source: Option<&DeclaredMarketplace>,
) -> Result<bool> {
    let Some(source) = source else {
        return Ok(false);
    };
    let path = manifest_path_for(root);
    let mut document = if path.exists() {
        edit::ManifestDocument::open(&path)?
    } else {
        let mut fresh = edit::ManifestDocument::empty(&path)?;
        fresh.append_block(SCAFFOLD)?;
        fresh
    };
    let already_declared = load(root)?
        .map(|manifest| manifest.marketplaces.contains_key(marketplace))
        .unwrap_or(false);
    if already_declared {
        // Pushing into the list rather than rewriting the entry: the source
        // above it is the author's, comments and all.
        document.push_unique(&format!("marketplaces.{marketplace}.plugins"), plugin)?;
    } else {
        let mut declared = source.clone();
        declared.plugins = vec![plugin.to_owned()];
        document.upsert("marketplaces", marketplace, &declared.declaration()?)?;
    }
    document.save()?;
    // Re-read through the typed path: a write that produces a manifest the
    // schema rejects is a bug in this function, and must surface here rather
    // than on the next command.
    load(root)?;
    Ok(true)
}

/// Removes a plugin's declaration. Reports whether it was there, so a
/// caller can tell "removed" from "never declared". The marketplace stays:
/// it may be feeding another plugin, and a source declared with nothing
/// taken from it is inert rather than wrong.
pub fn undeclare_plugin(root: &Path, plugin: &str) -> Result<bool> {
    let path = manifest_path_for(root);
    if !path.exists() {
        return Ok(false);
    }
    let Some(marketplace) = load(root)?
        .as_ref()
        .and_then(|manifest| manifest.marketplace_of(plugin))
        .map(str::to_owned)
    else {
        return Ok(false);
    };
    let mut document = edit::ManifestDocument::open(&path)?;
    document.remove_from(&format!("marketplaces.{marketplace}.plugins"), plugin)?;
    document.save()?;
    Ok(true)
}

/// The value UZE writes for a declaration. Typed, never a fragment of YAML
/// text: the spelling — indentation, quoting, whether a path holding a
/// comma needs quotes — belongs to the emitter, and a caller that composes
/// syntax by hand is a caller that eventually composes it wrong.
fn declaration<T: Serialize>(declared: &T, what: &str) -> Result<serde_yaml::Value> {
    serde_yaml::to_value(declared).map_err(|error| UzeError::MalformedManifest {
        path: PathBuf::from(MANIFEST_FILE_NAME),
        reason: format!("{what} could not be written ({error})"),
    })
}

impl DeclaredMarketplace {
    pub fn declaration(&self) -> Result<serde_yaml::Value> {
        declaration(self, "a marketplace declaration")
    }
}

pub fn parse(text: &str, path: &Path) -> Result<ProjectManifest> {
    // A file that is empty, or holds only comments, declares nothing —
    // which is a valid manifest, not a type error about a missing mapping.
    if text
        .lines()
        .all(|line| line.trim().is_empty() || line.trim_start().starts_with('#'))
    {
        return Ok(ProjectManifest::default());
    }
    // A key written twice is a mistake, not a precedence question. YAML's
    // own answer — and the loader's default — is to keep the last one, so
    // declaring `flow` twice would silently drop one declaration in a file
    // whose whole contract is that a typo is an error rather than silence.
    let config = ParserConfig::serde_yaml_compat().duplicate_key_policy(DuplicateKeyPolicy::Error);
    let document: serde_yaml::Value =
        from_str_with_config(text, &config).map_err(|error| UzeError::MalformedManifest {
            path: path.to_path_buf(),
            reason: explain(&error.to_string()),
        })?;
    // Asked of the document before the schema sees it, the way the lock
    // refuses the keys that moved here: a key whose answer is about the
    // *file* cannot be told apart from a same-named key inside an entry
    // once serde has reduced both to "unknown field".
    for (key, why) in REFUSED_ROOT_KEYS {
        if document
            .as_mapping()
            .is_some_and(|mapping| mapping.contains_key(key))
        {
            return Err(UzeError::MalformedManifest {
                path: path.to_path_buf(),
                reason: format!("`{key}` is not a key of agents.yaml: {why}"),
            });
        }
    }
    let manifest: ProjectManifest =
        serde_yaml::from_value(document).map_err(|error| UzeError::MalformedManifest {
            path: path.to_path_buf(),
            reason: explain(&error.to_string()),
        })?;
    validate(&manifest, path)?;
    Ok(manifest)
}

/// Keys that are refused at the root of the manifest, with the reason a
/// reader needs. Each is a key somebody reasonably expects to exist; the
/// refusal explains why it does not, rather than reporting it as a typo.
const REFUSED_ROOT_KEYS: [(&str, &str); 1] = [(
    "version",
    "this file carries no schema version. A change to the schema is reported by naming the key it \
     affects — which is what an older UZE already does with a key it does not know — and a number \
     to compare would only be needed to keep reading a file UZE no longer understands",
)];

/// serde's "unknown field" message is accurate and unhelpful for the two
/// mistakes a person actually makes: writing a resolution into the
/// manifest, or writing the policy into the lock.
fn explain(reason: &str) -> String {
    // `version` is not the lock's to hold either: its field there is
    // permanently `None`, because nothing reads a plugin's version yet.
    // Sending a person to a field that never populates is worse than
    // saying not-yet.
    if reason.contains("unknown field") && reason.contains("version") {
        return "UZE does not resolve plugin versions yet — nothing in a plugin or a marketplace \
                catalog declares one. Pin the marketplace with its `ref:` instead."
            .to_owned();
    }
    for resolved in ["revision", "integrity"] {
        // Matched loosely on purpose: the wording of an unknown-field error
        // belongs to the YAML library, and this message must survive it
        // changing.
        if reason.contains("unknown field") && reason.contains(resolved) {
            return format!(
                "`{resolved}` is written by resolution, not declared: it belongs to agents.lock, \
                 which UZE regenerates. Remove it here."
            );
        }
    }
    reason.to_owned()
}

fn validate(manifest: &ProjectManifest, path: &Path) -> Result<()> {
    let malformed = |reason: String| UzeError::MalformedManifest {
        path: path.to_path_buf(),
        reason,
    };
    for (name, marketplace) in &manifest.marketplaces {
        if name == BUILT_IN_MARKETPLACE {
            return Err(malformed(format!(
                "`{name}` is the marketplace built into UZE; it is always available and cannot \
                 be declared. Remove the entry — the plugins that come from it stay."
            )));
        }
        let sources =
            usize::from(marketplace.git.is_some()) + usize::from(marketplace.path.is_some());
        if sources == 0 {
            return Err(malformed(format!(
                "marketplace `{name}` declares no source; give it a `git:` or a `path:`"
            )));
        }
        if sources > 1 {
            return Err(malformed(format!(
                "marketplace `{name}` declares more than one source; a marketplace comes from one \
                 place"
            )));
        }
    }
    // A plugin is a bare name under the marketplace that carries it, so
    // the questions left are about the name itself: that it is one, and
    // that only one entry claims it.
    let mut claimed: BTreeMap<&str, &str> = BTreeMap::new();
    for (marketplace, declared) in &manifest.marketplaces {
        for plugin in &declared.plugins {
            if plugin.trim().is_empty() {
                return Err(malformed(format!(
                    "marketplace `{marketplace}` declares a plugin with no name"
                )));
            }
            if let Some(first) = claimed.insert(plugin, marketplace) {
                return Err(malformed(format!(
                    "plugin `{plugin}` is declared under `{first}` and under `{marketplace}`. \
                     They are two different plugins, and this file has no way to say which one \
                     answers to `{plugin}` locally — take it from one of them."
                )));
            }
        }
    }
    if let Some(policy) = &manifest.worktrees
        && policy.slots == Some(0)
    {
        // Zero is honored, and honoring it refuses every checkout: the cap
        // is compared with `>=`, so the first task fails with "cap
        // reached" and nothing says the manifest is why.
        return Err(malformed(
            "`worktrees.slots` is 0, which would refuse every checkout there is; leave it \
             undeclared for no cap at all, or give it at least 1"
                .to_owned(),
        ));
    }
    if let Some(policy) = &manifest.worktrees
        && let Some((link, why)) = policy.misplaced_links().into_iter().next()
    {
        return Err(malformed(format!(
            "`worktrees.link` names `{}`, which is {why}; a link is a relative path inside the \
             repository",
            link.display()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worktree::CompletionBehavior;

    fn parsed(text: &str) -> Result<ProjectManifest> {
        parse(text, Path::new("/p/agents.yaml"))
    }

    #[test]
    fn an_empty_manifest_declares_nothing() {
        assert_eq!(parsed("").unwrap(), ProjectManifest::default());
    }

    #[test]
    fn a_policy_only_manifest_is_valid() {
        let manifest = parsed("worktrees:\n  completion: pr\n").unwrap();
        assert_eq!(
            manifest.worktrees.unwrap().completion,
            CompletionBehavior::Pr
        );
        assert!(manifest.marketplaces.is_empty());
    }

    #[test]
    fn a_plugin_is_a_name_under_the_marketplace_that_carries_it() {
        let manifest = parsed(
            "marketplaces:\n  ai:\n    git: https://example.invalid/ai\n    plugins:\n      - \
             flow\n      - review\n",
        )
        .unwrap();
        assert_eq!(
            manifest.declared_plugins().collect::<Vec<_>>(),
            vec![("flow", "ai"), ("review", "ai")]
        );
        assert_eq!(manifest.marketplace_of("review"), Some("ai"));
        assert_eq!(manifest.marketplace_of("never-declared"), None);
    }

    /// ADR-036 makes `plugin@marketplace` the identity, so two of them can
    /// coexist in the Store under different local names — but this file has
    /// no way to spell which one answers to the bare name, so declaring
    /// both is refused here rather than at install.
    #[test]
    fn a_plugin_declared_under_two_marketplaces_is_refused() {
        let error = parsed(
            "marketplaces:\n  one:\n    git: https://example.invalid/one\n    plugins: [git]\n  \
             two:\n    git: https://example.invalid/two\n    plugins: [git]\n",
        )
        .unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("`one`") && message.contains("`two`"),
            "{message}"
        );
    }

    #[test]
    fn a_plugin_with_no_name_is_refused() {
        let error = parsed(
            "marketplaces:\n  ai:\n    git: https://example.invalid/ai\n    plugins: [\"\"]\n",
        )
        .unwrap_err();
        assert!(error.to_string().contains("no name"), "{error}");
    }

    #[test]
    fn the_built_in_marketplace_is_not_a_thing_a_project_declares() {
        let error = parsed(&format!(
            "marketplaces:\n  {BUILT_IN_MARKETPLACE}:\n    plugins: [uze]\n"
        ))
        .unwrap_err();
        assert!(error.to_string().contains("built into UZE"), "{error}");
    }

    #[test]
    fn declaring_the_built_in_marketplace_is_refused() {
        let error = parsed(&format!(
            "marketplaces:\n  {BUILT_IN_MARKETPLACE}:\n    git: https://example.invalid/fake\n"
        ))
        .unwrap_err();
        let message = error.to_string();
        assert!(message.contains(BUILT_IN_MARKETPLACE), "{message}");
        assert!(message.contains("built into UZE"), "{message}");
    }

    /// The built-in marketplace's plugins are installed for every project
    /// by the machine's own bootstrap, so declaring one in a project file
    /// would record a choice the project never made.
    #[test]
    fn a_plugin_from_the_built_in_marketplace_is_not_declared_at_all() {
        let root = uze_testkit::temp::scratch("manifest-built-in");
        assert!(
            !declare_plugin(&root, "uze", BUILT_IN_MARKETPLACE, None).unwrap(),
            "nothing to declare"
        );
        assert!(
            !manifest_path_for(&root).exists(),
            "a manifest was created for a plugin the project does not declare"
        );
    }

    #[test]
    fn a_marketplace_may_declare_a_source_and_take_nothing_from_it() {
        let manifest =
            parsed("marketplaces:\n  ai:\n    git: https://example.invalid/ai\n").unwrap();
        assert!(manifest.marketplaces.contains_key("ai"));
        assert_eq!(manifest.declared_plugins().count(), 0);
    }

    #[test]
    fn a_marketplace_with_two_sources_is_rejected() {
        let error = parsed("marketplaces:\n  ai:\n    git: https://a.invalid\n    path: ../a\n")
            .unwrap_err();
        assert!(
            error.to_string().contains("more than one source"),
            "{error}"
        );
    }

    #[test]
    fn a_marketplace_with_no_source_is_rejected() {
        let error = parsed("marketplaces:\n  ai:\n    ref: main\n").unwrap_err();
        assert!(error.to_string().contains("declares no source"), "{error}");
    }

    #[test]
    fn a_key_declared_twice_is_an_error_rather_than_the_last_one_winning() {
        let error = parsed(
            "marketplaces:\n  ai:\n    git: https://example.invalid/ai\nplugins:\n  flow: ai\n  \
             flow: ai\n",
        )
        .unwrap_err();
        assert!(error.to_string().contains("flow"), "{error}");
    }

    #[test]
    fn a_pin_belongs_to_the_marketplace_that_carries_the_plugins() {
        let manifest = parsed(
            "marketplaces:\n  ai:\n    git: https://example.invalid/ai\n    ref: v0.3.1\n    \
             plugins: [flow]\n",
        )
        .unwrap();
        assert_eq!(manifest.marketplaces["ai"].r#ref.as_deref(), Some("v0.3.1"));
        assert_eq!(manifest.marketplace_of("flow"), Some("ai"));
    }

    #[test]
    fn a_plugin_entry_carrying_fields_of_its_own_is_refused() {
        let error = parsed(
            "marketplaces:\n  ai:\n    git: https://example.invalid/ai\n    plugins:\n      - \
             flow:\n          ref: v1\n",
        )
        .unwrap_err();
        assert!(
            !error.to_string().is_empty(),
            "a mapping item must not load"
        );
    }

    #[test]
    fn a_version_is_answered_with_what_uze_can_actually_pin() {
        let error =
            parsed("marketplaces:\n  ai:\n    git: https://a.invalid\n    version: \"^0.3\"\n")
                .unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("does not resolve plugin versions"),
            "{message}"
        );
        assert!(message.contains("`ref:`"), "{message}");
        assert!(
            !message.contains("agents.lock"),
            "the lock's version field is permanently empty; do not send anyone there: {message}"
        );
    }

    #[test]
    fn a_resolution_field_is_rejected_by_name_and_points_at_the_lock() {
        let error =
            parsed("marketplaces:\n  ai:\n    git: https://a.invalid\n    revision: deadbeef\n")
                .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("agents.lock"), "{message}");
        assert!(message.contains("revision"), "{message}");
    }

    #[test]
    fn a_misspelled_field_is_named_rather_than_ignored() {
        let error = parsed("worktrees:\n  completon: pr\n").unwrap_err();
        assert!(error.to_string().contains("completon"), "{error}");
    }

    #[test]
    fn a_schema_version_is_refused_and_says_why_there_is_none() {
        let error = parsed("version: 1\nworktrees:\n  completion: pr\n").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("no schema version"), "{message}");
        assert!(
            !message.contains("plugin versions"),
            "the plugin answer must not be given to the schema question: {message}"
        );
    }

    #[test]
    fn a_command_and_a_list_of_commands_are_both_accepted() {
        let one = parsed("worktrees:\n  gate: cargo test\n").unwrap();
        assert_eq!(one.worktrees.unwrap().gate, vec!["cargo test".to_owned()]);
        let many = parsed("worktrees:\n  gate:\n    - cargo test\n    - cargo clippy\n").unwrap();
        assert_eq!(many.worktrees.unwrap().gate.len(), 2);
    }

    #[test]
    fn declaring_the_policy_creates_the_manifest_and_says_that_it_did() {
        let root = uze_testkit::temp::scratch("manifest-set-completion");
        assert!(
            set_completion(&root, CompletionBehavior::Pr).unwrap(),
            "the first call creates the file, and the caller is told so"
        );
        assert_eq!(
            worktree_policy(&root).unwrap().completion,
            CompletionBehavior::Pr
        );

        assert!(
            !set_completion(&root, CompletionBehavior::Merge).unwrap(),
            "the second call changes a file that already exists"
        );
        assert_eq!(
            worktree_policy(&root).unwrap().completion,
            CompletionBehavior::Merge
        );
    }

    /// The scaffold's own commentary is what teaches the choices, so a
    /// click that changes one must not take the explanation with it.
    #[test]
    fn declaring_the_policy_keeps_the_comment_that_explains_it() {
        let root = uze_testkit::temp::scratch("manifest-set-completion-comments");
        set_completion(&root, CompletionBehavior::Pr).unwrap();
        let written = fs::read_to_string(manifest_path_for(&root)).unwrap();
        assert!(written.contains("completion: pr"), "{written}");
        assert!(written.contains("handoff | merge | pr"), "{written}");
        assert!(written.contains("# slots: 3"), "{written}");
    }

    #[test]
    fn declaring_the_policy_into_a_manifest_that_has_no_policy_block_adds_one() {
        let root = uze_testkit::temp::scratch("manifest-set-completion-marketless");
        let authored = "# ours\nmarketplaces:\n  ai:\n    path: ../ai\n    plugins: [flow]\n";
        fs::write(manifest_path_for(&root), authored).unwrap();

        assert!(
            !set_completion(&root, CompletionBehavior::Merge).unwrap(),
            "a manifest that exists is not created"
        );
        let manifest = load(&root).unwrap().unwrap();
        assert_eq!(
            manifest.worktrees.unwrap().completion,
            CompletionBehavior::Merge
        );
        let written = fs::read_to_string(manifest_path_for(&root)).unwrap();
        assert!(written.contains("# ours"), "{written}");
        assert_eq!(manifest.marketplaces["ai"].plugins, vec!["flow".to_owned()]);
    }

    #[test]
    fn a_cap_of_zero_is_refused_rather_than_honored() {
        let error = parsed("worktrees:\n  slots: 0\n").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("refuse every checkout"), "{message}");
        assert!(parsed("worktrees:\n  slots: 1\n").is_ok());
        assert!(parsed("worktrees:\n  completion: pr\n").is_ok());
    }

    #[test]
    fn a_link_escaping_the_repository_is_rejected() {
        let error = parsed("worktrees:\n  link: [../outside]\n").unwrap_err();
        assert!(error.to_string().contains("worktrees.link"), "{error}");
    }

    #[test]
    fn a_link_to_a_tracked_file_is_rejected_and_an_ignored_one_loads() {
        let repository = uze_testkit::git::Repository::new("manifest-links");
        repository.commit_file(".gitignore", ".env\n");
        let root = repository.root();

        fs::write(
            root.join(MANIFEST_FILE_NAME),
            "worktrees:\n  link: [.env]\n",
        )
        .unwrap();
        let manifest = load(root).unwrap().unwrap();
        assert_eq!(
            manifest.worktrees.unwrap().link,
            vec![PathBuf::from(".env")]
        );

        fs::write(
            root.join(MANIFEST_FILE_NAME),
            "worktrees:\n  link: [README.md]\n",
        )
        .unwrap();
        let error = load(root).unwrap_err();
        let UzeError::MalformedManifest { reason, .. } = error else {
            panic!("a tracked link must be a malformed manifest");
        };
        assert!(
            reason.contains("README.md") && reason.contains("ignore"),
            "{reason}"
        );
    }

    #[test]
    fn an_unknown_key_inside_the_policy_block_is_refused_by_name() {
        let error =
            parsed("worktrees:\n  completion: merge\n  directory: ./.worktrees\n").unwrap_err();
        assert!(error.to_string().contains("directory"), "{error}");
    }

    #[test]
    fn the_policy_round_trips_with_every_field() {
        let manifest = parsed(
            "worktrees:\n  target: develop\n  completion: pr\n  link: [.env, .env.local]\n  \
             setup: pnpm install\n  gate:\n    - cargo test\n    - cargo clippy\n  slots: 3\n",
        )
        .unwrap();
        let policy = manifest.worktrees.unwrap();
        assert_eq!(policy.target.as_deref(), Some("develop"));
        assert_eq!(policy.completion, CompletionBehavior::Pr);
        assert_eq!(policy.link.len(), 2);
        assert_eq!(policy.setup, vec!["pnpm install".to_owned()]);
        assert_eq!(
            policy.gate,
            vec!["cargo test".to_owned(), "cargo clippy".to_owned()],
            "a list is what makes a failure say which step failed"
        );
        assert_eq!(policy.slots, Some(3));
    }

    #[test]
    fn ensure_exists_creates_a_commented_default_and_is_idempotent() {
        let root = uze_testkit::temp::scratch("manifest-ensure");
        assert!(ensure_exists(&root).unwrap(), "the first call creates it");
        let first = fs::read_to_string(manifest_path_for(&root)).unwrap();
        assert!(first.contains("completion: handoff"), "{first}");
        assert!(
            first.contains("handoff | merge | pr"),
            "the choices must be discoverable by opening the file: {first}"
        );
        assert_eq!(
            worktree_policy(&root).unwrap(),
            WorktreePolicy::default(),
            "everything but the live line is commented, so a created manifest declares exactly \
             what was already in force"
        );

        assert!(
            !ensure_exists(&root).unwrap(),
            "the second call changes nothing"
        );
        assert_eq!(fs::read_to_string(manifest_path_for(&root)).unwrap(), first);
    }

    /// The scaffold is documentation that must not drift from the schema.
    /// Each struct literal is exhaustive, so a field added to the manifest
    /// has to be named here, and this then asks the created file to offer
    /// it — a new knob nobody can discover fails the build.
    #[test]
    fn a_created_manifest_offers_every_field_the_schema_accepts() {
        // The keys a shape declares, as serde spells them. Only column
        // zero: an entry's *name* inside a mapping is the author's word,
        // not a key the schema defines.
        fn declared_keys(shape: &impl Serialize) -> Vec<String> {
            serde_yaml::to_string(shape)
                .unwrap()
                .lines()
                .filter(|line| !line.starts_with(char::is_whitespace))
                .filter_map(|line| line.split_once(':').map(|(key, _)| key.to_owned()))
                .collect()
        }

        let policy = WorktreePolicy {
            target: Some("main".to_owned()),
            completion: CompletionBehavior::Handoff,
            link: vec![PathBuf::from(".env")],
            setup: vec!["pnpm install".to_owned()],
            gate: vec!["pnpm test".to_owned()],
            slots: Some(3),
        };
        let marketplace = DeclaredMarketplace {
            git: Some("https://example.invalid/ai".to_owned()),
            path: Some(PathBuf::from("../ai")),
            r#ref: Some("main".to_owned()),
            subdirectory: Some(PathBuf::from("plugins")),
            plugins: vec!["flow".to_owned()],
        };
        let manifest = ProjectManifest {
            worktrees: Some(policy.clone()),
            marketplaces: BTreeMap::from([("ai".to_owned(), marketplace.clone())]),
        };

        let keys = [
            declared_keys(&manifest),
            declared_keys(&policy),
            declared_keys(&marketplace),
        ]
        .concat();
        assert!(keys.len() > 10, "the shapes emitted nothing to check");
        for key in keys {
            assert!(
                SCAFFOLD.contains(&format!("{key}:")),
                "the created manifest never mentions `{key}`, so nothing tells a reader it \
                 exists:\n{SCAFFOLD}"
            );
        }
    }

    #[test]
    fn ensure_exists_never_touches_a_manifest_somebody_wrote() {
        let root = uze_testkit::temp::scratch("manifest-ensure-existing");
        let authored = "# mine\nworktrees:\n  completion: pr   # deliberate\n";
        fs::write(manifest_path_for(&root), authored).unwrap();
        assert!(!ensure_exists(&root).unwrap());
        assert_eq!(
            fs::read_to_string(manifest_path_for(&root)).unwrap(),
            authored
        );
    }

    #[test]
    fn declaring_a_plugin_creates_the_manifest_when_the_project_has_none() {
        let root = uze_testkit::temp::scratch("manifest-declare");
        declare_plugin(
            &root,
            "flow",
            "ai",
            Some(&DeclaredMarketplace {
                git: Some("https://example.invalid/ai".to_owned()),
                path: None,
                r#ref: None,
                subdirectory: None,
                plugins: Vec::new(),
            }),
        )
        .unwrap();

        let written = fs::read_to_string(manifest_path_for(&root)).unwrap();
        assert!(
            written.starts_with("# This project's agent environment."),
            "{written}"
        );
        let manifest = load(&root).unwrap().unwrap();
        assert_eq!(manifest.marketplace_of("flow"), Some("ai"));
        assert_eq!(
            manifest.marketplaces["ai"].git.as_deref(),
            Some("https://example.invalid/ai")
        );
    }

    #[test]
    fn declaring_a_second_plugin_keeps_the_first_and_its_comments() {
        let root = uze_testkit::temp::scratch("manifest-declare-second");
        let source = DeclaredMarketplace {
            git: Some("https://example.invalid/ai".to_owned()),
            path: None,
            r#ref: None,
            subdirectory: None,
            plugins: Vec::new(),
        };
        declare_plugin(&root, "flow", "ai", Some(&source)).unwrap();

        let path = manifest_path_for(&root);
        let annotated = fs::read_to_string(&path)
            .unwrap()
            .replace("      - flow", "      # pinned deliberately\n      - flow");
        fs::write(&path, &annotated).unwrap();

        declare_plugin(&root, "git", "ai", Some(&source)).unwrap();
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains("# pinned deliberately"), "{written}");
        let manifest = load(&root).unwrap().unwrap();
        assert_eq!(manifest.declared_plugins().count(), 2);
        assert_eq!(
            manifest.marketplaces["ai"].git.as_deref(),
            Some("https://example.invalid/ai"),
            "the source the author wrote is untouched by a second push"
        );
    }

    #[test]
    fn undeclaring_reports_whether_it_was_there_and_empties_cleanly() {
        let root = uze_testkit::temp::scratch("manifest-undeclare");
        declare_plugin(
            &root,
            "flow",
            "ai",
            Some(&DeclaredMarketplace {
                git: Some("https://example.invalid/ai".to_owned()),
                path: None,
                r#ref: None,
                subdirectory: None,
                plugins: Vec::new(),
            }),
        )
        .unwrap();

        assert!(!undeclare_plugin(&root, "never-declared").unwrap());
        assert!(undeclare_plugin(&root, "flow").unwrap());

        let written = fs::read_to_string(manifest_path_for(&root)).unwrap();
        assert!(!written.contains("{}"), "left an empty mapping: {written}");
        assert_eq!(load(&root).unwrap().unwrap().declared_plugins().count(), 0);
    }

    /// serde quotes for block context; `inline` writes into a flow
    /// mapping. A path or a ref carrying a comma or a brace used to be
    /// spliced in bare, and the re-read at the end of `declare_plugin`
    /// then refused the file UZE had just written.
    #[test]
    fn a_value_carrying_a_flow_indicator_stays_one_entry() {
        let root = uze_testkit::temp::scratch("manifest-flow-quoting");
        declare_plugin(
            &root,
            "flow",
            "odd",
            Some(&DeclaredMarketplace {
                git: None,
                path: Some(PathBuf::from("../my, dir")),
                r#ref: Some("release/{next}".to_owned()),
                subdirectory: None,
                plugins: Vec::new(),
            }),
        )
        .unwrap();

        let manifest = load(&root).unwrap().unwrap();
        let declared = &manifest.marketplaces["odd"];
        assert_eq!(declared.path.as_deref(), Some(Path::new("../my, dir")));
        assert_eq!(declared.r#ref.as_deref(), Some("release/{next}"));
    }

    #[test]
    fn a_value_that_would_change_meaning_unquoted_is_written_quoted() {
        let root = uze_testkit::temp::scratch("manifest-quoting");
        declare_plugin(
            &root,
            "flow",
            "no",
            Some(&DeclaredMarketplace {
                git: None,
                path: Some(PathBuf::from("1.10")),
                r#ref: Some("2.0".to_owned()),
                subdirectory: None,
                plugins: Vec::new(),
            }),
        )
        .unwrap();
        let manifest = load(&root).unwrap().unwrap();
        let declared = &manifest.marketplaces["no"];
        assert_eq!(declared.path.as_deref(), Some(Path::new("1.10")));
        assert_eq!(declared.r#ref.as_deref(), Some("2.0"));
    }

    #[test]
    fn an_undeclared_policy_is_the_built_in_default() {
        let directory = std::env::temp_dir().join(format!("uze-manifest-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        assert_eq!(
            worktree_policy(&directory).unwrap(),
            WorktreePolicy::default()
        );
        fs::remove_dir_all(&directory).ok();
    }
}
