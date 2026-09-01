//! Best-effort prompt history for agent tabs (ADR-038 companion).
//!
//! # What this can and cannot know
//!
//! UZE forwards keystrokes to a PTY whose contents it never interprets, so
//! it cannot observe what an agent's own line editor did with them. The
//! caller reconstructs the submitted text client-side and is responsible
//! for discarding any reconstruction it cannot vouch for; this module only
//! stores what it is handed. The history is therefore a convenience log —
//! entries may be missing, and nothing downstream may treat it as a
//! complete record of a session.
//!
//! # Storage
//!
//! One newline-delimited JSON file per workspace under
//! `UzeHome::state_dir()/prompt-history/<workspace id>.json`, oldest line
//! first. Per-workspace rather than one shared file so a busy workspace
//! cannot evict another's entries, and append-only so recording a prompt
//! costs one small `O_APPEND` write on the caller's thread instead of a
//! read-modify-write of every entry — concurrent appends from two attached
//! clients interleave safely instead of overwriting each other. The file is
//! compacted back to `MAX_ENTRIES` once it grows past `COMPACT_ABOVE_BYTES`.
//!
//! Prompt text is user content: the file and its directory are owner-only,
//! and `clear` exists so a workspace's history can be deleted outright.

use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{
    Result, UzeError, harness_runtime::project_id_for, home::UzeHome, persistence::write_atomic,
};

/// Entries kept per workspace after a compaction.
const MAX_ENTRIES: usize = 100;
/// Enough for the widest terminal a single-line row is rendered into; the
/// stored text is never the authoritative prompt, only a label for it.
const MAX_PREVIEW_CHARS: usize = 160;
/// Appending past this size triggers a compaction. Sized so the steady
/// state is a single small read, not so tight that compaction is frequent.
const COMPACT_ABOVE_BYTES: u64 = 64 * 1024;

/// Where a prompt was submitted. Carried separately from the workspace root
/// because the root selects the file rather than living inside it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PromptOrigin {
    pub space_label: String,
    pub tab_id: u64,
    pub tab_label: String,
    /// Short agent binary/alias the tab was recognized as.
    pub agent_binary: String,
}

/// One prompt submitted to an agent tab.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PromptEntry {
    pub space_label: String,
    /// Identifies the tab to re-select when the entry is activated. Stale
    /// ids are expected — a tab can be closed after its prompt was logged.
    pub tab_id: u64,
    pub tab_label: String,
    pub agent_binary: String,
    /// The prompt, whitespace-collapsed to one line and truncated to
    /// `MAX_PREVIEW_CHARS`. Only a preview is stored: the history is a
    /// navigation aid, and keeping full prompt bodies on disk would be a
    /// larger promise about user content than this feature needs to make.
    pub preview: String,
    /// Seconds since UNIX epoch.
    pub timestamp_secs: u64,
}

impl PromptEntry {
    /// `None` when `raw_prompt` carries no text worth recording.
    pub fn new(origin: &PromptOrigin, raw_prompt: &str) -> Option<Self> {
        let preview = collapse_whitespace(raw_prompt);
        if preview.is_empty() {
            return None;
        }
        Some(Self {
            space_label: origin.space_label.clone(),
            tab_id: origin.tab_id,
            tab_label: origin.tab_label.clone(),
            agent_binary: origin.agent_binary.clone(),
            preview: truncate_chars(&preview, MAX_PREVIEW_CHARS),
            timestamp_secs: now_secs(),
        })
    }

    /// Human relative time like `2m ago`, `3h ago`, `1d ago`.
    pub fn relative_time(&self) -> String {
        let delta = now_secs().saturating_sub(self.timestamp_secs);
        if delta < 60 {
            "just now".to_owned()
        } else if delta < 3600 {
            format!("{}m ago", delta / 60)
        } else if delta < 86400 {
            format!("{}h ago", delta / 3600)
        } else {
            format!("{}d ago", delta / 86400)
        }
    }
}

/// Appends one prompt to `workspace_root`'s history. A prompt with no text
/// is silently ignored so callers need no pre-check of their own.
pub fn record(
    home: &UzeHome,
    workspace_root: &Path,
    origin: &PromptOrigin,
    raw_prompt: &str,
) -> Result<()> {
    let Some(entry) = PromptEntry::new(origin, raw_prompt) else {
        return Ok(());
    };
    let file = path(home, workspace_root);
    let mut line = serde_json::to_vec(&entry).expect("prompt entry serialization is infallible");
    line.push(b'\n');
    append_line(&file, &line)?;
    if fs::metadata(&file).is_ok_and(|metadata| metadata.len() > COMPACT_ABOVE_BYTES) {
        compact(&file)?;
    }
    Ok(())
}

