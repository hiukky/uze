//! What UZE has recorded about one project, and how a sweep finds it.
//!
//! A project's records — its agents, their conversations, its prompt
//! history — live in one directory keyed by
//! [`harness_runtime::project_id_for`], which is a one-way hash. So the
//! directory names the root it was written for, and that one fact is what
//! makes the id reversible: a sweep of [`UzeHome::projects_dir`] can see
//! every project UZE knows *and* locate each one's checkouts, which four
//! directories keyed by the same hash and no marker between them could
//! not.
//!
//! It is the same answer [`harness_runtime::PROJECTION_MARKER`] already
//! gives the generated tree, for the same reason: naming the root makes a
//! sweep a `readdir` rather than an exercise in reading back what
//! something derived.

use std::{fs, path::Path, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::{error::Result, harness_runtime::project_id_for, home::UzeHome};

/// The canonical root a project's records were written for.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProjectMarker {
    pub root: PathBuf,
}

/// A record: it is the only thing that says what a project id means, and
/// nothing on the machine re-derives it.
impl uze_document::Shaped for ProjectMarker {
    const SHAPE: u32 = uze_document::FIRST_SHAPE;
    const KIND: &'static str = "project";
}

/// The directory holding `project_root`'s records, created and marked with
/// the root it belongs to.
///
/// Idempotent, and cheap on the path that matters: the marker is written
/// only when it is absent or names a different root, so the ordinary case
/// is a `stat` and a read. Called by every writer of a project record
/// before it writes, so a project that has records always has a marker and
/// a sweep never meets one without the other.
pub fn ensure(home: &UzeHome, project_root: &Path) -> Result<PathBuf> {
    let canonical = canonical(project_root);
    let id = project_id_for(&canonical);
    let directory = home.project_dir(&id);
    let fresh = !directory.is_dir();
    create_private(&directory)?;
    if fresh {
        carry_across_the_move(home, &id, &directory);
    }
    let marker = home.project_marker_path(&id);
    let recorded = uze_document::read::<ProjectMarker>(&marker)
        .ok()
        .and_then(uze_document::Carried::record);
    if recorded.is_none_or(|recorded| recorded.root != canonical) {
        let payload = serde_json::to_vec_pretty(&ProjectMarker { root: canonical })
            .expect("a project marker serializes");
        crate::persistence::write_atomic(&marker, &payload)?;
    }
    Ok(directory)
}

/// Every project root UZE has recorded anything for.
///
/// A directory whose marker cannot be read is skipped rather than refused:
/// one project's records being unreadable must not withhold every other
/// project's, which is the whole reason a sweep exists.
pub fn roots(home: &UzeHome) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(home.projects_dir()) else {
        return Vec::new();
    };
    let mut roots: Vec<PathBuf> = entries
        .filter_map(std::result::Result::ok)
        .filter_map(|entry| {
            let marker = entry.path().join("project.json");
            uze_document::read::<ProjectMarker>(&marker)
                .ok()?
                .record()
                .map(|marker| marker.root)
        })
        .collect();
    roots.sort();
    roots.dedup();
    roots
}

/// Removes everything UZE recorded about `project_root`.
///
/// One removal, which is what the directory buys: the same act used to be
/// four, and forgetting three of them left records nothing could resolve.
pub fn forget(home: &UzeHome, project_root: &Path) -> Result<()> {
    let directory = home.project_dir(&project_id_for(&canonical(project_root)));
    match fs::remove_dir_all(&directory) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(crate::error::UzeError::Write {
            path: directory,
            source,
        }),
    }
}

/// The rung for the release that gathered a project's records into one
/// directory.
///
/// Moving a path *is* a version change, and a version change with no rung
/// is how state gets lost — which is the whole reason this module exists.
/// A machine coming from the previous release has its agents at
/// `state/tasks/<id>.json`, its conversations at
/// `state/conversations/<id>/` and its prompt history at
/// `state/prompt-history/<id>.json`; leaving them there would have this
/// build open on a project with no agents and every checkout orphaned.
///
/// Best-effort and silent, like every other climb: what it carries is
/// UZE's own bookkeeping, and a rename that fails leaves the bytes exactly
/// where they were for the operator to find. Runs once — only when the
/// project directory is being created — and is removable once no machine
/// can still be on the previous layout.
fn carry_across_the_move(home: &UzeHome, id: &str, directory: &Path) {
    let state = home.state_dir();
    let carried: [(PathBuf, PathBuf); 3] = [
        (
            state.join("tasks").join(format!("{id}.json")),
            directory.join("agents.json"),
        ),
        (
            state.join("conversations").join(id),
            directory.join("conversations"),
        ),
        (
            state.join("prompt-history").join(format!("{id}.json")),
            directory.join("prompt-history.json"),
        ),
    ];
    for (was, now) in carried {
        if was.exists() && !now.exists() {
            let _ = fs::rename(&was, &now);
        }
    }
    // The lock beside the old store is not carried: it guards a path that
    // no longer exists, and the new store mints its own. Left behind it is
    // a file nothing will ever open again.
    let _ = fs::remove_file(state.join("tasks").join(format!("{id}.lock")));
    // And the directories the three used to live in, when nothing else is
    // in them. `remove_dir` refuses a directory with anything left, so a
    // project still on the old layout keeps its own.
    for emptied in ["tasks", "conversations", "prompt-history"] {
        let _ = fs::remove_dir(state.join(emptied));
    }
}

