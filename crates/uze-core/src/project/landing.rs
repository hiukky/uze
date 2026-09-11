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
    /// What runs in the task's checkout on the rebased commits, in order;
    /// the first non-zero exit refuses delivery.
    pub gate: &'a [String],
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
        /// The step that failed — named, because a gate of several steps
        /// that reports only output leaves the reader diffing the output
        /// against the manifest to find out which one.
        command: String,
        output: String,
    },
    /// The operator has uncommitted changes to files the task changed.
    Overlap {
        files: Vec<PathBuf>,
    },
    /// The target already carries the branch's work under commits of its
    /// own — a squash or rebase merge made elsewhere.
    AlreadyDelivered,
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
            Self::GateFailed { command, .. } => write!(formatter, "the gate failed at `{command}`"),
            Self::Overlap { files } => write!(
                formatter,
                "the primary checkout has uncommitted changes to {}",
                join_paths(files)
            ),
            Self::AlreadyDelivered => formatter.write_str("the target already carries this work"),
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

/// What the remote holds for a task's branch.
///
/// Publication is a Git fact, exactly like readiness. `git push` writes
/// `refs/remotes/origin/<name>` whoever ran it and from whichever
/// checkout, so a branch its own agent pushed by hand is as published as
/// one UZE pushed, and commits the agent added to an open request are as
/// synced. Reading UZE's record of its own pushes instead made every
/// surface report UZE's history rather than the remote's state — a
/// button offering to send what the remote already had.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Publication {
    /// The name the branch is published under on the remote.
    pub branch: String,
    /// The commit the remote-tracking ref points at.
    pub tip: String,
}

/// How long the remote may go unasked about a request that does not exist
/// yet. The branch is pushed and the agent opens the request moments
/// later, so the answer changes on a human's clock, not a machine's — and
/// this is the only publication question that leaves the machine.
const REQUEST_INTERVAL: Duration = Duration::from_secs(60);

/// What the remote holds for this task's branch, or `None` when the
/// branch is not on the remote at all.
///
/// The name UZE published under is tried first and the branch's own name
/// second: an unnamed task leaves under a derived name that nothing but
/// [`Task::published_as`] ties back to it, while a task whose agent
/// pushed on its own is on the remote under the only name it has.
pub fn publication(primary: &Path, task: &Task) -> Option<Publication> {
    task.published_as
        .iter()
        .chain(std::iter::once(&task.branch))
        .find_map(|branch| {
            let tip = checkout::tip_of(primary, &format!("refs/remotes/{REMOTE}/{branch}"));
            (!tip.is_empty()).then(|| Publication {
                branch: branch.clone(),
                tip,
            })
        })
}

/// Asks the remote whether a request is open for this task's published
/// branch, and records the number when one is.
///
/// Called on the evaluation pass rather than only from [`publish`],
/// because the request is not always UZE's doing: an agent told to push
/// and open the request itself leaves a forge that UZE would otherwise
/// never learn about, and the button would go on offering to publish a
/// branch that already has a request open. Gated three ways so the round
/// trip stays rare — a published branch, no number yet, and at most one
/// question per [`REQUEST_INTERVAL`] — and it stops for good the moment
/// it is answered.
pub fn observe_request(primary: &Path, task: &mut Task) {
    if task.published_request.is_some() {
        return;
    }
    let now = crate::task::now_unix();
    let asked_recently = task
        .request_asked_at_unix
        .is_some_and(|asked| now.saturating_sub(asked) < REQUEST_INTERVAL.as_secs());
    if asked_recently {
        return;
    }
    let Some(published) = publication(primary, task) else {
        return;
    };
    task.request_asked_at_unix = Some(now);
    task.published_request = discover_request(primary, &published.tip);
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

/// What bringing the local target in line with the remote's did.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TargetSync {
    /// Nothing to sync against: no remote, or the target is not on it yet.
    Unpublished,
    /// The local target already carries everything the remote's does.
    Current,
    /// The local target was moved forward onto the remote's tip.
    FastForwarded { commits: usize },
    /// The remote is ahead and the local target stayed where it was.
    Stalled { behind: usize, reason: String },
}

