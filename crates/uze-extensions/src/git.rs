//! The workspace TUI's Git extension — a read-only "quick peek" at
//! `git status`/`git diff` for whichever tab is active, so seeing what
//! changed never requires leaving the terminal for an external editor, and
//! the commit [`timeline`] of that same checkout, which the sidebar draws
//! as a section of its own.
//!
//! Scoped to the checkout the active tab is *in*, and nothing else. An
//! agent UZE isolated works in `.worktrees/<name>`, and `git worktree list`
//! answers repository-wide from anywhere inside it — so listing linked
//! worktrees here put every other agent's diff, plus every checkout nobody
//! ever cleaned up, inside a tab that owns exactly one of them. One tab,
//! one checkout, one diff: the scope the tab strip's badge
//! ([`change_summary`]) always had, and the `Workspace > Space >
//! Agent/Shell > Git` hierarchy the overlay is opened under.
//! `uze-extensions`' first extension (see the crate root doc comment for
//! the shape every extension after this one follows).
//!
//! Same popup shape the workspace TUI's `AgentPicker`/`ContextMenu` already
//! use (an `Option<T>` the caller renders last, on top of everything, and
//! discards on `Esc`), just sized to the whole frame instead of a small
//! anchored box — see `openspec/changes/add-git-diff-overlay/design.md`.
//! Its own module (originally its own file inside the TUI crate itself,
//! before the `uze-extensions` split) for the unified-diff parsing and
//! syntax highlighting this needs that nothing else in the client does.
//! Speaking to Git is `uze-git`'s job, and drawing is the host's — this
//! answers with a [`crate::view::View`].

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{Duration, Instant},
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use syntect::{easy::HighlightLines, highlighting::ThemeSet, parsing::SyntaxSet};

use crate::view::{
    Content, ContentLine, LineTone, Navigator, NavigatorRow, Rgb, Role, ScrollDirection,
    ScrollTarget, Section, SectionRow, Size, Span, View, ViewHit,
};

use crate::Host;

/// What to do after a key/mouse event reaches an open [`GitView`] —
/// `orchestrator`'s event loop only needs to know whether to keep the
/// overlay open or clear `WorkspaceModel::git_view`, never any of this
/// module's internals.
pub enum GitViewOutcome {
    Stay,
    Close,
}

/// This extension's registry entry — registered once in
/// `ExtensionRegistry::builtin`; the management TUI's Extensions screen
/// renders it. One `CATALOG` per extension module is the shape the crate
/// root doc promises every extension follows.
pub const CATALOG: crate::registry::BuiltinExtension = crate::registry::BuiltinExtension {
    id: "git",
    name: "Git",
    description: "Diff review and commit timeline for the active checkout.",
    surface: "Workspace TUI",
    usage: "The timeline sits in the sidebar; open the changes with the git button in the tab strip, or Ctrl+G while attached.",
};

const REFRESH_INTERVAL: Duration = Duration::from_millis(750);

/// A compact summary for the workspace tab strip. It is deliberately
/// separate from [`GitView`]: the strip needs only a cheap status indicator,
/// while opening the overlay can afford to load and highlight a full diff.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GitChangeSummary {
    pub additions: u32,
    pub deletions: u32,
}

/// Returns a summary only when `cwd` resolves to a git repository with
/// changes. `None` covers a non-repository, a missing/unusable `git`, and a
/// clean worktree alike, which lets the caller omit its badge entirely.
pub fn change_summary(host: &dyn Host, cwd: &Path) -> Option<GitChangeSummary> {
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

    let mut summary = GitChangeSummary {
        additions: 0,
        deletions: 0,
    };
    for args in [
        ["diff", "--numstat"].as_slice(),
        ["diff", "--cached", "--numstat"].as_slice(),
    ] {
        let output = run_git(host, &root, args).ok()?;
        let (additions, deletions) = parse_numstat(&output);
        summary.additions += additions;
        summary.deletions += deletions;
    }
    for file in files
        .iter()
        .filter(|file| file.status == FileStatus::Untracked)
    {
        summary.additions += untracked_line_count(host, &file.path);
    }
    Some(summary)
}

/// One commit of a checkout's [`Timeline`], newest first in the list it
/// belongs to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Commit {
    pub hash: String,
    pub subject: String,
    /// How long ago, in the compact form a narrow column can afford —
    /// `3h`, `2d`, `now` — rather than Git's own "3 hours ago" (see
    /// [`compact_age`]).
    pub age: String,
    /// No equivalent of it yet in what the checkout is measured against —
    /// the delivery target from any other branch, the upstream from the
    /// target itself (see [`comparison_base`]). What a delivery or a push
    /// would still move.
    pub ahead: bool,
}

/// The recent history of one checkout and the branch it is on: what the
/// sidebar's timeline section draws. Data only, like [`change_summary`] —
/// the host lays the rows out.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Timeline {
    pub branch: String,
    /// Newest first; the first one is `HEAD`.
    pub commits: Vec<Commit>,
}

/// The newest `limit` commits reachable from `HEAD` in the checkout `cwd`
/// is in, each marked whether it is still ahead of what the checkout is
/// measured against (`target` being the repository's delivery target, if
/// it has one — see [`comparison_base`]). `None` outside a repository,
/// without a usable `git`, and in a repository with no commits yet —
/// every case where there is no history to show, so the caller can omit
/// the section entirely rather than draw an empty one.
pub fn timeline(
    host: &dyn Host,
    cwd: &Path,
    limit: usize,
    target: Option<&str>,
) -> Option<Timeline> {
    let root = repository_root(host, cwd).ok()?;
    let count = format!("--max-count={limit}");
    let log = run_git(host, &root, &["log", &count, LOG_FORMAT]).ok()?;
    let mut commits = parse_log(&log);
    if commits.is_empty() {
        return None;
    }
    let branch = current_branch(host, &root);
    if let Some(base) = comparison_base(host, &root, &branch, target) {
        // By patch, not by ancestry: a delivery rebases the branch onto
        // the target, so what landed there carries another hash, and by
        // ancestry every delivered commit would still read as ahead.
        // `--cherry-pick` drops the ones the base holds an equivalent of;
        // a commit reworked on the way in stays ahead, which is the truth.
        let ahead = run_git(
            host,
            &root,
            &[
                "rev-list",
                "--cherry-pick",
                "--right-only",
                &count,
                &format!("{base}...HEAD"),
            ],
        )
        .map(|hashes| {
            hashes
                .lines()
                .map(str::trim)
                .map(str::to_owned)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
        for commit in &mut commits {
            commit.ahead = ahead.contains(&commit.hash);
        }
    }
    Some(Timeline { branch, commits })
}

/// The sidebar section a checkout's [`Timeline`] draws as.
///
/// Everything the host used to decide for this extension: the fold
/// marker's meaning, which hue a commit's dot wears, which text gives way
/// when the column is narrow. The host still owns every rectangle — see
/// [`crate::view::Section`].
///
/// `collapsed` and `scroll` come back from the host because the gestures
/// that change them are the host's (a click on the header, a wheel over
/// the rows); the extension is told what they are, and says what the
/// section looks like as a result.
pub fn timeline_section(timeline: &Timeline, collapsed: bool, scroll: usize) -> Section {
    Section {
        title: "timeline".to_owned(),
        caption: Span::new(timeline.branch.clone(), Role::Dim),
        collapsed,
        resizable: true,
        scroll,
        rows: timeline
            .commits
            .iter()
            .enumerate()
            .map(|(index, commit)| {
                let head = index == 0;
                SectionRow {
                    // The hue is the commit's standing, the ring is
                    // `HEAD`: the badge hue for what is still ahead of
                    // the base — what a delivery or a push would move —
                    // and the target's own warning hue for what has
                    // landed in it.
                    marker: Span::new(
                        if head { "\u{25c9}" } else { "\u{25cf}" },
                        if commit.ahead {
                            Role::Info
                        } else {
                            Role::Warning
                        },
                    ),
                    name: Span::new(
                        commit.subject.clone(),
                        if head { Role::Inactive } else { Role::Dim },
                    ),
                    trailing: Span::new(commit.age.clone(), Role::Faint),
                }
            })
            .collect(),
    }
}

/// What "ahead" is measured against: the delivery target from any other
/// branch, and the upstream from the target itself or where no target is
/// known — the two the sidebar's captions already measure a branch by. A
/// target with no local ref falls back to the upstream too. `None` when
/// neither exists, and nothing is ahead of nothing.
fn comparison_base(
    host: &dyn Host,
    root: &Path,
    branch: &str,
    target: Option<&str>,
) -> Option<String> {
    target
        .filter(|target| *target != branch)
        .into_iter()
        .chain(std::iter::once("@{upstream}"))
        .find(|candidate| ref_exists(host, root, candidate))
        .map(str::to_owned)
}

/// `--quiet --verify` exits `1` for a name that resolves to nothing, which
/// the host reports as an empty answer rather than a failure.
fn ref_exists(host: &dyn Host, root: &Path, name: &str) -> bool {
    run_git(
        host,
        root,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{name}^{{commit}}"),
        ],
    )
    .is_ok_and(|stdout| !stdout.trim().is_empty())
}

/// Unit-separated so a subject holding a tab, a pipe or any other
/// punctuation a person might type still splits into exactly three fields.
/// The full hash, since it is matched against `rev-list` rather than
/// shown.
const LOG_FORMAT: &str = "--format=%H\x1f%s\x1f%cr";
const LOG_FIELD_SEPARATOR: char = '\u{1f}';

/// Everything the sidebar's commit popup says about one commit: what
/// `git show` knows without its patch, and the shape of that patch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitDetail {
    pub hash: String,
    pub short_hash: String,
    pub author: String,
    /// Git's own relative date, unabridged — the popup has the room the
    /// timeline row has not.
    pub age: String,
    /// When it was committed, `YYYY-MM-DD HH:MM` in the committer's zone.
    pub date: String,
    /// The branches and tags at this commit, as bare names: the popup
    /// pins them as labels, and `HEAD ->` or `tag:` is not a name.
    pub refs: Vec<String>,
    pub subject: String,
    /// The message past its subject, trailing blank lines dropped; empty
    /// for a one-line message.
    pub body: String,
    pub files_changed: u32,
    pub insertions: u32,
    pub deletions: u32,
}

