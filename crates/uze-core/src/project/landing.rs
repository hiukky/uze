//! How a task's work reaches the target.
//!
//! Readiness is a Git fact read from the task's checkout — commits ahead
//! of the base on a clean tree — never something an agent announces.
//! Delivery is performed by UZE on an explicit operator action, one task at
//! a time under the repository write lock: rebase the branch onto the
//! target's tip inside the task's own checkout, run the declared gate on
//! the rebased commits, then do what the project's completion behaviour
//! says. The target is written here, in the fast-forward step of `merge`,
//! and nowhere else.
//!
//! A conflict or a failed gate leaves the target untouched and returns the
//! task to the agent that owns it: the rebase stays paused in its checkout
//! with the markers in place, because that agent is the only party holding
//! the intent behind the change.

use std::{
    fmt,
    path::{Path, PathBuf},
    time::Duration,
};

use crate::{
    checkout::{self, commits_ahead, is_dirty},
    subprocess::run_shell_bounded,
    task::{Task, TaskState},
    worktree::{CompletionBehavior, WORKTREES_DIRECTORY},
};

/// A gate that has not finished in this long is a hung gate.
pub const GATE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const REMOTE: &str = "origin";

/// What the task's checkout says about the task.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Readiness {
    /// No commits beyond the base yet, or no checkout to read.
    Running,
    /// Uncommitted changes in the checkout.
    Uncommitted,
    /// A rebase is paused in the checkout on these files.
    Rebasing { files: Vec<PathBuf> },
    /// Commits ahead of `base` on a clean tree.
    Ready { ahead: usize, base: String },
}

/// The project's say in delivery.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Policy<'a> {
    pub completion: CompletionBehavior,
    /// A shell command run in the task's checkout on the rebased commits;
    /// a non-zero exit refuses delivery.
    pub gate: Option<&'a str>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Delivered {
    /// The branch is left for the operator.
    Handoff,
    /// The target now points at the branch's tip.
    Merged { target_tip: String },
    /// The branch was pushed under its readable name and the forge
    /// already has `request` open for it: a sync, which is Git alone.
    Published { branch: String, request: u32 },
    /// The branch was pushed and no request is open for it yet. Opening
    /// one is the owning agent's, which is why this carries the words to
    /// hand it: only that agent knows what the change is for, and only it
    /// can reach whichever forge this remote is.
    AwaitingRequest { branch: String, instruction: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeliveryFailure {
    NotReady(Readiness),
    /// The rebase stopped on these files and stays paused in the checkout;
    /// `target_moved` is how many commits the target gained since the task's
    /// base.
    Conflict {
        files: Vec<PathBuf>,
        target_moved: usize,
    },
    GateFailed {
        output: String,
    },
    /// The operator has uncommitted changes to files the task changed.
    Overlap {
        files: Vec<PathBuf>,
    },
    NoRemote,
    Git(String),
}

impl fmt::Display for DeliveryFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotReady(Readiness::Running) => formatter.write_str("nothing to deliver yet"),
            Self::NotReady(Readiness::Uncommitted) => {
                formatter.write_str("the checkout has uncommitted changes")
            }
            Self::NotReady(Readiness::Rebasing { .. }) => {
                formatter.write_str("a rebase is still paused in the checkout")
            }
            Self::NotReady(Readiness::Ready { .. }) => formatter.write_str("ready"),
            Self::Conflict { files, .. } => {
                write!(formatter, "conflicts in {}", join_paths(files))
            }
            Self::GateFailed { .. } => formatter.write_str("the gate failed"),
            Self::Overlap { files } => write!(
                formatter,
                "the primary checkout has uncommitted changes to {}",
                join_paths(files)
            ),
            Self::NoRemote => formatter.write_str("the repository has no `origin` remote"),
            Self::Git(reason) => formatter.write_str(reason),
        }
    }
}

impl std::error::Error for DeliveryFailure {}