impl TargetSync {
    /// What the operator has to be told, if anything: an agent about to be
    /// placed on a target that could not be brought up to date starts
    /// behind the work everyone else is already on, and will hear about it
    /// as a conflict much later.
    pub fn concern(&self, target: &str) -> Option<String> {
        match self {
            Self::Stalled { behind, reason } if *behind > 0 => {
                let plural = if *behind == 1 { "" } else { "s" };
                Some(format!(
                    "`{target}` is {behind} commit{plural} behind `{REMOTE}` and could not be \
                     moved: {reason}. This agent starts from the local tip."
                ))
            }
            Self::Stalled { reason, .. } => Some(format!(
                "`{REMOTE}` could not be read: {reason}. This agent starts from the local tip, \
                 which may be behind."
            )),
            _ => None,
        }
    }
}

/// Brings the local target in line with the remote's before anything is
/// branched from it, by fast-forward and never by anything else.
///
/// An agent is placed on the target's tip, and every judgement made about
/// its work afterwards — what it is ahead of, whether its slot holds
/// anything, what it rebases onto — is made against that same local branch.
/// Left unfetched, the branch drifts a whole day's merges behind the target
/// everyone else is on: the agent starts from history nobody has, and the
/// divergence surfaces as conflicts in a request already opened, which is
/// the most expensive place to learn it.
///
/// Fast-forward only, so this can never lose or reorder an operator's own
/// commits: a target that has commits the remote lacks is left exactly
/// where it is and reported, as is one Git refuses to move because the
/// primary checkout has local modifications in the way.
pub fn sync_target(primary: &Path, target: &str) -> TargetSync {
    if !has_remote(primary) {
        return TargetSync::Unpublished;
    }
    uze_git::locked(primary, uze_git::DEFAULT_WRITE_TIMEOUT, || {
        sync_target_locked(primary, target)
    })
    .unwrap_or_else(|error| TargetSync::Stalled {
        behind: 0,
        reason: error.to_string(),
    })
}

fn sync_target_locked(primary: &Path, target: &str) -> TargetSync {
    let tracking = format!("refs/remotes/{REMOTE}/{target}");
    // An explicit refspec, so the tracking ref moves whatever the remote's
    // configured fetch refspecs say.
    let refspec = format!("+refs/heads/{target}:{tracking}");
    if let Err(reason) = git(primary, &["fetch", "--quiet", REMOTE, &refspec]) {
        return TargetSync::Stalled { behind: 0, reason };
    }
    if checkout::tip_of(primary, &tracking).is_empty() {
        return TargetSync::Unpublished;
    }
    let behind = commits_ahead(primary, target, &tracking);
    if behind == 0 {
        return TargetSync::Current;
    }
    let local_tip = checkout::tip_of(primary, target);
    if !is_ancestor(primary, &local_tip, &tracking) {
        return TargetSync::Stalled {
            behind,
            reason: format!("it carries commits `{REMOTE}` does not"),
        };
    }
    match fast_forward(primary, target, &tracking) {
        Ok(()) => TargetSync::FastForwarded { commits: behind },
        Err(reason) => TargetSync::Stalled { behind, reason },
    }
}

