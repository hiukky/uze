//! Vendor-neutral concurrent-work isolation.
//!
//! See `openspec/changes/add-portable-worktree-policy/` for the change that
//! introduced it; its ADR is numbered when that change is archived.
//!
//! Isolation itself is performed where UZE launches an agent, by choosing
//! its working directory — deterministic, and requiring nothing of the
//! harness; the slots it chooses among live in [`crate::checkout`]. What
//! lives here is the small remainder that cannot be delivered that way:
//!
//! - the fixed layout every layer must agree on (`.worktrees/<id>`,
//!   branch `agent/<id>`), and the lexical questions asked of it;
//! - what a project declares — what happens to finished work;
//! - the text projected into the project's shared instruction file, whose
//!   only audience is a writer UZE did not place: a subagent spawned inside
//!   a harness session.
//!
//! Deliberately *not* here: any instruction to create a top-level worktree.
//! A harness with its own worktree primitive activates on exactly that kind
//! of instruction, and would isolate a second time on top of the checkout
//! UZE already placed it in.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The prefix every managed region this module owns carries inside a
/// project's shared instruction file. Project-scoped, deliberately outside
/// the `package:*:instructions` shape `crate::context` owns, so neither
/// module can ever see the other's region as an orphan to remove.
pub const POLICY_REGION_PREFIX: &str = "project:worktree-policy";

/// Where isolated checkouts live, relative to the primary checkout. Fixed,
/// not configurable: the location is infrastructure, and every tool in this
/// space either fixes it or demands it per invocation — none offers a
/// project-level default to configure.
pub const WORKTREES_DIRECTORY: &str = ".worktrees";

/// The branch prefix isolated work is created under. Fixed for the same
/// reason. Generic on purpose: a branch name travels to remotes and
/// reviewers, and says what it is, not what made it.
pub const BRANCH_PREFIX: &str = "agent/";

/// What happens to an isolated agent's work once it is done. The only axis
/// a project declares, because it is the only one that is a team decision
/// rather than infrastructure.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionBehavior {
    /// Leave the branch for a human to integrate. The default: nothing ever
    /// reaches the primary branch without someone deciding it should.
    #[default]
    Handoff,
    /// Integrate into the target branch once checks pass.
    Merge,
    /// Publish the branch and open a pull request against the target.
    Pr,
}

impl CompletionBehavior {
    pub const fn abi_name(self) -> &'static str {
        match self {
            Self::Handoff => "handoff",
            Self::Merge => "merge",
            Self::Pr => "pr",
        }
    }

    /// The imperative clause this behavior contributes to the projected
    /// text. Kept beside `abi_name` so a behavior can never be added to the
    /// vocabulary without also being explainable to a model reading it.
    pub const fn instruction_clause(self) -> &'static str {
        match self {
            Self::Handoff => "your branch is left for a person to integrate; commit on it and stop",
            Self::Merge => {
                "UZE rebases your branch onto the target, runs the project's checks, and \
                 fast-forwards the target itself; commit on your branch and stop"
            }
            Self::Pr => {
                "UZE publishes your branch and opens a pull request for it; commit on your \
                 branch and stop"
            }
        }
    }
}

/// A project's declaration: which branch finished work targets, what
/// happens to it, what a fresh checkout needs, what gates delivery, and how
/// many checkouts may exist at once. Every field is optional with a safe
/// default, so a lock declaring nothing still loads.
///
/// A closed vocabulary, unlike the lock that carries it: everything a
/// project may declare about isolation is named here, so an unrecognized key
/// is a mistake to report rather than a field from a future version to
/// tolerate. Silently ignoring one would read as a policy honored.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorktreePolicy {
    /// The branch finished work targets. Undeclared, it is the branch the
    /// primary checkout is on when a task is created.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default)]
    pub completion: CompletionBehavior,
    /// Ignored files a fresh checkout links from the primary checkout —
    /// `.env` and friends. Relative, inside the repository, and ignored by
    /// it: a symlink the agent writes through reaches the primary, so only
    /// what the agent reads belongs here.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub link: Vec<PathBuf>,
    /// A shell command that prepares a checkout, run in it after linking.
    /// Its failure warns and never blocks a launch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup: Option<String>,
    /// A shell command run in the task's checkout on the rebased commits;
    /// a non-zero exit refuses delivery.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<String>,
    /// The most checkouts that may exist at once. Undeclared, peak
    /// concurrency is the only bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slots: Option<usize>,
}

impl WorktreePolicy {
    /// The links that are not relative paths staying inside the
    /// repository — each with the reason. Pure, so it runs at parse time.
    pub fn misplaced_links(&self) -> Vec<(PathBuf, &'static str)> {
        self.link
            .iter()
            .filter_map(|link| {
                if link.is_absolute() {
                    Some((link.clone(), "an absolute path"))
                } else if link
                    .components()
                    .any(|component| matches!(component, std::path::Component::ParentDir))
                {
                    Some((link.clone(), "a path leaving the repository"))
                } else if link.as_os_str().is_empty() {
                    Some((link.clone(), "an empty path"))
                } else {
                    None
                }
            })
            .collect()
    }
}

