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

// Exact palette from the imported design (`UZE TUI.dc.html`), not a
// generic terminal-app palette: a near-black backdrop, warm off-white
// text, and one signature accent (soft sage green) doubling as the
// success/native/pass color everywhere, per the design's own `levelColor`.
// Structure comes from thin hairline dividers (1px rgba-white borders in
// the source), never from filled surface slabs or boxed panels.
const BASE: Color = Color::Rgb(10, 12, 13); // #0a0c0d
const TEXT_BRIGHT: Color = Color::Rgb(242, 240, 234); // #f2f0ea — headings, active state
const TEXT_PRIMARY: Color = Color::Rgb(230, 228, 222); // #e6e4de — body default
const TEXT_SECONDARY: Color = Color::Rgb(168, 166, 160); // #a8a6a0 — descriptions
const TEXT_TERTIARY: Color = Color::Rgb(201, 199, 192); // #c9c7c0 — key/value content
const MUTED: Color = Color::Rgb(107, 113, 118); // #6b7176 — labels, eyebrows
const TEXT_DIM: Color = Color::Rgb(91, 96, 101); // #5b6065 — versions, source tags
const TEXT_FAINT: Color = Color::Rgb(61, 66, 71); // #3d4247 — tree-prefix glyphs
const ACCENT: Color = Color::Rgb(143, 209, 158); // #8fd19e — the one signature hue
const SUCCESS: Color = ACCENT;
const WARNING: Color = Color::Rgb(224, 181, 103); // #e0b567 (amber)
const DANGER: Color = Color::Rgb(224, 118, 95); // #e0765f (red)
const BLUE: Color = Color::Rgb(125, 151, 201); // #7d97c9 — badges and tags

/// Hairline dividers — solid approximations of the design's
/// `rgba(255,255,255,a)` borders, pre-blended over `BASE` since ratatui has
/// no alpha compositing. `BORDER_FAINT` (a≈0.05) separates list rows;
/// `BORDER` (a≈0.08) sits under the titlebar and around the sidebar/inputs.
const BORDER_FAINT: Color = Color::Rgb(22, 24, 25);
const BORDER: Color = Color::Rgb(30, 31, 32);
/// The Marketplace/Harnesses selected-row tint — `rgba(143,209,158,0.09)`
/// (the accent itself, barely-there) pre-blended over `BASE`.
const SELECTED_BG: Color = Color::Rgb(22, 30, 26);
/// A hue-neutral highlight overlay — `rgba(255,255,255,0.09)` pre-blended
/// over `BASE`, the same strength as `SELECTED_BG` but white instead of
/// accent-tinted. For a highlight that marks "this whole block is where
/// you are" without borrowing the accent's meaning (e.g. the active
/// workspace space's envelope) — not every raised surface should read as
/// "on-brand selected", just "raised above the background".
const SURFACE_OVERLAY: Color = Color::Rgb(32, 34, 35);
/// A touch darker than `SURFACE_OVERLAY` — `rgba(255,255,255,0.07)` instead
/// of `0.09`, pre-blended the same way. Fills the active space's tab/
/// detail/cwd rows in the sidebar tree, one shade below the lighter
/// `SURFACE_OVERLAY` its own title row keeps — so the header lifts
/// slightly above the block it names instead of blending into it.
const ACTIVE_SPACE_OVERLAY: Color = Color::Rgb(27, 29, 30);
/// A subtler, darker surface — `rgba(255,255,255,0.025)` pre-blended over
/// `BASE`. Used for unselected cards and detail drawers so `SELECTED_BG`
/// pops with higher contrast while unselected surfaces stay distinct from
/// the backdrop but visually recessed.
const SURFACE_SUBTLE: Color = Color::Rgb(16, 18, 19);
/// A brighter variant of `SURFACE_OVERLAY` — `rgba(255,255,255,0.14)`
/// instead of `0.09`, pre-blended the same way — for the tab strip's
/// "+"/"✦" pair. At the plain overlay's strength the icons read as barely
/// there against the strip's own backdrop; unlike the sidebar surfaces
/// `SURFACE_OVERLAY` marks (which sit next to plain unfilled rows and so
/// only need to read as "raised" by a little), this pair is plain
/// decoration with no bold/color weight otherwise carrying it, so it needs
/// the extra contrast.
const SURFACE_OVERLAY_BRIGHT: Color = Color::Rgb(44, 46, 47);

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Runs the TUI. `home` is passed to workers, which construct the same
/// production application composition root as the CLI.
pub fn run(home: UzeHome) -> Result<()> {
    let mut terminal = TerminalSession::start()?;
    // Shared across both modes for the whole run() call, not owned by
    // either model: a drag in one sidebar must still be there — same
    // width — when Ctrl+O switches to the other, not reset back to the
    // responsive default every round trip.
    let mut sidebar_width: Option<u16> = None;
    // Set when management asks to return to a specific tab (activating a
    // prompt-history row); consumed by the next attach.
    let mut pending_tab: Option<uze_terminal::TabId> = None;
    loop {
        let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        match orchestrator::attach_workspace(
            &mut terminal,
            &root,
            &mut sidebar_width,
            &home,
            pending_tab.take(),
        )? {
            orchestrator::WorkspaceExit::Quit => return Ok(()),
            orchestrator::WorkspaceExit::Management => {
                match management::run_management(&mut terminal, home.clone(), &mut sidebar_width)? {
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

/// The inactive nav label color (`#9a9892` in the design) — close to but
/// distinct from the other muted tones used elsewhere, so it's its own
/// constant rather than reusing `MUTED`/`TEXT_SECONDARY`.
const NAV_INACTIVE: Color = Color::Rgb(154, 152, 146);

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
        .bg(ACCENT)
        .fg(BASE)
        .add_modifier(Modifier::BOLD);
    let ghost = Style::default().bg(SURFACE_SUBTLE).fg(NAV_INACTIVE);
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
fn hint_spans(hint: &str) -> Vec<Span<'static>> {
    let command = Style::default().fg(ACCENT).add_modifier(Modifier::BOLD);
    let muted = Style::default().fg(MUTED);
    let mut spans = Vec::new();
    for (i, chunk) in hint.split(" · ").enumerate() {
        if i > 0 {
            spans.push(Span::raw(" · "));
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

/// Every content screen's outer inset — the design's `padding: 36px 44px`
/// on each route's root div, translated to terminal cells. No border, no
/// background: content just sits indented on the shared backdrop.
pub(crate) fn content_area(area: Rect) -> Rect {
    Rect::new(
        area.x + 2,
        area.y + 1,
        area.width.saturating_sub(4),
        area.height.saturating_sub(1),
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
        .fg(TEXT_BRIGHT)
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
            Paragraph::new(Span::styled(subtitle, Style::default().fg(MUTED))),
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
        "native" => Style::default().fg(SUCCESS),
        "adapted" | "decomposed" => Style::default().fg(ACCENT),
        "degraded" => Style::default().fg(WARNING),
        _ => Style::default().fg(DANGER),
    }
}

#[cfg(test)]
mod tests;
