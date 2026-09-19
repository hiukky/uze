//! A task: one agent launch UZE made in a project, from the moment it
//! started to the moment its work reached the target.
//!
//! # Identity is not the label
//!
//! An agent's identifier is generated once and never changes; for a task
//! it keys the branch (`agent/<id>`), the checkout it runs in, and its
//! persisted state. The label is derived from the prompt and names the
//! tab. Keeping them apart is what makes a name free to change and a
//! collision impossible. The identifier is an *agent's*, not a task's:
//! an agent that never left the project's own root carries one too, and
//! it is what a reader holds before it knows which kind it has.
//!
//! # Storage
//!
//! One JSON document per project under `UzeHome::state_dir()/tasks/<project
//! id>.json`, outside every checkout by construction, so removing a
//! worktree can never remove history. Written atomically, and carrying a
//! schema version from the first commit: a document from a schema this
//! build does not know is refused, never guessed at.
//!
//! Atomic is not the same as serialized: every change goes through
//! [`locked`], which holds the document for the whole read-modify-write so
//! two of the client's threads cannot each write back the version they
//! read.

use std::{
    cell::RefCell,
    collections::BTreeSet,
    fmt, fs,
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{
    Result, UzeError, checkout::CheckoutId, digest, harness_runtime::project_id_for, home::UzeHome,
    persistence::write_atomic, worktree::BRANCH_PREFIX,
};

pub const SCHEMA_VERSION: u32 = 3;

/// Long enough to read, short enough for a sidebar.
const LABEL_MAX_CHARS: usize = 40;
const IDENTIFIER_CHARS: usize = 6;

/// A generated, immutable identifier for an agent UZE launched — what a
/// launch carries, what a conversation is keyed by, and what an agent
/// keeps whether or not it is isolated.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct AgentId(String);

impl AgentId {
    pub fn generate() -> Self {
        Self(generated_identifier(b"agent"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The branch a task's work lives on while it stays local and nobody has
/// named it: the identifier under UZE's own prefix.
fn generated_branch(id: &AgentId) -> String {
    format!("{BRANCH_PREFIX}{}", id.as_str())
}

impl fmt::Display for AgentId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Six lowercase alphanumerics from a digest of time, process and a
/// per-process counter. Not a security token: it only has to be unique
/// among the identifiers one machine generates, and readable in a path.
pub(crate) fn generated_identifier(kind: &[u8]) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut material = Vec::with_capacity(48);
    material.extend_from_slice(kind);
    material.extend_from_slice(&nanos.to_le_bytes());
    material.extend_from_slice(&std::process::id().to_le_bytes());
    material.extend_from_slice(&counter.to_le_bytes());
    let mut value = digest::fnv1a64(&material);
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    (0..IDENTIFIER_CHARS)
        .map(|_| {
            let index = (value % ALPHABET.len() as u64) as usize;
            value /= ALPHABET.len() as u64;
            ALPHABET[index] as char
        })
        .collect()
}

/// Where a task's branch starts: a ref, normally the target.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Base {
    Ref(String),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TaskState {
    /// The agent is live and nothing has been observed yet.
    Running,
    /// The last evaluation found uncommitted changes in the checkout.
    Uncommitted,
    /// Commits ahead of the base on a clean tree: delivery may be offered.
    Ready,
    /// Delivery is in progress.
    Integrating,
    /// A rebase stopped on these files; the rebase is paused in the checkout.
    Conflicted { files: Vec<PathBuf> },
    /// The gate failed on the rebased commits.
    GateFailed,
    /// The work is in the target.
    Integrated,
    /// The agent is gone and the checkout still holds work.
    Parked,
    /// The agent is gone and its branch held nothing to deliver. Ended,
    /// like `Integrated` and `Parked`, but the only one of the three that
    /// never had work: saying "delivered" of it would claim a delivery
    /// nobody made.
    Closed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Agent {
    pub id: AgentId,
    /// The harness this agent runs, recorded at launch. A fact rather
    /// than an inference: the process a pane is running answers "what is
    /// alive there now", which is a different question and stops being
    /// answerable the moment the harness exits.
    pub harness: String,
    /// The name a person reads for this agent. Derived from the branch
    /// once the work is named (`worktree::label_of`), and the identifier
    /// until then.
    pub label: String,
    pub created_at_unix: u64,
    /// When no live pane carried this agent any more. `None` while live.
    pub ended_at_unix: Option<u64>,
    /// The checkout of its own, once it has one. `None` is an agent
    /// working in the project's root, on whatever branch the operator is
    /// on: it has no branch of its own to deliver, nothing to be ready,
    /// and nothing to preserve — which is why every one of those facts
    /// lives inside this and not beside it.
    pub isolation: Option<Isolation>,
}

/// What an isolated agent has that an agent in the root does not: a
/// branch of its own, a checkout to work in, and the whole vocabulary of
/// readiness and delivery that only means anything against them.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Isolation {
    pub base: Base,
    /// The base's tip when the branch was last rebased onto it — what a
    /// restack would use as the old base, so the parent's commits are never
    /// replayed into a child.
    pub base_commit: String,
    pub target: String,
    pub branch: String,
    pub checkout: Option<CheckoutId>,
    pub state: TaskState,
    /// The readable name UZE published the branch under, when that is not
    /// the branch's own name. The one half of publication that Git cannot
    /// be asked for: an unnamed agent's branch leaves under a name derived
    /// for it, and nothing on the machine ties the two together but this.
    /// Everything else about publication — whether the branch is on the
    /// remote at all, and what the remote holds — is read from the
    /// repository's remote-tracking refs, so a push somebody else made
    /// counts exactly as much as one UZE made.
    pub published_as: Option<String>,
    /// The number of the request open on the forge for the published
    /// branch, once one was found. Read off the remote like every other
    /// readiness fact, never announced by the agent that opened it.
    pub published_request: Option<u32>,
    /// The branch `published_request` was found for. A number answers for
    /// one branch, and the agent outlives it: the agent that delivered keeps
    /// working, often on a new branch with a request of its own.
    pub request_branch: Option<String>,
    /// When the remote was last asked whether a request exists for this
    /// branch, so the question is asked on a clock instead of on every
    /// evaluation: it is the one publication fact that costs a network
    /// round trip, and it stops being asked the moment it is answered.
    pub request_asked_at_unix: Option<u64>,
}

impl Agent {
    /// The agent's own isolation, when it has one.
    pub fn isolation(&self) -> Option<&Isolation> {
        self.isolation.as_ref()
    }

    pub fn isolation_mut(&mut self) -> Option<&mut Isolation> {
        self.isolation.as_mut()
    }

    pub fn is_isolated(&self) -> bool {
        self.isolation.is_some()
    }

    pub fn is_live(&self) -> bool {
        self.ended_at_unix.is_none()
    }

    /// Records that the agent's last pane is gone. Idempotent: the first
    /// ending stands.
    pub fn end(&mut self) {
        if self.ended_at_unix.is_none() {
            self.ended_at_unix = Some(now_unix());
        }
    }

    /// The directory this agent may stand in, under the project root the
    /// store is keyed by: its own slot, or the project's root. `None` for
    /// an isolated agent whose checkout is gone — nowhere is its own any
    /// more.
    pub fn own_directory(&self, project_root: &Path) -> Option<PathBuf> {
        match &self.isolation {
            Some(isolation) => isolation
                .checkout
                .as_ref()
                .map(|checkout| checkout.directory(project_root)),
            None => Some(project_root.to_path_buf()),
        }
    }

    /// Whether this work carries a name somebody chose, rather than one
    /// UZE is still holding for it.
    ///
    /// The test is the namespace, not the identifier: `agent/` is UZE's
    /// own, and a branch inside it — the generated identifier, or one a
    /// checkout was adopted on — is still UZE's to name. Putting a name
    /// outside the namespace is exactly what naming does, so "outside the
    /// prefix" and "somebody chose this" are the same fact.
    ///
    /// One predicate for the whole codebase: a name anybody chose — the
    /// agent, the operator, an earlier automatic step — is final, and every
    /// later mechanism asks this same question rather than inventing its
    /// own notion of "unnamed". An agent with no branch has nothing to
    /// name, and answers `false` to a question nobody should be asking it.
    pub fn is_named(&self) -> bool {
        self.isolation
            .as_ref()
            .is_some_and(|isolation| !isolation.branch.starts_with(BRANCH_PREFIX))
    }

    /// Takes `branch` as this agent's name, deriving the visible label
    /// from it. The caller owns the Git rename and the validation; this is
    /// the record of it, and only an isolated agent has a branch to be
    /// named by — naming any other is refused before this is reached.
    pub fn take_name(&mut self, branch: String) {
        debug_assert!(
            self.is_isolated(),
            "an agent with no branch has no name to take"
        );
        self.label = crate::worktree::label_of(&branch);
        if let Some(isolation) = &mut self.isolation {
            isolation.branch = branch;
        }
    }

    /// An agent launched into the project's own root: no branch, no
    /// checkout, nothing to deliver.
    pub fn in_the_root(harness: &str) -> Self {
        let id = AgentId::generate();
        Self {
            label: id.as_str().to_owned(),
            id,
            harness: harness.to_owned(),
            created_at_unix: now_unix(),
            ended_at_unix: None,
            isolation: None,
        }
    }

    /// An agent launched straight into a checkout of its own, as a
    /// project that declares isolation by default does it.
    pub fn isolated(
        harness: &str,
        prompt: Option<&str>,
        base: Base,
        base_commit: String,
        target: String,
    ) -> Self {
        let mut agent = Self::in_the_root(harness);
        if let Some(prompt) = prompt {
            agent.label = label_from_prompt(prompt, &agent.id);
        }
        agent.isolation = Some(Isolation::cut(&agent.id, base, base_commit, target));
        agent
    }
}

impl Isolation {
    /// A branch of this agent's own, cut from `base_commit`.
    pub fn cut(id: &AgentId, base: Base, base_commit: String, target: String) -> Self {
        Self {
            base,
            base_commit,
            target,
            branch: generated_branch(id),
            checkout: None,
            state: TaskState::Running,
            published_as: None,
            published_request: None,
            request_branch: None,
            request_asked_at_unix: None,
        }
    }

    /// Drops the request this agent had, so the next evaluation asks the
    /// remote afresh.
    pub fn forget_request(&mut self) {
        self.published_request = None;
        self.request_branch = None;
        self.request_asked_at_unix = None;
    }

    /// Drops the request unless it was found for `branch` — the name the
    /// work is published under now, or `None` while it is not published.
    pub fn forget_request_unless_for(&mut self, branch: Option<&str>) {
        if self.published_request.is_some() && self.request_branch.as_deref() != branch {
            self.forget_request();
        }
    }
}

/// Seconds since the epoch — how every time this record keeps is written.
pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

/// The first non-empty line of `prompt`, lower-cased, non-alphanumerics
/// collapsed to single hyphens, cut at a word boundary; the identifier when
/// nothing usable remains.
pub fn label_from_prompt(prompt: &str, fallback: &AgentId) -> String {
    let Some(line) = prompt.lines().map(str::trim).find(|line| !line.is_empty()) else {
        return fallback.as_str().to_owned();
    };
    let mut slug = String::new();
    let mut pending_separator = false;
    for character in line.chars() {
        if character.is_alphanumeric() {
            if pending_separator && !slug.is_empty() {
                slug.push('-');
            }
            pending_separator = false;
            slug.extend(character.to_lowercase());
        } else {
            pending_separator = true;
        }
    }
    if slug.is_empty() {
        return fallback.as_str().to_owned();
    }
    if slug.chars().count() <= LABEL_MAX_CHARS {
        return slug;
    }
    let cut: String = slug.chars().take(LABEL_MAX_CHARS).collect();
    match cut.rfind('-') {
        Some(boundary) if boundary > 0 => cut[..boundary].to_owned(),
        _ => cut,
    }
}

/// Every agent a project's launches recorded, isolated in a slot of its
/// own or running in the project's own directory. One document, one lock,
/// one sweep, because a launch is the same event either way and a reader
/// handed an identifier does not know yet which it names.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TaskStore {
    pub schema_version: u32,
    /// Every agent this project's launches recorded, isolated or not.
    /// One collection, because one identifier names one agent: a reader
    /// handed an id used to have to ask which of two lists it came from,
    /// and isolating an agent used to mean moving it between them while
    /// a sweep could see it twice.
    pub agents: Vec<Agent>,
}

impl Default for TaskStore {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            agents: Vec::new(),
        }
    }
}

impl TaskStore {
    pub fn get(&self, id: &AgentId) -> Option<&Agent> {
        self.agents.iter().find(|agent| &agent.id == id)
    }

    pub fn get_mut(&mut self, id: &AgentId) -> Option<&mut Agent> {
        self.agents.iter_mut().find(|agent| &agent.id == id)
    }

    /// The agent an identifier names.
    pub fn agent(&self, id: &str) -> Option<&Agent> {
        self.agents.iter().find(|agent| agent.id.as_str() == id)
    }

    pub fn agent_mut(&mut self, id: &str) -> Option<&mut Agent> {
        self.agents.iter_mut().find(|agent| agent.id.as_str() == id)
    }

    /// Every isolated agent, in the order they were recorded.
    pub fn isolated(&self) -> impl Iterator<Item = &Agent> {
        self.agents.iter().filter(|agent| agent.is_isolated())
    }

    pub fn isolated_mut(&mut self) -> impl Iterator<Item = &mut Agent> {
        self.agents.iter_mut().filter(|agent| agent.is_isolated())
    }

    /// The agent standing in `checkout` now: the newest to have been given
    /// it. A slot outlives the agents that ran in it and each went on
    /// naming it; anything older is history, and answering for it would
    /// hand the current agent's work to one long gone.
    pub fn slot_owner(&self, checkout: &CheckoutId) -> Option<&Agent> {
        self.agents
            .iter()
            .filter(|agent| {
                agent
                    .isolation
                    .as_ref()
                    .is_some_and(|isolation| isolation.checkout.as_ref() == Some(checkout))
            })
            .max_by_key(|agent| agent.created_at_unix)
    }

    /// Every agent that is the [`slot_owner`](Self::slot_owner) of its own
    /// checkout.
    pub fn slot_owners(&self) -> BTreeSet<AgentId> {
        self.agents
            .iter()
            .filter_map(|agent| self.slot_owner(agent.isolation.as_ref()?.checkout.as_ref()?))
            .map(|owner| owner.id.clone())
            .collect()
    }

    /// Adds or replaces by identifier.
    pub fn upsert(&mut self, agent: Agent) {
        match self.get_mut(&agent.id) {
            Some(existing) => *existing = agent,
            None => self.agents.push(agent),
        }
    }
}

/// The document for `project_root`, keyed on the canonical root.
pub fn store_path(home: &UzeHome, project_root: &Path) -> PathBuf {
    let canonical = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    home.tasks_path(&project_id_for(&canonical))
}

/// The project's tasks; empty when nothing was ever recorded.
pub fn load(home: &UzeHome, project_root: &Path) -> Result<TaskStore> {
    Ok(read_document(&store_path(home, project_root))?.unwrap_or_default())
}

/// The one shape every version of the document shares, read before the
/// document itself.
///
/// Every field of a [`Task`] is required, so a document written under an
/// older schema fails to deserialize — `missing field ...` — long before
/// the version guard below could look at it: the guard was dead for
/// exactly the case it exists for, and what the operator saw instead was
/// a parse error about a file they never wrote.
#[derive(Deserialize)]
struct DeclaredSchema {
    schema_version: u32,
}

/// The document at `path`, or `None` when nothing was ever recorded
/// there. An error says this build cannot read what is there: the schema
/// it declares is not this one, or the bytes are not the document at all.
fn read_document(path: &Path) -> Result<Option<TaskStore>> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(path).map_err(|source| UzeError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let declared: DeclaredSchema =
        serde_json::from_slice(&bytes).map_err(|source| UzeError::Json {
            path: path.to_path_buf(),
            source,
        })?;
    if declared.schema_version != SCHEMA_VERSION {
        return Err(UzeError::UnsupportedStateSchema {
            path: path.to_path_buf(),
            found: declared.schema_version,
            expected: SCHEMA_VERSION,
        });
    }
    let store: TaskStore = serde_json::from_slice(&bytes).map_err(|source| UzeError::Json {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(Some(store))
}

/// What reading the document had to do before it could answer.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Recovery {
    /// The document this build could not read, moved out of the way with
    /// the reason it could not be read.
    pub set_aside: Option<SetAside>,
}

/// A document UZE could not read, kept rather than overwritten.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SetAside {
    /// Where the bytes are now. Nothing reads them again; they are kept
    /// because a document UZE cannot understand is still not one it may
    /// throw away. The name deliberately stops being a `.json` in this
    /// directory: what is set aside must not read as a second task
    /// document to anything that lists the directory.
    pub path: PathBuf,
    pub reason: String,
}

/// Whether a document this build cannot read is one it may set aside.
///
/// Bytes that are not the document at all, and a document from a schema
/// this build is *ahead* of: what is lost there is bookkeeping this build
/// would rewrite anyway.
///
/// Never a document from a schema ahead of this one. Setting that aside
/// takes a newer UZE's record away from it, and two builds on one machine
/// — the ordinary state of this repository, `target/debug/uze` beside
/// `~/.cargo/bin/uze` — would then take turns destroying each other's
/// records, one adoption at a time. The older build reports and leaves it
/// where it is.
fn may_be_set_aside(reason: &UzeError) -> bool {
    match reason {
        UzeError::Json { .. } => true,
        UzeError::UnsupportedStateSchema {
            found, expected, ..
        } => found < expected,
        _ => false,
    }
}

/// Moves the document aside so the project can be recorded again, and
/// says what was moved.
///
/// Only ever reached under the mutation lock: with the lock held no other
/// pass is publishing the file, so bytes that do not read are genuinely
/// unreadable rather than a write caught halfway. What the project loses
/// is UZE's own labels and publication records — `checkout::reconcile`
/// adopts every checkout Git still registers on the same pass, and the
/// work itself was never in this file to begin with.
fn set_aside(path: &Path, reason: &UzeError) -> Result<Recovery> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("tasks.json");
    let moved = path.with_file_name(format!("{name}.unreadable-{}", now_unix()));
    fs::rename(path, &moved).map_err(|source| UzeError::Write {
        path: moved.clone(),
        source,
    })?;
    tracing::warn!(
        document = %path.display(),
        set_aside = %moved.display(),
        reason = %reason,
        "the project's task document could not be read and was set aside"
    );
    Ok(Recovery {
        set_aside: Some(SetAside {
            path: moved,
            reason: reason.to_string(),
        }),
    })
}

