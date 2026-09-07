//! Conversation continuity for OpenCode.
//!
//! OpenCode names its own conversation and continues one with
//! `--session <id>`. Its sessions live in a database UZE will not open;
//! what it does read is the harness's own API — `opencode api GET
//! /api/session` — which lists each conversation with the directory it
//! belongs to and when it was created. Asking the vendor through the
//! interface it publishes is both cheaper to keep working than a private
//! store and the only one it documents.
//!
//! That call spawns the real binary, so it belongs off the launch path: the
//! read-back runs where a background answer already runs, never in the shim.
//!
//! This is also the only channel for this harness. The others tell UZE which
//! conversation they are in through a hook dispatch UZE itself runs; here the
//! generated plugin *is* the runner and spawns the handler directly, so no
//! UZE process ever sees the payload. Nothing is lost — the listing answers
//! the same question — but it is answered on the refresh cadence rather than
//! at the agent's next tool call.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use uze_core::{conversation::SessionId, integration::ObservationContext};

use crate::shared::process;

const RESUME_FLAG: &str = "--session";
const LISTING_ARGS: [&str; 3] = ["api", "GET", "/api/session"];
/// The API answers in milliseconds; every comparison here is in seconds.
const MILLISECONDS: u64 = 1_000;

pub(super) fn resume_args(session: &SessionId) -> Vec<OsString> {
    vec![
        OsString::from(RESUME_FLAG),
        OsString::from(session.as_str()),
    ]
}

/// Not needed here: the listing timestamps every conversation, so the
/// launch time is guard enough.
pub(super) fn recorded_for(_cwd: &Path) -> Option<SessionId> {
    None
}

/// One conversation as the harness lists it.
struct Listed {
    id: String,
    directory: String,
    created_unix: u64,
}

/// The newest conversation the harness lists for `ctx.cwd` since the launch.
pub(super) fn observe(
    executable: &Path,
    home: &Path,
    ctx: &ObservationContext,
) -> Option<SessionId> {
    listing(executable, home)?
        .into_iter()
        .filter(|listed| {
            Path::new(&listed.directory) == ctx.cwd && listed.created_unix >= ctx.since_unix
        })
        .max_by_key(|listed| listed.created_unix)
        .map(|listed| SessionId::new(listed.id))
}

/// Whether the harness still lists `session`.
pub(super) fn exists(executable: &Path, home: &Path, session: &SessionId) -> bool {
    // Unreadable is not gone: a harness that cannot answer keeps its
    // conversation, and a resume that then fails is its own error rather
    // than a conversation UZE dropped on a guess.
    let Some(listed) = listing(executable, home) else {
        return true;
    };
    listed.iter().any(|entry| entry.id == session.as_str())
}

fn listing(executable: &Path, home: &Path) -> Option<Vec<Listed>> {
    let output = process::capture(executable, home, &LISTING_ARGS).ok()?;
    if !output.status.success() {
        return None;
    }
    parse_listing(&output.stdout)
}

fn parse_listing(payload: &[u8]) -> Option<Vec<Listed>> {
    let document: serde_json::Value = serde_json::from_slice(payload).ok()?;
    let entries = document.get("data")?.as_array()?;
    Some(
        entries
            .iter()
            .filter_map(|entry| {
                Some(Listed {
                    id: entry
                        .get("id")
                        .and_then(serde_json::Value::as_str)?
                        .to_owned(),
                    directory: entry
                        .get("location")?
                        .get("directory")
                        .and_then(serde_json::Value::as_str)?
                        .to_owned(),
                    created_unix: entry
                        .get("time")?
                        .get("created")
                        .and_then(serde_json::Value::as_u64)?
                        / MILLISECONDS,
                })
            })
            .collect(),
    )
}

/// The real binary behind the harness, for the API call. `None` when it is
/// not installed, which reads as "nothing to observe".
pub(super) fn executable(shims_dir: &Path) -> Option<PathBuf> {
    uze_core::harness_runtime::resolve_real_executable(&["opencode", "opencode2"], shims_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(entries: &[(&str, &str, u64)]) -> Vec<u8> {
        let data: Vec<serde_json::Value> = entries
            .iter()
            .map(|(id, directory, created)| {
                serde_json::json!({
                    "id": id,
                    "location": {"directory": directory},
                    "time": {"created": created * MILLISECONDS},
                })
            })
            .collect();
        serde_json::to_vec(&serde_json::json!({"data": data})).unwrap()
    }

    fn observed(entries: &[(&str, &str, u64)], cwd: &str, since: u64) -> Option<SessionId> {
        let listed = parse_listing(&payload(entries)).unwrap();
        listed
            .into_iter()
            .filter(|listed| {
                Path::new(&listed.directory) == Path::new(cwd) && listed.created_unix >= since
            })
            .max_by_key(|listed| listed.created_unix)
            .map(|listed| SessionId::new(listed.id))
    }

    #[test]
    fn resuming_names_the_session() {
        assert_eq!(
            resume_args(&SessionId::new("ses_1")),
            vec![OsString::from("--session"), OsString::from("ses_1")]
        );
    }

    #[test]
    fn the_newest_conversation_for_the_checkout_is_the_one_observed() {
        let entries = [
            ("ses_old", "/work/slot", 100),
            ("ses_new", "/work/slot", 200),
            ("ses_other", "/work/other", 300),
        ];
        assert_eq!(
            observed(&entries, "/work/slot", 50),
            Some(SessionId::new("ses_new"))
        );
    }

    #[test]
    fn a_conversation_from_before_the_launch_is_never_adopted() {
        let entries = [("ses_previous", "/work/slot", 100)];
        assert_eq!(observed(&entries, "/work/slot", 500), None);
    }

    #[test]
    fn an_answer_that_is_not_a_listing_is_no_listing_at_all() {
        assert!(parse_listing(b"<!doctype html>").is_none());
    }
}
