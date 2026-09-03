//! Isolated checkouts as slots.
//!
//! A slot is a long-lived working tree under the primary checkout's fixed
//! isolation directory, named by an identifier that never changes, and
//! reused by one task after another. Reuse is what makes isolation cheap:
//! ignored artifacts — build caches, dependency directories — survive from
//! one task to the next, and the number of directories is bounded by peak
//! concurrency rather than by history.
//!
//! Nothing here is persisted on its own. A slot's state is derived from two
//! sources that already exist: the directories Git registers as worktrees,
//! and the tasks recorded for the project. That is also why adoption is
//! cheap — a directory nobody recorded is a slot whose task was never
//! written down, and its Git state says whether it holds work.
//!
//! Nothing that can hold work is removed here on any automatic path. A
//! dirty tree, or a branch with commits the target lacks, is parked; the
//! two removals that are safe — a branch fully contained in the target, and
//! the *directory* of a clean slot idle beyond an age, its branch kept —
//! are the only ones offered.

use std::{
    fmt, fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use serde::{Deserialize, Serialize};

use crate::{
    task::{Base, Task, TaskId, TaskState, TaskStore},
    worktree::{BRANCH_PREFIX, WORKTREES_DIRECTORY},
};

/// A clean slot nobody has used for this long may lose its directory.
pub const IDLE_SLOT_AGE: Duration = Duration::from_secs(14 * 24 * 60 * 60);

/// The generated, immutable name of a slot — the directory under the
/// isolation directory. Never derived from a task or a label, so a slot
/// outlives every task that runs in it. A legacy checkout keeps the name it
/// already has.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CheckoutId(String);

impl CheckoutId {
    pub fn generate() -> Self {
        Self(crate::task::generated_identifier(b"checkout"))
    }

    pub fn adopted(name: &str) -> Self {
        Self(name.to_owned())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CheckoutId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SlotState {
    /// A live task runs here.
    Occupied { task: TaskId },
    /// Clean, and everything on its branch is in the target or was
    /// declared done: the next agent may take it.
    Free,
    /// Holds work nobody delivered: uncommitted changes, or commits the
    /// target lacks. Only the operator moves it on.
    Parked,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Slot {
    pub id: CheckoutId,
    pub path: PathBuf,
    pub branch: Option<String>,
    pub state: SlotState,
}

/// What acquiring a slot produced.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Acquired {
    pub id: CheckoutId,
    pub path: PathBuf,
    pub branch: String,
    /// `false` when a free slot was reused.
    pub created: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AcquireError {
    /// Every slot is occupied or parked and the declared cap is reached.
    CapReached { cap: usize },
    /// The repository has no commit to branch from, or Git refused.
    Git(String),
}

impl fmt::Display for AcquireError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CapReached { cap } => write!(
                formatter,
                "every one of the {cap} declared checkouts is in use; deliver or park a task first"
            ),
            Self::Git(reason) => formatter.write_str(reason),
        }
    }
}

impl std::error::Error for AcquireError {}

/// The slots of `primary` and their state, derived from Git and `store`.
pub fn slots(primary: &Path, store: &TaskStore) -> Vec<Slot> {
    registered_checkouts(primary)
        .into_iter()
        .map(|(path, branch)| {
            let id = CheckoutId::adopted(&slot_name(&path));
            let state = slot_state(primary, &path, branch.as_deref(), &id, store);
            Slot {
                id,
                path,
                branch,
                state,
            }
        })
        .collect()
}

/// Takes a slot for `task`, branching from `base_tip`: a free slot first,
/// a new directory only when none is free and `cap` allows it. Runs as one
/// critical section under the repository write lock.
pub fn acquire(
    primary: &Path,
    store: &TaskStore,
    task: &Task,
    base_tip: &str,
    cap: Option<usize>,
) -> Result<Acquired, AcquireError> {
    uze_git::locked(primary, uze_git::DEFAULT_WRITE_TIMEOUT, || {
        let existing = slots(primary, store);
        if let Some(free) = existing
            .iter()
            .filter(|slot| slot.state == SlotState::Free)
            .max_by_key(|slot| modified_at(&slot.path))
        {
            return reuse(free, &task.branch, base_tip);
        }
        if let Some(cap) = cap
            && existing.len() >= cap
        {
            return Err(AcquireError::CapReached { cap });
        }
        create(primary, &task.branch, base_tip)
    })
    .map_err(|error| AcquireError::Git(error.to_string()))?
}

fn reuse(slot: &Slot, branch: &str, base_tip: &str) -> Result<Acquired, AcquireError> {
    let root = &slot.path;
    git(root, &["switch", "--quiet", "-c", branch, base_tip])?;
    git(root, &["reset", "--quiet", "--hard", base_tip])?;
    // Without `-x` on purpose: ignored artifacts are what make the slot
    // worth keeping.
    git(root, &["clean", "--quiet", "-fd"])?;
    Ok(Acquired {
        id: slot.id.clone(),
        path: root.clone(),
        branch: branch.to_owned(),
        created: false,
    })
}

fn create(primary: &Path, branch: &str, base_tip: &str) -> Result<Acquired, AcquireError> {
    // A checkout removed outside UZE leaves its registry entry behind, and
    // `worktree add` then refuses the path. Adoption already looked at
    // every directory, so pruning here drops only entries with nothing
    // behind them.
    let _ = git(primary, &["worktree", "prune"]);
    let id = CheckoutId::generate();
    let relative = format!("{WORKTREES_DIRECTORY}/{id}");
    git(
        primary,
        &[
            "worktree", "add", "--quiet", "-b", branch, &relative, base_tip,
        ],
    )?;
    exclude_isolation_directory(primary)?;
    Ok(Acquired {
        id,
        path: primary.join(relative),
        branch: branch.to_owned(),
        created: true,
    })
}

/// A checkout's preparation, in order: links from the primary, then the
/// declared setup command. Every problem is a warning — a checkout without
/// its `.env` or its dependencies is still better than no agent — and the
/// warnings are what the tab shows.
pub const SETUP_TIMEOUT: Duration = Duration::from_secs(20 * 60);

pub fn materialize(
    primary: &Path,
    slot: &Path,
    links: &[PathBuf],
    setup: Option<&str>,
) -> Vec<String> {
    let mut warnings = Vec::new();
    for link in links {
        let source = primary.join(link);
        let destination = slot.join(link);
        if !source.exists() {
            warnings.push(format!(
                "`{}` is not in the primary checkout; not linked",
                link.display()
            ));
            continue;
        }
        if destination.exists() || fs::symlink_metadata(&destination).is_ok() {
            continue;
        }
        if let Some(parent) = destination.parent()
            && let Err(error) = fs::create_dir_all(parent)
        {
            warnings.push(format!("could not prepare `{}`: {error}", link.display()));
            continue;
        }
        if let Err(error) = symlink(&source, &destination) {
            warnings.push(format!("could not link `{}`: {error}", link.display()));
        }
    }
    if let Some(setup) = setup {
        let (passed, output) = crate::subprocess::run_shell_bounded(slot, setup, SETUP_TIMEOUT);
        if !passed {
            let tail = output.lines().last().unwrap_or("").to_owned();
            warnings.push(format!("setup `{setup}` failed: {tail}"));
        }
    }
    warnings
}

#[cfg(unix)]
fn symlink(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(source, destination)
}

#[cfg(not(unix))]
fn symlink(source: &Path, destination: &Path) -> std::io::Result<()> {
    if source.is_dir() {
        std::os::windows::fs::symlink_dir(source, destination)
    } else {
        std::os::windows::fs::symlink_file(source, destination)
    }
}

/// What reconciling the isolation directory against `store` found.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Reconciliation {
    /// Tasks created for checkouts nobody had recorded.
    pub adopted: Vec<TaskId>,
    /// Tasks whose checkout is gone, now marked from where their branch stands.
    pub orphaned: Vec<TaskId>,
}

