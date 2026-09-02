//! Vendor-neutral concurrent-work isolation.
//!
//! See `openspec/changes/add-portable-worktree-policy/` for the change that
//! introduced it; its ADR is numbered when that change is archived.
//!
//! Isolation itself is performed where UZE launches an agent, by choosing
//! its working directory — deterministic, and requiring nothing of the
//! harness. What lives here is the small remainder that cannot be delivered
//! that way:
//!
//! - the fixed layout every layer must agree on (`.worktrees/<name>`,
//!   branch `agent/<name>`);
//! - the one thing a project declares — what happens to finished work;
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
/// reason, and so one name identifies a tab, a directory, and a branch.
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
    /// Integrate into the primary branch once checks pass.
    Merge,
}

impl CompletionBehavior {
    pub const fn abi_name(self) -> &'static str {
        match self {
            Self::Handoff => "handoff",
            Self::Merge => "merge",
        }
    }

    /// The imperative clause this behavior contributes to the projected
    /// text. Kept beside `abi_name` so a behavior can never be added to the
    /// vocabulary without also being explainable to a model reading it.
    pub const fn instruction_clause(self) -> &'static str {
        match self {
            Self::Handoff => {
                "leave your branch and its commits for review — never merge, rebase, or reset \
                 the primary branch"
            }
            Self::Merge => {
                "integrate your branch into the primary branch once its checks pass, and stop \
                 and report instead if the primary checkout has uncommitted work"
            }
        }
    }
}

/// A project's declaration. One field today, and a struct rather than a bare
/// enum because this is the shape `agents.lock` carries and the shape a
/// second axis would extend without breaking existing locks.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorktreePolicy {
    #[serde(default)]
    pub completion: CompletionBehavior,
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
             - Isolated checkouts live in `{directory}/<name>` under the primary checkout, one \
             writer each, on branch `{prefix}<name>`.\n\
             - If your working directory is already inside `{directory}/`, you are already \
             isolated. Do not create another worktree, and do not switch branches.\n\
             - Before spawning parallel subagents that write files, give each its own checkout \
             so they cannot collide:\n\
             \n\
             ```bash\n\
             git worktree add -b {prefix}<topic> \"$(git rev-parse --path-format=absolute \
             --git-common-dir)/../{directory}/<topic>\" HEAD\n\
             ```\n\
             \n\
             - The path above is resolved against the *primary* checkout on purpose — a path \
             relative to your own would nest one worktree inside another.\n\
             - When work is done: {completion}.\n",
            directory = WORKTREES_DIRECTORY,
            prefix = BRANCH_PREFIX,
            completion = self.completion.instruction_clause()
        )
    }
}

/// The linked worktrees Git records for a checkout, read straight from
/// `.git/worktrees/*/gitdir` rather than by running `git`.
///
/// Status views are budgeted commands; a subprocess spawn per invocation is
/// exactly the cost that classification exists to keep out. Each `gitdir`
/// file holds the absolute path of a linked worktree's `.git` pointer, so
/// its parent is the checkout — a handful of small reads.
///
/// Every linked worktree registers here, including one created from inside
/// another linked worktree: Git keeps this registry in the common directory,
/// so the record stays flat however the directories nest.
///
/// Returns empty for anything that is not a primary checkout with linked
/// worktrees. Purely observational, and never an error — an unreadable Git
/// layout is "nothing observed", not a reason to fail a status view.
pub fn discover_linked_worktrees(project_root: &Path) -> Vec<PathBuf> {
    let registry = project_root.join(".git").join("worktrees");
    let Ok(entries) = std::fs::read_dir(&registry) else {
        return Vec::new();
    };
    let mut roots: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| std::fs::read_to_string(entry.path().join("gitdir")).ok())
        .filter_map(|gitdir| PathBuf::from(gitdir.trim()).parent().map(Path::to_path_buf))
        .collect();
    roots.sort();
    roots.dedup();
    roots
}

/// The primary checkout `cwd` belongs to, or `None` when `cwd` is not in a
/// Git working tree.
///
/// Answers with the *primary* checkout even when `cwd` is inside a linked
/// worktree: Git keeps one common directory per repository, so this is the
/// stable answer to "which repository is this", which is what the seat and
/// the worktree layout are both scoped to.
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
    /// The isolated checkout's own name — the `<name>` in the fixed
    /// `.worktrees/<name>` layout, and the suffix of its `agent/<name>`
    /// branch.
    pub name: &'a str,
}

/// The isolated checkout `path` sits in, or `None` for a path that is not
/// isolated.
///
/// Lexical against the fixed layout for the same reason [`is_in_primary`]
/// is: a display asks this of every open tab on every frame, and a
/// subprocess per tab there is a cost with no matching benefit.
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

