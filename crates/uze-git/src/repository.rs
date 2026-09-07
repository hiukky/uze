//! The two questions about a repository whose answer cannot change while
//! the path asked about is still there.
//!
//! Both are a `git rev-parse`, both cost a process, and both are asked on a
//! timer rather than once: the workspace client resolves a checkout's root
//! on every badge refresh, and every Git write resolves the common
//! directory to find the write lock. Over one run of the gate journeys they
//! were 890 of 3484 Git invocations — a quarter of them — for two answers
//! that never differed.
//!
//! Remembered per path asked, and only when Git answered. A failure is
//! never remembered: a directory that is not a repository yet is exactly
//! the thing `uze setup` and `git init` turn into one, and a remembered
//! "no" would outlive the truth. A path that *stops* being a repository
//! leaves a remembered answer behind, and the command that follows it fails
//! on its own — the same outcome as asking again, one process later.
//!
//! Within a repository, a path is only ever reused by a checkout of that
//! same repository (a slot handed to the next agent), so a reused path
//! answers with what it answered before.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

/// Which of the two stable answers is wanted. Part of the key, so one
/// memo serves both without either shadowing the other.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum Question {
    /// `rev-parse --show-toplevel`.
    Root,
    /// `rev-parse --path-format=absolute --git-common-dir`.
    CommonDir,
}

fn remembered() -> &'static Mutex<HashMap<(Question, PathBuf), PathBuf>> {
    static MEMO: OnceLock<Mutex<HashMap<(Question, PathBuf), PathBuf>>> = OnceLock::new();
    MEMO.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The working tree `cwd` sits in — `git rev-parse --show-toplevel`.
///
/// Doubles as "is this inside a repository at all": Git's own message on
/// stderr comes back as the error, which is what callers show.
pub fn root(cwd: &Path) -> Result<PathBuf, String> {
    answer(Question::Root, cwd, &["rev-parse", "--show-toplevel"])
}

/// The repository's common directory, absolute — `<primary>/.git` for a
/// linked worktree as much as for the checkout that owns it, which is what
/// makes it the stable identity of "which repository is this".
pub fn common_dir(cwd: &Path) -> Result<PathBuf, String> {
    answer(
        Question::CommonDir,
        cwd,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
}

fn answer(question: Question, cwd: &Path, args: &[&str]) -> Result<PathBuf, String> {
    let key = (question, cwd.to_path_buf());
    if let Ok(memo) = remembered().lock()
        && let Some(known) = memo.get(&key)
    {
        return Ok(known.clone());
    }
    let stdout = crate::read(cwd, args)
        .map_err(|error| error.to_string())?
        .successful()?;
    let resolved = PathBuf::from(stdout.trim());
    if resolved.as_os_str().is_empty() {
        return Err(format!("git {} answered with nothing", args.join(" ")));
    }
    if let Ok(mut memo) = remembered().lock() {
        memo.insert(key, resolved.clone());
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The memo must not turn "not a repository" into a standing answer:
    /// a directory becoming a repository is an ordinary event here (`uze
    /// setup` in a fresh project, a journey's world being built), and a
    /// remembered failure would outlive the truth by the life of the
    /// process.
    #[test]
    fn a_directory_that_becomes_a_repository_is_seen_as_one() {
        let scratch = uze_testkit::temp::TempDir::new("git-memo-negative");
        let path = scratch.path().join("not-yet");
        std::fs::create_dir_all(&path).unwrap();

        assert!(
            root(&path).is_err(),
            "a plain directory is not inside a repository"
        );

        crate::write(&path, &["init", "-q"])
            .expect("git init runs")
            .successful()
            .expect("git init succeeds");

        assert_eq!(
            root(&path).ok().map(|found| found.canonicalize().unwrap()),
            Some(path.canonicalize().unwrap()),
            "the same path, asked again after `git init`, is its own root"
        );
    }

    /// The answer is the same one Git gives, and asking twice does not
    /// change it — the property the memo exists to exploit.
    #[test]
    fn the_remembered_answer_is_the_one_git_gave() {
        let repository = uze_testkit::git::Repository::new("git-memo-stable");
        let path = repository.root().to_path_buf();
        repository.commit_file("f.txt", "one\n");

        let first = root(&path).expect("a repository has a root");
        let again = root(&path).expect("and still has it");
        assert_eq!(first, again);
        assert_eq!(
            first.canonicalize().unwrap(),
            path.canonicalize().unwrap(),
            "the root of a checkout is the checkout"
        );

        let common = common_dir(&path).expect("a repository has a common directory");
        assert!(
            common.ends_with(".git"),
            "an ordinary checkout's common directory is its own .git: {}",
            common.display()
        );
        assert_eq!(common, common_dir(&path).unwrap());
    }
}