/// Brings `store` in line with the isolation directory: adopts checkouts
/// without a task (parked when they hold work), marks tasks without a
/// checkout from where their branch stands, and prunes Git's registry only
/// after every directory has been looked at.
pub fn reconcile(primary: &Path, store: &mut TaskStore, target: &str) -> Reconciliation {
    let mut report = Reconciliation::default();
    let registered = registered_checkouts(primary);

    for (path, branch) in &registered {
        let id = CheckoutId::adopted(&slot_name(path));
        if store
            .tasks
            .iter()
            .any(|task| task.checkout.as_ref() == Some(&id))
        {
            continue;
        }
        let holds_work = is_dirty(path)
            || branch
                .as_deref()
                .is_some_and(|branch| commits_ahead(primary, target, branch) > 0);
        let label = branch
            .as_deref()
            .and_then(|branch| branch.strip_prefix(BRANCH_PREFIX))
            .unwrap_or(id.as_str())
            .to_owned();
        let mut task = Task::new(
            None,
            Base::Ref(target.to_owned()),
            tip_of(primary, target),
            target.to_owned(),
        );
        task.label = label;
        task.branch = branch.clone().unwrap_or_else(|| task.id.branch());
        task.checkout = Some(id);
        task.state = if holds_work {
            TaskState::Parked
        } else {
            TaskState::Integrated
        };
        report.adopted.push(task.id.clone());
        store.upsert(task);
    }

    for task in &mut store.tasks {
        let Some(checkout) = &task.checkout else {
            continue;
        };
        if registered
            .iter()
            .any(|(path, _)| slot_name(path) == checkout.as_str())
        {
            continue;
        }
        task.checkout = None;
        task.state = if branch_exists(primary, &task.branch)
            && commits_ahead(primary, target, &task.branch) > 0
        {
            TaskState::Parked
        } else {
            TaskState::Integrated
        };
        report.orphaned.push(task.id.clone());
    }

    let _ = git(primary, &["worktree", "prune"]);
    report
}

