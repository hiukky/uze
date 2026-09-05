//! Terminal presentation over [`UzeApplication`] — product surface, not a
//! debug console.
//!
//! This module owns only navigation/selection/overlay state and input
//! transitions. Every product operation runs in a short-lived worker against
//! a fresh application facade, so the terminal never reads Store, vendor
//! files, integrations, or `marketplace.json` directly — it calls
//! `UzeApplication` read models exactly like the CLI does, and renders what
//! comes back.
//!
//! Module map — start at [`run`], the entry point:
//! - `ui.rs` (this file): the entry point, plus chrome shared by both TUI
//!   modes — the color palette, [`TerminalSession`] (the one alternate-screen
//!   lifecycle both modes draw into), the Work/Manage toggle, and the
//!   sidebar-geometry math (`clamp_sidebar_width`/`sidebar_width_for`) both
//!   sidebars resize by.
//! - [`orchestrator`]: the terminal workspace mode (ADR-038) — tabs, panes,
//!   the persistent runtime client. Self-contained: owns its own model, hit
//!   type, and render loop in one file.
//! - `management`: the management mode (routes below) — this mode's
//!   counterpart to `orchestrator`, same shape (model loop + render loop in
//!   one file).
//! - [`model`]/[`input`]/[`hit`]/[`worker`]: management's MVU pieces —
//!   `TuiModel` (state), key/mouse handling, hit-testing, and the
//!   intent/worker dispatch that runs product operations off-thread.
//! - [`view`]: one file per management route (Overview, Plugins,
//!   Extensions, Harnesses, Profiles, Doctor).
//! - `overlay`: modal dialogs shared across management routes.

use std::{
    io::{self, Stdout},
    path::PathBuf,
    time::Duration,
};