/// Replaces the document atomically: readers see the previous version or
/// this one, never a truncated file.
pub fn save(home: &UzeHome, project_root: &Path, store: &TaskStore) -> Result<()> {
    let payload = serde_json::to_vec_pretty(store).expect("task store serialization is infallible");
    write_atomic(&store_path(home, project_root), &payload)
}

/// How long a mutation waits for another one to finish. Longer than
/// `uze_git::DEFAULT_WRITE_TIMEOUT` on purpose: this lock is the outer
/// one, and what it guards may itself wait that long for Git.
const MUTATION_TIMEOUT: Duration = Duration::from_secs(120);
const MUTATION_RETRY: Duration = Duration::from_millis(20);

/// Reads, changes and writes back the project's tasks as one operation.
///
/// [`load`] → mutate → [`save`] is not one operation without this. The
/// client evaluates, delivers, places and reconciles occupancy on four
/// threads against the same document, so two overlapping passes each write
/// back the version they read and the later one erases the other: a
/// delivered task comes back `Ready` and is delivered a second time, or a
/// placed agent's record is dropped and nothing knows its slot is taken.
///
/// A mutation that answers `Err` is not saved, so a pass that gave up
/// leaves the document exactly as it found it.
///
/// # Lock order
///
/// This is always the **outer** lock. Everything it guards speaks to Git,
/// and Git serializes itself under `uze_git`'s repository write lock;
/// taking the two the other way round anywhere would be an inversion.
/// Keep what runs inside to the read-modify-write and the Git it needs —
/// a project's `setup` command, or anything else unbounded, belongs
/// outside.
///
/// Unbounded is the word, not slow: a project's gate has half an hour and
/// the `git fetch` and `git push` a delivery makes have no bound at all.
/// Both of the passes that run one — placing an agent, delivering a task
/// — take this twice around it rather than once through it: once to write
/// down what they are about to do, once to write down what happened. The
/// state they write in between (`Integrating`, for a delivery) is what
/// every other pass reads to leave the task alone while it runs.
pub fn locked<T>(
    home: &UzeHome,
    project_root: &Path,
    mutate: impl FnOnce(&mut TaskStore) -> Result<T>,
) -> Result<T> {
    locked_reporting(home, project_root, mutate).map(|(outcome, _)| outcome)
}