/// The full account of `hash` in the checkout `cwd` is in, or `None` when
/// there is no such commit to give one of.
pub fn commit_detail(host: &dyn Host, cwd: &Path, hash: &str) -> Option<CommitDetail> {
    let root = repository_root(host, cwd).ok()?;
    let shown = run_git(
        host,
        &root,
        &["show", "--no-patch", SHOW_DATE, SHOW_FORMAT, hash],
    )
    .ok()?;
    let mut detail = parse_show(&shown)?;
    let numstat = run_git(host, &root, &["show", "--numstat", "--format=", hash]).ok()?;
    let (insertions, deletions) = parse_numstat(&numstat);
    detail.files_changed = numstat
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count() as u32;
    detail.insertions = insertions;
    detail.deletions = deletions;
    Some(detail)
}

const SHOW_DATE: &str = "--date=format:%Y-%m-%d %H:%M";
/// The body last: it is the one field that spans lines, so everything
/// before it splits on the separator and it keeps whatever remains.
const SHOW_FORMAT: &str = "--format=%H\x1f%h\x1f%an\x1f%cr\x1f%cd\x1f%D\x1f%s\x1f%b";

fn parse_show(output: &str) -> Option<CommitDetail> {
    let mut fields = output.splitn(8, LOG_FIELD_SEPARATOR);
    let hash = fields.next()?.trim();
    if hash.is_empty() {
        return None;
    }
    let short_hash = fields.next()?.trim().to_owned();
    let author = fields.next()?.trim().to_owned();
    let age = fields.next()?.trim().to_owned();
    let date = fields.next()?.trim().to_owned();
    let refs = parse_refs(fields.next()?);
    let subject = fields.next()?.trim().to_owned();
    let body = fields.next().unwrap_or_default().trim().to_owned();
    Some(CommitDetail {
        hash: hash.to_owned(),
        short_hash,
        author,
        age,
        date,
        refs,
        subject,
        body,
        files_changed: 0,
        insertions: 0,
        deletions: 0,
    })
}

/// `%D` — `HEAD -> main, origin/main, tag: v1` — as the names alone. A
/// detached `HEAD` names nothing and is dropped.
fn parse_refs(decorations: &str) -> Vec<String> {
    decorations
        .split(',')
        .map(str::trim)
        .map(|reference| {
            reference
                .strip_prefix("HEAD -> ")
                .or_else(|| reference.strip_prefix("tag: "))
                .unwrap_or(reference)
        })
        .filter(|name| !name.is_empty() && *name != "HEAD")
        .map(str::to_owned)
        .collect()
}

fn parse_log(output: &str) -> Vec<Commit> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split(LOG_FIELD_SEPARATOR);
            let hash = fields.next()?.trim();
            let subject = fields.next()?.trim();
            let age = fields.next()?.trim();
            (!hash.is_empty()).then(|| Commit {
                hash: hash.to_owned(),
                subject: subject.to_owned(),
                age: compact_age(age),
                ahead: false,
            })
        })
        .collect()
}

/// Git's relative date (`%cr`, "3 hours ago") shortened to what a column
/// beside a subject can afford: the leading count and one letter for its
/// unit, `now` for anything under a minute. Git's compound form ("1 year,
/// 2 months ago") keeps only its leading part, since the tail is precision
/// the column has no room for. Anything else is handed back as it came,
/// so a wording this does not know is still shown rather than dropped.
fn compact_age(relative: &str) -> String {
    let mut words = relative.split_whitespace();
    let (Some(count), Some(unit)) = (words.next(), words.next()) else {
        return relative.to_owned();
    };
    let unit = unit.trim_end_matches(',');
    let suffix = match unit.trim_end_matches('s') {
        "second" => return "now".to_owned(),
        "minute" => "m",
        "hour" => "h",
        "day" => "d",
        "week" => "w",
        "month" => "mo",
        "year" => "y",
        _ => return relative.to_owned(),
    };
    format!("{count}{suffix}")
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum GitViewFocus {
    #[default]
    Files,
    Diff,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
}

impl FileStatus {
    fn glyph(self) -> &'static str {
        match self {
            FileStatus::Modified => "M",
            FileStatus::Added => "A",
            FileStatus::Deleted => "D",
            FileStatus::Renamed => "R",
            FileStatus::Untracked => "U",
        }
    }

    fn role(self) -> Role {
        match self {
            FileStatus::Modified => Role::Warning,
            FileStatus::Added | FileStatus::Untracked => Role::Success,
            FileStatus::Deleted => Role::Danger,
            FileStatus::Renamed => Role::Info,
        }
    }
}

struct ChangedFile {
    status: FileStatus,
    /// Absolute — resolved against the repository root (`GitView::root`),
    /// never the tab's `cwd` directly. `git status` reports paths relative
    /// to the repository root regardless of `-C`, which may differ from a
    /// tab whose `cwd` is a subdirectory; resolving to an absolute path
    /// once here means nothing downstream has to re-derive that.
    path: PathBuf,
}

#[derive(Default)]
struct FileTreeNode {
    file_index: Option<usize>,
    children: BTreeMap<String, FileTreeNode>,
}