/// Owner-only, because of what is in here.
///
/// A project's records hold the prompts its agents were given — which is
/// the operator's, and was `0700` when prompt history had a directory of
/// its own. Gathering the records into one directory must not quietly
/// widen that: the least private thing decides the mode, so the whole
/// directory takes the strictest one any of its contents needs.
#[cfg(unix)]
fn create_private(directory: &Path) -> Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    if directory.is_dir() {
        return Ok(());
    }
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(directory)
        .map_err(|source| crate::error::UzeError::Write {
            path: directory.to_path_buf(),
            source,
        })
}

#[cfg(not(unix))]
fn create_private(directory: &Path) -> Result<()> {
    fs::create_dir_all(directory).map_err(|source| crate::error::UzeError::Write {
        path: directory.to_path_buf(),
        source,
    })
}

/// The root as the marker records it. A path that cannot be canonicalized
/// — the repository moved or went — is kept verbatim: the records are
/// still that project's, and a sweep that dropped them would be the bug
/// this module exists to prevent.
fn canonical(project_root: &Path) -> PathBuf {
    project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(name: &str) -> (UzeHome, PathBuf) {
        let root = uze_testkit::temp::scratch(name);
        fs::create_dir_all(&root).unwrap();
        (UzeHome::at(&root), root)
    }

    #[test]
    fn a_projects_records_name_the_repository_they_belong_to() {
        let (home, scratch) = home("project-marker");
        let project = scratch.join("demo");
        fs::create_dir_all(&project).unwrap();

        ensure(&home, &project).unwrap();

        assert_eq!(
            roots(&home),
            vec![project.canonicalize().unwrap()],
            "the id is a one-way hash, and this is what makes it reversible"
        );
        fs::remove_dir_all(scratch).unwrap();
    }

    #[test]
    fn forgetting_a_project_is_one_removal_and_touches_no_other() {
        let (home, scratch) = home("project-forget");
        let kept = scratch.join("kept");
        let gone = scratch.join("gone");
        fs::create_dir_all(&kept).unwrap();
        fs::create_dir_all(&gone).unwrap();
        ensure(&home, &kept).unwrap();
        ensure(&home, &gone).unwrap();

        forget(&home, &gone).unwrap();

        assert_eq!(
            roots(&home),
            vec![kept.canonicalize().unwrap()],
            "one project's records go, and no other project's do"
        );
        fs::remove_dir_all(scratch).unwrap();
    }

    /// The repository being gone is exactly when the records matter most:
    /// they are what says the work existed at all.
    #[test]
    fn a_project_whose_repository_is_gone_is_still_readable_and_still_named() {
        let (home, scratch) = home("project-vanished");
        let project = scratch.join("vanished");
        fs::create_dir_all(&project).unwrap();
        ensure(&home, &project).unwrap();
        let recorded = project.canonicalize().unwrap();
        fs::remove_dir_all(&project).unwrap();

        assert_eq!(
            roots(&home),
            vec![recorded],
            "the records still name the root they were written for"
        );
        fs::remove_dir_all(scratch).unwrap();
    }

    #[test]
    fn one_unreadable_project_does_not_withhold_the_others() {
        let (home, scratch) = home("project-unreadable");
        let good = scratch.join("good");
        fs::create_dir_all(&good).unwrap();
        ensure(&home, &good).unwrap();
        let broken = home.project_dir("deadbeefdeadbeef");
        fs::create_dir_all(&broken).unwrap();
        fs::write(broken.join("project.json"), b"not a marker").unwrap();

        assert_eq!(
            roots(&home),
            vec![good.canonicalize().unwrap()],
            "a sweep answers for every project it can read"
        );
        fs::remove_dir_all(scratch).unwrap();
    }

    /// The release that gathered these records into one directory moved
    /// three paths, and a moved path is a version change like any other:
    /// without a rung, a machine coming from the previous release opens on
    /// a project with no agents and every checkout orphaned.
    #[test]
    fn the_previous_layouts_records_are_carried_into_the_directory() {
        let (home, scratch) = home("project-carried");
        let project = scratch.join("demo");
        fs::create_dir_all(&project).unwrap();
        let canonical = project.canonicalize().unwrap();
        let id = project_id_for(&canonical);

        // Where the previous release left them.
        let state = home.state_dir();
        fs::create_dir_all(state.join("tasks")).unwrap();
        fs::create_dir_all(state.join("conversations").join(&id)).unwrap();
        fs::create_dir_all(state.join("prompt-history")).unwrap();
        fs::write(
            state.join("tasks").join(format!("{id}.json")),
            br#"{"schema_version": 3, "agents": []}"#,
        )
        .unwrap();
        fs::write(
            state.join("conversations").join(&id).join("abc123.json"),
            b"{}",
        )
        .unwrap();
        fs::write(
            state.join("prompt-history").join(format!("{id}.json")),
            b"{}\n",
        )
        .unwrap();

        ensure(&home, &project).unwrap();

        assert!(
            home.tasks_path(&id).exists(),
            "the agents come with the project, rather than being left behind"
        );
        assert!(
            home.conversation_path(&id, "abc123").exists(),
            "and so does what each of them was talking to"
        );
        assert!(
            home.prompt_history_path(&id).exists(),
            "and the prompts they were given"
        );
        assert!(
            !state.join("tasks").join(format!("{id}.json")).exists(),
            "nothing is left at the old name to be read as a second record"
        );
        fs::remove_dir_all(scratch).unwrap();
    }

    #[test]
    fn marking_a_project_twice_is_one_marker() {
        let (home, scratch) = home("project-idempotent");
        let project = scratch.join("demo");
        fs::create_dir_all(&project).unwrap();

        ensure(&home, &project).unwrap();
        ensure(&home, &project).unwrap();

        assert_eq!(roots(&home).len(), 1);
        fs::remove_dir_all(scratch).unwrap();
    }
}
