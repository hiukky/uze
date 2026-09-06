//! Conversation continuity for Claude Code.
//!
//! The one harness that lets UZE name the conversation before it starts:
//! `--session-id <uuid>` creates it, `--resume <uuid>` continues it. The two
//! are not interchangeable — a second `--session-id` for the same
//! conversation is refused ("Session ID … is already in use") — which is
//! exactly why the choice is made per launch rather than baked into a
//! persisted command.
//!
//! Its transcripts are read for two things only, never for content: proving
//! a recorded conversation still exists before resuming into it, and
//! noticing that the agent has since moved to another one (a cleared or
//! forked conversation is a new identifier in the same directory).

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use uze_core::{conversation::SessionId, integration::ObservationContext};

const START_FLAG: &str = "--session-id";
const RESUME_FLAG: &str = "--resume";
const TRANSCRIPT_EXTENSION: &str = "jsonl";

pub(super) fn start_args(session: &SessionId) -> Vec<OsString> {
    vec![OsString::from(START_FLAG), OsString::from(session.as_str())]
}

pub(super) fn resume_args(session: &SessionId) -> Vec<OsString> {
    vec![
        OsString::from(RESUME_FLAG),
        OsString::from(session.as_str()),
    ]
}

/// Claude Code keeps one transcript per conversation under a directory
/// named after the working directory, with every character that is not
/// alphanumeric replaced by a hyphen (`/home/x/.worktrees/y` →
/// `-home-x--worktrees-y`).
fn transcripts_dir(projects_root: &Path, cwd: &Path) -> PathBuf {
    let slug: String = cwd
        .to_string_lossy()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect();
    projects_root.join(slug)
}

/// Whether the transcript for `session` is still there. The identifier is
/// the file's own name, so this is one `exists` call and no parsing.
pub(super) fn exists(projects_root: &Path, cwd: &Path, session: &SessionId) -> bool {
    transcripts_dir(projects_root, cwd)
        .join(format!("{}.{TRANSCRIPT_EXTENSION}", session.as_str()))
        .is_file()
}

/// The newest conversation started in `cwd` since the launch.
///
/// Claude Code names its own conversation when it is not told which to use,
/// and moves a running agent into a new one when the person clears or forks
/// it. Reading the newest transcript back is how a record stays true to
/// where the work actually is.
pub(super) fn observe(projects_root: &Path, ctx: &ObservationContext) -> Option<SessionId> {
    let directory = transcripts_dir(projects_root, ctx.cwd);
    let mut newest: Option<(u64, SessionId)> = None;
    for entry in fs::read_dir(directory).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some(TRANSCRIPT_EXTENSION) {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or_default();
        if modified < ctx.since_unix {
            continue;
        }
        if newest.as_ref().is_none_or(|(latest, _)| modified > *latest) {
            newest = Some((modified, SessionId::new(name)));
        }
    }
    newest.map(|(_, session)| session)
}

/// Not needed here: transcripts carry their own timestamps, so the launch
/// time is guard enough for telling a new conversation from the one the
/// previous task left in this checkout.
pub(super) fn recorded_for(_cwd: &Path) -> Option<SessionId> {
    None
}

/// Marks `path` as modified now, so a freshly created transcript is never
/// filtered out by a launch recorded in the same second.
#[cfg(test)]
fn touch(path: &Path, at_unix: u64) {
    let time = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(at_unix);
    let file = fs::File::options().write(true).open(path).unwrap();
    file.set_modified(time).unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transcript(root: &Path, cwd: &Path, id: &str, at_unix: u64) {
        let directory = transcripts_dir(root, cwd);
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join(format!("{id}.jsonl"));
        fs::write(&path, b"{}\n").unwrap();
        touch(&path, at_unix);
    }

    fn scratch(label: &str) -> PathBuf {
        uze_testkit::temp::scratch(label)
    }

    #[test]
    fn the_transcript_directory_is_the_working_directory_with_every_separator_flattened() {
        let root = Path::new("/root");
        assert_eq!(
            transcripts_dir(root, Path::new("/home/x/.worktrees/y")),
            root.join("-home-x--worktrees-y")
        );
    }

    #[test]
    fn starting_and_resuming_are_different_arguments() {
        let session = SessionId::new("11111111-2222-4333-8444-555555555555");
        assert_eq!(
            start_args(&session),
            vec![
                OsString::from("--session-id"),
                OsString::from("11111111-2222-4333-8444-555555555555")
            ]
        );
        assert_eq!(
            resume_args(&session),
            vec![
                OsString::from("--resume"),
                OsString::from("11111111-2222-4333-8444-555555555555")
            ]
        );
    }

    #[test]
    fn a_conversation_exists_only_while_its_transcript_does() {
        let root = scratch("claude-session-exists");
        let cwd = Path::new("/work/slot");
        transcript(&root, cwd, "kept", 100);

        assert!(exists(&root, cwd, &SessionId::new("kept")));
        assert!(!exists(&root, cwd, &SessionId::new("deleted")));
        assert!(!exists(
            &root,
            Path::new("/work/other"),
            &SessionId::new("kept")
        ));
    }

    /// The clear/fork case: the agent is no longer in the conversation it
    /// started in, and the record has to follow it.
    #[test]
    fn the_newest_conversation_since_the_launch_is_the_one_observed() {
        let root = scratch("claude-session-observe");
        let cwd = Path::new("/work/slot");
        transcript(&root, cwd, "started-with", 100);
        transcript(&root, cwd, "moved-to", 200);

        let observed = observe(
            &root,
            &ObservationContext {
                cwd,
                since_unix: 50,
                preceded_by: None,
            },
        );
        assert_eq!(observed, Some(SessionId::new("moved-to")));
    }

    /// A slot is reused, and the previous task's conversation is still
    /// sitting in the same directory.
    #[test]
    fn a_conversation_from_before_the_launch_is_never_adopted() {
        let root = scratch("claude-session-previous");
        let cwd = Path::new("/work/slot");
        transcript(&root, cwd, "previous-tenant", 100);

        assert_eq!(
            observe(
                &root,
                &ObservationContext {
                    cwd,
                    since_unix: 500,
                    preceded_by: None,
                },
            ),
            None
        );
    }
}