/// What a collection removed, so a caller can say so.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Collected {
    pub branches: Vec<String>,
    pub slots: Vec<CheckoutId>,
}

/// The two removals that cannot lose work, as one critical section: a
/// branch whose every commit is in the target, and the directory of a clean
/// slot nobody has touched for `age`, its branch kept.
///
/// Both are safe on their own; taking the write lock once around them is
/// what keeps a branch from being pruned in the moment a concurrent
/// acquisition is creating it.
pub fn collect(primary: &Path, store: &TaskStore, target: &str, age: Duration) -> Collected {
    uze_git::locked(primary, uze_git::DEFAULT_WRITE_TIMEOUT, || Collected {
        branches: prune_integrated_branches(primary, store, target),
        slots: remove_idle_slots(primary, store, age),
    })
    .unwrap_or_default()
}

/// Deletes every `agent/` branch whose commits are all reachable from
/// `target` and which no live task and no checkout is using. Returns the
/// branches removed.
pub fn prune_integrated_branches(primary: &Path, store: &TaskStore, target: &str) -> Vec<String> {
    let checked_out: Vec<String> = registered_checkouts(primary)
        .into_iter()
        .filter_map(|(_, branch)| branch)
        .collect();
    let live: Vec<&str> = store
        .tasks
        .iter()
        .filter(|task| is_live(&task.state))
        .map(|task| task.branch.as_str())
        .collect();
    let mut removed = Vec::new();
    for branch in agent_branches(primary) {
        if checked_out.contains(&branch) || live.contains(&branch.as_str()) {
            continue;
        }
        if commits_ahead(primary, target, &branch) == 0
            && git(primary, &["branch", "-D", &branch]).is_ok()
        {
            removed.push(branch);
        }
    }
    removed
}

/// Removes the directory of every free slot untouched for longer than
/// `age`, keeping its branch. Returns the slots removed.
pub fn remove_idle_slots(primary: &Path, store: &TaskStore, age: Duration) -> Vec<CheckoutId> {
    let now = SystemTime::now();
    let mut removed = Vec::new();
    for slot in slots(primary, store) {
        if slot.state != SlotState::Free {
            continue;
        }
        let idle = now
            .duration_since(modified_at(&slot.path))
            .unwrap_or_default();
        if idle < age {
            continue;
        }
        let path = slot.path.to_string_lossy().into_owned();
        if git(primary, &["worktree", "remove", &path]).is_ok() {
            removed.push(slot.id);
        }
    }
    removed
}

/// Ends `task` because nothing is in front of its checkout any more: the
/// slot goes back to the pool when it holds nothing, and is parked for the
/// operator when it holds work.
///
/// This is what an agent's departure means for its slot, and the only
/// transition besides delivery that frees one. Without it a task stays live
/// for as long as its record does — its slot occupied, its directory never
/// reused, and every new agent paying for a working tree of its own.
pub fn release(primary: &Path, task: &mut Task, target: &str) -> SlotState {
    let directory = task
        .checkout
        .as_ref()
        .map(|checkout| primary.join(WORKTREES_DIRECTORY).join(checkout.as_str()))
        .filter(|path| path.is_dir());
    let holds_work = directory.is_some_and(|path| is_dirty(&path))
        || (branch_exists(primary, &task.branch)
            && commits_ahead(primary, target, &task.branch) > 0);
    if is_live(&task.state) {
        task.state = if holds_work {
            TaskState::Parked
        } else {
            TaskState::Integrated
        };
    }
    // A task the operator declared done keeps its branch and frees its
    // slot, exactly as `slot_state` reads it: the commits are not lost,
    // they are simply nobody's turn any more.
    if holds_work && task.state != TaskState::Integrated {
        SlotState::Parked
    } else {
        SlotState::Free
    }
}

