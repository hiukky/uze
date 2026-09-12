//! What a checkout currently has that its last commit does not: the
//! compact summary the tab strip badges, and the changed-file list the
//! overlay navigates.
//!
//! Its own module because "what changed" is a question with one answer
//! and several readers — the badge asks it on a timer and the overlay
//! asks it on every refresh — so the parsing of Git's porcelain has to
//! be one thing rather than the same shape written twice.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::Instant,
};

use super::{
    changes_tree::{FileTreeItem, tree_items},
    diff::{DiffRow, highlight_diff_rows, pair_side_by_side, parse_unified_diff},
    repository_root, run_git,
};
use crate::{
    Host,
    view::{Role, ScrollDirection},
};

/// What the checkout has that its last commit does not, and the diff of
/// whichever file the surface is on.
///
/// One of the two halves [`super::CodeView`] holds. It answers about a
/// path and knows nothing about the other half — see that type for why
/// the two stay apart.
#[derive(Default)]
pub(super) struct Changes {
    pub(super) files: Vec<ChangedFile>,
    /// The directories folded shut in the navigator, by the path
    /// [`FileTreeItem::Directory`] gives them. A fold never moves the
    /// selection: the diff being read stays the diff being read, its row
    /// just stops being drawn until the directory opens again.
    pub(super) folded: BTreeSet<String>,
    pub(super) diff: Vec<DiffRow>,
    /// Set when the selection moved and cleared when a read catches up.
    ///
    /// Reading and highlighting a diff is the one thing here whose cost
    /// has no bound, and an arrow key may not pay for it on the thread
    /// that draws. So selecting records only *what* is selected; the host
    /// re-reads, and until it answers this says the diff on screen is not
    /// the one being asked for.
    pub(super) diff_pending: bool,
    /// Set instead of populating `files`/`diff` when the checkout isn't a
    /// git repository, `git` isn't on `PATH`, or a `git diff` fails —
    /// shown in place of the changed-file list rather than refusing to
    /// open at all.
    pub(super) error: Option<String>,
    pub(super) refreshed_at: Option<Instant>,
}

impl Changes {
    /// Everything `git status` says, plus the diff of `selected`.
    ///
    /// Takes no `&self`: this is the slow half of the surface — a
    /// `status`, a `diff`, and the highlighting of that diff — and an
    /// associated function can run wherever the host puts it. The host
    /// reads on a thread and installs the answer when it lands, which is
    /// why nothing here may borrow the view being refreshed.
    ///
    /// It answers with the changes *only*. The whole view used to be
    /// rebuilt this way, which was safe while nothing in it was authored
    /// by the viewer; the same struct now holds a buffer, so a refresh
    /// that could reach it would be a refresh that eats what was typed.
    pub(super) fn read(host: &dyn Host, root: &Path, selected: Option<&Path>) -> Self {
        let status = match run_git(
            host,
            root,
            &["status", "--porcelain=v1", "--untracked-files=all"],
        ) {
            Ok(output) => output,
            Err(message) => {
                return Self {
                    error: Some(message),
                    refreshed_at: Some(Instant::now()),
                    ..Self::default()
                };
            }
        };
        let mut changes = Self {
            files: parse_porcelain_status(&status, root),
            refreshed_at: Some(Instant::now()),
            ..Self::default()
        };
        changes.load_diff(host, root, selected);
        changes
    }

    /// Where `path` sits in the changed-file list, if it changed at all.
    pub(super) fn position_of(&self, path: Option<&Path>) -> Option<usize> {
        let path = path?;
        self.files.iter().position(|file| file.path == path)
    }

    fn load_diff(&mut self, host: &dyn Host, root: &Path, selected: Option<&Path>) {
        let Some(file) = self
            .position_of(selected)
            .and_then(|index| self.files.get(index))
        else {
            self.diff = Vec::new();
            return;
        };
        let path = file.path.clone();
        let status = file.status;
        let raw = if status == FileStatus::Untracked {
            run_git(
                host,
                root,
                &[
                    "diff",
                    "--no-index",
                    "--",
                    "/dev/null",
                    &path.to_string_lossy(),
                ],
            )
        } else {
            run_git(host, root, &["diff", "HEAD", "--", &path.to_string_lossy()])
        };
        self.diff = match raw {
            Ok(output) => highlight_diff_rows(
                pair_side_by_side(parse_unified_diff(&output)),
                &path,
                &host.syntax_theme(),
            ),
            Err(message) => {
                self.error = Some(message);
                Vec::new()
            }
        };
    }