/// [`locked`], saying what reading the document had to set aside first.
///
/// A document this build cannot read is not a reason to refuse the work:
/// it is how every agent in the project stops being placeable at once,
/// over a file the operator never wrote — an older UZE's schema, a hand
/// edit, corruption. The document is set aside, the project is recorded
/// again from an empty one, and the caller is handed the fact so it can
/// be said once rather than inferred from a sidebar that emptied.
///
/// Reading and setting aside both happen under the lock, which is what
/// makes the judgement safe: a reader without it can catch a write
/// halfway and would move a document that was never broken.
pub fn locked_reporting<T>(
    home: &UzeHome,
    project_root: &Path,
    mutate: impl FnOnce(&mut TaskStore) -> Result<T>,
) -> Result<(T, Recovery)> {
    let path = store_path(home, project_root);
    let _held = MutationGuard::acquire(&path)?;
    let (mut store, recovery) = match read_document(&path) {
        Ok(document) => (document.unwrap_or_default(), Recovery::default()),
        Err(reason) if may_be_set_aside(&reason) => {
            (TaskStore::default(), set_aside(&path, &reason)?)
        }
        Err(error) => return Err(error),
    };
    let outcome = mutate(&mut store)?;
    save(home, project_root, &store)?;
    Ok((outcome, recovery))
}

