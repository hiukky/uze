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

use noyalib::compat::serde_yaml;
use serde::{Deserialize, Serialize};

use crate::{Result, UzeError, worktree::WorktreePolicy};

pub const MANIFEST_FILE_NAME: &str = "agents.yaml";

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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub plugins: BTreeMap<String, DeclaredPlugin>,
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
    /// The embedded snapshot compiled into the binary.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub embedded: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subdirectory: Option<PathBuf>,
}

/// What the project wants, never what resolution found. A `revision` or an
/// `integrity` here is rejected by name rather than ignored — those are
/// the lock's to write, and a person putting them here has misunderstood
/// which file they are editing.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DeclaredPlugin {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marketplace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subdirectory: Option<PathBuf>,
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

/// The header a manifest UZE created carries, so the first person to open
/// it knows which of the two files is theirs.
const CREATED_HEADER: &str = "\
# This project's agent environment. UZE reads this file and writes
# agents.lock from it — edit this one; the lock regenerates.
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
    document.append_block(CREATED_HEADER)?;
    document.append_block(DEFAULT_POLICY_BLOCK)?;
    document.save()?;
    Ok(true)
}

/// The policy written into a fresh manifest: the built-in default, spelled
/// out and commented, so the knobs are discoverable by opening the file
/// rather than by reading documentation.
const DEFAULT_POLICY_BLOCK: &str = "\nworktrees:\n  # handoff | merge | pr — what UZE does with an agent's\n  # finished branch. `handoff` leaves it for you to integrate.\n  completion: handoff\n";

/// Declares a plugin and the marketplace it comes from, creating the
/// manifest when the project has none. The document is patched in place,
/// so a comment a person wrote beside an unrelated entry survives.
pub fn declare_plugin(
    root: &Path,
    plugin: &str,
    marketplace: &str,
    source: &DeclaredMarketplace,
) -> Result<()> {
    let path = manifest_path_for(root);
    let mut document = if path.exists() {
        edit::ManifestDocument::open(&path)?
    } else {
        let mut fresh = edit::ManifestDocument::empty(&path)?;
        fresh.append_block(CREATED_HEADER)?;
        fresh
    };
    document.upsert("marketplaces", marketplace, &source.fragment())?;
    document.upsert(
        "plugins",
        plugin,
        &DeclaredPlugin {
            marketplace: Some(marketplace.to_owned()),
            ..DeclaredPlugin::default()
        }
        .fragment(),
    )?;
    document.save()?;
    // Re-read through the typed path: a write that produces a manifest the
    // schema rejects is a bug in this function, and must surface here rather
    // than on the next command.
    load(root)?;
    Ok(())
}

/// Removes a plugin's declaration. Reports whether it was there, so a
/// caller can tell "removed" from "never declared". The marketplace stays:
/// it may be feeding another plugin, and an unused declaration is inert.
pub fn undeclare_plugin(root: &Path, plugin: &str) -> Result<bool> {
    let path = manifest_path_for(root);
    if !path.exists() {
        return Ok(false);
    }
    let mut document = edit::ManifestDocument::open(&path)?;
    let declared = load(root)?
        .map(|manifest| manifest.plugins.contains_key(plugin))
        .unwrap_or(false);
    if !declared {
        return Ok(false);
    }
    document.remove("plugins", plugin)?;
    document.save()?;
    Ok(true)
}

/// One YAML scalar, quoted the way the value needs. Delegating the quoting
/// rather than guessing at it is what keeps a URL, a Windows path or a
/// version that looks like a float from changing meaning on the way in.
fn scalar(value: &str) -> String {
    serde_yaml::to_string(&value.to_owned())
        .map(|emitted| emitted.trim().to_owned())
        .unwrap_or_else(|_| format!("{value:?}"))
}

fn inline(entries: Vec<(&str, String)>) -> String {
    let body = entries
        .into_iter()
        .map(|(key, value)| format!("{key}: {value}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{ {body} }}")
}

impl DeclaredMarketplace {
    /// The inline spelling UZE writes. Authored entries keep whatever shape
    /// their author chose; only what UZE adds looks like this.
    pub fn fragment(&self) -> String {
        let mut entries = Vec::new();
        if let Some(git) = &self.git {
            entries.push(("git", scalar(git)));
        }
        if let Some(path) = &self.path {
            entries.push(("path", scalar(&path.to_string_lossy())));
        }
        if self.embedded {
            entries.push(("embedded", "true".to_owned()));
        }
        if let Some(reference) = &self.r#ref {
            entries.push(("ref", scalar(reference)));
        }
        if let Some(subdirectory) = &self.subdirectory {
            entries.push(("subdirectory", scalar(&subdirectory.to_string_lossy())));
        }
        inline(entries)
    }
}

impl DeclaredPlugin {
    pub fn fragment(&self) -> String {
        let mut entries = Vec::new();
        if let Some(marketplace) = &self.marketplace {
            entries.push(("marketplace", scalar(marketplace)));
        }
        if let Some(git) = &self.git {
            entries.push(("git", scalar(git)));
        }
        if let Some(reference) = &self.r#ref {
            entries.push(("ref", scalar(reference)));
        }
        if let Some(subdirectory) = &self.subdirectory {
            entries.push(("subdirectory", scalar(&subdirectory.to_string_lossy())));
        }
        inline(entries)
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
    let manifest: ProjectManifest =
        serde_yaml::from_str(text).map_err(|error| UzeError::MalformedManifest {
            path: path.to_path_buf(),
            reason: explain(&error.to_string()),
        })?;
    validate(&manifest, path)?;
    Ok(manifest)
}