enum FileTreeItem {
    Directory {
        name: String,
        depth: usize,
    },
    File {
        index: usize,
        name: String,
        depth: usize,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DiffLineKind {
    Context,
    Added,
    Removed,
}

/// One line on one side of the side-by-side diff — see [`DiffRow`].
struct DiffCell {
    line_no: u32,
    kind: DiffLineKind,
    /// Pre-highlighted (see `highlight_diff_rows`) — no syntect types
    /// beyond this module's boundary, and the colour travels as data
    /// because it comes from the syntax theme rather than from the host's
    /// palette (see [`crate::view::Rgb`]).
    spans: Vec<(Rgb, String)>,
}

/// One row of a before/after side-by-side diff (see `pair_side_by_side`) —
/// a context line has both `left` and `right`; a pure addition has only
/// `right`; a pure removal has only `left`. Two consecutive runs of
/// removed/added lines pair up row by row, the same visual convention
/// VS Code's own split diff view uses, rather than the unified `+`/`-`
/// stream this is built from (see `parse_unified_diff`).
struct DiffRow {
    left: Option<DiffCell>,
    right: Option<DiffCell>,
}

/// Open state of the git changes overlay (`WorkspaceModel::git_view`).
/// Built when raised and refreshed at a bounded cadence while it stays open,
/// so collaborators and commands in another pane show up without reopening.
pub struct GitView {
    /// The checkout the view is scoped to, resolved once at open time from
    /// the active tab's live `cwd` — see `open`'s doc comment. Inside a
    /// linked worktree this is that worktree, not the primary it hangs off.
    root: PathBuf,
    /// `root` as a person would recognise it, resolved once through the
    /// host that read the checkout. Drawing this view then needs no host
    /// at all, which is what lets the renderer hold none — see
    /// [`view`].
    display_root: String,
    /// The branch `root` is on, for the overlay's title — the one place a
    /// scoped view still has to say *which* checkout you are looking at.
    branch: String,
    files: Vec<ChangedFile>,
    selected: usize,
    diff: Vec<DiffRow>,
    /// Set instead of populating `files`/`diff` when the active tab's
    /// `cwd` isn't inside a git repository, `git` isn't on `PATH`, or a
    /// `git diff` invocation itself fails — shown in place of the
    /// changed-files list rather than refusing to open at all.
    error: Option<String>,
    scroll: u16,
    focus: GitViewFocus,
    /// Set when the selection moved and cleared when a read catches up.
    ///
    /// Selecting a file means reading and highlighting its diff, which is
    /// the one thing in this extension whose cost has no bound — a large
    /// file's syntax highlighting is not something an arrow key may pay
    /// for on the thread that draws. So selecting only records *what* is
    /// selected; the host reloads, and until it answers this says the
    /// diff on screen is not the one being asked for.
    diff_pending: bool,
    refreshed_at: Instant,
}

/// What a viewer did inside an open [`GitView`] that a re-read must not
/// undo: the file they were on, which half had focus, how far they had
/// scrolled.
///
/// Opaque to the host — it takes one from [`GitView::placement`] and hands
/// it back to [`GitView::reload`] without ever looking inside. Comparable,
/// so a host that reads in the background can tell an answer describing
/// where the viewer *is* from one describing where they *were*.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ViewPlacement {
    path: Option<PathBuf>,
    focus: GitViewFocus,
    scroll: u16,
}

impl GitView {
    /// `cwd` is the active tab's live working directory at the moment the
    /// view opens (see `orchestrator::open_git_view`) — this resolves the
    /// enclosing checkout from it once, up front, and every subsequent
    /// `git` call in this view uses that root, not `cwd` again.
    ///
    /// Inside a linked worktree `rev-parse --show-toplevel` answers with
    /// that worktree's own root, which is exactly the scope wanted here:
    /// the tab's checkout, never the primary it hangs off and never a
    /// sibling agent's.
    pub fn open(host: &dyn Host, cwd: PathBuf) -> Self {
        let root = match repository_root(host, &cwd) {
            Ok(root) => root,
            Err(message) => return Self::with_error(host, cwd, message),
        };
        let status = match run_git(
            host,
            &root,
            &["status", "--porcelain=v1", "--untracked-files=all"],
        ) {
            Ok(output) => output,
            Err(message) => return Self::with_error(host, root, message),
        };
        let mut view = Self {
            display_root: host.display_path(&root),
            branch: current_branch(host, &root),
            files: parse_porcelain_status(&status, &root),
            root,
            selected: 0,
            diff: Vec::new(),
            error: None,
            scroll: 0,
            focus: GitViewFocus::Files,
            diff_pending: false,
            refreshed_at: Instant::now(),
        };
        view.load_selected_diff(host);
        view
    }

    /// A view that has been asked for but not read yet.
    ///
    /// Opening reads a repository — `rev-parse`, `status`, a `diff`, and
    /// the highlighting of that diff — which is exactly the cost a
    /// reload pays and belongs on exactly the same thread. So the overlay
    /// appears the instant it is asked for, saying it is reading, and
    /// fills in when the host's reload lands.
    ///
    /// `display_root` is the path as a person would recognise it; the
    /// caller has it without a host, because formatting a path reads
    /// nothing.
    pub fn opening(cwd: PathBuf, display_root: String) -> Self {
        Self {
            display_root,
            root: cwd,
            branch: String::new(),
            files: Vec::new(),
            selected: 0,
            diff: Vec::new(),
            error: None,
            scroll: 0,
            focus: GitViewFocus::Files,
            diff_pending: true,
            refreshed_at: Instant::now(),
        }
    }

    fn with_error(host: &dyn Host, root: PathBuf, message: String) -> Self {
        Self {
            display_root: host.display_path(&root),
            root,
            branch: String::new(),
            files: Vec::new(),
            selected: 0,
            diff: Vec::new(),
            error: Some(message),
            scroll: 0,
            focus: GitViewFocus::Files,
            diff_pending: false,
            refreshed_at: Instant::now(),
        }
    }

    /// Selects `index` (clamped to the file list) and reloads its diff —
    /// the one place both keyboard navigation and a file-row click funnel
    /// through, so the two can never disagree about what "selected" means.
    fn select(&mut self, index: usize) {
        if self.files.is_empty() {
            return;
        }
        let wanted = index.min(self.files.len() - 1);
        if wanted == self.selected && !self.diff.is_empty() {
            return;
        }
        self.selected = wanted;
        self.scroll = 0;
        self.diff = Vec::new();
        self.diff_pending = true;
    }

    /// Whether the diff being shown is the selected file's yet.
    pub fn diff_pending(&self) -> bool {
        self.diff_pending
    }

    /// The checkout this view is scoped to, so the host can say which
    /// repository an answer it is holding was read from.
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn refresh_due(&self) -> bool {
        self.refreshed_at.elapsed() >= REFRESH_INTERVAL
    }

    /// Where the viewer had this view when it was asked — see
    /// [`ViewPlacement`].
    pub fn placement(&self) -> ViewPlacement {
        ViewPlacement {
            path: self.files.get(self.selected).map(|file| file.path.clone()),
            focus: self.focus,
            scroll: self.scroll,
        }
    }

    /// A whole new view of `root`, positioned where `placement` left the
    /// last one.
    ///
    /// Takes no `&self` on purpose: reading a repository is the slow part
    /// of this extension — a `status`, a `diff`, and the highlighting of
    /// that diff — and an associated function can run wherever the host
    /// puts it. The host reads on a thread of its own and installs the
    /// answer when it lands, which is why nothing here may borrow the
    /// view being replaced.
    pub fn reload(host: &dyn Host, root: PathBuf, placement: ViewPlacement) -> Self {
        let mut reloaded = Self::open(host, root);
        reloaded.focus = placement.focus;
        reloaded.scroll = placement.scroll;
        if let Some(path) = placement.path
            && let Some(file) = reloaded.files.iter().position(|file| file.path == path)
        {
            reloaded.selected = file;
            reloaded.load_selected_diff(host);
        }
        reloaded.diff_pending = false;
        reloaded.refreshed_at = Instant::now();
        reloaded
    }