use crossterm::{
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use uze_application::{
    ProcessOutput, ProcessResult, ProcessRunner, ProcessSpec, Result, SystemProcessRunner,
    UzeApplication, UzeHome,
};

mod agent_support;
mod extension_host;
mod extension_view;
mod hit;
mod input;
mod management;
mod model;
mod orchestrator;
mod overlay;
mod root_picker;
pub(crate) mod theme;

use theme::{Symbol, Token};
pub mod view;
mod worker;

const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Forces every process the TUI spawns to run silently, regardless of
/// whether the integration asked for inherited output. The TUI owns the
/// terminal's alternate screen for its own rendering; a vendor installer's
/// progress written directly to the real stdout (as `uze setup`'s inherited
/// output is designed to do on the CLI) has nowhere sane to land here — it
/// prints straight through the ratatui frame and corrupts the layout, which
/// is exactly what `SystemProcessRunner`'s `ProcessOutput::Inherit` does.
/// Every `UzeApplication` the TUI constructs uses this instead.
struct SilentProcessRunner;

impl ProcessRunner for SilentProcessRunner {
    fn run(&self, spec: &ProcessSpec) -> Result<ProcessResult> {
        let quiet = ProcessSpec {
            output: ProcessOutput::Quiet,
            ..spec.clone()
        };
        SystemProcessRunner.run(&quiet)
    }
}

/// The TUI's one composition point for `UzeApplication` — every worker
/// thread builds its application through this, never `UzeApplication::from_env`
/// directly, so no code path can accidentally let a provisioning command's
/// output loose on the terminal.
fn tui_application(home: UzeHome) -> Result<UzeApplication> {
    UzeApplication::from_env_with_runner(home, Box::new(SilentProcessRunner))
}

/// Runs the TUI. `home` is passed to workers, which construct the same
/// production application composition root as the CLI.
pub fn run(home: UzeHome) -> Result<()> {
    let mut terminal = TerminalSession::start()?;
    // The sidebar as this user last left it — read once, here, because
    // both modes share the column and neither owns it (see
    // `uze_application::SidebarLayout`).
    let stored = tui_application(home.clone())
        .map(|app| app.workspace().sidebar_layout())
        .unwrap_or_default();
    // Shared across both modes for the whole run() call, not owned by
    // either model: a drag in one sidebar must still be there — same
    // width — when Ctrl+O switches to the other, not reset back to the
    // responsive default every round trip.
    let mut sidebar_width: Option<u16> = stored.width;
    // Likewise what the workspace client resolved for itself — the
    // sidebar's tasks, branches and agent statuses among them — which each
    // attach takes over from the last instead of deriving again in front
    // of the user (see `orchestrator::WorkspaceMemory`).
    let mut workspace_memory = orchestrator::WorkspaceMemory::restored(stored);
    // Set when management asks to return to a specific tab (activating a
    // prompt-history row); consumed by the next attach.
    let mut pending_tab: Option<uze_terminal::TabId> = None;
    loop {
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        match orchestrator::attach_workspace(
            &mut terminal,
            &root,
            &mut sidebar_width,
            &mut workspace_memory,
            &home,
            pending_tab.take(),
        )? {
            orchestrator::WorkspaceExit::Quit => return Ok(()),
            orchestrator::WorkspaceExit::Management => {
                let exit =
                    management::run_management(&mut terminal, home.clone(), &mut sidebar_width)?;
                // The workspace client writes its own changes as they
                // happen; management has no such sink, so a drag there is
                // kept on the way out of it — the one moment this side
                // holds both halves of the layout.
                remember_sidebar(&home, workspace_memory.sidebar_layout(sidebar_width));
                match exit {
                    management::ManagementExit::Quit => return Ok(()),
                    management::ManagementExit::Workspace => {}
                    management::ManagementExit::WorkspaceTab(tab) => {
                        pending_tab = Some(uze_terminal::TabId(tab));
                    }
                }
            }
        }
    }
}

/// Keeps the sidebar's shape for the next run. Best-effort: a layout that
/// cannot be written is a preference lost, never a session lost.
fn remember_sidebar(home: &UzeHome, layout: uze_application::SidebarLayout) {
    let _ =
        tui_application(home.clone()).and_then(|app| app.workspace().save_sidebar_layout(&layout));
}

// --- Terminal lifecycle ------------------------------------------------------

/// Owns the raw-mode/alternate-screen/mouse-capture lifecycle for the whole
/// `run()` call, not per mode: management and the terminal workspace used to
/// each open and tear down their own alternate screen on every Ctrl+O round
/// trip, which — even done back-to-back with no perceptible gap — is two
/// consecutive full-screen buffer swaps most terminal emulators render as a
/// visible flash, reading as uze itself closing and reopening. One session,
/// entered once and left once (on quit), makes switching modes just a
/// different `draw` call into the same already-open screen.
pub(crate) struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalSession {
    fn start() -> Result<Self> {
        enable_raw_mode().map_err(io_error)?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(
            stdout,
            EnterAlternateScreen,
            crossterm::cursor::Hide,
            EnableMouseCapture,
            EnableBracketedPaste
        ) {
            let _ = disable_raw_mode();
            return Err(io_error(error));
        }
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend).map_err(io_error)?;
        Ok(Self { terminal })
    }

    pub(crate) fn size(&self) -> Result<ratatui::layout::Size> {
        self.terminal.size().map_err(io_error)
    }

    pub(crate) fn draw(&mut self, render: impl FnOnce(&mut ratatui::Frame<'_>)) -> Result<()> {
        self.terminal.draw(render).map(|_| ()).map_err(io_error)
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture,
            DisableBracketedPaste,
            crossterm::cursor::Show
        );
        let _ = self.terminal.show_cursor();
    }
}

fn io_error(source: io::Error) -> uze_application::UzeError {
    uze_application::UzeError::Write {
        path: PathBuf::from("terminal"),
        source,
    }
}

/// `n` in subscript digits (`12` -> `₁₂`): a count that sits beside a
/// label without competing with it for weight — the route counts in the
/// management sidebar, the pull/push counts under an agent's branch.
/// The first row of an informational popup: its name, and the key that
/// dismisses it pinned to the right.
pub(crate) fn title_row(name: &str, dismiss: &str, width: usize) -> ratatui::text::Line<'static> {
    use ratatui::{
        style::{Modifier, Style},
        text::{Line, Span},
    };
    let gap = width
        .saturating_sub(name.chars().count() + dismiss.chars().count())
        .max(1);
    Line::from(vec![
        Span::styled(
            name.to_owned(),
            Style::default()
                .fg(theme::color(Token::TextBright))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" ".repeat(gap)),
        Span::styled(dismiss.to_owned(), theme::fg(Token::TextMuted)),
    ])
}