/// Recent entries for `workspace_root`, newest first, capped to `limit`.
/// A missing, unreadable, or partly corrupt file yields whatever is
/// readable rather than an error — history never blocks a refresh.
pub fn list_for_workspace(home: &UzeHome, workspace_root: &Path, limit: usize) -> Vec<PromptEntry> {
    let mut entries = load(&path(home, workspace_root));
    entries.reverse();
    entries.truncate(limit);
    entries
}

/// Deletes `workspace_root`'s history. Succeeds when there is none.
pub fn clear(home: &UzeHome, workspace_root: &Path) -> Result<()> {
    let file = path(home, workspace_root);
    match fs::remove_file(&file) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(UzeError::Write { path: file, source }),
    }
}

fn path(home: &UzeHome, workspace_root: &Path) -> PathBuf {
    home.state_dir()
        .join("prompt-history")
        .join(format!("{}.json", project_id_for(workspace_root)))
}

/// Oldest first — the order the file is written in.
fn load(file: &Path) -> Vec<PromptEntry> {
    let Ok(contents) = fs::read_to_string(file) else {
        return Vec::new();
    };
    contents
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

fn append_line(file: &Path, line: &[u8]) -> Result<()> {
    let parent = file.parent().expect("UZE state paths have a parent");
    create_private_dir(parent)?;
    let mut handle = private_append_options()
        .open(file)
        .map_err(|source| UzeError::Write {
            path: file.to_path_buf(),
            source,
        })?;
    // Deliberately not fsynced: this runs on the keystroke path, and losing
    // the most recent line to a crash costs a log entry, not correctness.
    handle.write_all(line).map_err(|source| UzeError::Write {
        path: file.to_path_buf(),
        source,
    })
}

fn compact(file: &Path) -> Result<()> {
    let entries = load(file);
    let kept = entries
        .iter()
        .skip(entries.len().saturating_sub(MAX_ENTRIES));
    let mut payload = Vec::new();
    for entry in kept {
        payload.extend_from_slice(
            &serde_json::to_vec(entry).expect("prompt entry serialization is infallible"),
        );
        payload.push(b'\n');
    }
    write_atomic(file, &payload)?;
    restrict_to_owner(file);
    Ok(())
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    if dir.is_dir() {
        return Ok(());
    }
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(|source| UzeError::Write {
            path: dir.to_path_buf(),
            source,
        })
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir).map_err(|source| UzeError::Write {
        path: dir.to_path_buf(),
        source,
    })
}

#[cfg(unix)]
fn private_append_options() -> OpenOptions {
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = OpenOptions::new();
    options.create(true).append(true).mode(0o600);
    options
}

#[cfg(not(unix))]
fn private_append_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    options
}

