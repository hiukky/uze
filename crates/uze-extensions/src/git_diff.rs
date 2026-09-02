//! The workspace TUI's Git changes extension — a read-only "quick peek" at
//! `git status`/`git diff` for whichever tab is active, so seeing what
//! changed never requires leaving the terminal for an external editor.
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
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{Duration, Instant},
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use syntect::{easy::HighlightLines, highlighting::ThemeSet, parsing::SyntaxSet};

use crate::view::{
    Content, ContentLine, LineTone, Navigator, NavigatorRow, Rgb, Role, ScrollDirection,
    ScrollTarget, Size, Span, View, ViewHit,
};

use crate::display_project_path;

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
    id: "git-changes",
    name: "Git Changes",
    description: "Side-by-side working-tree and diff review inside the workspace client.",
    surface: "Workspace TUI",
    usage: "Open with the git button in the workspace tab strip, or Ctrl+G while attached.",
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
pub fn change_summary(cwd: &Path) -> Option<GitChangeSummary> {
    let root = repository_root(cwd).ok()?;
    let status = run_git(
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
        let output = run_git(&root, args).ok()?;
        let (additions, deletions) = parse_numstat(&output);
        summary.additions += additions;
        summary.deletions += deletions;
    }
    for file in files
        .iter()
        .filter(|file| file.status == FileStatus::Untracked)
    {
        summary.additions += untracked_line_count(&file.path);
    }
    Some(summary)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GitViewFocus {
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
    refreshed_at: Instant,
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
    pub fn open(cwd: PathBuf) -> Self {
        let root = match repository_root(&cwd) {
            Ok(root) => root,
            Err(message) => return Self::with_error(cwd, message),
        };
        let status = match run_git(
            &root,
            &["status", "--porcelain=v1", "--untracked-files=all"],
        ) {
            Ok(output) => output,
            Err(message) => return Self::with_error(root, message),
        };
        let mut view = Self {
            branch: current_branch(&root),
            files: parse_porcelain_status(&status, &root),
            root,
            selected: 0,
            diff: Vec::new(),
            error: None,
            scroll: 0,
            focus: GitViewFocus::Files,
            refreshed_at: Instant::now(),
        };
        view.load_selected_diff();
        view
    }

    fn with_error(root: PathBuf, message: String) -> Self {
        Self {
            root,
            branch: String::new(),
            files: Vec::new(),
            selected: 0,
            diff: Vec::new(),
            error: Some(message),
            scroll: 0,
            focus: GitViewFocus::Files,
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
        self.selected = index.min(self.files.len() - 1);
        self.scroll = 0;
        self.load_selected_diff();
    }

    pub fn refresh_due(&self) -> bool {
        self.refreshed_at.elapsed() >= REFRESH_INTERVAL
    }

    pub fn refresh(&mut self) {
        let selected_path = self.files.get(self.selected).map(|file| file.path.clone());
        let focus = self.focus;
        let scroll = self.scroll;
        let mut refreshed = Self::open(self.root.clone());
        refreshed.focus = focus;
        refreshed.scroll = scroll;
        if let Some(path) = selected_path
            && let Some(file) = refreshed.files.iter().position(|file| file.path == path)
        {
            refreshed.selected = file;
            refreshed.load_selected_diff();
        }
        refreshed.refreshed_at = Instant::now();
        *self = refreshed;
    }

    fn load_selected_diff(&mut self) {
        let Some(file) = self.files.get(self.selected) else {
            self.diff = Vec::new();
            return;
        };
        let path = file.path.clone();
        let status = file.status;
        let root = self.root.clone();
        let raw = if status == FileStatus::Untracked {
            run_git(
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
            run_git(&root, &["diff", "HEAD", "--", &path.to_string_lossy()])
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
fn repository_root(cwd: &Path) -> Result<PathBuf, String> {
    uze_git::read(cwd, &["rev-parse", "--show-toplevel"])
        .map_err(|error| error.to_string())?
        .successful()
        .map(|stdout| PathBuf::from(stdout.trim()))
}

/// The branch [`GitView::root`] is on, for the overlay's title. Answers
/// `detached HEAD` for a checkout with no branch, the same wording
/// `git worktree list` used to supply here.
fn current_branch(root: &Path) -> String {
    match run_git(root, &["rev-parse", "--abbrev-ref", "HEAD"]) {
        Ok(name) if !name.trim().is_empty() && name.trim() != "HEAD" => name.trim().to_owned(),
        _ => "detached HEAD".to_owned(),
    }
}

/// Every command this view runs is an observation, so they all go through
/// the transport's read path — which also keeps the overlay from taking
/// Git's optional index lock while an agent is writing in a sibling
/// checkout of the same repository.
///
/// Exit `1` is an answer rather than a failure here: `git diff` uses it for
/// "there are differences", which is the ordinary case for a view whose
/// whole job is showing them.
fn run_git(root: &Path, args: &[&str]) -> Result<String, String> {
    uze_git::read(root, args)
        .map_err(|error| error.to_string())?
        .or_exit(1)
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
fn untracked_line_count(path: &Path) -> u32 {
    let Ok(contents) = std::fs::read_to_string(path) else {
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

/// Applies syntax highlighting (by `path`'s extension) to `rows`, producing
/// ratatui-ready spans — the only place `syntect` types cross into this
/// module's own `DiffRow`/`DiffCell` shape. The left and right columns are
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
pub fn view(git: &GitView, space: Size) -> View {
    let title = format!(
        " git changes — {}{} ",
        display_project_path(&git.root),
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
    } else {
        Content::Lines {
            heading: format!(
                "DIFF · {}",
                git.files
                    .get(git.selected)
                    .and_then(|file| file.path.strip_prefix(&git.root).ok())
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| display_project_path(&git.root))
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
        let _environment = uze_testkit::env::scope();
        let root = uze_testkit::temp::scratch("git-diff-test");
        std::fs::create_dir_all(&root).unwrap();
        let git = |args: &[&str]| {
            uze_git::write(&root, args)
                .expect("git must be on PATH for this test")
                .successful()
                .unwrap_or_else(|error| panic!("git {args:?} failed: {error}"));
        };
        git(&["init", "--quiet"]);
        git(&["config", "user.email", "test@example.com"]);
        git(&["config", "user.name", "Test"]);
        git(&["config", "commit.gpgsign", "false"]);
        std::fs::write(root.join("tracked.rs"), "fn one() {}\n").unwrap();
        git(&["add", "tracked.rs"]);
        git(&["commit", "--quiet", "-m", "initial"]);

        assert_eq!(change_summary(&root), None);

        std::fs::write(root.join("tracked.rs"), "fn one() {}\nfn two() {}\n").unwrap();
        std::fs::write(root.join("staged.rs"), "fn staged() {}\n").unwrap();
        git(&["add", "staged.rs"]);
        std::fs::write(root.join("new.rs"), "fn brand_new() {}\n").unwrap();

        let mut view = GitView::open(root.clone());
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
            change_summary(&root),
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
        view.refresh();
        assert!(
            view.files
                .iter()
                .any(|file| file.path == root.join("later.rs")),
            "refresh must pick up a change made while the viewer is open"
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
    fn a_view_is_scoped_to_the_checkout_it_was_opened_in() {
        let _environment = uze_testkit::env::scope();
        let parent = uze_testkit::temp::scratch("git-diff-scope");
        let root = parent.join("project");
        // Where UZE isolates agents, mirrored here so the fixture matches
        // what the overlay actually meets in a running workspace.
        let linked = root.join(".worktrees").join("feature");
        std::fs::create_dir_all(&root).unwrap();
        let git = |directory: &Path, args: &[&str]| {
            uze_git::write(directory, args)
                .expect("git must be on PATH for this test")
                .successful()
                .unwrap_or_else(|error| panic!("git {args:?} failed: {error}"));
        };
        git(&root, &["init", "--quiet"]);
        git(&root, &["config", "user.email", "test@example.com"]);
        git(&root, &["config", "user.name", "Test"]);
        git(&root, &["config", "commit.gpgsign", "false"]);
        std::fs::write(root.join("README.md"), "# test\n").unwrap();
        std::fs::write(root.join(".gitignore"), ".worktrees/\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "--quiet", "-m", "initial"]);
        git(
            &root,
            &[
                "worktree",
                "add",
                "--quiet",
                "-b",
                "feature",
                linked.to_str().unwrap(),
            ],
        );

        std::fs::write(root.join("primary-only.rs"), "fn primary() {}\n").unwrap();
        std::fs::write(linked.join("agent-only.rs"), "fn agent() {}\n").unwrap();

        let from_agent = GitView::open(linked.clone());
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

        let from_primary = GitView::open(root.clone());
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

        let _ = std::fs::remove_dir_all(parent);
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
}

#[cfg(test)]
mod view_tests {
    use super::*;

    fn fixture() -> GitView {
        let root = PathBuf::from("/repo");
        GitView {
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
            &GitView::with_error(PathBuf::from("/nope"), "boom".to_owned()),
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
}