pub(crate) fn small_digits(n: usize) -> String {
    n.to_string()
        .chars()
        .map(|c| match c {
            '0' => '₀',
            '1' => '₁',
            '2' => '₂',
            '3' => '₃',
            '4' => '₄',
            '5' => '₅',
            '6' => '₆',
            '7' => '₇',
            '8' => '₈',
            '9' => '₉',
            _ => c,
        })
        .collect()
}

/// `~/relative/path` when `root` is under the user's home directory, else
/// the path as-is — mirrors what a shell prompt usually shows.
pub(crate) fn display_project_path(root: &std::path::Path) -> String {
    if let Some(home) = std::env::var_os("HOME")
        && let Ok(relative) = root.strip_prefix(&home)
    {
        return format!("~/{}", relative.display());
    }
    root.display().to_string()
}

// --- Shared helpers ---------------------------------------------------------

/// Narrowest either sidebar (workspace or management) can be dragged. Needs
/// to comfortably fit an agent tab's indented detail line — connector +
/// cwd on the left, the running agent's alias pinned to the row's own
/// right edge (see `orchestrator::render_sidebar`'s agent-tab loop) — the
/// widest row this menu draws; a bound tight enough to only fit a short
/// label broke that layout once the alias moved off the end of the cwd
/// text and onto a fixed right column.
const MIN_SIDEBAR_WIDTH: u16 = 28;
/// Widest either sidebar can be dragged, regardless of how wide the
/// terminal is — it's navigation, not the workspace; past this it's just
/// width the content column could otherwise use.
const MAX_SIDEBAR_WIDTH: u16 = 40;
/// Dragging either sidebar's border never shrinks its content column below
/// this many columns.
const MIN_CONTENT_WIDTH: u16 = 30;

/// Shared by both TUIs' sidebar drag-resize — same bounds, so the two feel
/// identical to drag rather than just similarly shaped.
fn clamp_sidebar_width(width: u16, total_width: u16) -> u16 {
    let max = total_width
        .saturating_sub(MIN_CONTENT_WIDTH)
        .clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH);
    width.clamp(MIN_SIDEBAR_WIDTH, max)
}

/// Shared responsive default sidebar width (no user drag override yet) for
/// both TUIs. Every bucket stays at or above `MIN_SIDEBAR_WIDTH` — this
/// path isn't run through `clamp_sidebar_width`, so a bucket smaller than
/// the drag floor would reintroduce the same overflow on a narrow terminal
/// that raising the floor was meant to fix.
fn sidebar_width_for(total_width: u16) -> u16 {
    if total_width < 60 {
        MIN_SIDEBAR_WIDTH
    } else if total_width < 90 {
        30
    } else {
        32
    }
}

/// The shared Work / Manage segmented control for both TUIs. The active
/// mode is a filled chip so the compact menu preserves a clear, clickable
/// indication of which surface is open. Returns both segment hit rects so
/// each caller can map the inactive side to its own switch intent.
fn render_mode_toggle(
    frame: &mut ratatui::Frame<'_>,
    rect: Rect,
    workspace_active: bool,
) -> (Rect, Rect) {
    let filled = Style::default()
        .bg(theme::color(Token::Accent))
        .fg(theme::color(Token::SurfaceBackground))
        .add_modifier(Modifier::BOLD);
    let ghost = theme::on(Token::TextInactive, Token::SurfaceRecessed);
    let button_width = "Manage".len() as u16 + 2;
    let centered = |label: &str| {
        let extra = button_width.saturating_sub(label.len() as u16);
        let left = extra / 2;
        let right = extra - left;
        format!(
            "{}{label}{}",
            " ".repeat(left as usize),
            " ".repeat(right as usize)
        )
    };
    let work = Span::styled(
        centered("Work"),
        if workspace_active { filled } else { ghost },
    );
    let gap = Span::raw(" ");
    let manage = Span::styled(
        centered("Manage"),
        if workspace_active { ghost } else { filled },
    );
    let work_width = work.width() as u16;
    let gap_width = gap.width() as u16;
    let manage_width = manage.width() as u16;
    let total_width = work_width + gap_width + manage_width;
    let start_x = rect.x + rect.width.saturating_sub(total_width) / 2;
    frame.render_widget(
        Paragraph::new(Line::from(vec![work, gap, manage]))
            .alignment(ratatui::layout::Alignment::Center),
        rect,
    );
    (
        Rect::new(start_x, rect.y, work_width, 1),
        Rect::new(start_x + work_width + gap_width, rect.y, manage_width, 1),
    )
}