fn join_paths(files: &[PathBuf]) -> String {
    files
        .iter()
        .map(|file| file.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The task's checkout directory, when it has one.
pub fn slot_path(primary: &Path, task: &Task) -> Option<PathBuf> {
    let checkout = task.checkout.as_ref()?;
    let path = primary.join(WORKTREES_DIRECTORY).join(checkout.as_str());
    path.is_dir().then_some(path)
}

/// Reads the task's state from its checkout.
///
/// The base is the newest of the recorded one and the target's local tip
/// when the branch already descends from it — the case after an agent
/// finished a paused rebase itself — so `ahead` counts the task's own
/// commits and never the target's.
pub fn readiness(primary: &Path, task: &Task) -> Readiness {
    let Some(slot) = slot_path(primary, task) else {
        return Readiness::Running;
    };
    if let Some(files) = paused_rebase(&slot) {
        return Readiness::Rebasing { files };
    }
    if is_dirty(&slot) {
        return Readiness::Uncommitted;
    }
    let base = effective_base(primary, task);
    let ahead = commits_ahead(primary, &base, &task.branch);
    if ahead == 0 {
        Readiness::Running
    } else {
        Readiness::Ready { ahead, base }
    }
}

fn effective_base(primary: &Path, task: &Task) -> String {
    let local_tip = checkout::tip_of(primary, &task.target);
    if !local_tip.is_empty()
        && local_tip != task.base_commit
        && is_ancestor(primary, &local_tip, &task.branch)
    {
        return local_tip;
    }
    task.base_commit.clone()
}

/// Delivers a ready task according to `policy`, under the repository write
/// lock. Updates `task` to say what happened, whatever that was.
pub fn deliver(
    primary: &Path,
    task: &mut Task,
    policy: &Policy<'_>,
) -> Result<Delivered, DeliveryFailure> {
    uze_git::locked(primary, uze_git::DEFAULT_WRITE_TIMEOUT, || {
        deliver_locked(primary, task, policy)
    })
    .map_err(|error| DeliveryFailure::Git(error.to_string()))?
}

fn deliver_locked(
    primary: &Path,
    task: &mut Task,
    policy: &Policy<'_>,
) -> Result<Delivered, DeliveryFailure> {
    match readiness(primary, task) {
        Readiness::Ready { base, .. } => task.base_commit = base,
        other => return Err(DeliveryFailure::NotReady(other)),
    }
    let slot = slot_path(primary, task).ok_or(DeliveryFailure::NotReady(Readiness::Running))?;
    task.state = TaskState::Integrating;
    let tip = target_tip(primary, task, policy.completion)?;
    rebase_in_slot(primary, &slot, task, &tip)?;
    if let Some(gate) = policy.gate {
        let (passed, output) = run_shell_bounded(&slot, gate, GATE_TIMEOUT);
        if !passed {
            task.state = TaskState::GateFailed;
            return Err(DeliveryFailure::GateFailed { output });
        }
    }
    match policy.completion {
        CompletionBehavior::Handoff => {
            task.state = TaskState::Ready;
            Ok(Delivered::Handoff)
        }
        CompletionBehavior::Merge => {
            let overlap = overlapping_files(primary, &tip, &task.branch);
            if !overlap.is_empty() {
                task.state = TaskState::Ready;
                return Err(DeliveryFailure::Overlap { files: overlap });
            }
            git(primary, &["merge", "--quiet", "--ff-only", &task.branch]).map_err(|reason| {
                task.state = TaskState::Ready;
                DeliveryFailure::Git(format!("fast-forward refused: {reason}"))
            })?;
            task.state = TaskState::Integrated;
            Ok(Delivered::Merged {
                target_tip: checkout::tip_of(primary, &task.target),
            })
        }
        CompletionBehavior::Pr => {
            let published = publish(primary, task)?;
            task.state = TaskState::Ready;
            Ok(published)
        }
    }
}

/// Rebases a live task onto the target when the target has moved, under
/// the same rules as delivery. Only for a clean, ready checkout: never
/// under an agent mid-edit. Returns whether anything moved.
pub fn refresh(
    primary: &Path,
    task: &mut Task,
    completion: CompletionBehavior,
) -> Result<bool, DeliveryFailure> {
    uze_git::locked(primary, uze_git::DEFAULT_WRITE_TIMEOUT, || {
        let slot = slot_path(primary, task).ok_or(DeliveryFailure::NotReady(Readiness::Running))?;
        match readiness(primary, task) {
            Readiness::Ready { base, .. } => task.base_commit = base,
            // Nothing committed yet, but clean: following the target costs
            // the agent nothing.
            Readiness::Running => {}
            other => return Err(DeliveryFailure::NotReady(other)),
        }
        let tip = target_tip(primary, task, completion)?;
        if tip == task.base_commit || is_ancestor(primary, &tip, &task.branch) {
            task.base_commit = tip;
            return Ok(false);
        }
        rebase_in_slot(primary, &slot, task, &tip)?;
        Ok(true)
    })
    .map_err(|error| DeliveryFailure::Git(error.to_string()))?
}

/// The target's tip where the target lives: the remote's after a fetch when
/// delivery publishes a pull request, the local branch otherwise.
pub fn target_tip(
    primary: &Path,
    task: &Task,
    completion: CompletionBehavior,
) -> Result<String, DeliveryFailure> {
    if completion == CompletionBehavior::Pr {
        if !has_remote(primary) {
            return Err(DeliveryFailure::NoRemote);
        }
        let tracking = format!("refs/remotes/{REMOTE}/{}", task.target);
        // An explicit refspec, so the tracking ref moves whatever the
        // remote's configured fetch refspecs say.
        let refspec = format!("+refs/heads/{}:{tracking}", task.target);
        git(primary, &["fetch", "--quiet", REMOTE, &refspec]).map_err(DeliveryFailure::Git)?;
        let tip = checkout::tip_of(primary, &tracking);
        if tip.is_empty() {
            return Err(DeliveryFailure::Git(format!(
                "`{}` does not exist on `{REMOTE}`",
                task.target
            )));
        }
        return Ok(tip);
    }
    let tip = checkout::tip_of(primary, &task.target);
    if tip.is_empty() {
        return Err(DeliveryFailure::Git(format!(
            "`{}` has no commit",
            task.target
        )));
    }
    Ok(tip)
}

fn rebase_in_slot(
    primary: &Path,
    slot: &Path,
    task: &mut Task,
    tip: &str,
) -> Result<(), DeliveryFailure> {
    if tip == task.base_commit || is_ancestor(primary, tip, &task.branch) {
        task.base_commit = tip.to_owned();
        return Ok(());
    }
    let moved = commits_ahead(primary, &task.base_commit, tip);
    match uze_git::write(slot, &["rebase", "--quiet", tip]) {
        Ok(output) if output.is_success() => {
            task.base_commit = tip.to_owned();
            Ok(())
        }
        Ok(output) => {
            if let Some(files) = paused_rebase(slot) {
                task.state = TaskState::Conflicted {
                    files: files.clone(),
                };
                Err(DeliveryFailure::Conflict {
                    files,
                    target_moved: moved,
                })
            } else {
                task.state = TaskState::Ready;
                Err(DeliveryFailure::Git(output.stderr.trim().to_owned()))
            }
        }
        Err(error) => {
            task.state = TaskState::Ready;
            Err(DeliveryFailure::Git(error.to_string()))
        }
    }
}

/// The files a paused rebase stopped on, or `None` when no rebase is
/// paused in `slot`.
pub fn paused_rebase(slot: &Path) -> Option<Vec<PathBuf>> {
    let in_progress = ["rebase-merge", "rebase-apply"].iter().any(|kind| {
        uze_git::read(slot, &["rev-parse", "--git-path", kind])
            .ok()
            .and_then(|output| output.successful().ok())
            .is_some_and(|path| {
                let path = path.trim();
                let path = Path::new(path);
                let absolute = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    slot.join(path)
                };
                absolute.exists()
            })
    });
    if !in_progress {
        return None;
    }
    let files = uze_git::read(slot, &["diff", "--name-only", "--diff-filter=U"])
        .ok()
        .and_then(|output| output.successful().ok())
        .map(|stdout| stdout.lines().map(PathBuf::from).collect())
        .unwrap_or_default();
    Some(files)
}

/// The message written into the owning agent's pane when its task cannot be
/// rebased. One line, so a harness's prompt takes it as one submission.
pub fn conflict_message(task: &Task, files: &[PathBuf], target_moved: usize) -> String {
    format!(
        "Your branch no longer rebases onto {target}: conflicts in {files}. {target} gained \
         {moved} commit{plural} since you started. The rebase is paused in this checkout — \
         resolve the conflicts preserving the intent of your change, run `git rebase \
         --continue`, run the project's checks, and end your turn.",
        target = task.target,
        files = join_paths(files),
        moved = target_moved,
        plural = if target_moved == 1 { "" } else { "s" },
    )
}

/// The message written into the owning agent's pane when the gate refused
/// its rebased commits.
pub fn gate_failure_message(task: &Task, output: &str) -> String {
    let tail: String = output
        .lines()
        .rev()
        .take(12)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join(" | ");
    format!(
        "The project's checks failed on your branch after it was rebased onto {target}. Fix \
         them on this branch, commit, and end your turn. Last lines: {tail}",
        target = task.target,
    )
}

/// The name a branch is published under: the label as a slug, the
/// identifier when the label is one already.
pub fn readable_branch_name(task: &Task) -> String {
    let slug: String = task
        .label
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' || character == '.' {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-').to_owned();
    let slug = if slug.is_empty() {
        task.id.as_str().to_owned()
    } else {
        slug
    };
    format!("{}{slug}", crate::worktree::BRANCH_PREFIX)
}