/// serde's "unknown field" message is accurate and unhelpful for the two
/// mistakes a person actually makes: writing a resolution into the
/// manifest, or writing the policy into the lock.
fn explain(reason: &str) -> String {
    for resolved in ["revision", "integrity", "version"] {
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
        let sources = usize::from(marketplace.git.is_some())
            + usize::from(marketplace.path.is_some())
            + usize::from(marketplace.embedded);
        if sources == 0 {
            return Err(malformed(format!(
                "marketplace `{name}` declares no source; give it a `git:`, a `path:`, or \
                 `embedded: true`"
            )));
        }
        if sources > 1 {
            return Err(malformed(format!(
                "marketplace `{name}` declares more than one source; a marketplace comes from one \
                 place"
            )));
        }
    }
    for (name, plugin) in &manifest.plugins {
        match (&plugin.marketplace, &plugin.git) {
            (None, None) => {
                return Err(malformed(format!(
                    "plugin `{name}` declares no source; name a `marketplace:` or a `git:` remote"
                )));
            }
            (Some(_), Some(_)) => {
                return Err(malformed(format!(
                    "plugin `{name}` declares both a marketplace and a Git remote; it comes from \
                     one of them"
                )));
            }
            (Some(marketplace), None) if !manifest.marketplaces.contains_key(marketplace) => {
                return Err(malformed(format!(
                    "plugin `{name}` names marketplace `{marketplace}`, which this manifest does \
                     not declare"
                )));
            }
            _ => {}
        }
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
        assert!(manifest.plugins.is_empty());
    }

    #[test]
    fn a_plugin_names_a_declared_marketplace() {
        let manifest = parsed(
            "marketplaces:\n  ai:\n    git: https://example.invalid/ai\nplugins:\n  flow:\n    \
             marketplace: ai\n",
        )
        .unwrap();
        assert_eq!(manifest.plugins["flow"].marketplace.as_deref(), Some("ai"));
    }

    #[test]
    fn a_plugin_naming_an_undeclared_marketplace_is_rejected() {
        let error = parsed("plugins:\n  flow:\n    marketplace: ghost\n").unwrap_err();
        assert!(error.to_string().contains("does not declare"), "{error}");
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
    fn a_resolution_field_is_rejected_by_name_and_points_at_the_lock() {
        let error = parsed(
            "marketplaces:\n  ai:\n    git: https://a.invalid\nplugins:\n  flow:\n    \
             marketplace: ai\n    revision: deadbeef\n",
        )
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
             setup: pnpm install\n  gate: cargo test\n  slots: 3\n",
        )
        .unwrap();
        let policy = manifest.worktrees.unwrap();
        assert_eq!(policy.target.as_deref(), Some("develop"));
        assert_eq!(policy.completion, CompletionBehavior::Pr);
        assert_eq!(policy.link.len(), 2);
        assert_eq!(policy.setup.as_deref(), Some("pnpm install"));
        assert_eq!(policy.gate.as_deref(), Some("cargo test"));
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
            worktree_policy(&root).unwrap().completion,
            CompletionBehavior::Handoff,
            "what is written must be what was already in force"
        );

        assert!(
            !ensure_exists(&root).unwrap(),
            "the second call changes nothing"
        );
        assert_eq!(fs::read_to_string(manifest_path_for(&root)).unwrap(), first);
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
            &DeclaredMarketplace {
                git: Some("https://example.invalid/ai".to_owned()),
                path: None,
                embedded: false,
                r#ref: None,
                subdirectory: None,
            },
        )
        .unwrap();

        let written = fs::read_to_string(manifest_path_for(&root)).unwrap();
        assert!(
            written.starts_with("# This project's agent environment."),
            "{written}"
        );
        let manifest = load(&root).unwrap().unwrap();
        assert_eq!(manifest.plugins["flow"].marketplace.as_deref(), Some("ai"));
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
            embedded: false,
            r#ref: None,
            subdirectory: None,
        };
        declare_plugin(&root, "flow", "ai", &source).unwrap();

        let path = manifest_path_for(&root);
        let annotated = fs::read_to_string(&path)
            .unwrap()
            .replace("  flow:", "  # pinned deliberately\n  flow:");
        fs::write(&path, &annotated).unwrap();

        declare_plugin(&root, "git", "ai", &source).unwrap();
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.contains("# pinned deliberately"), "{written}");
        let manifest = load(&root).unwrap().unwrap();
        assert_eq!(manifest.plugins.len(), 2);
    }

    #[test]
    fn undeclaring_reports_whether_it_was_there_and_empties_cleanly() {
        let root = uze_testkit::temp::scratch("manifest-undeclare");
        declare_plugin(
            &root,
            "flow",
            "ai",
            &DeclaredMarketplace {
                git: Some("https://example.invalid/ai".to_owned()),
                path: None,
                embedded: false,
                r#ref: None,
                subdirectory: None,
            },
        )
        .unwrap();

        assert!(!undeclare_plugin(&root, "never-declared").unwrap());
        assert!(undeclare_plugin(&root, "flow").unwrap());

        let written = fs::read_to_string(manifest_path_for(&root)).unwrap();
        assert!(!written.contains("{}"), "left an empty mapping: {written}");
        assert!(load(&root).unwrap().unwrap().plugins.is_empty());
    }

    #[test]
    fn a_value_that_would_change_meaning_unquoted_is_written_quoted() {
        let root = uze_testkit::temp::scratch("manifest-quoting");
        declare_plugin(
            &root,
            "flow",
            "no",
            &DeclaredMarketplace {
                git: None,
                path: Some(PathBuf::from("1.10")),
                embedded: false,
                r#ref: Some("2.0".to_owned()),
                subdirectory: None,
            },
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