/// Styled spans for one footer hint: `key action · key action …` chunks are
/// split so the command/key part carries the accent (and bold) and the
/// description stays muted — the shortcut bar reads as "keys + what they
/// do" instead of one uniform wall of gray text. Chunks without a verb
/// (e.g. `y/n`) render as a command alone.
/// What a hint string is *written* with to mark where one clause ends and
/// the next begins. It is notation, not output: [`hint_spans`] replaces it
/// with whatever the active theme draws a separator as, so a hint stays
/// readable in the source without pinning the glyph.
const HINT_SEPARATOR: &str = " · ";

/// The key glyphs a hint line is *written* with, and the symbols they stand
/// for. Same idea as [`HINT_SEPARATOR`]: "↑↓ select" reads as itself in the
/// source, and comes out of [`hint_spans`] in whatever the active theme
/// draws those keys as — `^v select` under the ASCII theme.
const HINT_NOTATION: &[(char, Symbol)] = &[
    ('\u{2191}', Symbol::ArrowUp),
    ('\u{2193}', Symbol::ArrowDown),
    ('\u{21e7}', Symbol::ArrowShift),
];

fn hint_notation(chunk: &str) -> String {
    if !chunk
        .chars()
        .any(|c| HINT_NOTATION.iter().any(|(k, _)| *k == c))
    {
        return chunk.to_owned();
    }
    chunk
        .chars()
        .map(|c| match HINT_NOTATION.iter().find(|(key, _)| *key == c) {
            Some((_, symbol)) => theme::glyph(*symbol),
            None => c.to_string(),
        })
        .collect()
}

/// Two clauses of one help line, joined by whatever the active theme draws
/// a separator as — the same join [`hint_spans`] makes, for the lines that
/// are plain text rather than key/action pairs.
pub(crate) fn hint_aside(first: &str, second: &str) -> String {
    format!("{first}{}{second}", theme::glyph(Symbol::HintSeparator))
}

fn hint_spans(hint: &str) -> Vec<Span<'static>> {
    let command = theme::fg_bold(Token::Accent);
    let muted = theme::fg(Token::TextMuted);
    let mut spans = Vec::new();
    let separator = theme::glyph(Symbol::HintSeparator);
    for (i, chunk) in hint.split(HINT_SEPARATOR).enumerate() {
        let chunk = &hint_notation(chunk);
        if i > 0 {
            spans.push(Span::raw(separator.clone()));
        }
        match chunk.split_once(' ') {
            Some((key, action)) => {
                spans.push(Span::styled(key.to_owned(), command));
                spans.push(Span::styled(format!(" {action}"), muted));
            }
            None => spans.push(Span::styled(chunk.to_owned(), command)),
        }
    }
    spans
}

/// The inset [`content_area`] keeps on each side of a screen's content.
const CONTENT_INSET_LEFT: u16 = 2;
const CONTENT_INSET_RIGHT: u16 = 2;
const CONTENT_INSET_TOP: u16 = 1;

/// Every content screen's outer inset — the design's `padding: 36px 44px`
/// on each route's root div, translated to terminal cells. No border, no
/// background: content just sits indented on the shared backdrop.
pub(crate) fn content_area(area: Rect) -> Rect {
    Rect::new(
        area.x + CONTENT_INSET_LEFT,
        area.y + CONTENT_INSET_TOP,
        area.width
            .saturating_sub(CONTENT_INSET_LEFT + CONTENT_INSET_RIGHT),
        area.height.saturating_sub(CONTENT_INSET_TOP),
    )
}