/// Whether `path` sits in the primary checkout itself rather than in one of
/// its isolated checkouts.
///
/// Decided lexically against the fixed layout rather than by asking Git,
/// because it is answered once per open tab on a user-initiated action and
/// a subprocess per tab is a cost with no matching benefit.
pub fn is_in_primary(primary: &Path, path: &Path) -> bool {
    path.starts_with(primary) && !path.starts_with(primary.join(WORKTREES_DIRECTORY))
}

/// Creates an isolated checkout named for `slug` under `primary`, branching
/// from its current `HEAD`, and returns the checkout's path.
///
/// `HEAD` rather than a configured base: it is what the agent would have
/// seen had the seat been free, which is the least surprising thing for a
/// writer that did not ask to be isolated.
///
/// Never overwrites: a name already taken by a directory or a branch — the
/// ordinary case once checkouts are kept rather than deleted — is suffixed
/// until it is free.
pub fn isolate(primary: &Path, slug: &str) -> Result<PathBuf, String> {
    // A checkout removed outside UZE leaves its registry entry behind, and
    // `worktree add` then refuses the name. Pruning first is what makes
    // creation reliable across everything that happened while UZE was not
    // running.
    let _ = git(primary, &["worktree", "prune"]);

    let name = available_name(primary, slug);
    let relative = format!("{WORKTREES_DIRECTORY}/{name}");
    git(
        primary,
        &[
            "worktree",
            "add",
            "-b",
            &format!("{BRANCH_PREFIX}{name}"),
            &relative,
            "HEAD",
        ],
    )?;

    // Without this the agent holding the seat sweeps every other agent's
    // checkout into its own commit: `git add -A` stages a nested working
    // tree as an embedded repository rather than ignoring it.
    ignore_worktrees_directory(primary)?;

    Ok(primary.join(&relative))
}

/// The first name not already taken by a directory or a branch.
fn available_name(primary: &Path, slug: &str) -> String {
    let taken = |name: &str| {
        primary.join(WORKTREES_DIRECTORY).join(name).exists()
            || uze_git::read(
                primary,
                &[
                    "rev-parse",
                    "--verify",
                    "--quiet",
                    &format!("refs/heads/{BRANCH_PREFIX}{name}"),
                ],
            )
            .is_ok_and(|output| output.is_success())
    };
    if !taken(slug) {
        return slug.to_owned();
    }
    (2..)
        .map(|suffix| format!("{slug}-{suffix}"))
        .find(|candidate| !taken(candidate))
        .unwrap_or_else(|| slug.to_owned())
}

/// Adds the isolated-checkout directory to the project's ignore file when it
/// is not already ignored. Idempotent, and never rewrites an existing line.
fn ignore_worktrees_directory(primary: &Path) -> Result<(), String> {
    let entry = format!("{WORKTREES_DIRECTORY}/");
    let ignore = primary.join(".gitignore");
    let current = std::fs::read_to_string(&ignore).unwrap_or_default();
    if current
        .lines()
        .any(|line| line.trim() == entry || line.trim() == WORKTREES_DIRECTORY)
    {
        return Ok(());
    }
    let mut next = current;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(&entry);
    next.push('\n');
    std::fs::write(&ignore, next).map_err(|error| format!("could not update .gitignore: {error}"))
}