thread_local! {
    /// Lock files this thread already holds, so a nested mutation degrades
    /// to a lost update rather than to a deadlock against itself. No path
    /// nests today and the assertion below fails a test build over one.
    static HELD: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
}

/// Proof the document is this thread's to rewrite; releasing it is
/// dropping this.
struct MutationGuard {
    /// `None` when this thread already held the lock.
    owned: Option<(PathBuf, File)>,
}

impl MutationGuard {
    fn acquire(store: &Path) -> Result<Self> {
        let path = store.with_extension("lock");
        if HELD.with(|held| held.borrow().contains(&path)) {
            debug_assert!(
                false,
                "a task mutation nested inside another: the inner one reads a document the \
                 outer has not written yet"
            );
            return Ok(Self { owned: None });
        }
        let parent = path.parent().expect("UZE state paths have a parent");
        fs::create_dir_all(parent).map_err(|source| UzeError::Write {
            path: parent.to_path_buf(),
            source,
        })?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|source| UzeError::Write {
                path: path.clone(),
                source,
            })?;
        let started = Instant::now();
        while let Err(error) = crate::persistence::try_lock_exclusive(&file) {
            if error.kind() != std::io::ErrorKind::WouldBlock {
                return Err(UzeError::Write {
                    path,
                    source: error,
                });
            }
            if started.elapsed() >= MUTATION_TIMEOUT {
                // `flock` names no holder, so the pid is genuinely unknown
                // here rather than merely unread.
                return Err(UzeError::MutationInProgress { path, pid: None });
            }
            thread::sleep(MUTATION_RETRY);
        }
        HELD.with(|held| held.borrow_mut().push(path.clone()));
        Ok(Self {
            owned: Some((path, file)),
        })
    }
}