    fn load_selected_diff(&mut self, host: &dyn Host) {
        let Some(file) = self.files.get(self.selected) else {
            self.diff = Vec::new();
            return;
        };
        let path = file.path.clone();
        let status = file.status;
        let root = self.root.clone();
        let raw = if status == FileStatus::Untracked {
            run_git(
                host,
                &root,
                &[
                    "diff",
                    "--no-index",
                    "--",
                    "/dev/null",
                    &path.to_string_lossy(),
                ],
            )
        } else {
            run_git(
                host,
                &root,
                &["diff", "HEAD", "--", &path.to_string_lossy()],
            )
        };
        self.diff = match raw {
            Ok(output) => {
                highlight_diff_rows(pair_side_by_side(parse_unified_diff(&output)), &path)
            }
            Err(message) => {
                self.error = Some(message);
                Vec::new()
            }
        };
    }
}

/// `git -C <cwd> rev-parse --show-toplevel` — doubles as the "is this
/// inside a git repository" check the added spec scenario needs: a
/// non-repository `cwd` fails this with git's own "not a git repository"
/// message on stderr, which becomes `GitView::error` verbatim.
fn repository_root(host: &dyn Host, cwd: &Path) -> Result<PathBuf, String> {
    host.git(cwd, &["rev-parse", "--show-toplevel"])
        .map(|stdout| PathBuf::from(stdout.trim()))
}

/// The branch [`GitView::root`] is on, for the overlay's title. Answers
/// `detached HEAD` for a checkout with no branch, the same wording
/// `git worktree list` used to supply here.
fn current_branch(host: &dyn Host, root: &Path) -> String {
    match run_git(host, root, &["rev-parse", "--abbrev-ref", "HEAD"]) {
        Ok(name) if !name.trim().is_empty() && name.trim() != "HEAD" => name.trim().to_owned(),
        _ => "detached HEAD".to_owned(),
    }
}

/// Every command this view runs is an observation, and it reaches Git
/// through the host rather than spawning anything itself — see
/// [`crate::Host`].
fn run_git(host: &dyn Host, root: &Path, args: &[&str]) -> Result<String, String> {
    host.git(root, args)
}

/// Totals Git's tab-separated `--numstat` output. Binary entries use `-`
/// counts and intentionally contribute zero: there is no meaningful line
/// delta to show in the compact badge.
fn parse_numstat(output: &str) -> (u32, u32) {
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
fn untracked_line_count(host: &dyn Host, path: &Path) -> u32 {
    let Some(contents) = host.read_file(path) else {
        return 0;
    };
    if contents.is_empty() {
        0
    } else {
        contents.lines().count().max(1) as u32
    }
}

/// Parses `git status --porcelain=v1 --untracked-files=all` output.
/// Resolves each reported path (always repository-root-relative,
/// regardless of `-C` — see `ChangedFile::path`'s doc comment) against
/// `root` so every `ChangedFile` carries an absolute path.
fn parse_porcelain_status(output: &str, root: &Path) -> Vec<ChangedFile> {
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

/// Parses unified diff output (`git diff`'s own format) into line-numbered,
/// classified lines — text only, not yet paired into side-by-side rows
/// (see `pair_side_by_side`) or syntax-highlighted (see
/// `highlight_diff_rows`). Preamble lines (`diff --git`, `index`, `---`,
/// `+++`) are skipped; only content inside a `@@` hunk is kept.
fn parse_unified_diff(output: &str) -> Vec<(DiffLineKind, Option<u32>, Option<u32>, String)> {
    let mut lines = Vec::new();
    let mut old_no = 0u32;
    let mut new_no = 0u32;
    let mut in_hunk = false;
    for line in output.lines() {
        if let Some(header) = line.strip_prefix("@@ ") {
            if let Some(hunk) = parse_hunk_header(header) {
                (old_no, new_no) = hunk;
                in_hunk = true;
            }
            continue;
        }
        if !in_hunk {
            continue;
        }
        if let Some(text) = line.strip_prefix('+') {
            lines.push((DiffLineKind::Added, None, Some(new_no), text.to_owned()));
            new_no += 1;
        } else if let Some(text) = line.strip_prefix('-') {
            lines.push((DiffLineKind::Removed, Some(old_no), None, text.to_owned()));
            old_no += 1;
        } else if let Some(text) = line.strip_prefix(' ') {
            lines.push((
                DiffLineKind::Context,
                Some(old_no),
                Some(new_no),
                text.to_owned(),
            ));
            old_no += 1;
            new_no += 1;
        }
        // Anything else inside a hunk (e.g. "\ No newline at end of file")
        // carries no line of its own — skipped.
    }
    lines
}

/// `header` is everything after the hunk marker's leading `"@@ "`, e.g.
/// `"-12,7 +12,7 @@ fn context_hint"` — returns the hunk's starting
/// `(old_line, new_line)`, or `None` if the header doesn't parse (left as
/// a pre-hunk preamble line rather than guessing).
fn parse_hunk_header(header: &str) -> Option<(u32, u32)> {
    let (old_part, rest) = header.split_once(' ')?;
    let (new_part, _) = rest.split_once(" @@")?;
    let old_start = old_part
        .strip_prefix('-')?
        .split(',')
        .next()?
        .parse()
        .ok()?;
    let new_start = new_part
        .strip_prefix('+')?
        .split(',')
        .next()?
        .parse()
        .ok()?;
    Some((old_start, new_start))
}

/// A side-by-side row before syntax highlighting — see [`DiffRow`] for the
/// highlighted, render-ready shape this becomes via `highlight_diff_rows`.
struct PairedRow {
    left: Option<(u32, DiffLineKind, String)>,
    right: Option<(u32, DiffLineKind, String)>,
}

/// Pairs a unified diff's sequential `+`/`-`/context lines into
/// side-by-side rows — a context line appears on both sides at once; a run
/// of removed lines pairs up, row by row, against the run of added lines
/// immediately following it (the same convention VS Code's own split diff
/// view uses), with the longer run's extra rows left blank on the shorter
/// side.
fn pair_side_by_side(
    lines: Vec<(DiffLineKind, Option<u32>, Option<u32>, String)>,
) -> Vec<PairedRow> {
    let mut rows = Vec::new();
    let mut removed: Vec<(u32, String)> = Vec::new();
    let mut added: Vec<(u32, String)> = Vec::new();
    for (kind, old_no, new_no, text) in lines {
        match kind {
            DiffLineKind::Removed => removed.push((old_no.unwrap_or_default(), text)),
            DiffLineKind::Added => added.push((new_no.unwrap_or_default(), text)),
            DiffLineKind::Context => {
                flush_pending(&mut rows, &mut removed, &mut added);
                rows.push(PairedRow {
                    left: old_no.map(|no| (no, DiffLineKind::Context, text.clone())),
                    right: new_no.map(|no| (no, DiffLineKind::Context, text)),
                });
            }
        }
    }
    flush_pending(&mut rows, &mut removed, &mut added);
    rows
}

fn flush_pending(
    rows: &mut Vec<PairedRow>,
    removed: &mut Vec<(u32, String)>,
    added: &mut Vec<(u32, String)>,
) {
    let paired = removed.len().max(added.len());
    for index in 0..paired {
        rows.push(PairedRow {
            left: removed
                .get(index)
                .map(|(no, text)| (*no, DiffLineKind::Removed, text.clone())),
            right: added
                .get(index)
                .map(|(no, text)| (*no, DiffLineKind::Added, text.clone())),
        });
    }
    removed.clear();
    added.clear();
}

fn syntax_set() -> &'static SyntaxSet {
    static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme_set() -> &'static ThemeSet {
    static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();
    THEME_SET.get_or_init(ThemeSet::load_defaults)
}

/// Applies syntax highlighting (by `path`'s extension) to `rows` — the one
/// place `syntect` types cross into this module's own `DiffRow`/`DiffCell`
/// shape, and the one place a colour is produced at all: it comes from the
/// syntax theme, so it travels to the host as data rather than as a role. The left and right columns are
/// highlighted as two independent line streams, each with its own
/// `HighlightLines` — syntect's highlighter carries state (an open block
/// comment, for instance) across calls, and "before" and "after" are two
/// separate versions of the file, not one continuous stream.
fn highlight_diff_rows(rows: Vec<PairedRow>, path: &Path) -> Vec<DiffRow> {
    let syntax_set = syntax_set();
    let syntax = path
        .extension()
        .and_then(|ext| ext.to_str())
        .and_then(|ext| syntax_set.find_syntax_by_extension(ext))
        .unwrap_or_else(|| syntax_set.find_syntax_plain_text());
    let theme = &theme_set().themes["base16-ocean.dark"];
    let mut left_highlighter = HighlightLines::new(syntax, theme);
    let mut right_highlighter = HighlightLines::new(syntax, theme);
    rows.into_iter()
        .map(|row| DiffRow {
            left: row.left.map(|(line_no, kind, text)| DiffCell {
                line_no,
                kind,
                spans: highlight_one_line(&mut left_highlighter, syntax_set, &text),
            }),
            right: row.right.map(|(line_no, kind, text)| DiffCell {
                line_no,
                kind,
                spans: highlight_one_line(&mut right_highlighter, syntax_set, &text),
            }),
        })
        .collect()
}

fn highlight_one_line(
    highlighter: &mut HighlightLines<'_>,
    syntax_set: &SyntaxSet,
    text: &str,
) -> Vec<(Rgb, String)> {
    // syntect's line-oriented highlighter expects a trailing newline
    // (matches `load_defaults_newlines` above) to track multi-line
    // constructs correctly across calls.
    let newline_terminated = format!("{text}\n");
    let ranges = highlighter
        .highlight_line(&newline_terminated, syntax_set)
        .unwrap_or_default();
    ranges
        .into_iter()
        .map(|(style, piece)| {
            let fg = style.foreground;
            (
                Rgb(fg.r, fg.g, fg.b),
                piece.trim_end_matches('\n').to_owned(),
            )
        })
        .collect()
}

pub fn handle_key(view: &mut GitView, key: KeyEvent) -> GitViewOutcome {
    if key.code == KeyCode::Esc
        || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('g'))
    {
        return GitViewOutcome::Close;
    }
    match key.code {
        KeyCode::Tab => {
            view.focus = match view.focus {
                GitViewFocus::Files => GitViewFocus::Diff,
                GitViewFocus::Diff => GitViewFocus::Files,
            };
        }
        KeyCode::Up => match view.focus {
            GitViewFocus::Files => view.select(view.selected.saturating_sub(1)),
            GitViewFocus::Diff => view.scroll = view.scroll.saturating_sub(1),
        },
        KeyCode::Down => match view.focus {
            GitViewFocus::Files => view.select(view.selected + 1),
            GitViewFocus::Diff => view.scroll = view.scroll.saturating_add(1),
        },
        KeyCode::Enter if view.focus == GitViewFocus::Files => {
            if !view.files.is_empty() {
                view.focus = GitViewFocus::Diff;
            }
        }
        KeyCode::PageUp => view.scroll = view.scroll.saturating_sub(10),
        KeyCode::PageDown => view.scroll = view.scroll.saturating_add(10),
        _ => {}
    }
    GitViewOutcome::Stay
}

pub fn handle_mouse(view: &mut GitView, hit: Option<ViewHit>) -> GitViewOutcome {
    match hit {
        Some(ViewHit::SelectItem(index)) => view.select(index),
        Some(ViewHit::Close) => return GitViewOutcome::Close,
        _ => {}
    }
    GitViewOutcome::Stay
}

/// Mouse-wheel scroll over an open [`GitView`] — routed by *where the
/// cursor is*, not by `GitViewFocus` (which only reflects `Tab`/keyboard
/// navigation): hovering the file list moves the selection, hovering the
/// diff scrolls it, matching how a mouse wheel behaves everywhere else
/// (VS Code included) regardless of which panel last had keyboard focus.
///
/// *Where* is resolved by the host, which owns the layout — this used to
/// re-derive the columns from the frame rectangle, which meant two sides
/// computing the same geometry and only one of them being authoritative.
pub fn handle_scroll(view: &mut GitView, target: ScrollTarget, direction: ScrollDirection) {
    if view.error.is_some() || view.files.is_empty() {
        return;
    }
    match (target, direction) {
        (ScrollTarget::Navigator, ScrollDirection::Up) => {
            view.select(view.selected.saturating_sub(1));
        }
        (ScrollTarget::Navigator, ScrollDirection::Down) => view.select(view.selected + 1),
        (ScrollTarget::Content, ScrollDirection::Up) => {
            view.scroll = view.scroll.saturating_sub(3);
        }
        (ScrollTarget::Content, ScrollDirection::Down) => {
            view.scroll = view.scroll.saturating_add(3);
        }
    }
}

/// What this extension shows, as data — see [`crate::view`] for why it
/// hands back a description instead of drawing.
///
/// `space` is advisory: it bounds how much content is worth producing,
/// never where any of it goes.
///
/// Takes no [`Host`]: everything a view says was resolved when the
/// checkout was read (see [`GitView::display_root`]). Drawing is the one
/// thing in this crate that reaches nothing at all, and the architecture
/// suite holds the renderer to it.
pub fn view(git: &GitView, space: Size) -> View {
    let title = format!(
        " git — {}{} ",
        git.display_root,
        if git.branch.is_empty() {
            String::new()
        } else {
            format!(" · {}", git.branch)
        }
    );
    let footer_hint = "↑↓ navigate · ↵ diff · tab focus · esc close".to_owned();

    if let Some(message) = &git.error {
        return View {
            title,
            navigator: None,
            content: Content::Message {
                text: message.clone(),
                role: Role::Danger,
            },
            footer_hint,
        };
    }

    let content = if git.files.is_empty() {
        Content::Message {
            text: "no changes".to_owned(),
            role: Role::Muted,
        }
    } else if git.files.get(git.selected).is_none() {
        Content::Message {
            text: "no changes in this checkout".to_owned(),
            role: Role::Muted,
        }
    } else if git.diff_pending {
        // The selection moved and its diff is still being read. Saying so
        // beats showing the previous file's diff under the new file's
        // name, and beats an empty pane that reads as "no changes".
        Content::Message {
            text: "reading…".to_owned(),
            role: Role::Muted,
        }
    } else {
        Content::Lines {
            heading: format!(
                "DIFF · {}",
                git.files
                    .get(git.selected)
                    .and_then(|file| file.path.strip_prefix(&git.root).ok())
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| git.display_root.clone())
            ),
            scroll: git.scroll,
            // Bounded by the space plus what is scrolled past, not by the
            // space alone: a wrapped line occupies more than one row, and
            // only the host — which does the wrapping — knows how many.
            // Erring long costs a few unrendered lines; erring short would
            // show blank rows at the bottom of a long diff.
            lines: unified_lines(&git.diff)
                .into_iter()
                .take(usize::from(space.height).saturating_mul(2) + git.scroll as usize)
                .map(content_line)
                .collect(),
        }
    };