/// A Git command that changes the repository, with stdout trimmed — every
/// answer this module reads is a single line (a path, a branch name), never
/// content whose whitespace carries meaning.
fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    uze_git::write(root, args)
        .map_err(|error| error.to_string())?
        .successful()
        .map(|stdout| stdout.trim().to_owned())
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

    /// Git keeps its registry flat however the directories nest (see
    /// `discover_linked_worktrees`), and so must the name a display shows:
    /// the checkout the path is actually in, not the outermost one.
    #[test]
    fn the_deepest_checkout_wins_when_they_nest() {
        let checkout =
            isolated_checkout(Path::new("/repo/.worktrees/outer/.worktrees/inner/src")).unwrap();
        assert_eq!(checkout.name, "inner");
        assert_eq!(checkout.primary, Path::new("/repo/.worktrees/outer"));
    }

    #[test]
    fn discovery_reads_gits_own_registry_without_running_git() {
        let root = uze_testkit::temp::scratch("worktree-discovery");
        let registry = root.join(".git").join("worktrees").join("topic");
        std::fs::create_dir_all(&registry).unwrap();
        let linked = root.join(WORKTREES_DIRECTORY).join("topic");
        std::fs::write(
            registry.join("gitdir"),
            format!("{}\n", linked.join(".git").display()),
        )
        .unwrap();

        assert_eq!(discover_linked_worktrees(&root), vec![linked]);
    }

    fn repository(label: &str) -> PathBuf {
        let root = uze_testkit::temp::scratch(label);
        for args in [
            vec!["init", "-q", "-b", "main", "."],
            vec!["config", "user.email", "t@example.invalid"],
            vec!["config", "user.name", "t"],
        ] {
            git(&root, &args).expect("git must be available for these tests");
        }
        std::fs::write(root.join("file"), b"seed").unwrap();
        git(&root, &["add", "."]).unwrap();
        git(&root, &["commit", "-qm", "seed"]).unwrap();
        root
    }

    #[test]
    fn the_primary_checkout_is_the_same_answer_from_inside_an_isolated_one() {
        let root = repository("worktree-primary");
        let isolated = isolate(&root, "agent-1").expect("isolation must succeed");

        let from_root = primary_checkout(&root).expect("a working tree has a primary");
        let from_isolated = primary_checkout(&isolated).expect("so does a linked worktree");
        assert_eq!(from_root, from_isolated);
        assert_eq!(from_root, root.canonicalize().unwrap_or(root.clone()));
    }

    #[test]
    fn a_directory_outside_any_repository_has_no_primary_checkout() {
        let root = uze_testkit::temp::scratch("worktree-norepo");
        assert_eq!(primary_checkout(&root), None);
    }

    /// The seat is the primary checkout itself. An isolated checkout lives
    /// under the same repository but must never read as occupying it, or
    /// every agent after the first would be told the seat is taken by
    /// somebody who already left it.
    #[test]
    fn an_isolated_checkout_does_not_occupy_the_seat() {
        let primary = Path::new("/repo");
        assert!(is_in_primary(primary, Path::new("/repo")));
        assert!(is_in_primary(primary, Path::new("/repo/crates/core")));
        assert!(!is_in_primary(
            primary,
            &Path::new("/repo").join(WORKTREES_DIRECTORY).join("agent-1")
        ));
        assert!(!is_in_primary(primary, Path::new("/elsewhere")));
    }

    #[test]
    fn isolation_creates_a_checkout_on_its_own_branch_and_ignores_the_directory() {
        let root = repository("worktree-isolate");
        let isolated = isolate(&root, "agent-1").unwrap();

        assert_eq!(isolated, root.join(WORKTREES_DIRECTORY).join("agent-1"));
        assert!(isolated.join("file").is_file(), "the checkout is populated");
        assert_eq!(
            git(&isolated, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap(),
            format!("{BRANCH_PREFIX}agent-1")
        );
        assert!(
            std::fs::read_to_string(root.join(".gitignore"))
                .unwrap()
                .contains(WORKTREES_DIRECTORY),
            "an unignored checkout is swept into the seat agent's next commit"
        );
        // The seat's own status must not see the isolated checkout at all.
        // The freshly written `.gitignore` does show up — it is a real new
        // file for the operator to commit, which is the intended outcome.
        let status = git(&root, &["status", "--short"]).unwrap();
        assert!(!status.contains(WORKTREES_DIRECTORY), "{status}");
        assert_eq!(status, "?? .gitignore");
    }

    /// Checkouts are kept, not deleted, so the same agent label recurs. A
    /// second isolation must not fail and must not reuse the first branch.
    #[test]
    fn a_taken_name_is_suffixed_rather_than_reused_or_refused() {
        let root = repository("worktree-collision");
        let first = isolate(&root, "agent-1").unwrap();
        let second = isolate(&root, "agent-1").unwrap();

        assert_ne!(first, second);
        assert_eq!(second, root.join(WORKTREES_DIRECTORY).join("agent-1-2"));
        assert_eq!(
            git(&second, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap(),
            format!("{BRANCH_PREFIX}agent-1-2")
        );
    }

    #[test]
    fn ignoring_the_directory_is_idempotent_and_preserves_existing_entries() {
        let root = repository("worktree-ignore");
        std::fs::write(root.join(".gitignore"), "target/\n").unwrap();
        isolate(&root, "agent-1").unwrap();
        isolate(&root, "agent-2").unwrap();

        let ignore = std::fs::read_to_string(root.join(".gitignore")).unwrap();
        assert!(ignore.contains("target/"), "foreign entries survive");
        assert_eq!(
            ignore.matches(WORKTREES_DIRECTORY).count(),
            1,
            "the entry is added once, not once per isolation: {ignore}"
        );
    }

    /// A repository with no commits has no `HEAD` to branch from. Isolation
    /// must fail cleanly so the caller can seat the agent instead of
    /// refusing to launch it.
    #[test]
    fn isolation_fails_cleanly_on_a_repository_with_no_commits() {
        let root = uze_testkit::temp::scratch("worktree-unborn");
        git(&root, &["init", "-q", "-b", "main", "."]).unwrap();
        assert!(isolate(&root, "agent-1").is_err());
    }

    #[test]
    fn a_checkout_with_no_linked_worktrees_observes_nothing() {
        let root = uze_testkit::temp::scratch("worktree-empty");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        assert!(discover_linked_worktrees(&root).is_empty());
    }
}