/// The one removal that loses work, and therefore the one only an operator
/// takes on a named task: the checkout directory, forced, and the branch.
pub fn discard(primary: &Path, task: &Task) -> Result<(), String> {
    uze_git::locked(primary, uze_git::DEFAULT_WRITE_TIMEOUT, || {
        if let Some(checkout) = &task.checkout {
            let path = primary.join(WORKTREES_DIRECTORY).join(checkout.as_str());
            if path.is_dir() {
                git(
                    primary,
                    &["worktree", "remove", "--force", &path.to_string_lossy()],
                )
                .map_err(|error| error.to_string())?;
            }
        }
        if branch_exists(primary, &task.branch) {
            git(primary, &["branch", "-D", &task.branch]).map_err(|error| error.to_string())?;
        }
        Ok(())
    })
    .map_err(|error| error.to_string())?
}

/// Whether `state` means an agent may still be writing.
pub fn is_live(state: &TaskState) -> bool {
    !matches!(state, TaskState::Integrated | TaskState::Parked)
}

/// The branch checked out in `root`, or `None` for a detached `HEAD`.
/// `symbolic-ref` rather than `rev-parse --abbrev-ref`: it still names the
/// branch when it has no commit yet, which is the case that must be told
/// apart from "no branch at all".
pub fn current_branch(root: &Path) -> Option<String> {
    let branch = uze_git::read(root, &["symbolic-ref", "--short", "--quiet", "HEAD"])
        .ok()?
        .successful()
        .ok()?;
    let branch = branch.trim();
    (!branch.is_empty()).then(|| branch.to_owned())
}

/// The commit `reference` resolves to in `root`.
pub fn tip_of(root: &Path, reference: &str) -> String {
    uze_git::read(root, &["rev-parse", "--verify", "--quiet", reference])
        .ok()
        .and_then(|output| output.successful().ok())
        .map(|stdout| stdout.trim().to_owned())
        .unwrap_or_default()
}

/// Uncommitted changes, tracked or untracked-but-not-ignored.
pub fn is_dirty(root: &Path) -> bool {
    uze_git::read(root, &["status", "--porcelain"])
        .ok()
        .and_then(|output| output.successful().ok())
        .is_none_or(|status| !status.trim().is_empty())
}

/// How many commits `branch` has that `target` lacks.
pub fn commits_ahead(root: &Path, target: &str, branch: &str) -> usize {
    uze_git::read(
        root,
        &["rev-list", "--count", &format!("{target}..{branch}")],
    )
    .ok()
    .and_then(|output| output.successful().ok())
    .and_then(|count| count.trim().parse().ok())
    .unwrap_or(0)
}

fn branch_exists(root: &Path, branch: &str) -> bool {
    uze_git::read(
        root,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    )
    .is_ok_and(|output| output.is_success())
}

fn agent_branches(root: &Path) -> Vec<String> {
    uze_git::read(
        root,
        &[
            "for-each-ref",
            "--format=%(refname:short)",
            &format!("refs/heads/{BRANCH_PREFIX}"),
        ],
    )
    .ok()
    .and_then(|output| output.successful().ok())
    .map(|stdout| stdout.lines().map(str::to_owned).collect())
    .unwrap_or_default()
}

/// Every linked worktree Git registers under the isolation directory,
/// with the branch it has checked out. Read from `git worktree list`, so a
/// directory that exists but was never registered is not a slot.
fn registered_checkouts(primary: &Path) -> Vec<(PathBuf, Option<String>)> {
    let container = primary.join(WORKTREES_DIRECTORY);
    let Some(listing) = uze_git::read(primary, &["worktree", "list", "--porcelain"])
        .ok()
        .and_then(|output| output.successful().ok())
    else {
        return Vec::new();
    };
    let mut checkouts = Vec::new();
    let mut current: Option<(PathBuf, Option<String>)> = None;
    for line in listing.lines().chain(std::iter::once("")) {
        if let Some(path) = line.strip_prefix("worktree ") {
            current = Some((PathBuf::from(path), None));
        } else if let Some(reference) = line.strip_prefix("branch ")
            && let Some(entry) = current.as_mut()
        {
            entry.1 = Some(
                reference
                    .strip_prefix("refs/heads/")
                    .unwrap_or(reference)
                    .to_owned(),
            );
        } else if line.is_empty()
            && let Some(entry) = current.take()
            && entry.0.parent() == Some(container.as_path())
            && entry.0.is_dir()
        {
            checkouts.push(entry);
        }
    }
    checkouts.sort();
    checkouts
}

