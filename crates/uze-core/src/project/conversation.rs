//! The conversation an agent is in, remembered for the task it belongs to.
//!
//! # Bound to the task, never to the directory
//!
//! A slot is reused: the directory an agent stands in was somebody else's
//! yesterday and will be somebody else's tomorrow. A conversation keyed on
//! the directory would therefore hand the next task the previous one's
//! history. Keyed on the task, a recycled slot simply finds nothing, and a
//! task given its slot back finds exactly what it left.
//!
//! # Advisory, not authoritative
//!
//! Everything here answers one question — should this launch resume, and
//! what — and the answer "no idea" is always safe: the agent starts fresh.
//! So an unreadable, corrupt or unknown-schema document reads as *no
//! record* rather than as an error. The task store refuses a schema it does
//! not know because guessing at a task's state could destroy work; this
//! document only decides whether a conversation is carried over, and
//! refusing to launch over it would trade a lost conversation for a lost
//! agent.
//!
//! # Storage
//!
//! One JSON document per task, under
//! `UzeHome::state_dir()/conversations/<project id>/<task id>.json` — the
//! same project key `state/tasks/<project id>.json` uses, so both are
//! outside every checkout by construction. One file per task rather than a
//! field on the task store, because a launch writes this and launches
//! happen in their own processes: two agents starting at once would
//! otherwise rewrite one document and drop each other's work.

use std::{
    collections::BTreeMap,
    fmt, fs,
    io::Read,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{
    Result, digest,
    harness_runtime::project_id_for,
    home::UzeHome,
    persistence::write_atomic,
    task::{self, TaskId},
    worktree,
};

pub const SCHEMA_VERSION: u32 = 1;

/// A conversation as its harness names it. Opaque here on purpose: the
/// only code entitled to read it is the integration that resumes with it.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SessionId(String);

impl SessionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// A fresh RFC 4122 version 4 identifier, for a harness that lets UZE
    /// name the conversation it is about to start. The shape is not a
    /// preference: the harnesses that accept a name demand a UUID and
    /// refuse anything else.
    pub fn generate() -> Self {
        Self(format_uuid_v4(random_bytes()))
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Sixteen bytes of the best entropy available. `/dev/urandom` when it can
/// be read; otherwise a digest of time, process and a per-process counter —
/// the same material task identifiers are minted from. This names a local
/// conversation, so uniqueness on one machine is the whole requirement.
fn random_bytes() -> [u8; 16] {
    let mut bytes = [0u8; 16];
    if let Ok(mut source) = fs::File::open("/dev/urandom")
        && source.read_exact(&mut bytes).is_ok()
    {
        return bytes;
    }
    let seed = digest::fnv1a64(task::generated_identifier(b"conversation").as_bytes());
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos() as u64)
        .unwrap_or_default();
    bytes[..8].copy_from_slice(&seed.to_le_bytes());
    bytes[8..].copy_from_slice(&nanos.to_le_bytes());
    bytes
}

fn format_uuid_v4(mut bytes: [u8; 16]) -> String {
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

/// How the identifier in a record got there, which is what the next launch
/// needs to know: one that UZE named is minted again for a new task, one
/// the harness named is waited for.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversationOrigin {
    Assigned,
    Observed,
}

/// One harness's conversation for one task.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HarnessConversation {
    /// `None` while a launch is still waiting to be read back — the pending
    /// state is the absence of an answer, not a flag beside it.
    pub conversation: Option<SessionId>,
    pub origin: ConversationOrigin,
    /// The floor a read-back accepts a conversation above, and the token an
    /// answer carries back so a launch that has since been replaced cannot
    /// be overwritten by the previous one's answer.
    pub launched_at_unix: u64,
    pub observed_at_unix: Option<u64>,
    /// What the harness's own records pointed at for this checkout when
    /// this launch started, for a harness that keeps one entry per
    /// directory instead of timestamping its conversations. Reading back
    /// that same value means nothing new was started, so the previous
    /// occupant's conversation can never be adopted.
    pub preceded_by: Option<SessionId>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConversationRecord {
    pub schema_version: u32,
    pub task: TaskId,
    /// Keyed by integration id, so a fifth harness adds a key rather than a
    /// shape.
    pub harnesses: BTreeMap<String, HarnessConversation>,
}

impl ConversationRecord {
    pub fn new(task: TaskId) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            task,
            harnesses: BTreeMap::new(),
        }
    }

    pub fn get(&self, integration: &str) -> Option<&HarnessConversation> {
        self.harnesses.get(integration)
    }

    /// Records a launch, replacing whatever the previous one left. An
    /// assigned conversation is known here; an observed one is not, and
    /// stays pending until it is read back.
    pub fn launched(
        &mut self,
        integration: &str,
        origin: ConversationOrigin,
        conversation: Option<SessionId>,
        preceded_by: Option<SessionId>,
    ) {
        self.harnesses.insert(
            integration.to_owned(),
            HarnessConversation {
                conversation,
                origin,
                launched_at_unix: now_unix(),
                observed_at_unix: None,
                preceded_by,
            },
        );
    }

    /// Writes back what was observed for `launched_at_unix`'s launch.
    ///
    /// Returns whether it applied: an answer whose launch this record no
    /// longer names is an answer to a launch that has been replaced, and is
    /// dropped rather than written — the same rule the client applies to a
    /// Git answer that arrives after the viewer moved on.
    pub fn observed(
        &mut self,
        integration: &str,
        launched_at_unix: u64,
        conversation: SessionId,
    ) -> bool {
        let Some(entry) = self.harnesses.get_mut(integration) else {
            return false;
        };
        if entry.launched_at_unix != launched_at_unix {
            return false;
        }
        entry.conversation = Some(conversation);
        entry.observed_at_unix = Some(now_unix());
        true
    }

    /// Drops a conversation the harness no longer holds, so the next launch
    /// starts one instead of resuming into nothing.
    pub fn forget_harness(&mut self, integration: &str) {
        self.harnesses.remove(integration);
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

/// The document for one task of `project_root`.
pub fn store_path(home: &UzeHome, project_root: &Path, task: &TaskId) -> PathBuf {
    let canonical = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    home.conversation_path(&project_id_for(&canonical), task.as_str())
}

/// What was recorded for `task`, or an empty record. Never fails: see the
/// module's note on why continuity state is advisory.
pub fn load(home: &UzeHome, project_root: &Path, task: &TaskId) -> ConversationRecord {
    let path = store_path(home, project_root, task);
    let empty = || ConversationRecord::new(task.clone());
    let Ok(bytes) = fs::read(&path) else {
        return empty();
    };
    match serde_json::from_slice::<ConversationRecord>(&bytes) {
        Ok(record) if record.schema_version == SCHEMA_VERSION => record,
        _ => empty(),
    }
}

/// Replaces the document atomically.
pub fn save(home: &UzeHome, project_root: &Path, record: &ConversationRecord) -> Result<()> {
    let payload =
        serde_json::to_vec_pretty(record).expect("conversation record serialization is infallible");
    write_atomic(&store_path(home, project_root, &record.task), &payload)
}

/// Forgets a task's conversations. Best-effort by construction: a record
/// that is already gone is the outcome asked for.
pub fn forget(home: &UzeHome, project_root: &Path, task: &TaskId) {
    let _ = fs::remove_file(store_path(home, project_root, task));
}

/// The task a directory belongs to, and the checkout it stands in.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Owner {
    /// The primary checkout the isolated one belongs to — the project root
    /// every record for this task is keyed on.
    pub primary: PathBuf,
    pub task: TaskId,
}

