//! The workspace TUI's git changes overlay — a read-only "quick peek" at
//! `git status`/`git diff` for whichever tab is active, so seeing what
//! changed never requires leaving the terminal for an external editor.
//!
//! Same popup shape `orchestrator`'s `AgentPicker`/`ContextMenu` already
//! use (an `Option<T>` the caller renders last, on top of everything, and
//! discards on `Esc`), just sized to the whole frame instead of a small
//! anchored box — see `openspec/changes/add-git-diff-overlay/design.md`.
//! Its own module (not folded into `orchestrator.rs`, already large) for
//! the git subprocess handling, unified-diff parsing, and syntax
//! highlighting this needs that nothing else in the client does.

use std::{
    io,
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use syntect::{easy::HighlightLines, highlighting::ThemeSet, parsing::SyntaxSet};

use super::orchestrator::WorkspaceHit;

/// What to do after a key/mouse event reaches an open [`GitView`] —
/// `orchestrator`'s event loop only needs to know whether to keep the
/// overlay open or clear `WorkspaceModel::git_view`, never any of this
/// module's internals.
pub(super) enum GitViewOutcome {
    Stay,
    Close,
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

    fn color(self) -> Color {
        match self {
            FileStatus::Modified => super::WARNING,
            FileStatus::Added | FileStatus::Untracked => super::SUCCESS,
            FileStatus::Deleted => super::DANGER,
            FileStatus::Renamed => super::BLUE,
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
    /// Pre-highlighted (see `highlight_diff_rows`) — ready to render, no
    /// syntect types beyond this module's boundary.
    spans: Vec<(Style, String)>,
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
/// Built once via [`GitView::open`] when it's raised, discarded on close —
/// never re-fetches on its own; reopening picks up whatever changed.
pub(super) struct GitView {
    /// The repository root the view is scoped to, resolved once at open
    /// time from the active tab's live `cwd` — see `open`'s doc comment.
    root: PathBuf,
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
}

impl GitView {
    /// `cwd` is the active tab's live working directory at the moment the
    /// view opens (see `orchestrator::open_git_view`) — this resolves the
    /// enclosing repository root from it once, up front, and every
    /// subsequent `git` call in this view uses that root, not `cwd` again.
    pub(super) fn open(cwd: PathBuf) -> Self {
        match repository_root(&cwd) {
            Ok(root) => match run_git(
                &root,
                &["status", "--porcelain=v1", "--untracked-files=all"],
            ) {
                Ok(output) => {
                    let files = parse_porcelain_status(&output, &root);
                    let mut view = Self {
                        root,
                        files,
                        selected: 0,
                        diff: Vec::new(),
                        error: None,
                        scroll: 0,
                        focus: GitViewFocus::Files,
                    };
                    view.load_selected_diff();
                    view
                }
                Err(message) => Self::with_error(root, message),
            },
            Err(message) => Self::with_error(cwd, message),
        }
    }

    fn with_error(root: PathBuf, message: String) -> Self {
        Self {
            root,
            files: Vec::new(),
            selected: 0,
            diff: Vec::new(),
            error: Some(message),
            scroll: 0,
            focus: GitViewFocus::Files,
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

    fn load_selected_diff(&mut self) {
        let Some(file) = self.files.get(self.selected) else {
            self.diff = Vec::new();
            return;
        };
        let path = file.path.clone();
        let status = file.status;
        let raw = if status == FileStatus::Untracked {
            run_git(
                &self.root,
                &[
                    "diff",
                    "--no-index",
                    "--",
                    "/dev/null",
                    &path.to_string_lossy(),
                ],
            )
        } else {
            run_git(&self.root, &["diff", "HEAD", "--", &path.to_string_lossy()])
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
    let output = git_command(cwd, &["rev-parse", "--show-toplevel"])
        .output()
        .map_err(describe_spawn_failure)?;
    if output.status.success() {
        Ok(PathBuf::from(
            String::from_utf8_lossy(&output.stdout).trim(),
        ))
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

/// Runs `git -C <root> <args>`, treating exit `0` *or* `1` as success —
/// `git diff` (with or without `--no-index`) exits `1` whenever there's a
/// diff to show, which is the ordinary case here, never a failure.
fn run_git(root: &Path, args: &[&str]) -> Result<String, String> {
    let output = git_command(root, args)
        .output()
        .map_err(describe_spawn_failure)?;
    match output.status.code() {
        Some(0) | Some(1) => Ok(String::from_utf8_lossy(&output.stdout).into_owned()),
        _ => Err(String::from_utf8_lossy(&output.stderr).trim().to_owned()),
    }
}

fn git_command(root: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("git");
    command.arg("-C").arg(root).args(args);
    command
}

fn describe_spawn_failure(error: io::Error) -> String {
    if error.kind() == io::ErrorKind::NotFound {
        "git is not installed or not on PATH".to_owned()
    } else {
        format!("could not run git: {error}")
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
) -> Vec<(Style, String)> {
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
                Style::default().fg(Color::Rgb(fg.r, fg.g, fg.b)),
                piece.trim_end_matches('\n').to_owned(),
            )
        })
        .collect()
}

pub(super) fn handle_key(view: &mut GitView, key: KeyEvent) -> GitViewOutcome {
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
        KeyCode::PageUp => view.scroll = view.scroll.saturating_sub(10),
        KeyCode::PageDown => view.scroll = view.scroll.saturating_add(10),
        _ => {}
    }
    GitViewOutcome::Stay
}

pub(super) fn handle_mouse(view: &mut GitView, hit: Option<WorkspaceHit>) -> GitViewOutcome {
    if let Some(WorkspaceHit::GitSelectFile(index)) = hit {
        view.select(index);
    }
    GitViewOutcome::Stay
}

/// Mouse-wheel scroll over an open [`GitView`] — routed by *where the
/// cursor is*, not by `GitViewFocus` (which only reflects `Tab`/keyboard
/// navigation): hovering the file list moves the selection, hovering the
/// diff scrolls it, matching how a mouse wheel behaves everywhere else
/// (VS Code included) regardless of which panel last had keyboard focus.
pub(super) fn handle_scroll(view: &mut GitView, frame_area: Rect, mouse: MouseEvent) {
    if view.error.is_some() || view.files.is_empty() {
        return;
    }
    let (files_col, diff_col) = content_columns(frame_area);
    let in_column = |col: Rect| {
        col.x <= mouse.column
            && mouse.column < col.x + col.width
            && col.y <= mouse.row
            && mouse.row < col.y + col.height
    };
    match mouse.kind {
        MouseEventKind::ScrollUp if in_column(files_col) => {
            view.select(view.selected.saturating_sub(1));
        }
        MouseEventKind::ScrollDown if in_column(files_col) => {
            view.select(view.selected + 1);
        }
        MouseEventKind::ScrollUp if in_column(diff_col) => {
            view.scroll = view.scroll.saturating_sub(3);
        }
        MouseEventKind::ScrollDown if in_column(diff_col) => {
            view.scroll = view.scroll.saturating_add(3);
        }
        _ => {}
    }
}

/// Draws the overlay across the entire frame — every other row this frame
/// would otherwise have drawn (sidebar, tab strip, pane, any other popup)
/// is skipped by the caller for this frame instead of drawn and covered,
/// see `orchestrator::render`.
pub(super) fn render(
    frame: &mut ratatui::Frame<'_>,
    view: &GitView,
    area: Rect,
    hits: &mut Vec<(Rect, WorkspaceHit)>,
) {
    frame.render_widget(Clear, area);
    let block = Block::default()
        .title(format!(
            " git changes — {} ",
            super::display_project_path(&view.root)
        ))
        .title_style(
            Style::default()
                .fg(super::ACCENT)
                .add_modifier(Modifier::BOLD),
        )
        .borders(Borders::ALL)
        .border_style(Style::default().fg(super::BORDER))
        .style(Style::default().bg(super::BASE));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    let footer = rows[1];

    if let Some(message) = &view.error {
        frame.render_widget(
            Paragraph::new(Span::styled(
                message.clone(),
                Style::default().fg(super::DANGER),
            )),
            rows[0],
        );
        render_footer(frame, footer);
        return;
    }
    if view.files.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "no changes",
                Style::default().fg(super::MUTED),
            )),
            rows[0],
        );
        render_footer(frame, footer);
        return;
    }

    let (files_col, diff_col) = content_columns(area);
    render_file_list(frame, files_col, view, hits);
    render_diff(frame, diff_col, view);
    render_footer(frame, footer);
}

/// The file-list/diff column split of the overlay's content row, derived
/// straight from the *outer* frame area — shared by `render` and
/// `handle_scroll` so hit-testing a wheel event against "which column is
/// the cursor over" can never drift from what was actually drawn there.
fn content_columns(frame_area: Rect) -> (Rect, Rect) {
    let inner = Rect::new(
        frame_area.x + 1,
        frame_area.y + 1,
        frame_area.width.saturating_sub(2),
        frame_area.height.saturating_sub(2),
    );
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);
    let files_width = (rows[0].width / 3).clamp(20, 40);
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(files_width), Constraint::Min(10)])
        .split(rows[0]);
    (cols[0], cols[1])
}