/// Publishes the branch and says whether the forge already has a request
/// open for it.
///
/// The push is all UZE does here. Opening the request is deliberately not
/// automated: a title and a description are the change's argument, and
/// the agent that wrote the change is the only party holding it — a
/// generated one-liner is a worse request than none. It is also the only
/// half that is not portable, since every forge opens a request its own
/// way, while a push is a push.
fn publish(primary: &Path, task: &mut Task) -> Result<Delivered, DeliveryFailure> {
    let name = task
        .published_as
        .clone()
        .unwrap_or_else(|| readable_branch_name(task));
    let refspec = format!("{}:refs/heads/{name}", task.branch);
    let push = if task.pushed {
        vec![
            "push",
            "--quiet",
            "--force-with-lease",
            REMOTE,
            refspec.as_str(),
        ]
    } else {
        vec!["push", "--quiet", REMOTE, refspec.as_str()]
    };
    git(primary, &push).map_err(DeliveryFailure::Git)?;
    task.pushed = true;
    task.published_as = Some(name.clone());
    task.published_tip = Some(checkout::tip_of(primary, &task.branch));
    if task.published_request.is_none() {
        task.published_request = discover_request(primary, task);
    }
    match task.published_request {
        Some(request) => Ok(Delivered::Published {
            branch: name,
            request,
        }),
        None => Ok(Delivered::AwaitingRequest {
            instruction: open_request_message(task, &name),
            branch: name,
        }),
    }
}