impl Drop for MutationGuard {
    fn drop(&mut self) {
        if let Some((path, _file)) = self.owned.take() {
            HELD.with(|held| held.borrow_mut().retain(|held| held != &path));
            // Closing the file releases the `flock`.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::HashSet,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    fn home(label: &str) -> UzeHome {
        UzeHome::at(uze_testkit::temp::scratch(label))
    }

    fn task(prompt: &str) -> Agent {
        Agent::isolated(
            "claude",
            Some(prompt),
            Base::Ref("main".into()),
            "0123abcd".into(),
            "main".into(),
        )
    }

    /// The branch half of an agent the test built isolated.
    fn isolation(agent: &Agent) -> &Isolation {
        agent.isolation().expect("the agent was built isolated")
    }

    #[test]
    fn identifiers_are_short_path_safe_and_distinct() {
        let ids: HashSet<String> = (0..500)
            .map(|_| AgentId::generate().as_str().to_owned())
            .collect();
        assert_eq!(ids.len(), 500);
        for id in &ids {
            assert_eq!(id.chars().count(), IDENTIFIER_CHARS);
            assert!(
                id.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            );
        }
    }

    #[test]
    fn the_label_comes_from_the_prompt_and_the_branch_from_the_identifier() {
        let task = task("  \nFix the auth redirect loop!\nMore details below.");
        assert_eq!(task.label, "fix-the-auth-redirect-loop");
        assert_eq!(isolation(&task).branch, format!("agent/{}", task.id));
        assert!(!isolation(&task).branch.contains(&task.label));
    }

    #[test]
    fn a_long_prompt_is_cut_at_a_word_boundary() {
        let id = AgentId::generate();
        let label = label_from_prompt(
            "Refactor the orchestrator so that every agent tab reads its state from a read model",
            &id,
        );
        assert!(label.chars().count() <= LABEL_MAX_CHARS, "{label}");
        assert!(!label.ends_with('-'));
        assert_eq!(label, "refactor-the-orchestrator-so-that-every");
    }

    #[test]
    fn an_unusable_prompt_falls_back_to_the_identifier() {
        let id = AgentId::generate();
        assert_eq!(label_from_prompt("   \n\n", &id), id.as_str());
        assert_eq!(label_from_prompt("!!! ???", &id), id.as_str());
        let unprompted = Agent::isolated(
            "claude",
            None,
            Base::Ref("main".into()),
            "x".into(),
            "main".into(),
        );
        assert_eq!(unprompted.label, unprompted.id.as_str());
    }

    /// The property the split exists for: everything keyed on the task is
    /// untouched by a new label.
    #[test]
    fn the_identifier_is_stable_while_the_label_changes() {
        let home = home("tasks-relabel");
        let root = uze_testkit::temp::scratch("tasks-relabel-project");
        let original = task("first name");
        let mut store = TaskStore::default();
        store.upsert(original.clone());
        save(&home, &root, &store).unwrap();

        let mut store = load(&home, &root).unwrap();
        store.get_mut(&original.id).unwrap().label = "second-name".into();
        save(&home, &root, &store).unwrap();

        let reloaded = load(&home, &root).unwrap();
        let task = reloaded.get(&original.id).unwrap();
        assert_eq!(task.label, "second-name");
        assert_eq!(isolation(&task).branch, isolation(&original).branch);
        assert_eq!(reloaded.agents.len(), 1);
    }

    #[test]
    fn state_survives_checkout_removal() {
        let home = home("tasks-outlive-checkout");
        let root = uze_testkit::temp::scratch("tasks-outlive-checkout-project");
        let checkout_dir = root.join(".worktrees").join("abc123");
        fs::create_dir_all(&checkout_dir).unwrap();
        let mut task = task("work that outlives its directory");
        task.isolation_mut().unwrap().checkout = Some(CheckoutId::generate());
        task.isolation_mut().unwrap().state = TaskState::Ready;
        let mut store = TaskStore::default();
        store.upsert(task.clone());
        save(&home, &root, &store).unwrap();

        fs::remove_dir_all(&checkout_dir).unwrap();

        let reloaded = load(&home, &root).unwrap();
        assert_eq!(reloaded.get(&task.id), Some(&task));
        assert!(
            store_path(&home, &root).starts_with(home.state_dir()),
            "the document lives under UZE's own state, never inside the project"
        );
    }

    #[test]
    fn a_missing_document_is_an_empty_store_and_an_unknown_schema_is_refused() {
        let home = home("tasks-schema");
        let root = uze_testkit::temp::scratch("tasks-schema-project");
        assert_eq!(load(&home, &root).unwrap(), TaskStore::default());

        let path = store_path(&home, &root);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, br#"{"schema_version": 99, "agents": []}"#).unwrap();
        let error = load(&home, &root).unwrap_err();
        assert!(
            matches!(error, UzeError::UnsupportedStateSchema { found: 99, .. }),
            "{error}"
        );
    }

    /// An agent working in the project's root has none of the vocabulary
    /// that only means anything against a branch of its own. The first
    /// round of this change kept a second record to make that true; the
    /// type now says it, which is the whole reason the work is nested
    /// inside the isolation rather than flattened beside a flag.
    #[test]
    fn an_agent_in_the_root_has_no_branch_no_readiness_and_no_delivery() {
        let agent = Agent::in_the_root("claude");

        assert!(!agent.is_isolated());
        assert!(agent.isolation().is_none(), "nothing to be ready in");
        assert!(!agent.is_named(), "no branch, so nothing to be named by");
        assert!(agent.is_live());
        assert_eq!(agent.harness, "claude");
        assert_eq!(
            agent.own_directory(Path::new("/repo")),
            Some(PathBuf::from("/repo")),
            "it stands in the project's own root"
        );
    }

    /// The other half: an isolated agent stands in its slot, and every
    /// question about a branch has somewhere to be asked.
    #[test]
    fn an_isolated_agent_stands_in_its_slot_and_carries_the_work() {
        let mut agent = task("fix the redirect");
        agent.isolation_mut().unwrap().checkout = Some(CheckoutId::adopted("slot-1"));

        assert!(agent.is_isolated());
        assert_eq!(
            agent.own_directory(Path::new("/repo")),
            Some(PathBuf::from("/repo/.worktrees/slot-1"))
        );
        assert_eq!(isolation(&agent).state, TaskState::Running);
        assert!(isolation(&agent).branch.starts_with(BRANCH_PREFIX));

        agent.take_name("fix/the-redirect".to_owned());
        assert!(agent.is_named());
        assert_eq!(
            agent.label, "the redirect",
            "the label is the branch's subject, which is what a reviewer reads"
        );
        assert_eq!(isolation(&agent).branch, "fix/the-redirect");
    }

    /// An agent whose checkout is gone stands nowhere of its own — the
    /// one case where an isolated agent has no directory, and the reason
    /// `own_directory` answers with an option at all.
    #[test]
    fn an_isolated_agent_without_its_checkout_stands_nowhere() {
        let agent = task("lost");

        assert!(agent.is_isolated());
        assert_eq!(agent.own_directory(Path::new("/repo")), None);
    }

    /// A document from an older schema is named by its version, never by
    /// serde. Every field of a `Task` is required, so the strict parse
    /// used to fail first and the version guard never ran: what the
    /// operator was shown for a state file written by a previous UZE was
    /// `failed to parse JSON in ...: missing field`, about a file they
    /// never wrote and could not act on.
    #[test]
    fn an_older_schema_is_named_by_its_version_rather_than_by_a_missing_field() {
        let home = home("tasks-older-schema");
        let root = uze_testkit::temp::scratch("tasks-older-schema-project");
        let path = store_path(&home, &root);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        // Schema 1's document: a task with the fields of the day, under
        // the key it was written with.
        fs::write(
            &path,
            br#"{"schema_version": 1, "tasks": [{"id": "abc123", "label": "a", "pushed": false}]}"#,
        )
        .unwrap();
        let error = load(&home, &root).unwrap_err();
        assert!(
            matches!(error, UzeError::UnsupportedStateSchema { found: 1, .. }),
            "{error}"
        );
    }

    /// The condition this recovery exists for: a document UZE cannot read
    /// used to refuse every mutation of the project, so no agent could be
    /// placed at all — over a file nobody authored. It is moved aside,
    /// the bytes are kept, and the work carries on.
    #[test]
    fn a_document_this_build_cannot_read_is_set_aside_rather_than_refused() {
        let home = home("tasks-set-aside");
        let root = uze_testkit::temp::scratch("tasks-set-aside-project");
        let path = store_path(&home, &root);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"{ this is not the document").unwrap();

        let (recorded, recovery) = locked_reporting(&home, &root, |store| {
            store.upsert(task("after the recovery"));
            Ok(store.agents.len())
        })
        .expect("an unreadable document is recovered from, not refused");
        assert_eq!(
            recorded, 1,
            "the project is recorded again from an empty document"
        );

        let set_aside = recovery.set_aside.expect("the recovery is reported");
        assert_eq!(
            fs::read(&set_aside.path).unwrap(),
            b"{ this is not the document",
            "the bytes UZE could not read are kept, not overwritten"
        );
        assert!(
            !set_aside.reason.is_empty(),
            "and the reason travels with them"
        );
        assert_eq!(
            load(&home, &root).unwrap().agents.len(),
            1,
            "what the mutation wrote is what the next pass reads"
        );
    }