    View {
        title,
        navigator: Some(navigator(git)),
        content,
        footer_hint,
    }
}

fn navigator(git: &GitView) -> Navigator {
    let items = file_tree_items(git);
    Navigator {
        heading: "CHANGES".to_owned(),
        badge: git.files.len().to_string(),
        focused: git.focus == GitViewFocus::Files,
        anchor: selected_tree_row(&items, git).unwrap_or(0),
        rows: items
            .iter()
            .map(|item| match item {
                FileTreeItem::Directory { name, depth } => NavigatorRow::Group {
                    name: format!("{name}/"),
                    depth: *depth,
                },
                FileTreeItem::File { index, name, depth } => NavigatorRow::Item {
                    id: *index,
                    name: name.clone(),
                    depth: depth + 1,
                    marker: Span::new(
                        git.files[*index].status.glyph(),
                        git.files[*index].status.role(),
                    ),
                    selected: *index == git.selected,
                },
            })
            .collect(),
    }
}

fn content_line(cell: &DiffCell) -> ContentLine {
    let (gutter, tone) = match cell.kind {
        DiffLineKind::Context => (" ", LineTone::Neutral),
        DiffLineKind::Added => ("+", LineTone::Added),
        DiffLineKind::Removed => ("-", LineTone::Removed),
    };
    ContentLine {
        gutter: gutter.to_owned(),
        number: cell.line_no.to_string(),
        tone,
        spans: cell
            .spans
            .iter()
            .map(|(color, text)| Span {
                text: text.clone(),
                role: Role::Default,
                color: Some(*color),
                bold: false,
            })
            .collect(),
    }
}

/// Builds a stable, compact change navigator from repository-relative paths.
/// The model retains a flat `files` vec because diff loading and selection
/// are file-oriented; this projection is strictly presentation state.
fn file_tree_items(view: &GitView) -> Vec<FileTreeItem> {
    let mut tree = FileTreeNode::default();
    for (index, file) in view.files.iter().enumerate() {
        let relative = file.path.strip_prefix(&view.root).unwrap_or(&file.path);
        let components: Vec<String> = relative
            .components()
            .filter_map(|component| component.as_os_str().to_str().map(str::to_owned))
            .collect();
        let mut node = &mut tree;
        for component in &components {
            node = node.children.entry(component.clone()).or_default();
        }
        node.file_index = Some(index);
    }
    let mut items = Vec::new();
    collect_tree_items(&tree, 0, &mut items);
    items
}