/// `write_atomic` renames a fresh file into place, so a compaction would
/// otherwise reset the mode the append path established.
#[cfg(unix)]
fn restrict_to_owner(file: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(file, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_to_owner(_file: &Path) {}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// One rendered row is one line, so a multi-line prompt has to become one
/// too — and collapsing keeps the words of the second line rather than
/// discarding everything after the first newline.
fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_chars(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_owned();
    }
    let mut truncated: String = value.chars().take(max.saturating_sub(1)).collect();
    truncated.push('…');
    truncated
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempHome {
        home: UzeHome,
    }

    impl TempHome {
        fn new(label: &str) -> Self {
            Self {
                home: UzeHome::at(uze_testkit::temp::scratch(label)),
            }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(self.home.root());
        }
    }

    fn origin(tab_id: u64, agent: &str) -> PromptOrigin {
        PromptOrigin {
            space_label: "space 1".into(),
            tab_id,
            tab_label: "tab 1".into(),
            agent_binary: agent.into(),
        }
    }

    #[test]
    fn blank_prompts_are_never_recorded() {
        assert!(PromptEntry::new(&origin(1, "agent"), "   \n\t ").is_none());
    }

    #[test]
    fn a_multi_line_prompt_collapses_into_one_row() {
        let entry = PromptEntry::new(&origin(1, "agent"), "first line\n\n  second line  ").unwrap();
        assert_eq!(entry.preview, "first line second line");
    }

    #[test]
    fn preview_truncates_to_the_stored_limit() {
        let entry = PromptEntry::new(&origin(1, "agent"), &"a".repeat(500)).unwrap();
        assert_eq!(entry.preview.chars().count(), MAX_PREVIEW_CHARS);
        assert!(entry.preview.ends_with('…'));
    }

    #[test]
    fn each_workspace_keeps_its_own_history() {
        let temp = TempHome::new("per-workspace");
        let (a, b) = (Path::new("/tmp/ws-a"), Path::new("/tmp/ws-b"));

        record(&temp.home, a, &origin(1, "agent-a"), "prompt a").unwrap();
        record(&temp.home, b, &origin(2, "agent-b"), "prompt b").unwrap();

        let listed_a = list_for_workspace(&temp.home, a, 10);
        assert_eq!(listed_a.len(), 1);
        assert_eq!(listed_a[0].preview, "prompt a");

        let listed_b = list_for_workspace(&temp.home, b, 10);
        assert_eq!(listed_b.len(), 1);
        assert_eq!(listed_b[0].preview, "prompt b");
    }

    #[test]
    fn listing_is_newest_first_and_capped() {
        let temp = TempHome::new("ordering");
        let root = Path::new("/tmp/ws");
        for index in 0..5 {
            record(
                &temp.home,
                root,
                &origin(index, "agent"),
                &format!("p{index}"),
            )
            .unwrap();
        }

        let listed = list_for_workspace(&temp.home, root, 3);
        let previews: Vec<&str> = listed.iter().map(|entry| entry.preview.as_str()).collect();
        assert_eq!(previews, ["p4", "p3", "p2"]);
    }

    #[test]
    fn a_corrupt_line_is_skipped_without_losing_the_rest() {
        let temp = TempHome::new("corrupt");
        let root = Path::new("/tmp/ws");
        record(&temp.home, root, &origin(1, "agent"), "good").unwrap();

        let file = path(&temp.home, root);
        let mut contents = fs::read_to_string(&file).unwrap();
        contents.push_str("not json\n");
        fs::write(&file, contents).unwrap();
        record(&temp.home, root, &origin(1, "agent"), "later").unwrap();

        let previews: Vec<String> = list_for_workspace(&temp.home, root, 10)
            .into_iter()
            .map(|entry| entry.preview)
            .collect();
        assert_eq!(previews, ["later", "good"]);
    }

    #[test]
    fn compaction_caps_the_file_at_max_entries() {
        let temp = TempHome::new("compaction");
        let root = Path::new("/tmp/ws");
        let file = path(&temp.home, root);
        // Seed past the compaction threshold in one write, then let the
        // next recorded prompt trigger the compaction.
        let mut payload = Vec::new();
        for index in 0..4000 {
            let entry = PromptEntry::new(&origin(index, "agent"), &format!("p{index}")).unwrap();
            payload.extend_from_slice(&serde_json::to_vec(&entry).unwrap());
            payload.push(b'\n');
        }
        write_atomic(&file, &payload).unwrap();
        assert!(fs::metadata(&file).unwrap().len() > COMPACT_ABOVE_BYTES);

        record(&temp.home, root, &origin(9999, "agent"), "newest").unwrap();

        let remaining = load(&file);
        assert_eq!(remaining.len(), MAX_ENTRIES);
        assert_eq!(remaining.last().unwrap().preview, "newest");
        assert!(fs::metadata(&file).unwrap().len() < COMPACT_ABOVE_BYTES);
    }

    #[test]
    fn clear_removes_only_the_named_workspace_and_tolerates_absence() {
        let temp = TempHome::new("clear");
        let (a, b) = (Path::new("/tmp/ws-a"), Path::new("/tmp/ws-b"));
        record(&temp.home, a, &origin(1, "agent"), "prompt a").unwrap();
        record(&temp.home, b, &origin(2, "agent"), "prompt b").unwrap();

        clear(&temp.home, a).unwrap();
        assert!(list_for_workspace(&temp.home, a, 10).is_empty());
        assert_eq!(list_for_workspace(&temp.home, b, 10).len(), 1);

        clear(&temp.home, a).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn history_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let temp = TempHome::new("permissions");
        let root = Path::new("/tmp/ws");
        record(&temp.home, root, &origin(1, "agent"), "prompt").unwrap();

        let file = path(&temp.home, root);
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&file), 0o600);
        assert_eq!(mode(file.parent().unwrap()), 0o700);
    }
}
