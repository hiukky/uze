//! Conversation continuity for Antigravity CLI.
//!
//! It names its own conversation and continues one with
//! `--conversation <id>`. The identifier is read back from the index the
//! harness maintains for its own `--continue`: a plain
//! `cache/last_conversations.json` mapping a working directory to the last
//! conversation held there. The conversations themselves are one database
//! per conversation, and UZE opens none of them — the index is the whole
//! read.
//!
//! That index keeps one entry per directory and timestamps nothing, so the
//! launch time cannot be the guard here. What is recorded instead is the
//! value the index already held for this checkout when the launch started:
//! reading that same value back means nothing new was started, and the
//! conversation still named there belongs to whoever ran before.

use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

use uze_core::{conversation::SessionId, integration::ObservationContext};

const RESUME_FLAG: &str = "--conversation";

pub(super) fn resume_args(session: &SessionId) -> Vec<OsString> {
    vec![
        OsString::from(RESUME_FLAG),
        OsString::from(session.as_str()),
    ]
}

pub(super) fn index_path(cli_root: &Path) -> PathBuf {
    cli_root.join("cache").join("last_conversations.json")
}

/// What the index already names for `cwd`.
pub(super) fn recorded_for(cli_root: &Path, cwd: &Path) -> Option<SessionId> {
    index(cli_root)?
        .get(cwd.to_string_lossy().as_ref())
        .map(|id| SessionId::new(id.clone()))
}

/// The conversation the index names for `ctx.cwd`, once it is one this
/// launch could have started.
pub(super) fn observe(cli_root: &Path, ctx: &ObservationContext) -> Option<SessionId> {
    let current = recorded_for(cli_root, ctx.cwd)?;
    // Unchanged since the launch: the harness has not written a
    // conversation of its own yet, and what is there belongs to the
    // previous occupant of this checkout.
    if ctx.preceded_by == Some(&current) {
        return None;
    }
    Some(current)
}

/// Whether a conversation's own store is still there. The store is named by
/// the identifier, so this never opens one.
pub(super) fn exists(cli_root: &Path, session: &SessionId) -> bool {
    cli_root
        .join("conversations")
        .join(format!("{}.db", session.as_str()))
        .is_file()
}

fn index(cli_root: &Path) -> Option<BTreeMap<String, String>> {
    let bytes = fs::read(index_path(cli_root)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cli_root(label: &str) -> PathBuf {
        uze_testkit::temp::scratch(label).join("antigravity-cli")
    }

    fn write_index(root: &Path, entries: &[(&str, &str)]) {
        let map: BTreeMap<&str, &str> = entries.iter().copied().collect();
        let path = index_path(root);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, serde_json::to_vec(&map).unwrap()).unwrap();
    }

    #[test]
    fn resuming_names_the_conversation() {
        assert_eq!(
            resume_args(&SessionId::new("c-1")),
            vec![OsString::from("--conversation"), OsString::from("c-1")]
        );
    }

    #[test]
    fn the_conversation_the_index_names_for_the_checkout_is_the_one_observed() {
        let root = cli_root("agy-session-observe");
        write_index(
            &root,
            &[("/work/slot", "c-new"), ("/work/other", "c-other")],
        );

        assert_eq!(
            observe(
                &root,
                &ObservationContext {
                    cwd: Path::new("/work/slot"),
                    since_unix: 0,
                    preceded_by: Some(&SessionId::new("c-previous")),
                },
            ),
            Some(SessionId::new("c-new"))
        );
    }

    /// The recycled-slot case this harness needs a value guard for: the
    /// index still names the previous task's conversation because this
    /// launch has not written one yet.
    #[test]
    fn an_index_entry_unchanged_since_the_launch_is_not_this_task_s_conversation() {
        let root = cli_root("agy-session-previous");
        write_index(&root, &[("/work/slot", "c-previous")]);

        assert_eq!(
            observe(
                &root,
                &ObservationContext {
                    cwd: Path::new("/work/slot"),
                    since_unix: 0,
                    preceded_by: Some(&SessionId::new("c-previous")),
                },
            ),
            None
        );
    }

    #[test]
    fn what_the_index_held_at_launch_is_readable_on_its_own() {
        let root = cli_root("agy-session-recorded");
        write_index(&root, &[("/work/slot", "c-1")]);

        assert_eq!(
            recorded_for(&root, Path::new("/work/slot")),
            Some(SessionId::new("c-1"))
        );
        assert_eq!(recorded_for(&root, Path::new("/work/unknown")), None);
    }

    #[test]
    fn a_conversation_exists_only_while_its_own_store_does() {
        let root = cli_root("agy-session-exists");
        fs::create_dir_all(root.join("conversations")).unwrap();
        fs::write(root.join("conversations").join("kept.db"), b"").unwrap();

        assert!(exists(&root, &SessionId::new("kept")));
        assert!(!exists(&root, &SessionId::new("deleted")));
    }
}