fn collect_tree_items(node: &FileTreeNode, depth: usize, items: &mut Vec<FileTreeItem>) {
    for (name, child) in &node.children {
        if child.file_index.is_none() {
            let (name, child) = compact_directory(name, child);
            items.push(FileTreeItem::Directory { name, depth });
            collect_tree_items(child, depth + 1, items);
        }
    }
    for (name, child) in &node.children {
        if let Some(index) = child.file_index {
            items.push(FileTreeItem::File {
                index,
                name: name.clone(),
                depth,
            });
        }
    }
}

fn compact_directory<'a>(name: &str, mut node: &'a FileTreeNode) -> (String, &'a FileTreeNode) {
    let mut path = name.to_owned();
    while node.file_index.is_none() && node.children.len() == 1 {
        let Some((child_name, child)) = node.children.first_key_value() else {
            break;
        };
        if child.file_index.is_some() {
            break;
        }
        path.push('/');
        path.push_str(child_name);
        node = child;
    }
    (path, node)
}

fn selected_tree_row(items: &[FileTreeItem], view: &GitView) -> Option<usize> {
    items.iter().position(
        |item| matches!(item, FileTreeItem::File { index, .. } if *index == view.selected),
    )
}

/// Expands paired rows back into their unified representation. Context is
/// shared by both sides and shown once; a replacement retains its natural
/// `-old` then `+new` ordering.
fn unified_lines(rows: &[DiffRow]) -> Vec<&DiffCell> {
    let mut lines = Vec::new();
    for row in rows {
        match (&row.left, &row.right) {
            (Some(left), Some(right)) if left.kind == DiffLineKind::Context => {
                lines.push(right);
            }
            (Some(left), Some(right)) => {
                lines.push(left);
                lines.push(right);
            }
            (Some(left), None) => lines.push(left),
            (None, Some(right)) => lines.push(right),
            (None, None) => {}
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The same grant the workspace client makes, so these exercise the
    /// real path rather than a stub. A fake would be the right tool for
    /// testing *the view*; this file tests what the view reads.
    struct TestHost;

    impl Host for TestHost {
        fn git(&self, root: &Path, args: &[&str]) -> Result<String, String> {
            uze_git::read(root, args)
                .map_err(|error| error.to_string())?
                .or_exit(1)
        }

        fn read_file(&self, path: &Path) -> Option<String> {
            std::fs::read_to_string(path).ok()
        }

        fn display_path(&self, path: &Path) -> String {
            path.display().to_string()
        }
    }

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
    fn projects_changed_paths_as_a_compact_navigator() {
        let root = PathBuf::from("/repo");
        let view = GitView {
            display_root: root.display().to_string(),
            root: root.clone(),
            branch: "main".to_owned(),
            files: vec![
                ChangedFile {
                    status: FileStatus::Modified,
                    path: root.join("src/ui/git_diff.rs"),
                },
                ChangedFile {
                    status: FileStatus::Added,
                    path: root.join("src/ui.rs"),
                },
                ChangedFile {
                    status: FileStatus::Untracked,
                    path: root.join("README.md"),
                },
            ],
            selected: 0,
            diff: Vec::new(),
            error: None,
            scroll: 0,
            focus: GitViewFocus::Files,
            diff_pending: false,
            refreshed_at: Instant::now(),
        };
        let items = file_tree_items(&view);
        assert!(
            matches!(items[0], FileTreeItem::Directory { ref name, depth: 0 } if name == "src")
        );
        assert!(matches!(items[1], FileTreeItem::Directory { ref name, depth: 1 } if name == "ui"));
        assert!(
            matches!(items[2], FileTreeItem::File { ref name, depth: 2, .. } if name == "git_diff.rs")
        );
        assert!(
            matches!(items[3], FileTreeItem::File { ref name, depth: 1, .. } if name == "ui.rs")
        );
        assert!(
            matches!(items[4], FileTreeItem::File { ref name, depth: 0, .. } if name == "README.md")
        );
        assert_eq!(items.len(), 5, "no worktree header rows above the tree");
        assert_eq!(selected_tree_row(&items, &view), Some(2));
    }

    #[test]
    fn ignores_blank_lines() {
        assert!(parse_porcelain_status("\n", Path::new("/repo")).is_empty());
    }

    #[test]
    fn totals_text_numstat_and_ignores_binary_entries() {
        assert_eq!(parse_numstat("2\t1\tsrc/lib.rs\n-\t-\timage.png\n"), (2, 1));
    }

    #[test]
    fn parses_a_single_hunk() {
        let diff = "diff --git a/f.rs b/f.rs\nindex abc..def 100644\n--- a/f.rs\n+++ b/f.rs\n@@ -1,3 +1,3 @@\n context\n-old\n+new\n context2\n";
        let lines = parse_unified_diff(diff);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].0, DiffLineKind::Context);
        assert_eq!(lines[0].1, Some(1));
        assert_eq!(lines[0].2, Some(1));
        assert_eq!(lines[1].0, DiffLineKind::Removed);
        assert_eq!(lines[1].1, Some(2));
        assert_eq!(lines[1].2, None);
        assert_eq!(lines[1].3, "old");
        assert_eq!(lines[2].0, DiffLineKind::Added);
        assert_eq!(lines[2].2, Some(2));
        assert_eq!(lines[3].1, Some(3));
        assert_eq!(lines[3].2, Some(3));
    }

    #[test]
    fn tracks_line_numbers_across_multiple_hunks() {
        let diff = "diff --git a/f.rs b/f.rs\n--- a/f.rs\n+++ b/f.rs\n@@ -1,1 +1,1 @@\n-a\n+b\n@@ -10,1 +10,2 @@\n c\n+d\n";
        let lines = parse_unified_diff(diff);
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].1, Some(1));
        assert_eq!(lines[2].1, Some(10));
        assert_eq!(lines[2].2, Some(10));
        assert_eq!(lines[3].2, Some(11));
    }

    #[test]
    fn preamble_lines_outside_a_hunk_are_skipped() {
        let diff = "diff --git a/f.rs b/f.rs\nindex abc..def 100644\n--- a/f.rs\n+++ b/f.rs\n";
        assert!(parse_unified_diff(diff).is_empty());
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

        let mut view = GitView::open(&TestHost, root.clone());
        assert!(view.error.is_none(), "unexpected error: {:?}", view.error);
        assert_eq!(
            view.files.len(),
            3,
            "expected 3 changed files: {:?}",
            view.files
                .iter()
                .map(|f| (&f.path, f.status))
                .collect::<Vec<_>>()
        );
        let statuses: Vec<FileStatus> = view.files.iter().map(|f| f.status).collect();
        assert!(statuses.contains(&FileStatus::Modified));
        assert!(statuses.contains(&FileStatus::Added));
        assert!(statuses.contains(&FileStatus::Untracked));
        assert_eq!(
            change_summary(&TestHost, &root),
            Some(GitChangeSummary {
                additions: 3,
                deletions: 0,
            })
        );
        // The first file's diff loaded automatically on open.
        assert!(
            !view.diff.is_empty(),
            "expected a non-empty diff for the first file"
        );

        std::fs::write(root.join("later.rs"), "fn later() {}\n").unwrap();
        let placement = view.placement();
        view = GitView::reload(&TestHost, view.root().to_path_buf(), placement.clone());
        assert!(
            view.files
                .iter()
                .any(|file| file.path == root.join("later.rs")),
            "a reload must pick up a change made while the viewer is open"
        );
        assert_eq!(
            view.placement(),
            placement,
            "a reload lands the viewer where they were, not at the top of a new list"
        );

        let _ = std::fs::remove_dir_all(&root);
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

        let from_agent = GitView::open(&TestHost, linked.clone());
        assert!(from_agent.error.is_none(), "{:?}", from_agent.error);
        assert_eq!(from_agent.branch, "feature");
        assert_eq!(
            from_agent
                .files
                .iter()
                .map(|file| file.path.clone())
                .collect::<Vec<_>>(),
            vec![linked.join("agent-only.rs")],
            "an isolated agent sees its own checkout and nothing else"
        );

        let from_primary = GitView::open(&TestHost, root.clone());
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

    #[test]
    fn a_context_line_pairs_with_itself_on_both_sides() {
        let diff = "diff --git a/f.rs b/f.rs\n--- a/f.rs\n+++ b/f.rs\n@@ -1,1 +1,1 @@\n context\n";
        let rows = pair_side_by_side(parse_unified_diff(diff));
        assert_eq!(rows.len(), 1);
        let left = rows[0].left.as_ref().unwrap();
        let right = rows[0].right.as_ref().unwrap();
        assert_eq!(left.0, 1);
        assert_eq!(left.2, "context");
        assert_eq!(right.0, 1);
        assert_eq!(right.2, "context");
    }

    #[test]
    fn more_removed_than_added_leaves_the_extra_rows_blank_on_the_right() {
        let diff = "diff --git a/f.rs b/f.rs\n--- a/f.rs\n+++ b/f.rs\n@@ -1,3 +1,1 @@\n-one\n-two\n-three\n+only\n";
        let rows = pair_side_by_side(parse_unified_diff(diff));
        assert_eq!(rows.len(), 3);
        assert!(rows[0].left.is_some() && rows[0].right.is_some());
        assert!(rows[1].left.is_some() && rows[1].right.is_none());
        assert!(rows[2].left.is_some() && rows[2].right.is_none());
    }

    #[test]
    fn more_added_than_removed_leaves_the_extra_rows_blank_on_the_left() {
        let diff = "diff --git a/f.rs b/f.rs\n--- a/f.rs\n+++ b/f.rs\n@@ -1,1 +1,3 @@\n-only\n+one\n+two\n+three\n";
        let rows = pair_side_by_side(parse_unified_diff(diff));
        assert_eq!(rows.len(), 3);
        assert!(rows[0].left.is_some() && rows[0].right.is_some());
        assert!(rows[1].left.is_none() && rows[1].right.is_some());
        assert!(rows[2].left.is_none() && rows[2].right.is_some());
    }

    #[test]
    fn equal_removed_and_added_pair_one_to_one() {
        let diff = "diff --git a/f.rs b/f.rs\n--- a/f.rs\n+++ b/f.rs\n@@ -1,2 +1,2 @@\n-old one\n-old two\n+new one\n+new two\n";
        let rows = pair_side_by_side(parse_unified_diff(diff));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].left.as_ref().unwrap().2, "old one");
        assert_eq!(rows[0].right.as_ref().unwrap().2, "new one");
        assert_eq!(rows[1].left.as_ref().unwrap().2, "old two");
        assert_eq!(rows[1].right.as_ref().unwrap().2, "new two");
    }

    #[test]
    fn unified_lines_shows_context_once_and_replacements_in_diff_order() {
        let rows = highlight_diff_rows(
            pair_side_by_side(parse_unified_diff(
                "@@ -1,2 +1,2 @@\n context\n-old\n+new\n",
            )),
            Path::new("example.rs"),
        );
        let lines = unified_lines(&rows);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].kind, DiffLineKind::Context);
        assert_eq!(lines[1].kind, DiffLineKind::Removed);
        assert_eq!(lines[2].kind, DiffLineKind::Added);
    }

    #[test]
    fn a_relative_date_compacts_to_its_count_and_unit() {
        assert_eq!(compact_age("5 seconds ago"), "now");
        assert_eq!(compact_age("1 minute ago"), "1m");
        assert_eq!(compact_age("3 hours ago"), "3h");
        assert_eq!(compact_age("2 days ago"), "2d");
        assert_eq!(compact_age("6 weeks ago"), "6w");
        assert_eq!(compact_age("4 months ago"), "4mo");
        assert_eq!(compact_age("1 year, 2 months ago"), "1y");
        assert_eq!(compact_age("in the future"), "in the future");
    }

    #[test]
    fn a_log_line_splits_on_the_unit_separator_whatever_the_subject_holds() {
        let output = "abc1234\u{1f}fix: a | b\tc\u{1f}3 hours ago\ndef5678\u{1f}feat: first\u{1f}2 days ago\n";
        let commits = parse_log(output);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].hash, "abc1234");
        assert_eq!(commits[0].subject, "fix: a | b\tc");
        assert_eq!(commits[0].age, "3h");
        assert_eq!(commits[1].subject, "feat: first");
    }

    #[test]
    fn decorations_become_bare_names() {
        assert_eq!(
            parse_refs("HEAD -> main, origin/main, tag: v1"),
            ["main", "origin/main", "v1"]
        );
        assert_eq!(parse_refs("HEAD"), Vec::<String>::new());
        assert_eq!(parse_refs(""), Vec::<String>::new());
    }

    #[test]
    fn a_shown_commit_keeps_its_body_whole() {
        let shown = "abc\u{1f}abc1234\u{1f}Ada\u{1f}3 hours ago\u{1f}2026-09-03 19:39\u{1f}HEAD -> main\u{1f}feat: first\u{1f}One paragraph.\n\nAnother, with a | pipe.\n\n";
        let detail = parse_show(shown).expect("well formed");
        assert_eq!(detail.short_hash, "abc1234");
        assert_eq!(detail.author, "Ada");
        assert_eq!(detail.refs, ["main"]);
        assert_eq!(detail.subject, "feat: first");
        assert_eq!(detail.body, "One paragraph.\n\nAnother, with a | pipe.");
        assert!(parse_show("").is_none());
    }

    /// The popup's account is the commit's own: who, when, what it said,
    /// and how much it touched.
    #[test]
    fn the_detail_of_a_real_commit_counts_what_it_touched() {
        let repository = uze_testkit::git::Repository::new("git-commit-detail-test");
        let root = repository.root().to_path_buf();
        std::fs::write(root.join("a.rs"), "one\ntwo\n").unwrap();
        std::fs::write(root.join("b.rs"), "three\n").unwrap();
        repository.git(&["add", "."]);
        repository.git(&[
            "commit",
            "--quiet",
            "-m",
            "feat: two files\n\nWhy they were added.",
        ]);
        let head = repository.head();

        let detail = commit_detail(&TestHost, &root, &head).expect("the commit exists");
        assert_eq!(detail.hash, head);
        assert_eq!(detail.subject, "feat: two files");
        assert_eq!(detail.body, "Why they were added.");
        assert!(!detail.author.is_empty());
        assert!(detail.age.ends_with("ago"), "{}", detail.age);
        assert_eq!(
            detail.date.len(),
            "2026-09-03 19:39".len(),
            "{}",
            detail.date
        );
        assert_eq!(detail.refs, [repository.branch()]);
        assert_eq!(
            (detail.files_changed, detail.insertions, detail.deletions),
            (2, 3, 0)
        );
        assert!(commit_detail(&TestHost, &root, "0000000").is_none());
    }

    /// The timeline is the checkout's own history, newest first, and a
    /// repository with nothing committed yet has no timeline at all —
    /// the section is omitted, not drawn empty.
    #[test]
    fn the_timeline_lists_a_real_repositorys_commits_newest_first() {
        let repository = uze_testkit::git::Repository::empty("git-timeline-test");
        let root = repository.root().to_path_buf();
        assert_eq!(timeline(&TestHost, &root, 5, None), None);

        repository.commit_file("one.rs", "fn one() {}\n");
        repository.commit_file("two.rs", "fn two() {}\n");
        repository.commit_file("three.rs", "fn three() {}\n");

        let timeline = timeline(&TestHost, &root, 2, None).expect("history exists");
        assert!(!timeline.branch.is_empty());
        let subjects: Vec<&str> = timeline
            .commits
            .iter()
            .map(|commit| commit.subject.as_str())
            .collect();
        assert_eq!(subjects.len(), 2, "bounded by the limit: {subjects:?}");
        assert!(subjects[0].contains("three"), "{subjects:?}");
        assert!(subjects[1].contains("two"), "{subjects:?}");
        assert!(
            timeline
                .commits
                .iter()
                .all(|commit| commit.hash.len() == 40)
        );
        assert!(timeline.commits.iter().all(|commit| commit.age == "now"));
        assert!(
            timeline.commits.iter().all(|commit| !commit.ahead),
            "nothing is ahead of nothing: no target, no upstream"
        );
    }

    /// From a branch of its own, what is ahead is what the target lacks;
    /// on the target itself, what the upstream lacks — and a target the
    /// checkout is on is not measured against itself.
    #[test]
    fn commits_past_the_base_are_ahead_and_the_rest_have_landed() {
        let repository = uze_testkit::git::Repository::new("git-timeline-ahead-test");
        let root = repository.root().to_path_buf();
        let target = repository.branch();
        repository.commit_file("landed.rs", "");
        repository.git(&["checkout", "--quiet", "-b", "feature"]);
        repository.commit_file("first.rs", "");
        repository.commit_file("second.rs", "");

        let flags = |timeline: Timeline| -> Vec<(String, bool)> {
            timeline
                .commits
                .into_iter()
                .map(|commit| (commit.subject, commit.ahead))
                .collect()
        };
        let on_feature = timeline(&TestHost, &root, 10, Some(&target)).expect("history");
        assert_eq!(
            flags(on_feature),
            [
                ("second.rs".to_owned(), true),
                ("first.rs".to_owned(), true),
                ("landed.rs".to_owned(), false),
                ("README.md".to_owned(), false),
            ]
        );

        // Delivered by rebase, the target holds `first.rs` under another
        // hash — and the branch, never rebased since, still holds its own.
        // By patch it has landed; only `second.rs` is still ahead.
        let first = repository.git(&["rev-parse", "HEAD~1"]);
        repository.git(&["checkout", "--quiet", &target]);
        repository.git(&["cherry-pick", "--quiet", &first]);
        repository.git(&["checkout", "--quiet", "feature"]);
        let after_delivery = timeline(&TestHost, &root, 10, Some(&target)).expect("history");
        assert_eq!(
            flags(after_delivery)
                .into_iter()
                .filter(|(_, ahead)| *ahead)
                .map(|(subject, _)| subject)
                .collect::<Vec<_>>(),
            ["second.rs"]
        );

        // The same branch measured by its upstream instead, target unknown.
        repository.git(&["branch", "--quiet", "--set-upstream-to", &target]);
        let by_upstream = timeline(&TestHost, &root, 10, None).expect("history");
        assert_eq!(
            flags(by_upstream)
                .iter()
                .filter(|(_, ahead)| *ahead)
                .count(),
            1
        );

        repository.git(&["checkout", "--quiet", &target]);
        let on_target = timeline(&TestHost, &root, 10, Some(&target)).expect("history");
        assert!(
            flags(on_target).iter().all(|(_, ahead)| !ahead),
            "the target has no upstream, so nothing is ahead"
        );
    }
}