    /// A document from a schema *ahead* of this build is left exactly
    /// where it is. Two builds on one machine is the ordinary state of
    /// this repository — a release beside a debug build — and a rule that
    /// set aside whatever it could not read would have them take turns
    /// destroying each other's records, one adoption at a time, each
    /// saying "recovered" as it went.
    #[test]
    fn a_document_from_a_newer_uze_is_refused_rather_than_set_aside() {
        let home = home("tasks-newer-schema");
        let root = uze_testkit::temp::scratch("tasks-newer-schema-project");
        let path = store_path(&home, &root);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let written = format!(
            r#"{{"schema_version": {}, "agents": [], "futures": []}}"#,
            SCHEMA_VERSION + 1
        );
        fs::write(&path, &written).unwrap();

        let refused = locked(&home, &root, |store| {
            store.upsert(task("from the older build"));
            Ok(())
        })
        .expect_err("a newer document is not this build's to rewrite");
        assert!(
            matches!(refused, UzeError::UnsupportedStateSchema { .. }),
            "{refused}"
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            written,
            "and the newer build's record is untouched"
        );
        assert!(
            !path
                .parent()
                .unwrap()
                .read_dir()
                .unwrap()
                .flatten()
                .any(|entry| entry.file_name().to_string_lossy().contains("unreadable")),
            "nothing was set aside"
        );
    }