/// The number of the request open for the published branch, asked of the
/// remote itself.
///
/// Forges publish a request's head as a ref, so `ls-remote` answers this
/// with no CLI, no token beyond the one the push already used, and no
/// knowledge of which forge is on the other end: whichever namespace the
/// remote serves is the one that matches. A request is identified by the
/// commit it points at, since a ref under these namespaces carries a
/// number and nothing else.
///
/// `None` is the ordinary answer the first time — no request exists yet —
/// and stays the answer on a forge that publishes no such refs, where the
/// branch is still pushed and the sync still works, only unnumbered.
fn discover_request(primary: &Path, task: &Task) -> Option<u32> {
    let tip = checkout::tip_of(primary, &task.branch);
    if tip.is_empty() {
        return None;
    }
    let listing = uze_git::read(
        primary,
        &[
            "ls-remote",
            REMOTE,
            "refs/pull/*/head",
            "refs/merge-requests/*/head",
        ],
    )
    .ok()?
    .successful()
    .ok()?;
    listing
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .filter(|(sha, _)| *sha == tip)
        .filter_map(|(_, reference)| {
            reference
                .strip_prefix("refs/pull/")
                .or_else(|| reference.strip_prefix("refs/merge-requests/"))?
                .strip_suffix("/head")?
                .parse()
                .ok()
        })
        // Two requests over the same commits is unusual and the newest is
        // the one being worked on; taking the smaller would pin the task
        // to a request somebody already superseded.
        .max()
}

/// The message written into the owning agent's pane when its branch is
/// published and the request is still its to open.
///
/// Names no forge and no tool: the agent is in the repository and knows
/// which one this is, and the projects that reach different forges are
/// the reason this text does not pick one.
pub fn open_request_message(task: &Task, branch: &str) -> String {
    format!(
        "Your branch is published as `{branch}` on `{REMOTE}`, rebased onto `{target}` and past \
         the project's checks. Open a pull request — a merge request, on a forge that calls it \
         that — from `{branch}` against `{target}`, naming and describing it by this project's \
         own convention. Do not merge it and do not integrate the branch yourself: open the \
         request and end your turn.",
        target = task.target,
    )
}

/// Files changed on `branch` since `tip` that the primary checkout has
/// uncommitted changes to — the one case a fast-forward would collide with
/// the operator.
fn overlapping_files(primary: &Path, tip: &str, branch: &str) -> Vec<PathBuf> {
    let changed: Vec<String> = uze_git::read(primary, &["diff", "--name-only", tip, branch])
        .ok()
        .and_then(|output| output.successful().ok())
        .map(|stdout| stdout.lines().map(str::to_owned).collect())
        .unwrap_or_default();
    let dirty: Vec<String> =
        uze_git::read(primary, &["status", "--porcelain", "--untracked-files=no"])
            .ok()
            .and_then(|output| output.successful().ok())
            .map(|stdout| {
                stdout
                    .lines()
                    .filter_map(|line| line.get(3..))
                    .map(|path| path.trim().trim_matches('"').to_owned())
                    .collect()
            })
            .unwrap_or_default();
    changed
        .into_iter()
        .filter(|path| dirty.iter().any(|dirty| dirty == path))
        .map(PathBuf::from)
        .collect()
}

