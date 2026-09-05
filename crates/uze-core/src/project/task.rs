//! A task: one agent launch UZE made in a project, from the moment it
//! started to the moment its work reached the target.
//!
//! # Identity is not the label
//!
//! A task's identifier is generated once and never changes; it keys the
//! branch (`agent/<id>`), the checkout it runs in, and its persisted state.
//! The label is derived from the prompt and names the tab. Keeping them
//! apart is what makes a name free to change and a collision impossible.
//!
//! # Storage
//!
//! One JSON document per project under `UzeHome::state_dir()/tasks/<project
//! id>.json`, outside every checkout by construction, so removing a
//! worktree can never remove history. Written atomically, and carrying a
//! schema version from the first commit: a document from a schema this
//! build does not know is refused, never guessed at.

use std::{
    fmt, fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{
    Result, UzeError, checkout::CheckoutId, digest, harness_runtime::project_id_for, home::UzeHome,
    persistence::write_atomic, worktree::BRANCH_PREFIX,
};

pub const SCHEMA_VERSION: u32 = 1;

/// Long enough to read, short enough for a sidebar.
const LABEL_MAX_CHARS: usize = 40;
const IDENTIFIER_CHARS: usize = 6;

/// A generated, immutable task identifier.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct TaskId(String);

impl TaskId {
    pub fn generate() -> Self {
        Self(generated_identifier(b"task"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The branch the task's work lives on while it stays local.
    pub fn branch(&self) -> String {
        format!("{BRANCH_PREFIX}{}", self.0)
    }
}

impl fmt::Display for TaskId {
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

/// Where a task's branch starts: a ref (normally the target), or another
/// task's tip. The second variant is carried without behaviour today so
/// stacking is never a migration.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Base {
    Ref(String),
    Task(TaskId),
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
pub struct Task {
    pub id: TaskId,
    pub label: String,
    pub base: Base,
    /// The base's tip when the branch was last rebased onto it — what a
    /// restack would use as the old base, so the parent's commits are never
    /// replayed into a child.
    pub base_commit: String,
    pub target: String,
    pub branch: String,
    pub checkout: Option<CheckoutId>,
    pub state: TaskState,
    /// Frozen once the branch has left the machine.
    pub pushed: bool,
    /// The readable name the branch was published under, once it was.
    #[serde(default)]
    pub published_as: Option<String>,
    /// The number of the request open on the forge for the published
    /// branch, once one was found. Read off the remote like every other
    /// readiness fact, never announced by the agent that opened it.
    #[serde(default)]
    pub published_request: Option<u32>,
    /// The branch's tip as the remote last received it. What a sync would
    /// send is measured against this and never against the target: the
    /// request already carries what was pushed, however far the branch
    /// still is from the target it will land in.
    #[serde(default)]
    pub published_tip: Option<String>,
    pub created_at_unix: u64,
}

impl Task {
    pub fn new(prompt: Option<&str>, base: Base, base_commit: String, target: String) -> Self {
        let id = TaskId::generate();
        let label = prompt
            .map(|prompt| label_from_prompt(prompt, &id))
            .unwrap_or_else(|| id.as_str().to_owned());
        let branch = id.branch();
        Self {
            id,
            label,
            base,
            base_commit,
            target,
            branch,
            checkout: None,
            state: TaskState::Running,
            pushed: false,
            published_as: None,
            published_request: None,
            published_tip: None,
            created_at_unix: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|elapsed| elapsed.as_secs())
                .unwrap_or_default(),
        }
    }
}

/// The first non-empty line of `prompt`, lower-cased, non-alphanumerics
/// collapsed to single hyphens, cut at a word boundary; the identifier when
/// nothing usable remains.
pub fn label_from_prompt(prompt: &str, fallback: &TaskId) -> String {
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TaskStore {
    pub schema_version: u32,
    pub tasks: Vec<Task>,
}

impl Default for TaskStore {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            tasks: Vec::new(),
        }
    }
}

impl TaskStore {
    pub fn get(&self, id: &TaskId) -> Option<&Task> {
        self.tasks.iter().find(|task| &task.id == id)
    }

    pub fn get_mut(&mut self, id: &TaskId) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|task| &task.id == id)
    }

    /// Adds or replaces by identifier.
    pub fn upsert(&mut self, task: Task) {
        match self.get_mut(&task.id) {
            Some(existing) => *existing = task,
            None => self.tasks.push(task),
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
    let path = store_path(home, project_root);
    if !path.exists() {
        return Ok(TaskStore::default());
    }
    let bytes = fs::read(&path).map_err(|source| UzeError::Read {
        path: path.clone(),
        source,
    })?;
    let store: TaskStore = serde_json::from_slice(&bytes).map_err(|source| UzeError::Json {
        path: path.clone(),
        source,
    })?;
    if store.schema_version != SCHEMA_VERSION {
        return Err(UzeError::UnsupportedStateSchema {
            path,
            found: store.schema_version,
            expected: SCHEMA_VERSION,
        });
    }
    Ok(store)
}

/// Replaces the document atomically: readers see the previous version or
/// this one, never a truncated file.
pub fn save(home: &UzeHome, project_root: &Path, store: &TaskStore) -> Result<()> {
    let payload = serde_json::to_vec_pretty(store).expect("task store serialization is infallible");
    write_atomic(&store_path(home, project_root), &payload)
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

    fn task(prompt: &str) -> Task {
        Task::new(
            Some(prompt),
            Base::Ref("main".into()),
            "0123abcd".into(),
            "main".into(),
        )
    }

    #[test]
    fn identifiers_are_short_path_safe_and_distinct() {
        let ids: HashSet<String> = (0..500)
            .map(|_| TaskId::generate().as_str().to_owned())
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
        assert_eq!(task.branch, format!("agent/{}", task.id));
        assert!(!task.branch.contains(&task.label));
    }

    #[test]
    fn a_long_prompt_is_cut_at_a_word_boundary() {
        let id = TaskId::generate();
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
        let id = TaskId::generate();
        assert_eq!(label_from_prompt("   \n\n", &id), id.as_str());
        assert_eq!(label_from_prompt("!!! ???", &id), id.as_str());
        let unprompted = Task::new(None, Base::Ref("main".into()), "x".into(), "main".into());
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
        assert_eq!(task.branch, original.branch);
        assert_eq!(reloaded.tasks.len(), 1);
    }

    #[test]
    fn state_survives_checkout_removal() {
        let home = home("tasks-outlive-checkout");
        let root = uze_testkit::temp::scratch("tasks-outlive-checkout-project");
        let checkout_dir = root.join(".worktrees").join("abc123");
        fs::create_dir_all(&checkout_dir).unwrap();
        let mut task = task("work that outlives its directory");
        task.checkout = Some(CheckoutId::generate());
        task.state = TaskState::Ready;
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
        fs::write(&path, br#"{"schema_version": 99, "tasks": []}"#).unwrap();
        let error = load(&home, &root).unwrap_err();
        assert!(
            matches!(error, UzeError::UnsupportedStateSchema { found: 99, .. }),
            "{error}"
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
        assert_eq!(store.tasks.len(), 1);
        let round: u64 = store.tasks[0]
            .label
            .parse()
            .expect("a label the writer produced");
        assert!(round > 0, "the writer got past its first save");
    }
}