    /// The next changed file in tree order from `from`, in `direction`,
    /// skipping every file a fold hides. Measured on the unfolded tree,
    /// so a selection that is itself hidden still knows which way is
    /// which and steps out to the nearest file that shows.
    pub(super) fn neighbour(
        &self,
        root: &Path,
        from: usize,
        direction: ScrollDirection,
    ) -> Option<usize> {
        let order: Vec<usize> = tree_items(self, root, &BTreeSet::new())
            .iter()
            .filter_map(FileTreeItem::file_index)
            .collect();
        let shown: BTreeSet<usize> = tree_items(self, root, &self.folded)
            .iter()
            .filter_map(FileTreeItem::file_index)
            .collect();
        let position = order.iter().position(|index| *index == from)?;
        let (before, after) = order.split_at(position);
        match direction {
            ScrollDirection::Down => after.iter().skip(1).find(|index| shown.contains(index)),
            ScrollDirection::Up => before.iter().rev().find(|index| shown.contains(index)),
        }
        .copied()
    }

    /// The directories above `selected`, outermost first, by the path the
    /// navigator folds them under.
    fn ancestors_of(&self, root: &Path, selected: usize) -> Vec<String> {
        let items = tree_items(self, root, &BTreeSet::new());
        let Some(row) = items
            .iter()
            .position(|item| item.file_index() == Some(selected))
        else {
            return Vec::new();
        };
        let FileTreeItem::File { depth, .. } = items[row] else {
            return Vec::new();
        };
        let mut wanted = depth;
        let mut ancestors = Vec::new();
        for item in items[..row].iter().rev() {
            if wanted == 0 {
                break;
            }
            if let FileTreeItem::Directory { path, depth, .. } = item
                && *depth == wanted - 1
            {
                ancestors.push(path.clone());
                wanted = *depth;
            }
        }
        ancestors.reverse();
        ancestors
    }

    /// Folds the innermost open directory above the selection — pressed
    /// again, the one above that — so the tree closes outward from where
    /// the viewer is.
    pub(super) fn fold(&mut self, root: &Path, selected: usize) {
        if let Some(path) = self
            .ancestors_of(root, selected)
            .into_iter()
            .rev()
            .find(|path| !self.folded.contains(path))
        {
            self.folded.insert(path);
        }
    }

    /// Opens the outermost folded directory above the selection — the
    /// reverse of [`fold`](Self::fold), so a hidden selection comes back
    /// into view one level at a time.
    pub(super) fn unfold(&mut self, root: &Path, selected: usize) {
        if let Some(path) = self
            .ancestors_of(root, selected)
            .into_iter()
            .find(|path| self.folded.contains(path))
        {
            self.folded.remove(&path);
        }
    }

    pub(super) fn toggle_directory(&mut self, path: String) {
        if !self.folded.remove(&path) {
            self.folded.insert(path);
        }
    }
}

/// A compact summary for the workspace tab strip. It is deliberately
/// separate from [`Changes`]: the strip needs only a cheap indicator,
/// while opening the surface can afford to load and highlight a full
/// diff.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChangeSummary {
    pub additions: u32,
    pub deletions: u32,
}

/// Returns a summary only when `cwd` resolves to a git repository with
/// changes. `None` covers a non-repository, a missing/unusable `git`, and a
/// clean worktree alike, which lets the caller omit its badge entirely.
pub fn change_summary(host: &dyn Host, cwd: &Path) -> Option<ChangeSummary> {
    let root = repository_root(host, cwd).ok()?;
    let status = run_git(
        host,
        &root,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )
    .ok()?;
    let files = parse_porcelain_status(&status, &root);
    if files.is_empty() {
        return None;
    }

    // One diff against HEAD, not the staged and unstaged ones added
    // together: a line staged and then edited again appears in both, and
    // the sum counts it twice. A repository with no commit yet has no HEAD
    // to diff against and answers from the index alone.
    let numstat = run_git(host, &root, &["diff", "--numstat", "HEAD"])
        .or_else(|_| run_git(host, &root, &["diff", "--numstat", "--cached"]))
        .ok()?;
    let (additions, deletions) = parse_numstat(&numstat);
    let mut summary = ChangeSummary {
        additions,
        deletions,
    };
    for file in files
        .iter()
        .filter(|file| file.status == FileStatus::Untracked)
    {
        summary.additions += untracked_line_count(host, &file.path);
    }
    Some(summary)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum FileStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
}