fn slot_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn slot_state(
    primary: &Path,
    path: &Path,
    branch: Option<&str>,
    id: &CheckoutId,
    store: &TaskStore,
) -> SlotState {
    let task = store
        .tasks
        .iter()
        .filter(|task| task.checkout.as_ref() == Some(id))
        .max_by_key(|task| task.created_at_unix);
    if let Some(task) = task
        && is_live(&task.state)
    {
        return SlotState::Occupied {
            task: task.id.clone(),
        };
    }
    if is_dirty(path) {
        return SlotState::Parked;
    }
    let declared_done = task.is_some_and(|task| task.state == TaskState::Integrated);
    let target = task.map(|task| task.target.as_str());
    let holds_commits = match (branch, target) {
        (Some(branch), Some(target)) => commits_ahead(primary, target, branch) > 0,
        (Some(branch), None) => commits_ahead(primary, "HEAD", branch) > 0,
        (None, _) => false,
    };
    if holds_commits && !declared_done {
        SlotState::Parked
    } else {
        SlotState::Free
    }
}

fn modified_at(path: &Path) -> SystemTime {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// Excludes the isolation directory through `.git/info/exclude`, which
/// belongs to the local repository and never to the operator's tree: the
/// primary's status stays exactly what the operator left, and `git add -A`
/// there never sweeps a slot in as an embedded repository. Idempotent.
pub fn exclude_isolation_directory(primary: &Path) -> Result<(), AcquireError> {
    let common = uze_git::read(
        primary,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .map_err(|error| AcquireError::Git(error.to_string()))?
    .successful()
    .map_err(AcquireError::Git)?;
    let exclude = PathBuf::from(common.trim()).join("info").join("exclude");
    let entry = format!("/{WORKTREES_DIRECTORY}/");
    let current = fs::read_to_string(&exclude).unwrap_or_default();
    if current.lines().any(|line| {
        let line = line.trim();
        line == entry || line == format!("{WORKTREES_DIRECTORY}/") || line == WORKTREES_DIRECTORY
    }) {
        return Ok(());
    }
    if let Some(parent) = exclude.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            AcquireError::Git(format!("could not create {}: {error}", parent.display()))
        })?;
    }
    let mut next = current;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(&entry);
    next.push('\n');
    fs::write(&exclude, next).map_err(|error| {
        AcquireError::Git(format!("could not update {}: {error}", exclude.display()))
    })
}