#[cfg(test)]
mod view_tests {
    use super::*;

    struct TestHost;

    impl Host for TestHost {
        fn git(&self, _root: &Path, _args: &[&str]) -> Result<String, String> {
            unreachable!("building a view reads nothing: it renders what is already in memory")
        }

        fn read_file(&self, _path: &Path) -> Option<String> {
            unreachable!("building a view reads nothing")
        }

        fn display_path(&self, path: &Path) -> String {
            path.display().to_string()
        }
    }

    fn fixture() -> GitView {
        let root = PathBuf::from("/repo");
        GitView {
            display_root: root.display().to_string(),
            root: root.clone(),
            branch: "main".to_owned(),
            files: vec![
                ChangedFile {
                    status: FileStatus::Modified,
                    path: root.join("src/ui/git_diff.rs"),
                },
                ChangedFile {
                    status: FileStatus::Added,
                    path: root.join("src/ui.rs"),
                },
            ],
            selected: 1,
            diff: highlight_diff_rows(
                pair_side_by_side(parse_unified_diff(
                    "@@ -1,3 +1,4 @@\n context\n-removed line\n+added line\n",
                )),
                Path::new("/repo/src/ui.rs"),
            ),
            error: None,
            scroll: 0,
            focus: GitViewFocus::Files,
            diff_pending: false,
            refreshed_at: Instant::now(),
        }
    }