/// A panel that opens off the right of a content area — a drawer, or a
/// permanent right-hand column — drawn flush against the frame's own top
/// and right edges instead of stopping at the content inset: text sitting
/// two columns in reads as a margin, a filled slab ending two columns
/// short of the border reads as misaligned. `width` is how much of the
/// content area the panel takes; the inset it swallows on the right comes
/// on top of that, so whatever lays out to its left is unaffected, and
/// the panel's own inner padding keeps its text off the edge.
pub(crate) fn side_panel_area(content: Rect, width: u16) -> Rect {
    let width = width.min(content.width);
    Rect::new(
        content.right().saturating_sub(width),
        content.y.saturating_sub(CONTENT_INSET_TOP),
        width + CONTENT_INSET_RIGHT,
        content.height + CONTENT_INSET_TOP,
    )
}

/// Every screen's header: a bold bright title, a muted subtitle on the
/// next line, and an optional right-aligned trailer on the title's own
/// row (item count, doctor summary, source count — whatever that route
/// reports). Exactly the two-line header shape every route in the design
/// uses. Returns the area still available below the header plus its own
/// blank spacer row.
pub(crate) fn render_screen_header(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    title: &str,
    subtitle: &str,
    trailer: Option<Span<'static>>,
) -> Rect {
    let title_style = Style::default()
        .fg(theme::color(Token::TextBright))
        .add_modifier(Modifier::BOLD);
    let title_row = Rect::new(area.x, area.y, area.width.saturating_sub(1), 1);
    if let Some(trailer) = trailer {
        let trailer_width = trailer.width() as u16;
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(1), Constraint::Length(trailer_width)])
            .split(title_row);
        frame.render_widget(Paragraph::new(Span::styled(title, title_style)), columns[0]);
        frame.render_widget(
            Paragraph::new(trailer).alignment(ratatui::layout::Alignment::Right),
            columns[1],
        );
    } else {
        frame.render_widget(Paragraph::new(Span::styled(title, title_style)), title_row);
    }
    if area.height > 1 {
        let subtitle_row = Rect::new(area.x, area.y + 1, area.width, 1);
        frame.render_widget(
            Paragraph::new(Span::styled(subtitle, theme::fg(Token::TextMuted))),
            subtitle_row,
        );
    }
    let consumed = 3.min(area.height);
    Rect::new(
        area.x,
        area.y + consumed,
        area.width,
        area.height.saturating_sub(consumed),
    )
}

fn route_style(route: &str) -> Style {
    match route {
        "native" => theme::fg(Token::StateSuccess),
        "adapted" | "decomposed" => theme::fg(Token::Accent),
        "degraded" => theme::fg(Token::StateWarning),
        _ => theme::fg(Token::StateDanger),
    }
}

// --- Row chrome ----------------------------------------------------------
//
// One column, one row at a time: what both sidebars and every extension
// section are laid out with. Here rather than in either mode's own
// renderer because an extension's section is drawn by `extension_view`,
// which is a sibling of both — and two modules deriving the same trailing
// column independently is what this file already exists to prevent.

/// A downward cursor over one column's rows. The sidebar lays itself out a
/// row at a time and simply stops once the column is full, so nothing it
/// draws needs to know in advance how tall everything else came out.
pub(crate) struct Rows {
    x: u16,
    width: u16,
    y: u16,
    bottom: u16,
    /// Rows still to be scrolled past before anything lands on screen. A
    /// column that is scrolled still asks for the rows above its window —
    /// a list only knows what it is showing by walking what comes before
    /// it — and gets [`Slot::Hidden`] for them instead of a rectangle.
    skipped: u16,
}

/// Where a row landed once the column it belongs to is scrolled.
///
/// [`Rows::next`] flattens this back to an `Option` for the columns that
/// never scroll; a scrolled one has to tell "above the window" (keep
/// laying out) from "past the foot" (stop), which one `None` cannot say.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Slot {
    /// Scrolled out above the window: laid out, never drawn.
    Hidden,
    Visible(Rect),
    /// The column is full — nothing after this one fits either.
    Full,
}

impl Slot {
    /// The rectangle to draw into, if this row is on screen at all.
    pub(crate) fn visible(self) -> Option<Rect> {
        match self {
            Self::Visible(rect) => Some(rect),
            Self::Hidden | Self::Full => None,
        }
    }

    pub(crate) fn is_full(self) -> bool {
        self == Self::Full
    }
}