/// Moves the local target onto `tracking`. Through the working tree when
/// the operator is standing on the target — Git's own fast-forward, which
/// refuses rather than overwrite anything uncommitted in the way — and by
/// moving the ref when they are not, which Git refuses in turn while
/// another checkout has the branch.
fn fast_forward(primary: &Path, target: &str, tracking: &str) -> Result<(), String> {
    let args = if checkout::current_branch(primary).as_deref() == Some(target) {
        vec!["merge", "--quiet", "--ff-only", tracking]
    } else {
        vec!["branch", "--quiet", "--force", target, tracking]
    };
    git(primary, &args).map(|_| ())
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
    // Merged elsewhere — squashed on the forge before the local target
    // heard of it — the work is in the tip under commits of its own, and
    // rebasing would replay it onto itself.
    if checkout::is_integrated(primary, &tip, &task.branch) {
        task.state = TaskState::Integrated;
        return Err(DeliveryFailure::AlreadyDelivered);
    }
    rebase_in_slot(primary, &slot, task, &tip)?;
    for step in policy.gate {
        let (passed, output) = run_shell_bounded(&slot, step, GATE_TIMEOUT);
        if !passed {
            task.state = TaskState::GateFailed;
            return Err(DeliveryFailure::GateFailed {
                command: step.clone(),
                output,
            });
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
///
/// Always the local target, whatever the completion behaviour: this runs
/// whenever a pane goes quiet, and a fetch on that cadence would be paid
/// for on every tick of every task. `pr` reaches the remote's tip twice
/// anyway — where the target is brought in line before an agent is placed
/// on it, and at delivery, which is the one that decides.
pub fn refresh(primary: &Path, task: &mut Task) -> Result<bool, DeliveryFailure> {
    uze_git::locked(primary, uze_git::DEFAULT_WRITE_TIMEOUT, || {
        let slot = slot_path(primary, task).ok_or(DeliveryFailure::NotReady(Readiness::Running))?;
        match readiness(primary, task) {
            Readiness::Ready { base, .. } => task.base_commit = base,
            // Nothing committed yet, but clean: following the target costs
            // the agent nothing.
            Readiness::Running => {}
            other => return Err(DeliveryFailure::NotReady(other)),
        }
        let tip = target_tip(primary, task, CompletionBehavior::Handoff)?;
        if tip == task.base_commit || is_ancestor(primary, &tip, &task.branch) {
            task.base_commit = tip;
            return Ok(false);
        }
        // The target already holds this work under commits of its own — a
        // squash or rebase merge. Replaying it there conflicts with itself.
        if checkout::is_integrated(primary, &tip, &task.branch) {
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

/// Ends a task whose work the target already carries, when nothing of the
/// agent's is at stake. Returns whether it was ended.
///
/// A rebase paused in its checkout can only be replaying that work onto
/// itself, and is abandoned: the branch ref does not move until a rebase
/// finishes, so it still names every commit the agent made, and aborting
/// puts the checkout back exactly where the agent left it. A checkout with
/// changes of its own is never touched.
///
/// Only for a task that has something to settle — one parked, or with a
/// rebase paused: a branch with no commits of its own reads as integrated
/// too, and a live agent that has committed nothing yet is not done.
pub fn settle_delivered(primary: &Path, task: &mut Task) -> bool {
    if !checkout::branch_exists(primary, &task.branch)
        || !checkout::is_integrated(primary, &task.target, &task.branch)
    {
        return false;
    }
    if let Some(slot) = slot_path(primary, task) {
        if paused_rebase(&slot).is_some() && !abort_rebase(primary, &slot) {
            return false;
        }
        if is_dirty(&slot) {
            return false;
        }
    }
    task.state = TaskState::Integrated;
    true
}

fn abort_rebase(primary: &Path, slot: &Path) -> bool {
    uze_git::locked(primary, uze_git::DEFAULT_WRITE_TIMEOUT, || {
        uze_git::write(slot, &["rebase", "--abort"]).is_ok_and(|output| output.is_success())
    })
    .unwrap_or(false)
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
pub fn gate_failure_message(task: &Task, command: &str, output: &str) -> String {
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
        "The project's checks failed on your branch after it was rebased onto {target}: \
         `{command}`. Fix them on this branch, commit, and end your turn. Last lines: {tail}",
        target = task.target,
    )
}

/// The name a branch is published under when nobody named the work.
///
/// Sourced from the first commit on the branch rather than from the task's
/// label, because the label of an unnamed task is its generated
/// identifier — and a pull request titled `agent/zulqgq` teaches a
/// reviewer nothing. The agent that wrote that commit is the only party
/// that held the intent, and its subject line is the one place that intent
/// was already written down.
///
/// A Conventional Commits subject gives up its type as the branch's own
/// (`feat(ui): one layout file` -> `feat/one-layout-file`); anything else
/// keeps the project's prefix. This is the net under every other
/// mechanism, not the mechanism: a named task never reaches it.
pub fn readable_branch_name(primary: &Path, task: &Task) -> String {
    let fallback = || format!("{}{}", crate::worktree::BRANCH_PREFIX, task.id.as_str());
    let Some((kind, subject)) = commit_derived_halves(primary, task) else {
        return fallback();
    };
    match kind {
        Some(kind) => format!("{kind}/{subject}"),
        None => format!("{}{subject}", crate::worktree::BRANCH_PREFIX),
    }
}

/// The name the work would take from its own first commit, judged against
/// what the project accepts — `None` when nothing usable can be derived.
///
/// This is the automatic half of naming, and it is deliberately the
/// *later* half: it runs once the work has a commit, because until then
/// there is nothing to name it after. The agent naming its own work
/// arrives earlier and therefore wins, which is the whole of the
/// precedence rule — no ladder, no overwriting.
///
/// Judged rather than trusted: a derived name that the declared vocabulary
/// would refuse from an agent is not one UZE may write behind its back, so
/// a project whose types the commit does not match keeps the generated
/// name and says nothing.
pub fn derived_name(
    primary: &Path,
    task: &Task,
    vocabulary: &crate::worktree::BranchVocabulary,
) -> Option<String> {
    let (kind, subject) = commit_derived_halves(primary, task)?;
    let proposed = match kind {
        Some(kind) => format!("{kind}/{subject}"),
        None => subject,
    };
    vocabulary.accept(&proposed).ok()
}

/// The type and subject the branch's first commit yields, if any. A
/// Conventional Commits subject gives up its type (`feat(ui): one layout
/// file` -> `feat` + `one-layout-file`); anything else yields a subject
/// alone.
fn commit_derived_halves(primary: &Path, task: &Task) -> Option<(Option<String>, String)> {
    let subject = first_commit_subject(primary, task)?;
    let (kind, rest) = match subject.split_once(':') {
        Some((head, rest)) if !head.contains(' ') => {
            let kind = head.split('(').next().unwrap_or(head).trim_end_matches('!');
            (Some(slug(kind)).filter(|kind| !kind.is_empty()), rest)
        }
        _ => (None, subject.as_str()),
    };
    let subject = shorten(&slug(rest));
    (!subject.is_empty()).then_some((kind, subject))
}

/// The subject of the oldest commit the branch carries beyond its base.
fn first_commit_subject(primary: &Path, task: &Task) -> Option<String> {
    let range = format!("{}..{}", task.base_commit, task.branch);
    let listing = uze_git::read(primary, &["log", "--format=%s", "--reverse", &range])
        .ok()?
        .successful()
        .ok()?;
    listing
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_owned)
}

/// Lowercase, non-alphanumerics collapsed to single hyphens, trimmed.
fn slug(text: &str) -> String {
    let mut slug = String::new();
    let mut pending = false;
    for character in text.chars() {
        if character.is_ascii_alphanumeric() {
            if pending && !slug.is_empty() {
                slug.push('-');
            }
            pending = false;
            slug.extend(character.to_lowercase());
        } else {
            pending = true;
        }
    }
    slug.trim_matches('-').to_owned()
}

/// Cut at a word boundary, so a long subject reads as a name rather than
/// as a truncation.
fn shorten(slug: &str) -> String {
    let limit = crate::worktree::SUBJECT_MAX_CHARS;
    if slug.chars().count() <= limit {
        return slug.to_owned();
    }
    let cut: String = slug.chars().take(limit).collect();
    match cut.rfind('-') {
        Some(boundary) if boundary > 0 => cut[..boundary].to_owned(),
        _ => cut,
    }
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
    let published = publication(primary, task);
    let name = published
        .as_ref()
        .map(|published| published.branch.clone())
        .or_else(|| task.published_as.clone())
        .unwrap_or_else(|| readable_branch_name(primary, task));
    let refspec = format!("{}:refs/heads/{name}", task.branch);
    // A branch already on the remote is one a delivery has since rebased,
    // so its history no longer descends from what the remote holds and a
    // plain push is refused. Whether it is there is asked of Git rather
    // than remembered: an agent that pushed the branch itself left UZE no
    // record to remember, and the refusal landed on the operator as a
    // failed delivery. `--force-with-lease` reads the same
    // remote-tracking ref this did, so the two agree on what is being
    // overwritten.
    let push = if published.is_some() {
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
    task.published_as = Some(name.clone());
    if task.published_request.is_none() {
        task.published_request =
            discover_request(primary, &checkout::tip_of(primary, &task.branch));
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
fn discover_request(primary: &Path, tip: &str) -> Option<u32> {
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
    /// Commits with a subject of its own — what the publish-time fallback
    /// reads, since the branch is named from the work rather than from
    /// anything said at launch.
    fn agent_commits_saying(repository: &Repository, task: &Task, file: &str, subject: &str) {
        let slot = slot_path(repository.root(), task).unwrap();
        fs::write(slot.join(file), "").unwrap();
        repository.git_in(&slot, &["add", "--", file]);
        repository.git_in(&slot, &["commit", "-qm", subject]);
    }

    fn agent_commits(repository: &Repository, task: &Task, file: &str, contents: &str) {
        let slot = slot_path(repository.root(), task).unwrap();
        fs::write(slot.join(file), contents).unwrap();
        repository.git_in(&slot, &["add", "--", file]);
        repository.git_in(&slot, &["commit", "-qm", file]);
    }

    fn handoff() -> Policy<'static> {
        Policy {
            completion: CompletionBehavior::Handoff,
            gate: &[],
        }
    }

    fn merge(gate: &[String]) -> Policy<'_> {
        Policy {
            completion: CompletionBehavior::Merge,
            gate,
        }
    }

    fn steps(commands: &[&str]) -> Vec<String> {
        commands.iter().map(|step| (*step).to_owned()).collect()
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
            &merge(&steps(&["test -f a.rs && test -f b.rs"])),
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

        let outcome = deliver(
            primary,
            &mut task,
            &merge(&steps(&["test -f from-target.txt"])),
        );
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
            &merge(&steps(&["echo 'assertion failed: x'; exit 1"])),
        )
        .unwrap_err();
        assert!(
            matches!(&failure, DeliveryFailure::GateFailed { output, .. } if output.contains("assertion failed")),
            "{failure:?}"
        );
        assert_eq!(tip_of(primary, TARGET), before);
        assert_eq!(task.state, TaskState::GateFailed);
        assert!(
            gate_failure_message(&task, "cargo test", "assertion failed: x")
                .contains("assertion failed")
        );
    }

    /// A gate of several steps must say which one refused the delivery:
    /// the agent that has to fix it reads this message, not the manifest.
    #[test]
    fn a_multi_step_gate_stops_at_the_first_failure_and_names_it() {
        let repository = repository("landing-gate-steps");
        let primary = repository.root();
        let mut store = TaskStore::default();
        let mut task = launch(&repository, &mut store, "gate steps");
        agent_commits(&repository, &task, "a.rs", "");
        let slot = slot_path(primary, &task).unwrap();

        let failure = deliver(
            primary,
            &mut task,
            &merge(&steps(&[
                "touch ran-first",
                "echo 'lint: 3 problems' >&2; exit 1",
                "touch never-ran",
            ])),
        )
        .unwrap_err();

        let DeliveryFailure::GateFailed { command, output } = &failure else {
            panic!("expected a failed gate, got {failure:?}");
        };
        assert!(command.contains("lint"), "{command}");
        assert!(output.contains("3 problems"), "{output}");
        assert!(failure.to_string().contains("`echo 'lint"), "{failure}");
        assert!(slot.join("ran-first").exists(), "the first step ran");
        assert!(
            !slot.join("never-ran").exists(),
            "a step after the failure must not run"
        );
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

        let failure = deliver(primary, &mut task, &merge(&[])).unwrap_err();
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
        deliver(primary, &mut task, &merge(&[])).unwrap_err();

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
            deliver(primary, &mut task, &merge(&[])),
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

        deliver(primary, &mut first, &merge(&[])).unwrap();
        deliver(primary, &mut second, &merge(&steps(&["test -f first.rs"]))).unwrap();
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

        let failure = deliver(primary, &mut task, &merge(&[])).unwrap_err();
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
            deliver(primary, &mut task, &merge(&[])),
            Err(DeliveryFailure::NotReady(Readiness::Running))
        );
        let slot = slot_path(primary, &task).unwrap();
        fs::write(slot.join("wip"), "").unwrap();
        assert_eq!(
            deliver(primary, &mut task, &merge(&[])),
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

        assert_eq!(refresh(primary, &mut task), Ok(true));
        assert_eq!(task.base_commit, target);
        let slot = slot_path(primary, &task).unwrap();
        assert!(slot.join("theirs.rs").is_file() && slot.join("mine.rs").is_file());
        assert_eq!(refresh(primary, &mut task), Ok(false));

        fs::write(slot.join("editing"), "").unwrap();
        repository.commit_file("more.rs", "");
        assert_eq!(
            refresh(primary, &mut task),
            Err(DeliveryFailure::NotReady(Readiness::Uncommitted))
        );
        assert!(
            !slot.join("more.rs").exists(),
            "never rebased under an agent mid-edit"
        );
    }

    /// A repository with a bare `origin` behind it and a second checkout
    /// of that remote, standing in for whoever else pushes to it.
    fn published(label: &str) -> (Repository, PathBuf) {
        let repository = repository(label);
        let base = uze_testkit::temp::scratch(&format!("{label}-remote"));
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
        let other = base.join("other");
        repository.git(&[
            "clone",
            "--quiet",
            origin.to_str().unwrap(),
            other.to_str().unwrap(),
        ]);
        repository.git_in(&other, &["config", "user.name", "Other"]);
        repository.git_in(&other, &["config", "user.email", "other@uze.invalid"]);
        (repository, other)
    }

    /// What someone else merged, pushed from their own checkout.
    fn push_from(repository: &Repository, other: &Path, file: &str) -> String {
        fs::write(other.join(file), "").unwrap();
        repository.git_in(other, &["add", "."]);
        repository.git_in(other, &["commit", "-qm", file]);
        repository.git_in(other, &["push", "--quiet"]);
        repository.git_in(other, &["rev-parse", "HEAD"])
    }

    /// The reason this exists: every agent is placed on the local target,
    /// and a local target nobody fetched is a day of merges behind the one
    /// the rest of the team is on.
    #[test]
    fn the_local_target_is_fast_forwarded_onto_the_remotes() {
        let (repository, other) = published("landing-sync");
        let primary = repository.root();
        let pushed = push_from(&repository, &other, "merged-by-someone-else.rs");

        assert_eq!(
            sync_target(primary, TARGET),
            TargetSync::FastForwarded { commits: 1 }
        );
        assert_eq!(tip_of(primary, TARGET), pushed);
        assert!(
            primary.join("merged-by-someone-else.rs").is_file(),
            "the operator's own checkout is at the target it now names"
        );
        assert_eq!(
            sync_target(primary, TARGET),
            TargetSync::Current,
            "nothing moved the second time"
        );
    }

    /// Fast-forward only: an operator's unpushed commit is never rewound,
    /// reordered or merged into, and the placement says so instead.
    #[test]
    fn a_target_carrying_its_own_commits_is_left_alone_and_reported() {
        let (repository, other) = published("landing-sync-diverged");
        let primary = repository.root();
        push_from(&repository, &other, "theirs.rs");
        let mine = repository.commit_file("mine.rs", "");

        let sync = sync_target(primary, TARGET);
        assert!(
            matches!(sync, TargetSync::Stalled { behind: 1, .. }),
            "the remote is ahead and the local target cannot be moved: {sync:?}"
        );
        assert_eq!(tip_of(primary, TARGET), mine, "nothing was rewritten");
        assert!(
            sync.concern(TARGET)
                .is_some_and(|concern| concern.contains(TARGET)),
            "and an agent placed on it is told"
        );
    }

    #[test]
    fn a_repository_with_no_remote_has_nothing_to_sync_against() {
        let repository = repository("landing-sync-local");
        assert_eq!(
            sync_target(repository.root(), TARGET),
            TargetSync::Unpublished
        );
        assert_eq!(sync_target(repository.root(), TARGET).concern(TARGET), None);
    }

    /// A named task publishes under the name it already has: the
    /// publish-time derivation is a net under everything else, never a
    /// second naming that overrules the agent's own.
    #[test]
    fn a_named_task_publishes_under_its_own_name() {
        let repository = repository("landing-named-publish");
        let primary = repository.root();
        let mut store = TaskStore::default();
        let mut task = launch(&repository, &mut store, "anything");
        agent_commits_saying(
            &repository,
            &task,
            "auth.rs",
            "fix(auth): stop the redirect loop",
        );
        let slot = slot_path(primary, &task).unwrap();
        repository.git_in(&slot, &["branch", "--move", "fix/chosen-by-the-agent"]);
        task.take_name("fix/chosen-by-the-agent".to_owned());

        assert_eq!(
            readable_branch_name(primary, &task),
            "fix/stop-the-redirect-loop",
            "the derivation still has an answer of its own"
        );
        assert!(
            task.is_named(),
            "but the task carries a name, so publish never asks for it"
        );
    }

    /// A subject with no conventional type keeps UZE's own prefix rather
    /// than inventing one the project never declared.
    #[test]
    fn a_subject_without_a_type_keeps_the_prefix() {
        let repository = repository("landing-plain-subject");
        let primary = repository.root();
        let mut store = TaskStore::default();
        let task = launch(&repository, &mut store, "anything");
        agent_commits_saying(&repository, &task, "auth.rs", "make the thing work");

        assert_eq!(
            readable_branch_name(primary, &task),
            "agent/make-the-thing-work"
        );
    }

    /// Nothing committed, nothing to read: the identifier is the honest
    /// answer rather than an invented word.
    #[test]
    fn a_branch_with_no_commits_falls_back_to_the_identifier() {
        let repository = repository("landing-no-commits");
        let primary = repository.root();
        let mut store = TaskStore::default();
        let task = launch(&repository, &mut store, "anything");

        assert_eq!(
            readable_branch_name(primary, &task),
            format!("agent/{}", task.id)
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
        // The published name comes from the *work*, not from the launch
        // prompt: this is the first commit's subject, which is the only
        // place the agent wrote down what it was doing.
        agent_commits_saying(
            &repository,
            &task,
            "auth.rs",
            "fix(auth): stop the redirect loop",
        );
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
            gate: &steps(&["test -f remote-only.txt"]),
        };
        let Delivered::AwaitingRequest {
            branch,
            instruction,
        } = deliver(primary, &mut task, &policy).unwrap()
        else {
            panic!("no request exists yet, so opening one is the agent's");
        };
        assert_eq!(branch, "fix/stop-the-redirect-loop");
        assert!(
            instruction.contains("fix/stop-the-redirect-loop") && instruction.contains(TARGET),
            "the agent is told which branch and which target: {instruction}"
        );
        assert_eq!(
            publication(primary, &task).map(|published| published.branch),
            Some("fix/stop-the-redirect-loop".to_owned()),
            "the branch is on the remote, and that is read from Git"
        );
        assert_eq!(task.published_request, None);
        assert_eq!(
            task.published_as.as_deref(),
            Some("fix/stop-the-redirect-loop")
        );
        let remote_branches = repository.git_in(&other, &["ls-remote", "--heads", REMOTE]);
        assert!(
            remote_branches.contains("refs/heads/fix/stop-the-redirect-loop"),
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
                branch: "fix/stop-the-redirect-loop".into(),
                request: 11,
            },
            "a published branch with a request open for it is a sync"
        );
        assert_eq!(task.published_request, Some(11));
        assert_eq!(
            publication(primary, &task).map(|published| published.tip),
            Some(tip_of(primary, &task.branch)),
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
        agent_commits_saying(
            &repository,
            &task,
            "auth.rs",
            "fix(auth): stop the redirect loop",
        );
        let policy = Policy {
            completion: CompletionBehavior::Pr,
            gate: &[],
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
                branch: "fix/stop-the-redirect-loop".into(),
                request: 4,
            }
        );
    }

    /// The whole point of reading publication from Git: an agent told to
    /// push and open the request itself does everything UZE's `publish`
    /// would have done, and nothing about that reaches UZE's records. Read
    /// from the record, the branch looked unpublished and the button went
    /// on offering to send commits the remote already had.
    #[test]
    fn a_branch_its_own_agent_pushed_is_published_and_in_sync() {
        let (repository, _other) = published("landing-agent-push");
        let primary = repository.root();
        let mut store = TaskStore::default();
        let task = launch(&repository, &mut store, "agent push");
        agent_commits(&repository, &task, "a.rs", "");
        assert_eq!(
            publication(primary, &task),
            None,
            "nothing is on the remote yet"
        );

        let slot = slot_path(primary, &task).unwrap();
        repository.git_in(&slot, &["push", "--quiet", REMOTE, "HEAD"]);

        let published = publication(primary, &task).expect("the agent's own push is a push");
        assert_eq!(published.branch, task.branch);
        assert_eq!(published.tip, tip_of(primary, &task.branch));
        assert_eq!(
            commits_ahead(primary, &published.tip, &task.branch),
            0,
            "nothing is left to sync"
        );

        agent_commits(&repository, &task, "b.rs", "");
        assert_eq!(
            commits_ahead(
                primary,
                &publication(primary, &task).unwrap().tip,
                &task.branch
            ),
            1,
            "and a commit made after the push is one commit to sync"
        );
    }

    /// The other half the agent can do alone. UZE learns the number from
    /// the remote on the pass that already runs, so a request opened
    /// outside a delivery is still the request this branch has.
    #[test]
    fn a_request_the_agent_opened_is_discovered_on_the_evaluation_pass() {
        let (repository, _other) = published("landing-agent-request");
        let primary = repository.root();
        let mut store = TaskStore::default();
        let mut task = launch(&repository, &mut store, "agent request");
        agent_commits(&repository, &task, "a.rs", "");

        observe_request(primary, &mut task);
        assert_eq!(
            task.published_request, None,
            "an unpublished branch is never asked about"
        );
        assert_eq!(
            task.request_asked_at_unix, None,
            "and the round trip is not spent"
        );

        let slot = slot_path(primary, &task).unwrap();
        repository.git_in(&slot, &["push", "--quiet", REMOTE, "HEAD"]);
        let tip = tip_of(primary, &task.branch);
        repository.git(&[
            "push",
            "--quiet",
            REMOTE,
            &format!("{tip}:refs/merge-requests/7/head"),
        ]);

        observe_request(primary, &mut task);
        assert_eq!(task.published_request, Some(7));
        assert!(task.request_asked_at_unix.is_some());
    }

    /// The question that leaves the machine is asked on a clock, and stops
    /// being asked at all once it has an answer.
    #[test]
    fn the_remote_is_asked_about_a_missing_request_at_most_once_a_minute() {
        let (repository, _other) = published("landing-request-clock");
        let primary = repository.root();
        let mut store = TaskStore::default();
        let mut task = launch(&repository, &mut store, "request clock");
        agent_commits(&repository, &task, "a.rs", "");
        let slot = slot_path(primary, &task).unwrap();
        repository.git_in(&slot, &["push", "--quiet", REMOTE, "HEAD"]);

        observe_request(primary, &mut task);
        assert_eq!(task.published_request, None, "no request is open");
        let asked = task.request_asked_at_unix.expect("the remote was asked");

        // The request appears, but the minute has not passed.
        let tip = tip_of(primary, &task.branch);
        repository.git(&[
            "push",
            "--quiet",
            REMOTE,
            &format!("{tip}:refs/pull/9/head"),
        ]);
        observe_request(primary, &mut task);
        assert_eq!(task.published_request, None);
        assert_eq!(task.request_asked_at_unix, Some(asked));

        task.request_asked_at_unix = Some(asked - REQUEST_INTERVAL.as_secs());
        observe_request(primary, &mut task);
        assert_eq!(task.published_request, Some(9));

        // Answered, and never asked again: the number does not change.
        repository.git(&["push", "--quiet", REMOTE, ":refs/pull/9/head"]);
        task.request_asked_at_unix = None;
        observe_request(primary, &mut task);
        assert_eq!(task.published_request, Some(9));
        assert_eq!(task.request_asked_at_unix, None, "nothing was asked");
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
            gate: &[],
        };
        assert_eq!(
            deliver(primary, &mut task, &policy),
            Err(DeliveryFailure::NoRemote)
        );
        assert_eq!(publication(primary, &task), None);
    }
}