    /// The property the lock exists for: neither writer's record is the
    /// version the other read back, however the two passes interleave.
    #[test]
    fn overlapping_mutations_do_not_erase_each_other() {
        let home = home("tasks-locked");
        let root = uze_testkit::temp::scratch("tasks-locked-project");
        let (first, second) = (task("first"), task("second"));
        locked(&home, &root, |store| {
            store.upsert(first.clone());
            store.upsert(second.clone());
            Ok(())
        })
        .unwrap();

        let deliverer = {
            let (home, root, id) = (home.clone(), root.clone(), first.id.clone());
            std::thread::spawn(move || {
                locked(&home, &root, |store| {
                    // Long enough that an unlocked pass would certainly
                    // have read this document before it is written back.
                    std::thread::sleep(Duration::from_millis(80));
                    store.get_mut(&id).unwrap().isolation_mut().unwrap().state =
                        TaskState::Integrated;
                    Ok(())
                })
                .unwrap();
            })
        };
        let evaluator = {
            let (home, root, id) = (home.clone(), root.clone(), second.id.clone());
            std::thread::spawn(move || {
                locked(&home, &root, |store| {
                    std::thread::sleep(Duration::from_millis(80));
                    store.get_mut(&id).unwrap().isolation_mut().unwrap().state = TaskState::Ready;
                    Ok(())
                })
                .unwrap();
            })
        };
        deliverer.join().unwrap();
        evaluator.join().unwrap();

        let store = load(&home, &root).unwrap();
        assert_eq!(
            store.get(&first.id).unwrap().isolation().unwrap().state,
            TaskState::Integrated
        );
        assert_eq!(
            store.get(&second.id).unwrap().isolation().unwrap().state,
            TaskState::Ready
        );
    }