impl Rows {
    pub(crate) fn over(area: Rect) -> Self {
        Self {
            x: area.x,
            width: area.width,
            y: area.y,
            bottom: area.y + area.height,
            skipped: 0,
        }
    }

    /// Scrolls the next `rows` out above the window. Whole rows only: a
    /// column scrolls by its own items, never by half of one.
    pub(crate) fn scroll_past(&mut self, rows: u16) {
        self.skipped = rows;
    }

    pub(crate) fn slot(&mut self, height: u16) -> Slot {
        if self.skipped >= height {
            self.skipped -= height;
            return Slot::Hidden;
        }
        match self.next(height) {
            Some(rect) => Slot::Visible(rect),
            None => Slot::Full,
        }
    }

    pub(crate) fn next(&mut self, height: u16) -> Option<Rect> {
        if self.y + height > self.bottom {
            return None;
        }
        let rect = Rect::new(self.x, self.y, self.width, height);
        self.y += height;
        Some(rect)
    }

    /// One blank row, when the column still has one to spare — scrolled
    /// past like any other row when the column is scrolled.
    pub(crate) fn gap(&mut self) {
        let _ = self.slot(1);
    }

    pub(crate) fn remaining(&self) -> u16 {
        self.bottom.saturating_sub(self.y)
    }
}

/// The gap a row keeps between its right-most content and the divider (or
/// the frame) beside it. One place, because a row that reserves a different
/// amount than the row above it reads as ragged rather than as deliberate.
pub(crate) const TRAILING_PAD: u16 = 1;

/// The inset every anchored popup keeps between its border and its content.
/// Four popups had grown their own copy of this pair; they were all the same
/// number, which is the point — a popup that pads differently reads as a
/// different kind of surface.
pub(crate) const POPUP_H_PAD: u16 = 2;
pub(crate) const POPUP_V_PAD: u16 = 1;

/// Appends `text` pinned to the row's right edge, `TRAILING_PAD` off the
/// divider — the column the agent rows keep their alias in.
///
/// `text` is elided rather than allowed to overflow. It is the row's
/// caption, and a caption that does not fit used to run past the edge and
/// be cut there by the frame — which is how a long branch name on the Git
/// section header became an unreadable fragment with no "…" to say it had
/// been shortened.
pub(crate) fn push_trailing<'a>(spans: &mut Vec<Span<'a>>, width: u16, text: String, hue: Color) {
    let leading: u16 = spans.iter().map(|span| span.width() as u16).sum();
    // One column of gap between the leading spans and the caption, so the
    // two never read as one word.
    let room = width.saturating_sub(leading + TRAILING_PAD + 1).max(1);
    let text = elide_tail(&text, room as usize);
    let used = leading + text.chars().count() as u16 + TRAILING_PAD;
    let gap = width.saturating_sub(used).max(1);
    spans.push(Span::raw(" ".repeat(gap as usize)));
    spans.push(Span::styled(text, Style::default().fg(hue)));
    spans.push(Span::raw(" ".repeat(TRAILING_PAD as usize)));
}

/// `text` shortened from the right to `width`, keeping its head — a
/// subject says what it did in its first words.
pub(crate) fn elide_tail(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    let Some(kept) = width.checked_sub(theme::width(Symbol::Ellipsis) as usize) else {
        return String::new();
    };
    let mut kept: String = text.chars().take(kept).collect();
    kept.push_str(&theme::glyph(Symbol::Ellipsis));
    kept
}

/// Stamps `bg` onto every span already in the row, then appends a
/// trailing background-filled run of spaces so the highlight spans the
/// row's full width instead of stopping at the last glyph — same pattern
/// the management views' `render_plugin_row`/`header_line` use for their
/// own selected-row backgrounds.
pub(crate) fn fill_row_bg<'a>(spans: &mut Vec<Span<'a>>, width: u16, bg: Color) {
    for span in spans.iter_mut() {
        span.style = span.style.bg(bg);
    }
    let used: usize = spans.iter().map(Span::width).sum();
    let gap = (width as usize).saturating_sub(used);
    spans.push(Span::styled(" ".repeat(gap), Style::default().bg(bg)));
}

#[cfg(test)]
mod tests;