impl FileStatus {
    pub(super) fn glyph(self) -> &'static str {
        match self {
            FileStatus::Modified => "M",
            FileStatus::Added => "A",
            FileStatus::Deleted => "D",
            FileStatus::Renamed => "R",
            FileStatus::Untracked => "U",
        }
    }

    pub(super) fn role(self) -> Role {
        match self {
            FileStatus::Modified => Role::Warning,
            FileStatus::Added | FileStatus::Untracked => Role::Success,
            FileStatus::Deleted => Role::Danger,
            FileStatus::Renamed => Role::Info,
        }
    }
}

#[derive(Clone)]
pub(super) struct ChangedFile {
    pub(super) status: FileStatus,
    /// Absolute — resolved against the repository root (`GitView::root`),
    /// never the tab's `cwd` directly. `git status` reports paths relative
    /// to the repository root regardless of `-C`, which may differ from a
    /// tab whose `cwd` is a subdirectory; resolving to an absolute path
    /// once here means nothing downstream has to re-derive that.
    pub(super) path: PathBuf,
}

/// Totals Git's tab-separated `--numstat` output. Binary entries use `-`
/// counts and intentionally contribute zero: there is no meaningful line
/// delta to show in the compact badge.
pub(super) fn parse_numstat(output: &str) -> (u32, u32) {
    output.lines().fold((0, 0), |(additions, deletions), line| {
        let mut fields = line.split('\t');
        let addition = fields.next().and_then(|value| value.parse::<u32>().ok());
        let deletion = fields.next().and_then(|value| value.parse::<u32>().ok());
        match (addition, deletion) {
            (Some(addition), Some(deletion)) => (additions + addition, deletions + deletion),
            _ => (additions, deletions),
        }
    })
}

/// `git diff` excludes untracked files, but the overlay presents them via
/// `--no-index`; count their visible lines as additions so the badge and the
/// overlay agree that they are changes.
pub(super) fn untracked_line_count(host: &dyn Host, path: &Path) -> u32 {
    host.count_lines(path)
}