/// Whose task is this directory?
///
/// Lexical against the isolation layout plus one small read of the task
/// store: no subprocess, nothing that scales with the Store, because a
/// harness launch waits on this. `None` for a directory no managed task
/// owns — the operator's own checkout, or anywhere else on the machine —
/// which is what keeps an ordinary invocation ordinary.
///
/// The newest task naming the checkout is its owner, the same rule slot
/// occupancy is decided by: a recycled slot's previous tenants keep naming
/// it in their own records forever, and only the last one holds it.
pub fn owner_of(home: &UzeHome, cwd: &Path) -> Option<Owner> {
    let checkout = worktree::isolated_checkout(cwd)?;
    let primary = checkout.primary.to_path_buf();
    let store = task::load(home, &primary).ok()?;
    let task = store
        .tasks
        .iter()
        .filter(|task| {
            task.checkout
                .as_ref()
                .is_some_and(|id| id.as_str() == checkout.name)
        })
        .max_by_key(|task| task.created_at_unix)?;
    Some(Owner {
        primary,
        task: task.id.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::{Base, Task, TaskStore};

    fn home(label: &str) -> UzeHome {
        UzeHome::at(uze_testkit::temp::scratch(label))
    }

    /// A project root of its own per test, so two tests never key the same
    /// document.
    fn project(label: &str) -> PathBuf {
        let root = uze_testkit::temp::scratch(label).join("repo");
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn task_named(checkout: &str) -> Task {
        let mut task = Task::new(None, Base::Ref("main".into()), String::new(), "main".into());
        task.checkout = Some(crate::checkout::CheckoutId::adopted(checkout));
        task
    }

    #[test]
    fn a_minted_identifier_is_a_version_4_uuid() {
        let id = SessionId::generate();
        let text = id.as_str();
        assert_eq!(text.len(), 36, "{text}");
        assert_eq!(
            text.chars().filter(|c| *c == '-').count(),
            4,
            "grouping: {text}"
        );
        assert_eq!(&text[14..15], "4", "version nibble: {text}");
        assert!(
            matches!(&text[19..20], "8" | "9" | "a" | "b"),
            "variant nibble: {text}"
        );
        assert_ne!(SessionId::generate(), SessionId::generate());
    }

    #[test]
    fn a_record_round_trips_through_the_document() {
        let home = home("conversation-round-trip");
        let root = project("conversation-round-trip-project");
        let task = TaskId::generate();

        let mut record = ConversationRecord::new(task.clone());
        record.launched(
            "claude-code",
            ConversationOrigin::Assigned,
            Some(SessionId::new("c-1")),
            None,
        );
        save(&home, &root, &record).unwrap();

        let read = load(&home, &root, &task);
        assert_eq!(
            read.get("claude-code").and_then(|e| e.conversation.clone()),
            Some(SessionId::new("c-1"))
        );
    }

    #[test]
    fn an_unreadable_document_reads_as_no_record_rather_than_as_an_error() {
        let home = home("conversation-unreadable");
        let root = project("conversation-unreadable-project");
        let task = TaskId::generate();
        let path = store_path(&home, &root, &task);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"{ not json").unwrap();

        assert!(load(&home, &root, &task).harnesses.is_empty());
    }

    #[test]
    fn a_document_from_a_schema_this_build_does_not_know_is_ignored_not_refused() {
        let home = home("conversation-unknown-schema");
        let root = project("conversation-unknown-schema-project");
        let task = TaskId::generate();
        let path = store_path(&home, &root, &task);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, br#"{"schema_version":99,"task":"x","harnesses":{}}"#).unwrap();

        assert!(load(&home, &root, &task).harnesses.is_empty());
    }

    #[test]
    fn each_harness_keeps_its_own_conversation() {
        let mut record = ConversationRecord::new(TaskId::generate());
        record.launched(
            "claude-code",
            ConversationOrigin::Assigned,
            Some(SessionId::new("c-1")),
            None,
        );
        record.launched("codex", ConversationOrigin::Observed, None, None);

        assert_eq!(
            record.get("claude-code").unwrap().conversation,
            Some(SessionId::new("c-1"))
        );
        assert_eq!(record.get("codex").unwrap().conversation, None);
    }

    #[test]
    fn an_answer_to_a_replaced_launch_is_dropped() {
        let mut record = ConversationRecord::new(TaskId::generate());
        record.launched("codex", ConversationOrigin::Observed, None, None);
        let stale = record.get("codex").unwrap().launched_at_unix;

        // The pane was relaunched, so the entry names a different launch.
        record.launched("codex", ConversationOrigin::Observed, None, None);
        record.harnesses.get_mut("codex").unwrap().launched_at_unix = stale + 1;

        assert!(!record.observed("codex", stale, SessionId::new("late")));
        assert_eq!(record.get("codex").unwrap().conversation, None);
        assert!(record.observed("codex", stale + 1, SessionId::new("current")));
        assert_eq!(
            record.get("codex").unwrap().conversation,
            Some(SessionId::new("current"))
        );
    }

    #[test]
    fn a_directory_outside_the_isolation_layout_owns_no_task() {
        let home = home("conversation-no-owner");
        assert_eq!(
            owner_of(&home, &project("conversation-no-owner-project")),
            None
        );
    }

    #[test]
    fn the_newest_task_naming_a_checkout_owns_it() {
        let home = home("conversation-owner");
        let primary = project("conversation-owner-project");
        let slot = primary.join(".worktrees").join("slot-1");
        fs::create_dir_all(&slot).unwrap();

        let mut store = TaskStore::default();
        let mut previous = task_named("slot-1");
        previous.created_at_unix = 10;
        let mut current = task_named("slot-1");
        current.created_at_unix = 20;
        let expected = current.id.clone();
        store.upsert(previous);
        store.upsert(current);
        task::save(&home, &primary, &store).unwrap();

        let owner = owner_of(&home, &slot).expect("the slot has an owner");
        assert_eq!(owner.task, expected);
        assert_eq!(owner.primary, primary);
    }

    #[test]
    fn a_recycled_slots_new_task_finds_nothing_the_previous_one_left() {
        let home = home("conversation-recycled");
        let root = project("conversation-recycled-project");

        let previous = TaskId::generate();
        let mut record = ConversationRecord::new(previous.clone());
        record.launched(
            "claude-code",
            ConversationOrigin::Assigned,
            Some(SessionId::new("old")),
            None,
        );
        save(&home, &root, &record).unwrap();

        let recycled = TaskId::generate();
        assert!(load(&home, &root, &recycled).harnesses.is_empty());
        // And the previous task's own record is untouched by that.
        assert!(load(&home, &root, &previous).get("claude-code").is_some());
    }

    #[test]
    fn forgetting_a_task_takes_its_record_with_it() {
        let home = home("conversation-forget");
        let root = project("conversation-forget-project");
        let task = TaskId::generate();
        let mut record = ConversationRecord::new(task.clone());
        record.launched("codex", ConversationOrigin::Observed, None, None);
        save(&home, &root, &record).unwrap();

        forget(&home, &root, &task);

        assert!(!store_path(&home, &root, &task).exists());
        assert!(load(&home, &root, &task).harnesses.is_empty());
    }
}
