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
    task::{self, AgentId},
};

pub const SCHEMA_VERSION: u32 = 2;

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
    pub agent: AgentId,
    /// Keyed by integration id, so a fifth harness adds a key rather than a
    /// shape.
    pub harnesses: BTreeMap<String, HarnessConversation>,
}

impl ConversationRecord {
    pub fn new(agent: AgentId) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            agent,
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

/// The document for one agent of `project_root`.
pub fn store_path(home: &UzeHome, project_root: &Path, agent: &AgentId) -> PathBuf {
    let canonical = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    home.conversation_path(&project_id_for(&canonical), agent.as_str())
}

/// What was recorded for `task`, or an empty record. Never fails: see the
/// module's note on why continuity state is advisory.
pub fn load(home: &UzeHome, project_root: &Path, task: &AgentId) -> ConversationRecord {
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
    write_atomic(&store_path(home, project_root, &record.agent), &payload)
}

/// Forgets a task's conversations. Best-effort by construction: a record
/// that is already gone is the outcome asked for.
pub fn forget(home: &UzeHome, project_root: &Path, task: &AgentId) {
    let _ = fs::remove_file(store_path(home, project_root, task));
}

/// The agent a verified claim resolved to, and the project root every
/// record for it is keyed on.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Owner {
    pub primary: PathBuf,
    pub agent: AgentId,
}

/// What a process says about itself: the identifier its launch carried,
/// and the directory it stands in. The two are verified together — the
/// identifier says *which* agent, the directory says *where* it should be,
/// and a claim the record contradicts is no claim at all.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Claim<'a> {
    pub id: &'a str,
    pub cwd: &'a Path,
}

/// Whose agent is this process?
///
/// Ancestors of the claimed directory, nearest first, are asked whether a
/// store keyed on them names the identifier; the first that does answers,
/// provided the record's own directory contains the claimed one. A few
/// `stat`s and one small read: no subprocess, nothing that scales with the
/// Store, because a harness launch waits on this. `None` for a claim no
/// record backs — an identifier nobody recorded, a directory the record
/// does not allow — which is what keeps an ordinary invocation ordinary
/// and keeps a process that edits its own environment inside the directory
/// its record already gave it.
pub fn owner_of(home: &UzeHome, claim: Claim<'_>) -> Option<Owner> {
    let cwd = claim
        .cwd
        .canonicalize()
        .unwrap_or_else(|_| claim.cwd.to_path_buf());
    cwd.ancestors().find_map(|root| {
        if !task::store_path(home, root).exists() {
            return None;
        }
        let store = task::load(home, root).ok()?;
        let record = store.agent(claim.id)?;
        let own = record.own_directory(root)?;
        let own = own.canonicalize().unwrap_or(own);
        cwd.starts_with(&own).then(|| Owner {
            primary: root.to_path_buf(),
            agent: record.id().clone(),
        })
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
        let task = AgentId::generate();

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
        let task = AgentId::generate();
        let path = store_path(&home, &root, &task);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"{ not json").unwrap();

        assert!(load(&home, &root, &task).harnesses.is_empty());
    }

    #[test]
    fn a_document_from_a_schema_this_build_does_not_know_is_ignored_not_refused() {
        let home = home("conversation-unknown-schema");
        let root = project("conversation-unknown-schema-project");
        let task = AgentId::generate();
        let path = store_path(&home, &root, &task);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            br#"{"schema_version":99,"agent":"x","harnesses":{}}"#,
        )
        .unwrap();

        assert!(load(&home, &root, &task).harnesses.is_empty());
    }

    #[test]
    fn each_harness_keeps_its_own_conversation() {
        let mut record = ConversationRecord::new(AgentId::generate());
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
        let mut record = ConversationRecord::new(AgentId::generate());
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
    fn a_claim_no_record_backs_has_no_owner() {
        let home = home("conversation-no-owner");
        let primary = project("conversation-no-owner-project");
        let slot = primary.join(".worktrees").join("slot-1");
        fs::create_dir_all(&slot).unwrap();
        let mut store = TaskStore::default();
        let task = task_named("slot-1");
        let id = task.id.as_str().to_owned();
        store.upsert(task);
        task::save(&home, &primary, &store).unwrap();

        assert_eq!(
            owner_of(
                &home,
                Claim {
                    id: "unrecorded",
                    cwd: &slot
                }
            ),
            None,
            "an identifier nobody recorded"
        );
        assert_eq!(
            owner_of(
                &home,
                Claim {
                    id: &id,
                    cwd: &primary
                }
            ),
            None,
            "the operator's own checkout is not the task's directory"
        );
        let elsewhere = project("conversation-no-owner-elsewhere");
        assert_eq!(
            owner_of(
                &home,
                Claim {
                    id: &id,
                    cwd: &elsewhere
                }
            ),
            None,
            "a directory outside the project"
        );
    }

    #[test]
    fn a_verified_claim_resolves_to_its_own_record_wherever_inside_its_directory() {
        let home = home("conversation-owner");
        let primary = project("conversation-owner-project");
        let slot = primary.join(".worktrees").join("slot-1");
        let nested = slot.join("src").join("deep");
        fs::create_dir_all(&nested).unwrap();

        let mut store = TaskStore::default();
        let previous = task_named("slot-1");
        let current = task_named("slot-1");
        let previous_id = previous.id.clone();
        let current_id = current.id.clone();
        store.upsert(previous);
        store.upsert(current);
        task::save(&home, &primary, &store).unwrap();

        let owner = owner_of(
            &home,
            Claim {
                id: current_id.as_str(),
                cwd: &nested,
            },
        )
        .expect("the claim is backed by its record");
        assert_eq!(owner.agent, current_id);
        assert_eq!(owner.primary, primary.canonicalize().unwrap());
        // Two records over one slot are told apart by the identifier alone.
        let previous_owner = owner_of(
            &home,
            Claim {
                id: previous_id.as_str(),
                cwd: &slot,
            },
        )
        .expect("the earlier tenant of the slot still resolves by its own id");
        assert_eq!(previous_owner.agent, previous_id);
    }

    #[test]
    fn a_recycled_slots_new_task_finds_nothing_the_previous_one_left() {
        let home = home("conversation-recycled");
        let root = project("conversation-recycled-project");

        let previous = AgentId::generate();
        let mut record = ConversationRecord::new(previous.clone());
        record.launched(
            "claude-code",
            ConversationOrigin::Assigned,
            Some(SessionId::new("old")),
            None,
        );
        save(&home, &root, &record).unwrap();

        let recycled = AgentId::generate();
        assert!(load(&home, &root, &recycled).harnesses.is_empty());
        // And the previous task's own record is untouched by that.
        assert!(load(&home, &root, &previous).get("claude-code").is_some());
    }

    #[test]
    fn forgetting_a_task_takes_its_record_with_it() {
        let home = home("conversation-forget");
        let root = project("conversation-forget-project");
        let task = AgentId::generate();
        let mut record = ConversationRecord::new(task.clone());
        record.launched("codex", ConversationOrigin::Observed, None, None);
        save(&home, &root, &record).unwrap();

        forget(&home, &root, &task);

        assert!(!store_path(&home, &root, &task).exists());
        assert!(load(&home, &root, &task).harnesses.is_empty());
    }
}
