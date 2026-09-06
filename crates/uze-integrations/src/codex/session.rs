//! Conversation continuity for Codex.
//!
//! Codex names its own conversation and continues one by id —
//! `codex resume <id>`, a subcommand rather than a flag, which is why it is
//! only ever contributed to a launch that carries nothing else.
//!
//! The identifier is read back from Codex's own rollout files, whose first
//! line is a `session_meta` record naming the conversation and the working
//! directory it was started in. Only that first line is ever read: it is the
//! index Codex publishes for its own picker, and the rest of the file is
//! the conversation itself, which is none of UZE's business.

use std::{
    ffi::OsString,
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use uze_core::{conversation::SessionId, integration::ObservationContext};

const RESUME_SUBCOMMAND: &str = "resume";
const ROLLOUT_EXTENSION: &str = "jsonl";
/// Rollouts are filed under `sessions/<year>/<month>/<day>/`.
const ROLLOUT_DEPTH: usize = 3;

pub(super) fn resume_args(session: &SessionId) -> Vec<OsString> {
    vec![
        OsString::from(RESUME_SUBCOMMAND),
        OsString::from(session.as_str()),
    ]
}

/// Not needed here: rollouts carry their own start time, so the launch time
/// is guard enough for telling this task's conversation from the one the
/// previous occupant of the checkout left behind.
pub(super) fn recorded_for(_cwd: &Path) -> Option<SessionId> {
    None
}

/// The newest conversation Codex recorded for `ctx.cwd` since the launch.
pub(super) fn observe(sessions_root: &Path, ctx: &ObservationContext) -> Option<SessionId> {
    let mut newest: Option<(u64, SessionId)> = None;
    for (path, modified) in rollouts(sessions_root) {
        if modified < ctx.since_unix {
            continue;
        }
        let Some(meta) = session_meta(&path) else {
            continue;
        };
        if Path::new(&meta.cwd) != ctx.cwd {
            continue;
        }
        if newest.as_ref().is_none_or(|(latest, _)| modified > *latest) {
            newest = Some((modified, SessionId::new(meta.session_id)));
        }
    }
    newest.map(|(_, session)| session)
}

/// Whether a rollout for `session` is still filed. The identifier is the
/// tail of the file's own name, so this reads directory entries and never
/// opens one.
pub(super) fn exists(sessions_root: &Path, session: &SessionId) -> bool {
    let suffix = format!("-{}", session.as_str());
    rollouts(sessions_root).any(|(path, _)| {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| stem.ends_with(&suffix))
    })
}

/// Every rollout file with its modification time, walking only the fixed
/// date layout so an unrelated directory dropped into the sessions tree is
/// never descended into.
fn rollouts(sessions_root: &Path) -> impl Iterator<Item = (PathBuf, u64)> {
    let mut level = vec![sessions_root.to_path_buf()];
    for _ in 0..ROLLOUT_DEPTH {
        level = level.iter().flat_map(|path| directories(path)).collect();
    }
    level
        .into_iter()
        .flat_map(|day| fs::read_dir(day).into_iter().flatten().flatten())
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some(ROLLOUT_EXTENSION) {
                return None;
            }
            let modified = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|elapsed| elapsed.as_secs())
                .unwrap_or_default();
            Some((path, modified))
        })
}

fn directories(path: &Path) -> Vec<PathBuf> {
    fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect()
}

struct SessionMeta {
    session_id: String,
    cwd: String,
}

/// The `session_meta` record Codex writes as a rollout's first line.
fn session_meta(path: &Path) -> Option<SessionMeta> {
    let file = fs::File::open(path).ok()?;
    let mut first = String::new();
    BufReader::new(file).read_line(&mut first).ok()?;
    let record: serde_json::Value = serde_json::from_str(&first).ok()?;
    if record.get("type").and_then(serde_json::Value::as_str) != Some("session_meta") {
        return None;
    }
    let payload = record.get("payload")?;
    Some(SessionMeta {
        session_id: payload
            .get("session_id")
            .and_then(serde_json::Value::as_str)?
            .to_owned(),
        cwd: payload
            .get("cwd")
            .and_then(serde_json::Value::as_str)?
            .to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rollout(root: &Path, id: &str, cwd: &str, at_unix: u64) {
        let day = root.join("2026").join("09").join("06");
        fs::create_dir_all(&day).unwrap();
        let path = day.join(format!("rollout-2026-09-06T00-00-00-{id}.jsonl"));
        fs::write(
            &path,
            format!(
                "{}\n{}\n",
                serde_json::json!({
                    "type": "session_meta",
                    "payload": {"session_id": id, "cwd": cwd},
                }),
                serde_json::json!({"type": "user"}),
            ),
        )
        .unwrap();
        let time = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(at_unix);
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(time)
            .unwrap();
    }

    fn scratch(label: &str) -> PathBuf {
        uze_testkit::temp::scratch(label).join("sessions")
    }

    #[test]
    fn resuming_is_a_subcommand_and_carries_the_identifier() {
        assert_eq!(
            resume_args(&SessionId::new("01a05afb")),
            vec![OsString::from("resume"), OsString::from("01a05afb")]
        );
    }

    #[test]
    fn the_newest_conversation_recorded_for_the_checkout_is_the_one_observed() {
        let root = scratch("codex-session-observe");
        rollout(&root, "older", "/work/slot", 100);
        rollout(&root, "newer", "/work/slot", 200);
        rollout(&root, "elsewhere", "/work/other", 300);

        assert_eq!(
            observe(
                &root,
                &ObservationContext {
                    cwd: Path::new("/work/slot"),
                    since_unix: 50,
                    preceded_by: None,
                },
            ),
            Some(SessionId::new("newer"))
        );
    }

    #[test]
    fn a_conversation_from_before_the_launch_is_never_adopted() {
        let root = scratch("codex-session-previous");
        rollout(&root, "previous-tenant", "/work/slot", 100);

        assert_eq!(
            observe(
                &root,
                &ObservationContext {
                    cwd: Path::new("/work/slot"),
                    since_unix: 500,
                    preceded_by: None,
                },
            ),
            None
        );
    }

    #[test]
    fn a_conversation_exists_only_while_its_rollout_is_filed() {
        let root = scratch("codex-session-exists");
        rollout(&root, "kept", "/work/slot", 100);

        assert!(exists(&root, &SessionId::new("kept")));
        assert!(!exists(&root, &SessionId::new("deleted")));
    }
}