fn render_file_list(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    view: &GitView,
    hits: &mut Vec<(Rect, WorkspaceHit)>,
) {
    let focused = view.focus == GitViewFocus::Files;
    for (index, file) in view.files.iter().enumerate() {
        if index as u16 >= area.height {
            break;
        }
        let row = Rect::new(area.x, area.y + index as u16, area.width, 1);
        let selected = index == view.selected;
        let name = file
            .path
            .strip_prefix(&view.root)
            .unwrap_or(&file.path)
            .display()
            .to_string();
        let label_style = if selected && focused {
            Style::default()
                .fg(super::TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD)
        } else if selected {
            Style::default().fg(super::TEXT_BRIGHT)
        } else {
            Style::default().fg(super::NAV_INACTIVE)
        };
        let spans = vec![
            Span::styled(
                format!(" {} ", file.status.glyph()),
                Style::default().fg(file.status.color()),
            ),
            Span::styled(name, label_style),
        ];
        frame.render_widget(Paragraph::new(Line::from(spans)), row);
        hits.push((row, WorkspaceHit::GitSelectFile(index)));
    }
}

/// A subtle wash behind a removed line's left-column cell — same family as
/// `super::SELECTED_BG`/`super::SURFACE_OVERLAY` (barely-there tints over
/// `BASE`), just red-leaning instead of green/neutral.
const DIFF_REMOVED_BG: Color = Color::Rgb(38, 22, 20);
/// The added-line counterpart, right-column cells — green-leaning, same
/// family and strength as `DIFF_REMOVED_BG`.
const DIFF_ADDED_BG: Color = Color::Rgb(18, 32, 23);