impl WorktreePolicy {
    /// The managed-region identity this exact policy owns.
    ///
    /// The rendered content's digest is part of the identity, and that is
    /// what makes a declaration *editable*. With a fixed identity, a changed
    /// policy would render different bytes into a region that already
    /// exists, which `text_region` correctly refuses as drift — the policy
    /// could be projected once and never updated again. Keying the identity
    /// on the content instead turns an edit into "one region is now stale,
    /// another is missing": both answerable without ever overwriting content
    /// UZE did not write.
    ///
    /// A hand edit still drifts, because it changes the content *inside* an
    /// identity that stays exactly what it was.
    pub fn region_identity(&self) -> String {
        format!(
            "{POLICY_REGION_PREFIX}/{}",
            crate::digest::short_hex(self.instructions().as_bytes())
        )
    }

    /// Whether `identity` is a region this module owns — the test for a
    /// stale region left by a previous declaration.
    pub fn owns_region(identity: &str) -> bool {
        identity
            .strip_prefix(POLICY_REGION_PREFIX)
            .is_some_and(|rest| rest.starts_with('/'))
    }

    /// The rendered statement projected into the project's shared
    /// instruction file — the exact bytes the managed region carries.
    ///
    /// Written for a writer UZE did not place. It states the layout so a
    /// subagent can reproduce it, and it states where the reader already is,
    /// so an agent UZE isolated does not isolate itself again. It never asks
    /// anyone to create a top-level worktree: UZE already did that, at
    /// launch, for every agent it started.
    pub fn instructions(&self) -> String {
        format!(
            "## Concurrent work isolation\n\
             \n\
             - Every agent UZE launches works in a checkout of its own under \
             `{directory}/<id>`, on branch `{prefix}<id>`. If your working directory is inside \
             `{directory}/`, you are already isolated: do not create another worktree, and do \
             not switch branches.\n\
             - Commit your work on your own branch, as you go. Never commit to, merge into, \
             rebase, or reset the target branch{target}: delivery is UZE's — \
             {completion}.\n\
             - If UZE tells you a rebase is paused in your checkout, resolve the conflicts \
             preserving the intent of your change, run `git rebase --continue`, run the \
             project's checks, and end your turn.\n\
             - Before spawning parallel subagents that write files, give each its own checkout \
             so they cannot collide:\n\
             \n\
             ```bash\n\
             git worktree add -b {prefix}<topic> \"$(git rev-parse --path-format=absolute \
             --git-common-dir)/../{directory}/<topic>\" HEAD\n\
             ```\n\
             \n\
             - The path above is resolved against the *primary* checkout on purpose — a path \
             relative to your own would nest one worktree inside another.\n",
            directory = WORKTREES_DIRECTORY,
            prefix = BRANCH_PREFIX,
            target = self
                .target
                .as_deref()
                .map(|target| format!(" (`{target}`)"))
                .unwrap_or_default(),
            completion = self.completion.instruction_clause()
        )
    }
}

/// The primary checkout `cwd` belongs to, or `None` when `cwd` is not in a
/// Git working tree.
///
/// Answers with the *primary* checkout even when `cwd` is inside a linked
/// worktree: Git keeps one common directory per repository, so this is the
/// stable answer to "which repository is this", which is what the slot
/// layout is scoped to.
pub fn primary_checkout(cwd: &Path) -> Option<PathBuf> {
    let common = PathBuf::from(
        uze_git::read(
            cwd,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )
        .ok()?
        .successful()
        .ok()?
        .trim(),
    );
    // `<primary>/.git` for an ordinary checkout; a bare repository has no
    // working tree to seat anyone in.
    common
        .file_name()
        .filter(|name| *name == ".git")
        .and_then(|_| common.parent())
        .map(Path::to_path_buf)
}

/// An isolated checkout, named apart from the primary it belongs to.
///
/// Borrowed from the path it was read out of: this is derived from a path
/// a caller already holds, and every field is a slice of it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IsolatedCheckout<'a> {
    /// The primary checkout the isolated one hangs off.
    pub primary: &'a Path,
    /// The isolated checkout's own name — the `<id>` in the fixed
    /// `.worktrees/<id>` layout.
    pub name: &'a str,
}

impl IsolatedCheckout<'_> {
    /// The checkout's own directory — what a slot is keyed on when a path
    /// inside it is all the caller has.
    pub fn directory(&self) -> PathBuf {
        self.primary.join(WORKTREES_DIRECTORY).join(self.name)
    }
}