/// Parses `git status --porcelain=v1 --untracked-files=all` output.
/// Resolves each reported path (always repository-root-relative,
/// regardless of `-C` — see `ChangedFile::path`'s doc comment) against
/// `root` so every `ChangedFile` carries an absolute path.
pub(super) fn parse_porcelain_status(output: &str, root: &Path) -> Vec<ChangedFile> {
    output
        .lines()
        .filter(|line| line.len() > 3)
        .filter_map(|line| {
            let (code, rest) = line.split_at(2);
            let rest = rest.trim_start();
            // A rename/copy line is `old -> new`; only the destination
            // path is where the change actually lives now.
            let relative = rest
                .split_once(" -> ")
                .map(|(_, to)| to)
                .unwrap_or(rest)
                .trim_matches('"');
            if relative.is_empty() {
                return None;
            }
            let status = if code == "??" {
                FileStatus::Untracked
            } else if code.contains('R') || code.contains('C') {
                FileStatus::Renamed
            } else if code.contains('A') {
                FileStatus::Added
            } else if code.contains('D') {
                FileStatus::Deleted
            } else {
                FileStatus::Modified
            };
            Some(ChangedFile {
                status,
                path: root.join(relative),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ordinary_status_codes() {
        let root = Path::new("/repo");
        let output = " M modified.rs\nA  added.rs\n D deleted.rs\n?? untracked.rs\n";
        let files = parse_porcelain_status(output, root);
        assert_eq!(files.len(), 4);
        assert_eq!(files[0].status, FileStatus::Modified);
        assert_eq!(files[0].path, root.join("modified.rs"));
        assert_eq!(files[1].status, FileStatus::Added);
        assert_eq!(files[2].status, FileStatus::Deleted);
        assert_eq!(files[3].status, FileStatus::Untracked);
        assert_eq!(files[3].path, root.join("untracked.rs"));
    }
    #[test]
    fn parses_a_rename_using_the_destination_path() {
        let root = Path::new("/repo");
        let files = parse_porcelain_status("R  old-name.rs -> new-name.rs\n", root);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].status, FileStatus::Renamed);
        assert_eq!(files[0].path, root.join("new-name.rs"));
    }
    #[test]
    fn ignores_blank_lines() {
        assert!(parse_porcelain_status("\n", Path::new("/repo")).is_empty());
    }
    #[test]
    fn totals_text_numstat_and_ignores_binary_entries() {
        assert_eq!(parse_numstat("2\t1\tsrc/lib.rs\n-\t-\timage.png\n"), (2, 1));
    }
}

#[cfg(test)]
mod repository_tests {
    use super::*;

    /// The same grant the workspace client makes, so these exercise the
    /// real path rather than a stub. A fake would be the right tool for
    /// testing *the view*; these test what the view reads.
    struct TestHost;

    impl Host for TestHost {
        fn git(&self, root: &Path, args: &[&str]) -> Result<String, String> {
            uze_git::read(root, args)
                .map_err(|error| error.to_string())?
                .or_exit(1)
        }

        fn read_file(&self, path: &Path) -> Result<String, String> {
            std::fs::read_to_string(path).map_err(|error| error.to_string())
        }

        fn display_path(&self, path: &Path) -> String {
            path.display().to_string()
        }

        fn syntax_theme(&self) -> String {
            crate::shared::highlight::FALLBACK_SYNTAX_THEME.to_owned()
        }

        /// Never reached: what these read, they read through `git`.
        fn list_dir(&self, _path: &Path) -> Result<Vec<crate::DirEntry>, String> {
            unreachable!("the changes half lists no directory of its own")
        }

        fn write_file(&self, _path: &Path, _contents: &str) -> Result<(), String> {
            unreachable!("reading what changed writes nothing")
        }

        fn delete_file(&self, _path: &Path) -> Result<(), String> {
            unreachable!("reading what changed deletes nothing")
        }
    }

    /// Drives real `git` in a scratch repository — proves the actual
    /// `git status --porcelain=v1 --untracked-files=all`/`git diff HEAD`/
    /// `git diff --no-index` output this module depends on parses the way
    /// the fixture-string tests above assume, not just those fixtures.
    #[test]
    fn open_reads_a_real_repositorys_staged_unstaged_and_untracked_changes() {
        let repository = uze_testkit::git::Repository::new("git-diff-test");
        let root = repository.root().to_path_buf();
        repository.commit_file("tracked.rs", "fn one() {}\n");

        assert_eq!(change_summary(&TestHost, &root), None);

        std::fs::write(root.join("tracked.rs"), "fn one() {}\nfn two() {}\n").unwrap();
        std::fs::write(root.join("staged.rs"), "fn staged() {}\n").unwrap();
        repository.git(&["add", "staged.rs"]);
        std::fs::write(root.join("new.rs"), "fn brand_new() {}\n").unwrap();

        let changes = Changes::read(&TestHost, &root, None);
        assert!(
            changes.error.is_none(),
            "unexpected error: {:?}",
            changes.error
        );
        assert_eq!(
            changes.files.len(),
            3,
            "expected 3 changed files: {:?}",
            changes
                .files
                .iter()
                .map(|f| (&f.path, f.status))
                .collect::<Vec<_>>()
        );
        let statuses: Vec<FileStatus> = changes.files.iter().map(|f| f.status).collect();
        assert!(statuses.contains(&FileStatus::Modified));
        assert!(statuses.contains(&FileStatus::Added));
        assert!(statuses.contains(&FileStatus::Untracked));
        assert_eq!(
            change_summary(&TestHost, &root),
            Some(ChangeSummary {
                additions: 3,
                deletions: 0,
            })
        );
        // Read with a file named, the diff of that file comes with it.
        let first = changes.files[0].path.clone();
        let changes = Changes::read(&TestHost, &root, Some(&first));
        assert!(
            !changes.diff.is_empty(),
            "expected a non-empty diff for the file that was asked about"
        );

        std::fs::write(root.join("later.rs"), "fn later() {}\n").unwrap();
        let changes = Changes::read(&TestHost, &root, Some(&first));
        assert!(
            changes
                .files
                .iter()
                .any(|file| file.path == root.join("later.rs")),
            "a re-read must pick up a change made while the viewer is open"
        );
        assert_eq!(
            changes.position_of(Some(&first)),
            Some(0),
            "and the file that was asked about is still where it was"
        );

        let _ = std::fs::remove_dir_all(&root);
    }
    /// The badge counts the change against HEAD, which is what the viewer
    /// sees on screen — not the staged diff plus the unstaged one. A line
    /// staged and then edited again appears in both of those, and adding
    /// them reported the work twice.
    #[test]
    fn a_line_staged_and_then_edited_again_counts_once() {
        let repository = uze_testkit::git::Repository::new("git-badge-double-count");
        let root = repository.root().to_path_buf();
        repository.commit_file("f.rs", "a\nb\nc\n");

        std::fs::write(root.join("f.rs"), "a\nB\nc\n").unwrap();
        repository.git(&["add", "f.rs"]);
        std::fs::write(root.join("f.rs"), "a\nBB\nc\n").unwrap();

        assert_eq!(
            change_summary(&TestHost, &root),
            Some(ChangeSummary {
                additions: 1,
                deletions: 1,
            }),
            "one line differs from HEAD, however many times it was touched \
             on the way there"
        );
    }
    /// A repository whose first commit has not happened has no HEAD to
    /// diff against; the badge still has to answer for what is staged.
    #[test]
    fn a_repository_with_no_commit_yet_still_reports_what_is_staged() {
        let repository = uze_testkit::git::Repository::new("git-badge-unborn");
        let root = repository.root().to_path_buf();
        std::fs::write(root.join("first.rs"), "fn first() {}\n").unwrap();
        repository.git(&["add", "first.rs"]);

        assert_eq!(
            change_summary(&TestHost, &root),
            Some(ChangeSummary {
                additions: 1,
                deletions: 0,
            })
        );
    }
    /// The scoping rule this view is built on. `git worktree list` answers
    /// repository-wide from anywhere inside the repository, so a view opened
    /// in an agent's isolated checkout would otherwise show the primary's
    /// changes and every sibling agent's alongside its own — including
    /// checkouts whose agent is long gone. Scoping is by checkout, not by
    /// the isolation layout, so it holds for any worktree, however created.
    #[test]
    fn discovers_main_and_configured_linked_worktrees() {
        let repository = uze_testkit::git::Repository::new("worktree-test");
        let root = repository.root().to_path_buf();
        // Ignored, as UZE excludes it, so the primary's own status is not
        // dominated by the checkouts hanging off it.
        repository.commit_file(".gitignore", ".worktrees/\n");
        // Mirrors where UZE isolates agents. Spelled out rather than taken
        // from the domain constant: this crate does not depend on the
        // domain, and the scoping under test is by checkout rather than by
        // that layout — it holds for any worktree, however created.
        let linked = root.join(".worktrees").join("feature");
        std::fs::create_dir_all(linked.parent().unwrap()).unwrap();
        repository.git(&[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "feature",
            linked.to_str().unwrap(),
        ]);
        std::fs::write(root.join("primary-only.rs"), "fn primary() {}\n").unwrap();
        std::fs::write(linked.join("agent-only.rs"), "fn agent() {}\n").unwrap();

        let agent_root = crate::code::repository_root(&TestHost, &linked).expect("a checkout");
        let from_agent = Changes::read(&TestHost, &agent_root, None);
        assert!(from_agent.error.is_none(), "{:?}", from_agent.error);
        assert_eq!(
            crate::code::current_branch(&TestHost, &agent_root),
            "feature"
        );
        assert_eq!(
            from_agent
                .files
                .iter()
                .map(|file| file.path.clone())
                .collect::<Vec<_>>(),
            vec![linked.join("agent-only.rs")],
            "an isolated agent sees its own checkout and nothing else"
        );

        let primary_root = crate::code::repository_root(&TestHost, &root).expect("a checkout");
        let from_primary = Changes::read(&TestHost, &primary_root, None);
        assert!(from_primary.error.is_none(), "{:?}", from_primary.error);
        assert_eq!(
            from_primary
                .files
                .iter()
                .map(|file| file.path.clone())
                .collect::<Vec<_>>(),
            vec![root.join("primary-only.rs")],
            "and the seat sees the seat, not the agents hanging off it"
        );

        let _ = std::fs::remove_dir_all(root);
    }
}