#[derive(Clone, Copy)]
enum DiffSide {
    Left,
    Right,
}

/// Two columns — before (left, `Removed`/`Context`) and after (right,
/// `Context`/`Added`) — rather than one interleaved `+`/`-` stream, so a
/// change reads at a glance the way VS Code's own split diff view does,
/// not as a stream of ± prefixes to parse line by line.
fn render_diff(frame: &mut ratatui::Frame<'_>, area: Rect, view: &GitView) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Length(1),
            Constraint::Percentage(50),
        ])
        .split(area);
    let (left, divider, right) = (cols[0], cols[1], cols[2]);
    for (row_index, row) in view
        .diff
        .iter()
        .enumerate()
        .skip(view.scroll as usize)
        .take(left.height as usize)
    {
        let y = left.y + (row_index - view.scroll as usize) as u16;
        render_diff_cell(
            frame,
            Rect::new(left.x, y, left.width, 1),
            row.left.as_ref(),
            DiffSide::Left,
        );
        render_diff_cell(
            frame,
            Rect::new(right.x, y, right.width, 1),
            row.right.as_ref(),
            DiffSide::Right,
        );
        frame.render_widget(
            Paragraph::new(Span::styled("│", Style::default().fg(super::BORDER_FAINT))),
            Rect::new(divider.x, y, 1, 1),
        );
    }
}

/// One cell of one side of a diff row — blank (nothing drawn, so the
/// pane's own background shows through) when `cell` is `None`, which is
/// exactly what a pure addition's left side (or a pure removal's right
/// side) is: nothing on that side to show.
fn render_diff_cell(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    cell: Option<&DiffCell>,
    side: DiffSide,
) {
    let Some(cell) = cell else {
        return;
    };
    let bg = match (side, cell.kind) {
        (DiffSide::Left, DiffLineKind::Removed) => Some(DIFF_REMOVED_BG),
        (DiffSide::Right, DiffLineKind::Added) => Some(DIFF_ADDED_BG),
        _ => None,
    };
    let mut spans = vec![Span::styled(
        format!("{:>4} ", cell.line_no),
        Style::default().fg(super::TEXT_DIM),
    )];
    spans.extend(
        cell.spans
            .iter()
            .map(|(style, text)| Span::styled(text.clone(), *style)),
    );
    if let Some(bg) = bg {
        for span in &mut spans {
            span.style = span.style.bg(bg);
        }
        let used: usize = spans.iter().map(Span::width).sum();
        let pad = (area.width as usize).saturating_sub(used);
        if pad > 0 {
            spans.push(Span::styled(" ".repeat(pad), Style::default().bg(bg)));
        }
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_footer(frame: &mut ratatui::Frame<'_>, area: Rect) {
    frame.render_widget(
        Paragraph::new(Line::from(super::hint_spans(
            "↑↓ navigate · tab focus · esc close",
        ))),
        area,
    );
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
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("uze-git-diff-test-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let git = |args: &[&str]| {
            let status = Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .status()
                .expect("git must be on PATH for this test");
            assert!(status.success(), "git {args:?} failed");
        };
        git(&["init", "--quiet"]);
        git(&["config", "user.email", "test@example.com"]);
        git(&["config", "user.name", "Test"]);
        git(&["config", "commit.gpgsign", "false"]);
        std::fs::write(root.join("tracked.rs"), "fn one() {}\n").unwrap();
        git(&["add", "tracked.rs"]);
        git(&["commit", "--quiet", "-m", "initial"]);

        std::fs::write(root.join("tracked.rs"), "fn one() {}\nfn two() {}\n").unwrap();
        std::fs::write(root.join("staged.rs"), "fn staged() {}\n").unwrap();
        git(&["add", "staged.rs"]);
        std::fs::write(root.join("new.rs"), "fn brand_new() {}\n").unwrap();

        let view = GitView::open(root.clone());
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
        // The first file's diff loaded automatically on open.
        assert!(
            !view.diff.is_empty(),
            "expected a non-empty diff for the first file"
        );

        let _ = std::fs::remove_dir_all(&root);
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
}