/// The isolated checkout `path` sits in, or `None` for a path that is not
/// isolated.
///
/// Lexical against the fixed layout: a display asks this of every open tab
/// on every frame, and a subprocess per tab there is a cost with no
/// matching benefit.
///
/// The deepest match wins, so a path inside a checkout that itself sits
/// inside another names the one it is actually in.
pub fn isolated_checkout(path: &Path) -> Option<IsolatedCheckout<'_>> {
    path.ancestors().find_map(|checkout| {
        let container = checkout.parent()?;
        if container.file_name()? != WORKTREES_DIRECTORY {
            return None;
        }
        Some(IsolatedCheckout {
            primary: container.parent()?,
            name: checkout.file_name()?.to_str()?,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_projected_text_never_asks_for_a_top_level_worktree() {
        // A harness that ships its own worktree primitive activates on
        // exactly that instruction, and would isolate a second time on top
        // of the checkout UZE already placed the agent in.
        let text = WorktreePolicy::default().instructions();
        assert!(!text.to_lowercase().contains("before editing"), "{text}");
        assert!(!text.to_lowercase().contains("create or reuse"), "{text}");
        assert!(
            text.contains("already isolated"),
            "the reader must be told where it already is: {text}"
        );
    }

    #[test]
    fn the_projected_text_states_the_layout_and_the_completion_rule() {
        let text = WorktreePolicy::default().instructions();
        assert!(text.contains(WORKTREES_DIRECTORY));
        assert!(text.contains(BRANCH_PREFIX));
        assert!(text.contains(CompletionBehavior::Handoff.instruction_clause()));
        assert!(
            text.contains("git rev-parse"),
            "a subagent's worktree must resolve against the primary, not its own checkout"
        );

        let merging = WorktreePolicy {
            completion: CompletionBehavior::Merge,
            ..WorktreePolicy::default()
        };
        let text = merging.instructions();
        assert!(text.contains(CompletionBehavior::Merge.instruction_clause()));
        assert!(!text.contains(CompletionBehavior::Handoff.instruction_clause()));
    }

    #[test]
    fn rendering_is_deterministic_for_the_same_policy() {
        let policy = WorktreePolicy::default();
        assert_eq!(policy.instructions(), policy.instructions());
        assert_eq!(policy.region_identity(), policy.region_identity());
    }

    /// The property that makes a declaration editable rather than
    /// write-once: a different policy claims a different region, so updating
    /// one is never an overwrite of content UZE did not write.
    #[test]
    fn a_changed_policy_claims_a_different_region() {
        let handoff = WorktreePolicy::default();
        let merge = WorktreePolicy {
            completion: CompletionBehavior::Merge,
            ..WorktreePolicy::default()
        };
        assert_ne!(handoff.region_identity(), merge.region_identity());
        assert!(WorktreePolicy::owns_region(&handoff.region_identity()));
        assert!(WorktreePolicy::owns_region(&merge.region_identity()));
    }

    #[test]
    fn only_this_modules_regions_are_claimed() {
        assert!(!WorktreePolicy::owns_region(
            "package:uze:official:instructions"
        ));
        assert!(!WorktreePolicy::owns_region("instruction-bridge"));
        assert!(!WorktreePolicy::owns_region(POLICY_REGION_PREFIX));
    }

    #[test]
    fn an_isolated_path_names_the_checkout_it_sits_in() {
        let checkout = isolated_checkout(Path::new("/repo/.worktrees/ai/src/ui")).unwrap();
        assert_eq!(checkout.primary, Path::new("/repo"));
        assert_eq!(checkout.name, "ai");

        // The checkout root itself, not only something under it.
        let root = isolated_checkout(Path::new("/repo/.worktrees/ai")).unwrap();
        assert_eq!(root.primary, Path::new("/repo"));
        assert_eq!(root.name, "ai");
    }

    #[test]
    fn a_path_in_the_primary_checkout_is_not_isolated() {
        assert!(isolated_checkout(Path::new("/repo/src/ui")).is_none());
        assert!(isolated_checkout(Path::new("/repo")).is_none());
        // The container is not a checkout; only its children are.
        assert!(isolated_checkout(Path::new("/repo/.worktrees")).is_none());
    }

    /// Git keeps its registry flat however the directories nest, and so
    /// must the name a display shows: the checkout the path is actually in,
    /// not the outermost one.
    #[test]
    fn the_deepest_checkout_wins_when_they_nest() {
        let checkout =
            isolated_checkout(Path::new("/repo/.worktrees/outer/.worktrees/inner/src")).unwrap();
        assert_eq!(checkout.name, "inner");
        assert_eq!(checkout.primary, Path::new("/repo/.worktrees/outer"));
    }

    fn repository(label: &str) -> uze_testkit::git::Repository {
        let repository = uze_testkit::git::Repository::new(label);
        repository.commit_file("file", "seed");
        repository
    }

    #[test]
    fn the_primary_checkout_is_the_same_answer_from_inside_an_isolated_one() {
        let repository = repository("worktree-primary");
        let root = repository.root();
        repository.git(&[
            "worktree",
            "add",
            "-q",
            "-b",
            "agent/x",
            ".worktrees/x",
            "HEAD",
        ]);
        let isolated = root.join(".worktrees").join("x");

        let from_root = primary_checkout(root).expect("a working tree has a primary");
        let from_isolated = primary_checkout(&isolated).expect("so does a linked worktree");
        assert_eq!(from_root, from_isolated);
        assert_eq!(
            from_root,
            root.canonicalize().unwrap_or_else(|_| root.to_path_buf())
        );
    }

    #[test]
    fn a_directory_outside_any_repository_has_no_primary_checkout() {
        let root = uze_testkit::temp::scratch("worktree-norepo");
        assert_eq!(primary_checkout(&root), None);
    }
}