fn is_ancestor(root: &Path, ancestor: &str, descendant: &str) -> bool {
    uze_git::read(root, &["merge-base", "--is-ancestor", ancestor, descendant])
        .is_ok_and(|output| output.is_success())
}

fn has_remote(root: &Path) -> bool {
    uze_git::read(root, &["remote", "get-url", REMOTE]).is_ok_and(|output| output.is_success())
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
    uze_git::write(root, args)
        .map_err(|error| error.to_string())?
        .successful()
        .map(|stdout| stdout.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        checkout::{acquire, tip_of},
        task::{Base, TaskStore},
    };
    use std::fs;
    use uze_testkit::git::Repository;

    const TARGET: &str = "main";

    fn repository(label: &str) -> Repository {
        Repository::new(label)
    }

    /// A task launched in a slot of its own, the way the application does it.
    fn launch(repository: &Repository, store: &mut TaskStore, label: &str) -> Task {
        let primary = repository.root();
        let mut task = Task::new(
            Some(label),
            Base::Ref(TARGET.into()),
            tip_of(primary, TARGET),
            TARGET.into(),
        );
        let acquired = acquire(primary, store, &task, &task.base_commit, None, &[]).unwrap();
        task.checkout = Some(acquired.id);
        store.upsert(task.clone());
        task
    }

    /// The agent commits a file on its branch.
    fn agent_commits(repository: &Repository, task: &Task, file: &str, contents: &str) {
        let slot = slot_path(repository.root(), task).unwrap();
        fs::write(slot.join(file), contents).unwrap();
        repository.git_in(&slot, &["add", "--", file]);
        repository.git_in(&slot, &["commit", "-qm", file]);
    }

    fn handoff() -> Policy<'static> {
        Policy {
            completion: CompletionBehavior::Handoff,
            gate: None,
        }
    }

    fn merge(gate: Option<&str>) -> Policy<'_> {
        Policy {
            completion: CompletionBehavior::Merge,
            gate,
        }
    }

    #[test]
    fn readiness_is_read_from_the_checkout() {
        let repository = repository("landing-readiness");
        let primary = repository.root();
        let mut store = TaskStore::default();
        let task = launch(&repository, &mut store, "readiness");
        assert_eq!(readiness(primary, &task), Readiness::Running);

        let slot = slot_path(primary, &task).unwrap();
        fs::write(slot.join("draft.rs"), "").unwrap();
        assert_eq!(readiness(primary, &task), Readiness::Uncommitted);

        repository.git_in(&slot, &["add", "."]);
        repository.git_in(&slot, &["commit", "-qm", "draft"]);
        assert!(matches!(
            readiness(primary, &task),
            Readiness::Ready { ahead: 1, .. }
        ));
    }

    #[test]
    fn handoff_never_touches_the_target() {
        let repository = repository("landing-handoff");
        let primary = repository.root();
        let mut store = TaskStore::default();
        let mut task = launch(&repository, &mut store, "handoff");
        agent_commits(&repository, &task, "a.rs", "");
        let before = tip_of(primary, TARGET);

        assert_eq!(
            deliver(primary, &mut task, &handoff()),
            Ok(Delivered::Handoff)
        );
        assert_eq!(tip_of(primary, TARGET), before);
        assert_eq!(task.state, TaskState::Ready);
    }

    #[test]
    fn merge_advances_the_target_linearly_after_the_gate() {
        let repository = repository("landing-merge");
        let primary = repository.root();
        let mut store = TaskStore::default();
        let mut task = launch(&repository, &mut store, "merge");
        agent_commits(&repository, &task, "a.rs", "");
        agent_commits(&repository, &task, "b.rs", "");
        // The target moved underneath, without touching the same files.
        repository.commit_file("elsewhere.txt", "moved on");

        let delivered = deliver(
            primary,
            &mut task,
            &merge(Some("test -f a.rs && test -f b.rs")),
        )
        .unwrap();
        assert!(matches!(delivered, Delivered::Merged { .. }));
        assert_eq!(task.state, TaskState::Integrated);
        assert_eq!(tip_of(primary, TARGET), tip_of(primary, &task.branch));
        let log = repository.git(&["log", "--format=%p", "-n", "3"]);
        assert!(
            log.lines()
                .all(|parents| parents.split_whitespace().count() == 1),
            "linear history, no merge commit: {log}"
        );
        assert!(primary.join("a.rs").is_file() && primary.join("elsewhere.txt").is_file());
    }

    /// The gate must see the target's newest state, or it passes on a base
    /// that no longer exists.
    #[test]
    fn the_gate_runs_after_the_rebase_not_before() {
        let repository = repository("landing-gate-order");
        let primary = repository.root();
        let mut store = TaskStore::default();
        let mut task = launch(&repository, &mut store, "gate order");
        agent_commits(&repository, &task, "feature.rs", "");
        repository.commit_file("from-target.txt", "only on the target after launch");

        let outcome = deliver(primary, &mut task, &merge(Some("test -f from-target.txt")));
        assert!(
            outcome.is_ok(),
            "{outcome:?}: the gate saw the rebased tree"
        );
    }

    #[test]
    fn a_gate_failure_leaves_the_target_untouched_and_returns_to_the_owner() {
        let repository = repository("landing-gate-fails");
        let primary = repository.root();
        let mut store = TaskStore::default();
        let mut task = launch(&repository, &mut store, "gate fails");
        agent_commits(&repository, &task, "a.rs", "");
        let before = tip_of(primary, TARGET);

        let failure = deliver(
            primary,
            &mut task,
            &merge(Some("echo 'assertion failed: x'; exit 1")),
        )
        .unwrap_err();
        assert!(
            matches!(&failure, DeliveryFailure::GateFailed { output } if output.contains("assertion failed")),
            "{failure:?}"
        );
        assert_eq!(tip_of(primary, TARGET), before);
        assert_eq!(task.state, TaskState::GateFailed);
        assert!(gate_failure_message(&task, "assertion failed: x").contains("assertion failed"));
    }

    #[test]
    fn a_conflict_leaves_the_rebase_paused_and_the_target_untouched() {
        let repository = repository("landing-conflict");
        let primary = repository.root();
        let mut store = TaskStore::default();
        let mut task = launch(&repository, &mut store, "conflict");
        agent_commits(&repository, &task, "shared.rs", "agent's version\n");
        repository.commit_file("shared.rs", "operator's version\n");
        let before = tip_of(primary, TARGET);

        let failure = deliver(primary, &mut task, &merge(None)).unwrap_err();
        let DeliveryFailure::Conflict {
            files,
            target_moved,
        } = &failure
        else {
            panic!("{failure:?}");
        };
        assert_eq!(files, &[PathBuf::from("shared.rs")]);
        assert_eq!(*target_moved, 1);
        assert_eq!(tip_of(primary, TARGET), before);
        assert!(matches!(task.state, TaskState::Conflicted { .. }));
        let slot = slot_path(primary, &task).unwrap();
        assert!(
            paused_rebase(&slot).is_some(),
            "the rebase waits for the owner"
        );
        let message = conflict_message(&task, files, *target_moved);
        assert!(message.contains("shared.rs") && message.contains("rebase --continue"));
        assert!(
            !message.contains('\n'),
            "one submission for a harness prompt"
        );
        assert_eq!(
            readiness(primary, &task),
            Readiness::Rebasing {
                files: files.clone()
            }
        );
    }

    /// What the agent does after the message above, and what the next
    /// evaluation makes of it.
    #[test]
    fn a_resolved_conflict_reads_as_ready_on_the_next_evaluation() {
        let repository = repository("landing-resolved");
        let primary = repository.root();
        let mut store = TaskStore::default();
        let mut task = launch(&repository, &mut store, "resolved");
        agent_commits(&repository, &task, "shared.rs", "agent's version\n");
        repository.commit_file("shared.rs", "operator's version\n");
        deliver(primary, &mut task, &merge(None)).unwrap_err();

        let slot = slot_path(primary, &task).unwrap();
        fs::write(slot.join("shared.rs"), "both versions\n").unwrap();
        repository.git_in(&slot, &["add", "shared.rs"]);
        repository
            .try_git_in(&slot, &["-c", "core.editor=true", "rebase", "--continue"])
            .unwrap();

        let ready = readiness(primary, &task);
        let Readiness::Ready { ahead, base } = ready else {
            panic!("{ready:?}");
        };
        assert_eq!(ahead, 1, "the agent's commit, not the target's");
        assert_eq!(base, tip_of(primary, TARGET));
        assert!(matches!(
            deliver(primary, &mut task, &merge(None)),
            Ok(Delivered::Merged { .. })
        ));
        assert_eq!(
            fs::read_to_string(primary.join("shared.rs")).unwrap(),
            "both versions\n"
        );
    }

    #[test]
    fn the_second_task_sees_the_first() {
        let repository = repository("landing-sequence");
        let primary = repository.root();
        let mut store = TaskStore::default();
        let mut first = launch(&repository, &mut store, "first");
        let mut second = launch(&repository, &mut store, "second");
        agent_commits(&repository, &first, "first.rs", "");
        agent_commits(&repository, &second, "second.rs", "");

        deliver(primary, &mut first, &merge(None)).unwrap();
        deliver(primary, &mut second, &merge(Some("test -f first.rs"))).unwrap();
        assert!(primary.join("first.rs").is_file() && primary.join("second.rs").is_file());
        assert_eq!(second.base_commit, tip_of(primary, &first.branch));
    }

    #[test]
    fn overlap_with_the_operators_uncommitted_work_refuses_and_writes_nothing() {
        let repository = repository("landing-overlap");
        let primary = repository.root();
        let mut store = TaskStore::default();
        let mut task = launch(&repository, &mut store, "overlap");
        agent_commits(&repository, &task, "README.md", "the agent rewrote it\n");
        fs::write(primary.join("README.md"), "the operator is editing it\n").unwrap();
        let before = tip_of(primary, TARGET);

        let failure = deliver(primary, &mut task, &merge(None)).unwrap_err();
        assert!(
            matches!(&failure, DeliveryFailure::Overlap { files } if files == &[PathBuf::from("README.md")]),
            "{failure:?}"
        );
        assert_eq!(tip_of(primary, TARGET), before);
        assert_eq!(
            fs::read_to_string(primary.join("README.md")).unwrap(),
            "the operator is editing it\n"
        );
        assert_eq!(task.state, TaskState::Ready);
    }

    #[test]
    fn a_task_without_commits_is_not_delivered() {
        let repository = repository("landing-not-ready");
        let primary = repository.root();
        let mut store = TaskStore::default();
        let mut task = launch(&repository, &mut store, "empty");
        assert_eq!(
            deliver(primary, &mut task, &merge(None)),
            Err(DeliveryFailure::NotReady(Readiness::Running))
        );
        let slot = slot_path(primary, &task).unwrap();
        fs::write(slot.join("wip"), "").unwrap();
        assert_eq!(
            deliver(primary, &mut task, &merge(None)),
            Err(DeliveryFailure::NotReady(Readiness::Uncommitted))
        );
    }

    #[test]
    fn a_live_task_follows_the_target_when_clean_and_is_left_alone_when_dirty() {
        let repository = repository("landing-refresh");
        let primary = repository.root();
        let mut store = TaskStore::default();
        let mut task = launch(&repository, &mut store, "refresh");
        agent_commits(&repository, &task, "mine.rs", "");
        repository.commit_file("theirs.rs", "");
        let target = tip_of(primary, TARGET);

        assert_eq!(
            refresh(primary, &mut task, CompletionBehavior::Merge),
            Ok(true)
        );
        assert_eq!(task.base_commit, target);
        let slot = slot_path(primary, &task).unwrap();
        assert!(slot.join("theirs.rs").is_file() && slot.join("mine.rs").is_file());
        assert_eq!(
            refresh(primary, &mut task, CompletionBehavior::Merge),
            Ok(false)
        );

        fs::write(slot.join("editing"), "").unwrap();
        repository.commit_file("more.rs", "");
        assert_eq!(
            refresh(primary, &mut task, CompletionBehavior::Merge),
            Err(DeliveryFailure::NotReady(Readiness::Uncommitted))
        );
        assert!(
            !slot.join("more.rs").exists(),
            "never rebased under an agent mid-edit"
        );
    }

    /// `pr` against a bare remote, with no forge CLI anywhere: the push
    /// is UZE's, the request is the agent's, and the branch is rebased
    /// onto the *remote's* target rather than the operator's local one.
    #[test]
    fn pr_publishes_and_leaves_the_request_to_the_agent() {
        let repository = repository("landing-pr");
        let base = uze_testkit::temp::scratch("landing-pr-remote");
        let origin = base.join("origin.git");
        repository.git(&[
            "init",
            "--quiet",
            "--bare",
            "-b",
            TARGET,
            origin.to_str().unwrap(),
        ]);
        repository.git(&["remote", "add", REMOTE, origin.to_str().unwrap()]);
        repository.git(&["push", "--quiet", "-u", REMOTE, TARGET]);

        let primary = repository.root();
        let mut store = TaskStore::default();
        let mut task = launch(&repository, &mut store, "Fix the auth redirect");
        agent_commits(&repository, &task, "auth.rs", "");
        // The remote target moved: the rebase base must be the remote's tip.
        let other = uze_testkit::temp::scratch("landing-pr-other");
        repository.git(&[
            "clone",
            "--quiet",
            origin.to_str().unwrap(),
            other.to_str().unwrap(),
        ]);
        repository.git_in(&other, &["config", "user.name", "Other"]);
        repository.git_in(&other, &["config", "user.email", "other@uze.invalid"]);
        fs::write(other.join("remote-only.txt"), "").unwrap();
        repository.git_in(&other, &["add", "."]);
        repository.git_in(&other, &["commit", "-qm", "remote moved"]);
        repository.git_in(&other, &["push", "--quiet"]);
        let local_target_before = tip_of(primary, TARGET);

        let policy = Policy {
            completion: CompletionBehavior::Pr,
            gate: Some("test -f remote-only.txt"),
        };
        let Delivered::AwaitingRequest {
            branch,
            instruction,
        } = deliver(primary, &mut task, &policy).unwrap()
        else {
            panic!("no request exists yet, so opening one is the agent's");
        };
        assert_eq!(branch, "agent/fix-the-auth-redirect");
        assert!(
            instruction.contains("agent/fix-the-auth-redirect") && instruction.contains(TARGET),
            "the agent is told which branch and which target: {instruction}"
        );
        assert!(task.pushed);
        assert_eq!(task.published_request, None);
        assert_eq!(
            task.published_as.as_deref(),
            Some("agent/fix-the-auth-redirect")
        );
        let remote_branches = repository.git_in(&other, &["ls-remote", "--heads", REMOTE]);
        assert!(
            remote_branches.contains("refs/heads/agent/fix-the-auth-redirect"),
            "{remote_branches}"
        );
        assert!(
            !remote_branches.contains(&task.branch),
            "the local id never leaves the machine"
        );
        assert_eq!(
            tip_of(primary, TARGET),
            local_target_before,
            "the operator's local target is never pulled"
        );

        // The agent opened it: the forge now publishes the request's head
        // under its own namespace, which is the only thing UZE reads to
        // learn the number — no CLI, no token, no forge named.
        let tip = tip_of(primary, &task.branch);
        repository.git(&[
            "push",
            "--quiet",
            REMOTE,
            &format!("{tip}:refs/pull/11/head"),
        ]);
        assert_eq!(
            deliver(primary, &mut task, &policy).unwrap(),
            Delivered::Published {
                branch: "agent/fix-the-auth-redirect".into(),
                request: 11,
            },
            "a published branch with a request open for it is a sync"
        );
        assert_eq!(task.published_request, Some(11));
        assert_eq!(
            task.published_tip.as_deref(),
            Some(tip_of(primary, &task.branch).as_str()),
            "what the request carries, so a surface can tell a sync that \
             would send something from one that would send nothing"
        );
    }

    /// The same discovery, in the namespace the other family of forges
    /// serves. Nothing but the ref name differs, which is the point.
    #[test]
    fn a_merge_request_is_discovered_the_same_way_a_pull_request_is() {
        let repository = repository("landing-mr");
        let base = uze_testkit::temp::scratch("landing-mr-remote");
        let origin = base.join("origin.git");
        repository.git(&[
            "init",
            "--quiet",
            "--bare",
            "-b",
            TARGET,
            origin.to_str().unwrap(),
        ]);
        repository.git(&["remote", "add", REMOTE, origin.to_str().unwrap()]);
        repository.git(&["push", "--quiet", "-u", REMOTE, TARGET]);

        let primary = repository.root();
        let mut store = TaskStore::default();
        let mut task = launch(&repository, &mut store, "Fix the auth redirect");
        agent_commits(&repository, &task, "auth.rs", "");
        let policy = Policy {
            completion: CompletionBehavior::Pr,
            gate: None,
        };
        deliver(primary, &mut task, &policy).unwrap();
        let tip = tip_of(primary, &task.branch);
        repository.git(&[
            "push",
            "--quiet",
            REMOTE,
            &format!("{tip}:refs/merge-requests/4/head"),
        ]);
        assert_eq!(
            deliver(primary, &mut task, &policy).unwrap(),
            Delivered::Published {
                branch: "agent/fix-the-auth-redirect".into(),
                request: 4,
            }
        );
    }

    #[test]
    fn pr_without_a_remote_is_refused_before_anything_moves() {
        let repository = repository("landing-pr-no-remote");
        let primary = repository.root();
        let mut store = TaskStore::default();
        let mut task = launch(&repository, &mut store, "no remote");
        agent_commits(&repository, &task, "a.rs", "");
        let policy = Policy {
            completion: CompletionBehavior::Pr,
            gate: None,
        };
        assert_eq!(
            deliver(primary, &mut task, &policy),
            Err(DeliveryFailure::NoRemote)
        );
        assert!(!task.pushed);
    }
}