fn git(root: &Path, args: &[&str]) -> Result<String, AcquireError> {
    uze_git::write(root, args)
        .map_err(|error| AcquireError::Git(error.to_string()))?
        .successful()
        .map(|stdout| stdout.trim().to_owned())
        .map_err(AcquireError::Git)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uze_testkit::git::Repository;

    const TARGET: &str = "main";

    /// A repository whose `.gitignore` already ignores `target/`, the way a
    /// Rust project does — the artifact reuse exists to preserve.
    fn repository(label: &str) -> Repository {
        let repository = Repository::new(label);
        repository.commit_file(".gitignore", "target/\n");
        repository
    }

    fn task(label: &str) -> Task {
        let mut task = Task::new(
            Some(label),
            Base::Ref(TARGET.into()),
            String::new(),
            TARGET.into(),
        );
        task.label = label.to_owned();
        task
    }

    /// Acquires a slot for a fresh task and records it as occupied.
    fn launch(repository: &Repository, store: &mut TaskStore, label: &str) -> (Task, Acquired) {
        let primary = repository.root();
        let mut task = task(label);
        task.base_commit = tip_of(primary, TARGET);
        let acquired = acquire(primary, store, &task, &task.base_commit, None).unwrap();
        task.checkout = Some(acquired.id.clone());
        store.upsert(task.clone());
        (task, acquired)
    }

    fn set_state(store: &mut TaskStore, id: &TaskId, state: TaskState) {
        store.get_mut(id).unwrap().state = state;
    }

    /// The whole reason reuse ever happens: an agent that closed without
    /// delivering anything must not keep its slot.
    #[test]
    fn an_agent_that_left_an_empty_checkout_frees_its_slot_for_the_next() {
        let repository = repository("slots-release-empty");
        let mut store = TaskStore::default();

        let (first, slot) = launch(&repository, &mut store, "first");
        let target = TARGET.to_owned();
        let state = release(
            repository.root(),
            store.get_mut(&first.id).unwrap(),
            &target,
        );
        assert_eq!(state, SlotState::Free);
        assert_eq!(
            slots(repository.root(), &store)[0].state,
            SlotState::Free,
            "nothing is in front of it and it holds nothing"
        );

        let (_, reused) = launch(&repository, &mut store, "second");
        assert!(!reused.created, "the freed slot is taken, not a new one");
        assert_eq!(reused.path, slot.path);
    }

    #[test]
    fn an_agent_that_left_work_behind_parks_its_slot_instead_of_freeing_it() {
        let repository = repository("slots-release-work");
        let mut store = TaskStore::default();

        let (committed, slot) = launch(&repository, &mut store, "committed");
        fs::write(slot.path.join("feature.rs"), b"fn f() {}").unwrap();
        repository.git_in(&slot.path, &["add", "."]);
        repository.git_in(&slot.path, &["commit", "-qm", "undelivered"]);
        assert_eq!(
            release(
                repository.root(),
                store.get_mut(&committed.id).unwrap(),
                TARGET
            ),
            SlotState::Parked
        );

        let (dirty, other) = launch(&repository, &mut store, "dirty");
        fs::write(other.path.join("draft.rs"), b"unsaved").unwrap();
        assert_eq!(
            release(repository.root(), store.get_mut(&dirty.id).unwrap(), TARGET),
            SlotState::Parked
        );

        let (_, third) = launch(&repository, &mut store, "third");
        assert!(
            third.created,
            "a parked slot is never offered to a new agent"
        );
        assert!(
            other.path.join("draft.rs").is_file(),
            "every file of a parked checkout is preserved"
        );
    }

    #[test]
    fn a_free_slot_is_reused_and_ignored_artifacts_survive() {
        let repository = repository("slots-reuse");
        let mut store = TaskStore::default();

        let (first, slot) = launch(&repository, &mut store, "first");
        assert!(slot.created);
        fs::create_dir_all(slot.path.join("target")).unwrap();
        fs::write(slot.path.join("target").join("cache"), b"warm").unwrap();
        fs::write(slot.path.join("feature.rs"), b"fn f() {}").unwrap();
        repository.git_in(&slot.path, &["add", "."]);
        repository.git_in(&slot.path, &["commit", "-qm", "feature"]);
        repository.git(&["merge", "--ff-only", &first.branch]);
        set_state(&mut store, &first.id, TaskState::Integrated);

        let (second, reused) = launch(&repository, &mut store, "second");
        assert!(
            !reused.created,
            "the free slot is taken before a directory is created"
        );
        assert_eq!(reused.path, slot.path);
        assert_eq!(repository.branch_of(&reused.path), second.branch);
        assert_eq!(
            fs::read(reused.path.join("target").join("cache")).unwrap(),
            b"warm",
            "ignored artifacts are the point of reuse"
        );
        assert!(
            reused.path.join("feature.rs").exists(),
            "delivered work is in the base"
        );
        assert!(!is_dirty(&reused.path));
    }

    #[test]
    fn a_previous_tasks_edits_never_reach_the_next() {
        let repository = repository("slots-no-leak");
        let primary = repository.root();
        let mut store = TaskStore::default();

        let (first, slot) = launch(&repository, &mut store, "first");
        fs::write(slot.path.join("only-on-branch.rs"), b"").unwrap();
        repository.git_in(&slot.path, &["add", "."]);
        repository.git_in(&slot.path, &["commit", "-qm", "kept on the branch"]);
        // Declared done by the operator without reaching the target — handoff.
        set_state(&mut store, &first.id, TaskState::Integrated);

        let (_, reused) = launch(&repository, &mut store, "second");
        assert_eq!(reused.path, slot.path);
        assert!(
            !reused.path.join("only-on-branch.rs").exists(),
            "the tree is at the base, not at the previous branch"
        );
        assert!(
            commits_ahead(primary, TARGET, &first.branch) == 1,
            "the previous branch keeps its commit"
        );
    }

    #[test]
    fn a_checkout_holding_work_is_parked_with_every_file_preserved() {
        let repository = repository("slots-park");
        let primary = repository.root();
        let mut store = TaskStore::default();

        let (_, slot) = launch(&repository, &mut store, "abandoned");
        fs::write(slot.path.join("half-done.rs"), b"unfinished").unwrap();
        // The agent is gone and nobody recorded the task: the shape of a
        // crash, or of a checkout from before task state existed.
        let mut forgotten = TaskStore::default();

        let report = reconcile(primary, &mut forgotten, TARGET);
        assert_eq!(report.adopted.len(), 1);
        let adopted = forgotten.get(&report.adopted[0]).unwrap();
        assert_eq!(adopted.state, TaskState::Parked);
        assert_eq!(adopted.checkout, Some(slot.id.clone()));
        assert_eq!(
            fs::read(slot.path.join("half-done.rs")).unwrap(),
            b"unfinished"
        );

        let (_, fresh) = launch(&repository, &mut forgotten, "next");
        assert!(fresh.created, "a parked slot is never offered");
        assert_ne!(fresh.path, slot.path);
        assert_eq!(
            fs::read(slot.path.join("half-done.rs")).unwrap(),
            b"unfinished"
        );
    }

    #[test]
    fn an_unintegrated_branch_outlives_its_directory() {
        let repository = repository("slots-idle");
        let primary = repository.root();
        let mut store = TaskStore::default();

        let (first, slot) = launch(&repository, &mut store, "handed-off");
        fs::write(slot.path.join("work.rs"), b"").unwrap();
        repository.git_in(&slot.path, &["add", "."]);
        repository.git_in(&slot.path, &["commit", "-qm", "work"]);
        set_state(&mut store, &first.id, TaskState::Integrated);
        assert_eq!(slots(primary, &store)[0].state, SlotState::Free);

        let removed = remove_idle_slots(primary, &store, Duration::ZERO);
        assert_eq!(removed, vec![slot.id]);
        assert!(!slot.path.exists());
        assert!(
            branch_exists(primary, &first.branch),
            "the branch is never the directory's cost"
        );
        assert_eq!(commits_ahead(primary, TARGET, &first.branch), 1);
    }

    #[test]
    fn a_parked_slot_is_never_removed_for_being_idle() {
        let repository = repository("slots-idle-parked");
        let primary = repository.root();
        let mut store = TaskStore::default();
        let (_, slot) = launch(&repository, &mut store, "abandoned");
        fs::write(slot.path.join("dirty"), b"").unwrap();
        let mut forgotten = TaskStore::default();
        reconcile(primary, &mut forgotten, TARGET);
        assert!(remove_idle_slots(primary, &forgotten, Duration::ZERO).is_empty());
        assert!(slot.path.join("dirty").exists());
    }

    #[test]
    fn an_integrated_branch_is_pruned_and_an_unintegrated_one_is_not() {
        let repository = repository("slots-prune-branches");
        let primary = repository.root();
        let mut store = TaskStore::default();

        let (delivered, slot) = launch(&repository, &mut store, "delivered");
        fs::write(slot.path.join("a.rs"), b"").unwrap();
        repository.git_in(&slot.path, &["add", "."]);
        repository.git_in(&slot.path, &["commit", "-qm", "a"]);
        repository.git(&["merge", "--ff-only", &delivered.branch]);
        set_state(&mut store, &delivered.id, TaskState::Integrated);

        let (kept, second) = launch(&repository, &mut store, "kept");
        fs::write(second.path.join("b.rs"), b"").unwrap();
        repository.git_in(&second.path, &["add", "."]);
        repository.git_in(&second.path, &["commit", "-qm", "b"]);
        set_state(&mut store, &kept.id, TaskState::Integrated);
        // The reused slot moved on to a third branch, so neither of the
        // two above is checked out anywhere.
        let (_, _) = launch(&repository, &mut store, "third");
        assert_eq!(second.path, slot.path);

        let removed = prune_integrated_branches(primary, &store, TARGET);
        assert_eq!(removed, vec![delivered.branch.clone()]);
        assert!(!branch_exists(primary, &delivered.branch));
        assert!(
            branch_exists(primary, &kept.branch),
            "commits the target lacks are never deleted"
        );
    }

    #[test]
    fn a_new_directory_appears_only_when_none_is_free_and_the_cap_holds() {
        let repository = repository("slots-cap");
        let primary = repository.root();
        let mut store = TaskStore::default();

        let (first, a) = launch(&repository, &mut store, "a");
        let (_, b) = launch(&repository, &mut store, "b");
        assert!(a.created && b.created && a.path != b.path);

        let blocked = task("c");
        let error =
            acquire(primary, &store, &blocked, &tip_of(primary, TARGET), Some(2)).unwrap_err();
        assert!(
            matches!(error, AcquireError::CapReached { cap: 2 }),
            "{error}"
        );

        set_state(&mut store, &first.id, TaskState::Integrated);
        let reused = acquire(primary, &store, &blocked, &tip_of(primary, TARGET), Some(2)).unwrap();
        assert!(!reused.created);
        assert_eq!(reused.path, a.path);
    }

    #[test]
    fn prune_runs_after_adoption_and_an_orphaned_task_keeps_its_branch() {
        let repository = repository("slots-prune-order");
        let primary = repository.root();
        let mut store = TaskStore::default();

        let (task, slot) = launch(&repository, &mut store, "vanished");
        fs::write(slot.path.join("c.rs"), b"").unwrap();
        repository.git_in(&slot.path, &["add", "."]);
        repository.git_in(&slot.path, &["commit", "-qm", "c"]);
        fs::remove_dir_all(&slot.path).unwrap();
        assert!(
            repository
                .git(&["worktree", "list", "--porcelain"])
                .contains(&task.branch),
            "the registry entry is still there before reconciliation"
        );

        let report = reconcile(primary, &mut store, TARGET);
        assert_eq!(report.orphaned, vec![task.id.clone()]);
        let task = store.get(&task.id).unwrap();
        assert_eq!(task.state, TaskState::Parked);
        assert_eq!(task.checkout, None);
        assert!(branch_exists(primary, &task.branch));
        assert!(
            !repository
                .git(&["worktree", "list", "--porcelain"])
                .contains(&task.branch),
            "the stale entry is pruned once every directory was looked at"
        );
    }

    #[test]
    fn a_legacy_checkout_is_adopted_under_its_branch_name() {
        let repository = repository("slots-legacy");
        let primary = repository.root();
        repository.git(&[
            "worktree",
            "add",
            "-q",
            "-b",
            "agent/agent-2",
            ".worktrees/agent-2",
            "HEAD",
        ]);
        let mut store = TaskStore::default();
        let report = reconcile(primary, &mut store, TARGET);
        assert_eq!(report.adopted.len(), 1);
        let adopted = store.get(&report.adopted[0]).unwrap();
        assert_eq!(adopted.label, "agent-2");
        assert_eq!(
            adopted.branch, "agent/agent-2",
            "no branch is renamed: it may have been pushed"
        );
        assert_eq!(adopted.checkout, Some(CheckoutId::adopted("agent-2")));
        assert_eq!(
            adopted.state,
            TaskState::Integrated,
            "clean and nothing ahead: free to reuse"
        );
        assert_eq!(slots(primary, &store)[0].state, SlotState::Free);
    }

    #[test]
    fn the_isolation_directory_is_excluded_without_touching_the_primary_tree() {
        let repository = repository("slots-exclude");
        let primary = repository.root();
        let mut store = TaskStore::default();
        let gitignore_before = fs::read_to_string(primary.join(".gitignore")).unwrap();
        launch(&repository, &mut store, "a");
        launch(&repository, &mut store, "b");
        assert!(
            repository.git(&["status", "--porcelain"]).is_empty(),
            "the primary is untouched"
        );
        assert_eq!(
            fs::read_to_string(primary.join(".gitignore")).unwrap(),
            gitignore_before
        );
        let exclude = fs::read_to_string(primary.join(".git/info/exclude")).unwrap();
        assert_eq!(
            exclude.matches(WORKTREES_DIRECTORY).count(),
            1,
            "idempotent: {exclude}"
        );
    }

    #[test]
    fn a_repository_without_a_commit_cannot_host_a_slot() {
        let repository = Repository::empty("slots-unborn");
        let store = TaskStore::default();
        let error = acquire(repository.root(), &store, &task("x"), "HEAD", None).unwrap_err();
        assert!(matches!(error, AcquireError::Git(_)), "{error}");
    }

    #[test]
    fn a_linked_file_is_a_symlink_and_a_missing_target_only_warns() {
        let repository = repository("slots-materialize");
        let primary = repository.root();
        fs::write(primary.join(".env"), "SECRET=1\n").unwrap();
        let mut store = TaskStore::default();
        let (_, slot) = launch(&repository, &mut store, "materialize");

        let warnings = materialize(
            primary,
            &slot.path,
            &[PathBuf::from(".env"), PathBuf::from(".env.local")],
            None,
        );
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains(".env.local"));
        let linked = slot.path.join(".env");
        assert!(
            fs::symlink_metadata(&linked)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&linked).unwrap(), "SECRET=1\n");
        assert!(
            materialize(primary, &slot.path, &[PathBuf::from(".env")], None).is_empty(),
            "idempotent"
        );
    }

    #[test]
    fn a_failing_setup_warns_with_its_last_line_and_a_passing_one_is_silent() {
        let repository = repository("slots-setup");
        let primary = repository.root();
        let mut store = TaskStore::default();
        let (_, slot) = launch(&repository, &mut store, "setup");
        let warnings = materialize(
            primary,
            &slot.path,
            &[],
            Some("echo preparing; echo 'no such tool: pnpm' >&2; exit 3"),
        );
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("no such tool: pnpm"), "{warnings:?}");
        assert!(materialize(primary, &slot.path, &[], Some("touch prepared")).is_empty());
        assert!(
            slot.path.join("prepared").exists(),
            "setup runs in the checkout"
        );
    }
}