    /// A mutation that gives up writes nothing, so a caller can abandon a
    /// pass without having to undo what it had already changed in memory.
    #[test]
    fn a_refused_mutation_leaves_the_document_untouched() {
        let home = home("tasks-refused");
        let root = uze_testkit::temp::scratch("tasks-refused-project");
        let seed = task("seed");
        locked(&home, &root, |store| {
            store.upsert(seed.clone());
            Ok(())
        })
        .unwrap();

        let refused = locked(&home, &root, |store| {
            store
                .get_mut(&seed.id)
                .unwrap()
                .isolation_mut()
                .unwrap()
                .state = TaskState::Integrated;
            Err::<(), _>(UzeError::UnknownTask("gave up".into()))
        });

        assert!(refused.is_err());
        assert_eq!(
            load(&home, &root)
                .unwrap()
                .get(&seed.id)
                .unwrap()
                .isolation()
                .unwrap()
                .state,
            TaskState::Running
        );
    }

    /// Saves in a tight loop until killed; the process side of the test
    /// below. Ignored so it never runs on its own.
    #[test]
    #[ignore]
    fn save_until_killed() {
        let (Some(home), Some(root)) = (
            std::env::var_os("UZE_TASK_STORE_HOME"),
            std::env::var_os("UZE_TASK_STORE_ROOT"),
        ) else {
            return;
        };
        let home = UzeHome::at(home);
        let root = PathBuf::from(root);
        let mut store = TaskStore::default();
        let mut task = task("seed");
        let mut round = 0u64;
        loop {
            round += 1;
            task.label = round.to_string();
            store.upsert(task.clone());
            save(&home, &root, &store).unwrap();
        }
    }

    #[test]
    fn a_kill_mid_write_leaves_the_previous_or_the_new_document() {
        let home = home("tasks-kill");
        let root = uze_testkit::temp::scratch("tasks-kill-project");
        let mut writer = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "project::task::tests::save_until_killed",
                "--ignored",
            ])
            .env("UZE_TASK_STORE_HOME", home.root())
            .env("UZE_TASK_STORE_ROOT", &root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let path = store_path(&home, &root);
        let started = Instant::now();
        while !path.exists() {
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "writer never wrote"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(150));
        writer.kill().unwrap();
        writer.wait().unwrap();

        let store = load(&home, &root).expect("the document is whole or previous, never torn");
        assert_eq!(store.agents.len(), 1);
        let round: u64 = store.agents[0]
            .label
            .parse()
            .expect("a label the writer produced");
        assert!(round > 0, "the writer got past its first save");
    }
}

#[cfg(test)]
mod naming_tests {
    use super::*;

    fn task() -> Agent {
        Agent::isolated(
            "claude",
            None,
            Base::Ref("main".into()),
            "0123abcd".into(),
            "main".into(),
        )
    }

    /// The one predicate the whole codebase asks, and it is about the
    /// namespace rather than the identifier: everything under `agent/` is
    /// still UZE's to name, including the branch an adopted checkout was
    /// found on.
    #[test]
    fn only_a_branch_outside_uzes_namespace_reads_as_named() {
        let mut task = task();
        assert!(!task.is_named(), "the generated identifier is not a name");
        task.isolation_mut().unwrap().branch = format!("{BRANCH_PREFIX}adopted-from-somewhere");
        assert!(
            !task.is_named(),
            "a branch inside UZE's namespace is still UZE's to name"
        );
        task.take_name("fix/branch-naming".to_owned());
        assert!(task.is_named());
    }

    #[test]
    fn taking_a_name_sets_both_halves_and_disturbs_nothing_else() {
        let mut task = task();
        let (id, checkout, created) = (
            task.id.clone(),
            task.isolation().unwrap().checkout.clone(),
            task.created_at_unix,
        );
        task.take_name("fix/branch-naming".to_owned());
        assert_eq!(task.isolation().unwrap().branch, "fix/branch-naming");
        assert_eq!(task.label, "branch naming");
        assert!(task.is_named());
        assert_eq!(task.id, id);
        assert_eq!(task.isolation().unwrap().checkout, checkout);
        assert_eq!(task.created_at_unix, created);
    }
}