    fn space() -> Size {
        Size {
            width: 80,
            height: 20,
        }
    }

    /// The view carries meaning, never appearance: a status mark is a
    /// [`Role`], not a colour, so the host's palette stays the only place
    /// chrome colour is decided.
    #[test]
    fn the_view_names_meaning_rather_than_colour() {
        let view = view(&fixture(), space());
        let navigator = view.navigator.expect("files to navigate");
        assert_eq!(navigator.badge, "2");
        assert!(navigator.focused);

        let marker = navigator.rows.iter().find_map(|row| match row {
            NavigatorRow::Item { name, marker, .. } if name == "ui.rs" => Some(marker),
            _ => None,
        });
        let marker = marker.expect("the added file is listed");
        assert_eq!(marker.role, Role::Success, "added, not a colour");
        assert!(
            marker.color.is_none(),
            "chrome never carries its own colour"
        );
    }

    /// Syntax colour is the one thing that does travel as data: it comes
    /// from a theme the extension ships, and a role would throw it away.
    #[test]
    fn syntax_colour_survives_as_the_extensions_own_data() {
        let view = view(&fixture(), space());
        let Content::Lines { lines, .. } = view.content else {
            panic!("a selected file has a diff");
        };
        assert!(lines.iter().any(|line| line.tone == LineTone::Added));
        assert!(lines.iter().any(|line| line.tone == LineTone::Removed));
        assert!(
            lines
                .iter()
                .flat_map(|line| &line.spans)
                .any(|span| span.color.is_some()),
            "highlighting reaches the host"
        );
    }

    #[test]
    fn an_unreadable_checkout_has_nothing_to_navigate() {
        let view = view(
            &GitView::with_error(&TestHost, PathBuf::from("/nope"), "boom".to_owned()),
            space(),
        );
        assert!(view.navigator.is_none());
        assert!(matches!(
            view.content,
            Content::Message {
                role: Role::Danger,
                ..
            }
        ));
    }

    /// Moving the selection reads nothing.
    ///
    /// `TestHost::git` panics, so this passes only while selecting is
    /// pure — which is the property that keeps an arrow key off the
    /// thread that draws.
    #[test]
    fn selecting_a_file_asks_for_its_diff_rather_than_reading_it() {
        // The fixture opens on its second file, so `Up` is the move that
        // actually changes the selection.
        let mut open = fixture();
        assert!(!open.diff_pending(), "a fresh view shows what it read");

        let outcome = handle_key(&mut open, KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));

        assert!(matches!(outcome, GitViewOutcome::Stay));
        assert_eq!(open.selected, 0, "the selection moved");
        assert!(open.diff_pending(), "and a read is owed for it");
        assert!(
            open.diff.is_empty(),
            "the previous file's diff is not left under the new name"
        );

        // What the viewer sees in the meantime says so, rather than
        // reading as "this file has no changes".
        let rendered = view(
            &open,
            Size {
                width: 80,
                height: 24,
            },
        );
        assert!(
            matches!(&rendered.content, Content::Message { text, .. } if text == "reading…"),
            "{:?}",
            rendered.content
        );
    }

    /// The section names meaning, never colour — the same contract the
    /// full-frame view is held to.
    #[test]
    fn the_timeline_section_names_meaning_rather_than_colour() {
        let timeline = Timeline {
            branch: "agent/x".to_owned(),
            commits: vec![
                Commit {
                    hash: "a".to_owned(),
                    subject: "feat: ahead".to_owned(),
                    age: "3h".to_owned(),
                    ahead: true,
                },
                Commit {
                    hash: "b".to_owned(),
                    subject: "chore: landed".to_owned(),
                    age: "2d".to_owned(),
                    ahead: false,
                },
            ],
        };

        let section = timeline_section(&timeline, false, 0);

        assert_eq!(section.title, "timeline");
        assert_eq!(section.caption.text, "agent/x");
        assert!(section.resizable);
        // HEAD is ringed; standing is the hue, and the hue is a role.
        assert_eq!(section.rows[0].marker.text, "\u{25c9}");
        assert_eq!(section.rows[0].marker.role, Role::Info);
        assert_eq!(section.rows[1].marker.text, "\u{25cf}");
        assert_eq!(section.rows[1].marker.role, Role::Warning);
        assert_eq!(section.rows[0].trailing.text, "3h");

        let folded = timeline_section(&timeline, true, 0);
        assert!(folded.collapsed);
        assert_eq!(
            folded.rows.len(),
            section.rows.len(),
            "folding is the host's to draw; the section still holds what it holds"
        );
    }
}
