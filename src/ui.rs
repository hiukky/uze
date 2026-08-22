//! Terminal presentation over [`UzeApplication`] — product surface, not a
//! debug console.
//!
//! This module owns only navigation/selection/overlay state and input
//! transitions. Every product operation runs in a short-lived worker against
//! a fresh application facade, so the terminal never reads Store, vendor
//! files, integrations, or `marketplace.json` directly — it calls
//! `UzeApplication` read models exactly like the CLI does, and renders what
//! comes back.

use std::{
    io::{self, Stdout},
    path::PathBuf,
    sync::mpsc::{self},
    time::Duration,
};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Padding, Paragraph, Wrap},
};

use crate::{
    Result, UzeApplication, UzeHome,
    application::{MarketplacePluginSummary, PluginSummary},
    provisioning::{ProcessOutput, ProcessResult, ProcessRunner, ProcessSpec, SystemProcessRunner},
};

mod hit;
mod input;
mod model;
mod overlay;
pub mod view;
mod worker;

use hit::Hit;
use model::{Focus, Overlay, ROUTES, Route, TuiModel};
use worker::{Intent, dispatch, drain_worker_results, spawn_startup};

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

// Compact, low-chrome, deliberately desaturated palette. Full-saturation
// ANSI Cyan/Green/Yellow/Red read as a generic "AI product" look and fight
// whatever theme the terminal is already set to; these are toned down —
// closer to the terminal's own default foreground with a hint of hue —
// so real lifecycle states stay legible without shouting over the theme.
const ACCENT: Color = Color::Rgb(135, 155, 175);
/// ANSI `DarkGray` (bright-black, palette index 8) reads as near-invisible
/// against a true-black terminal background in many color schemes — this
/// is a genuine mid-gray instead, so secondary text stays legible without
/// competing with primary text.
const MUTED: Color = Color::Rgb(150, 150, 160);
const SUCCESS: Color = Color::Rgb(140, 170, 145);
const WARNING: Color = Color::Rgb(195, 165, 115);
const DANGER: Color = Color::Rgb(190, 115, 110);
/// Panel/box borders — same contrast reasoning as `MUTED`, one shade
/// dimmer so borders read as structure, not as another text color.
const PANEL: Color = Color::Rgb(110, 110, 120);
/// The selected-row highlight background across list panels — a neutral
/// dark slate rather than a saturated navy, so it reads as "this row is
/// highlighted" without asserting a brand hue.
const SELECTED_BG: Color = Color::Rgb(48, 50, 58);

/// Single-glyph icons used throughout the TUI. Plain Unicode/emoji, not
/// Nerd Font glyphs — those need a patched font the user's terminal may not
/// have, and a missing glyph renders as a box, which is worse than no icon.
mod icon {
    pub(crate) const OVERVIEW: &str = "🏠";
    pub(crate) const MARKETPLACE: &str = "🏪";
    pub(crate) const PLUGINS: &str = "🧩";
    pub(crate) const CONTEXT: &str = "📄";
    pub(crate) const HARNESSES: &str = "🔌";
    pub(crate) const DOCTOR: &str = "🩺";
    pub(crate) const SEARCH: &str = "🔍";
    pub(crate) const MARKETPLACE_GROUP: &str = "🗄";
    pub(crate) const SKILLS: &str = "👥";
    pub(crate) const AGENTS: &str = "🤖";
    pub(crate) const MCP: &str = "🔗";
    pub(crate) const CHECK: &str = "✓";
    pub(crate) const EXTERNAL_LINK: &str = "↗";
    pub(crate) const BRANCH: &str = "⑂";
}

const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

fn is_protected_plugin(
    plugin: &PluginSummary,
    marketplace_plugins: &[MarketplacePluginSummary],
) -> bool {
    // Only the verified official origin is protected: any `embedded:` provenance
    // whose id is listed in the compiled marketplace snapshot. A local or git
    // package that spoofs the name `uze` without `embedded:` provenance is not
    // considered official and remains removable.
    if !plugin.source.starts_with("embedded:") {
        return false;
    }
    if uze_application::bootstrap::DEFAULT_PLUGIN_IDS.contains(&plugin.id.as_str()) {
        return true;
    }
    if marketplace_plugins
        .iter()
        .any(|m| m.marketplace == "uze-official" && m.name == plugin.id)
    {
        return true;
    }
    // Fallback when marketplace hasn't loaded yet (startup): consult the
    // compiled snapshot directly. Keeps protection deterministic.
    if let Ok((_, entries)) = uze_application::bootstrap::entries() {
        return entries.iter().any(|entry| entry.name == plugin.id);
    }
    false
}

/// Runs the TUI. `home` is passed to workers, which construct the same
/// production application composition root as the CLI.
pub fn run(home: UzeHome) -> Result<()> {
    let mut terminal = TerminalSession::start()?;
    let (sender, receiver) = mpsc::channel();
    let mut model = TuiModel::default();
    spawn_startup(home.clone(), sender.clone(), model.context_root.clone());
    loop {
        terminal.draw(&mut model)?;
        drain_worker_results(&mut model, &receiver);
        if event::poll(POLL_INTERVAL).map_err(io_error)? {
            match event::read().map_err(io_error)? {
                Event::Key(key) => {
                    let intent = model.apply_key(key);
                    if intent == Intent::Quit {
                        return Ok(());
                    }
                    dispatch(intent, &home, &sender, &mut model);
                }
                Event::Mouse(mouse) => {
                    let intent = model.apply_mouse(mouse);
                    dispatch(intent, &home, &sender, &mut model);
                }
                Event::Resize(..) => {}
                _ => {}
            }
        }
    }
}

// --- Terminal lifecycle ------------------------------------------------------

struct TerminalSession {
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
            EnableMouseCapture
        ) {
            let _ = disable_raw_mode();
            return Err(io_error(error));
        }
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend).map_err(io_error)?;
        Ok(Self { terminal })
    }

    fn draw(&mut self, model: &mut TuiModel) -> Result<()> {
        model.tick = model.tick.wrapping_add(1);
        let mut hits = Vec::new();
        self.terminal
            .draw(|frame| render(frame, model, &mut hits))
            .map(|_| ())
            .map_err(io_error)?;
        model.hits = hits;
        Ok(())
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture,
            crossterm::cursor::Show
        );
        let _ = self.terminal.show_cursor();
    }
}

fn io_error(source: io::Error) -> crate::UzeError {
    crate::UzeError::Write {
        path: PathBuf::from("terminal"),
        source,
    }
}

// --- Rendering ----------------------------------------------------------

fn render(frame: &mut ratatui::Frame<'_>, model: &TuiModel, hits: &mut Vec<(Rect, Hit)>) {
    // A margin around the whole app keeps every panel — sidebar included —
    // off the raw terminal edge, instead of every border sitting flush
    // against column/row zero.
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(3),
            Constraint::Length(2),
        ])
        .split(frame.area());
    render_titlebar(frame, rows[0], model);

    let narrow = rows[1].width < 90;
    let sidebar_width = if rows[1].width < 60 {
        16
    } else if narrow {
        18
    } else {
        26
    };
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(sidebar_width), Constraint::Min(10)])
        .split(rows[1]);
    render_sidebar(frame, columns[0], model, narrow, hits);

    match model.route {
        Route::Overview => view::overview::render_overview(frame, columns[1], model),
        Route::Plugins => view::plugins::render_plugins(frame, columns[1], model, hits),
        Route::Marketplace => view::marketplace::render_marketplace(frame, columns[1], model, hits),
        Route::Context => view::context::render_context(frame, columns[1], model),
        Route::Harnesses => view::harnesses::render_harnesses(frame, columns[1], model, hits),
        Route::Doctor => view::doctor::render_doctor(frame, columns[1], model),
    }

    render_footer(frame, rows[2], model);

    match &model.overlay {
        Overlay::None => {}
        Overlay::Help => overlay::render_help(frame, frame.area()),
        Overlay::ConfirmRemove { id, focus } => {
            overlay::render_confirm_remove(frame, frame.area(), id, *focus)
        }
        Overlay::ConfirmUpdate(id) => overlay::render_confirm_update(frame, frame.area(), id),
        Overlay::ConfirmInstall { name, marketplace } => {
            overlay::render_confirm_install(frame, frame.area(), name, marketplace)
        }
        Overlay::ConfirmContextApply => overlay::render_confirm_context_apply(frame, frame.area()),
        Overlay::ProtectedPlugin(id) => overlay::render_protected_plugin(frame, frame.area(), id),
        Overlay::AddMarketplace(input) => {
            overlay::render_add_marketplace(frame, frame.area(), input)
        }
        Overlay::TrustRequired { plugin, detail, .. } => {
            overlay::render_trust_required(frame, frame.area(), plugin, detail)
        }
    }
}

fn render_titlebar(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(PANEL));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let issues = model.issues().len();
    let mut left = vec![
        Span::styled(
            " UZE",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
    ];
    let status_color = if model.doctor.is_none() {
        let frame = SPINNER_FRAMES[model.tick % SPINNER_FRAMES.len()];
        left.push(Span::styled(
            format!("{frame} "),
            Style::default().fg(MUTED),
        ));
        left.push(Span::styled("checking…", Style::default().fg(MUTED)));
        None
    } else if issues == 0 {
        left.push(Span::styled("healthy", Style::default().fg(SUCCESS)));
        Some(SUCCESS)
    } else {
        left.push(Span::styled(
            format!("{issues} issue(s)"),
            Style::default().fg(WARNING),
        ));
        Some(WARNING)
    };
    if let Some(color) = status_color {
        left.push(Span::raw("  "));
        left.push(Span::styled("●", Style::default().fg(color)));
    }

    // Path and branch sit together as one right-aligned group — not path
    // centered with branch off on its own further right — so the titlebar
    // reads as two blocks (status on the left, project identity on the
    // right) instead of three scattered pieces.
    let mut right = vec![Span::styled(
        display_project_path(&model.context_root),
        Style::default().fg(ACCENT),
    )];
    if let Some(branch) = git_branch(&model.context_root) {
        right.push(Span::raw("  "));
        right.push(Span::styled(
            format!("{} {branch}", icon::BRANCH),
            Style::default().fg(MUTED),
        ));
    }
    // Version deliberately lives in exactly one place — the global footer,
    // next to the keybinding hints — so it isn't repeated here and in the
    // sidebar too.

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(10), Constraint::Percentage(45)])
        .split(inner);
    frame.render_widget(Paragraph::new(Line::from(left)), columns[0]);
    frame.render_widget(
        Paragraph::new(Line::from(right)).alignment(ratatui::layout::Alignment::Right),
        columns[1],
    );
}

/// `~/relative/path` when `root` is under the user's home directory, else
/// the path as-is — mirrors what a shell prompt usually shows.
fn display_project_path(root: &std::path::Path) -> String {
    if let Some(home) = std::env::var_os("HOME")
        && let Ok(relative) = root.strip_prefix(&home)
    {
        return format!("~/{}", relative.display());
    }
    root.display().to_string()
}

/// Best-effort current branch name, read directly from `.git/HEAD` — no
/// `git` subprocess, so this stays as cheap as every other read-only TUI
/// refresh. `None` for anything not a plain git checkout (no repo, a
/// worktree's `.git` file, a detached-but-unreadable state): silently
/// omitted from the title bar rather than shown as an error.
fn git_branch(project_root: &std::path::Path) -> Option<String> {
    let head = std::fs::read_to_string(project_root.join(".git/HEAD")).ok()?;
    let head = head.trim();
    head.strip_prefix("ref: refs/heads/")
        .map(str::to_owned)
        .or_else(|| (head.len() >= 7).then(|| head[..7].to_owned()))
}

fn route_icon(route: Route) -> &'static str {
    match route {
        Route::Overview => icon::OVERVIEW,
        Route::Marketplace => icon::MARKETPLACE,
        Route::Plugins => icon::PLUGINS,
        Route::Context => icon::CONTEXT,
        Route::Harnesses => icon::HARNESSES,
        Route::Doctor => icon::DOCTOR,
    }
}

fn route_subtitle(route: Route) -> &'static str {
    match route {
        Route::Overview => "status & health",
        Route::Marketplace => "browse & install",
        Route::Plugins => "installed plugins",
        Route::Context => "AGENTS.md bridges",
        Route::Harnesses => "detected agents",
        Route::Doctor => "diagnostics",
    }
}

fn render_sidebar(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    narrow: bool,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(PANEL))
        .padding(Padding::new(1, 1, 1, 0));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut y = inner.y;
    let bottom = inner.y + inner.height;
    let mut row = |height: u16| -> Option<Rect> {
        if y + height > bottom {
            return None;
        }
        let rect = Rect::new(inner.x, y, inner.width, height);
        y += height;
        Some(rect)
    };

    if let Some(rect) = row(1) {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "NAVIGATION",
                Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
            )),
            rect,
        );
    }
    row(1); // spacer
    for route in ROUTES {
        // The active route always carries the highlight bar — matches a
        // tab/nav convention where "what's open" stays visible regardless
        // of where keyboard focus currently is; bold is reserved for
        // "the sidebar itself has focus right now, arrow keys move here".
        let selected = route == model.route;
        let bold = selected && model.focus == Focus::Sidebar;

        if narrow {
            // Icon-only single row — no room for a subtitle line here.
            let Some(rect) = row(1) else { break };
            let fg = if selected { ACCENT } else { MUTED };
            let mut style = Style::default().fg(fg);
            if selected {
                style = style.bg(SELECTED_BG);
            }
            if bold {
                style = style.add_modifier(Modifier::BOLD);
            }
            let padded = pad_line(route_icon(route), rect.width as usize, style);
            frame.render_widget(Paragraph::new(padded), rect);
            hits.push((rect, Hit::Route(route)));
            continue;
        }

        // Taller, two-line rows — icon+label, then a muted subtitle — with
        // the highlight bar (when selected) covering both, plus a blank
        // spacer row after so items read as distinct blocks rather than a
        // cramped stack.
        let Some(label_rect) = row(1) else { break };
        let subtitle_rect = row(1);
        row(1); // spacer between items

        let label_fg = if selected { Color::White } else { Color::Gray };
        let mut label_style = Style::default().fg(label_fg);
        if selected {
            label_style = label_style.bg(SELECTED_BG);
        }
        if bold {
            label_style = label_style.add_modifier(Modifier::BOLD);
        }
        let label_text = format!("{} {}", route_icon(route), route.label());
        frame.render_widget(
            Paragraph::new(pad_line(
                &label_text,
                label_rect.width as usize,
                label_style,
            )),
            label_rect,
        );
        hits.push((label_rect, Hit::Route(route)));

        if let Some(subtitle_rect) = subtitle_rect {
            let mut subtitle_style = Style::default().fg(MUTED);
            if selected {
                subtitle_style = subtitle_style.bg(SELECTED_BG);
            }
            let subtitle_text = format!("   {}", route_subtitle(route));
            frame.render_widget(
                Paragraph::new(pad_line(
                    &subtitle_text,
                    subtitle_rect.width as usize,
                    subtitle_style,
                )),
                subtitle_rect,
            );
            hits.push((subtitle_rect, Hit::Route(route)));
        }
    }

    // Quick Actions and Status were both dropped: Quick Actions only
    // repeated the shortcuts the global footer already shows on every
    // screen, and Status duplicated the Marketplace count already visible
    // in that panel's own title plus a Doctor summary nobody used from
    // here — an actual problem belongs in the titlebar's health indicator
    // (top-left "healthy" / "N issue(s)"), not a second count buried in
    // the sidebar.

    // Footer pinned to the very bottom, only when there's still room —
    // never overlaps the sections above on a short terminal. No version
    // here either — see `render_titlebar`'s note, it lives in the global
    // footer only.
    if bottom > y {
        let footer_rect = Rect::new(inner.x, bottom - 1, inner.width, 1);
        frame.render_widget(
            Paragraph::new(Span::styled("UZE", Style::default().fg(MUTED))),
            footer_rect,
        );
    }
}

/// Pads `text` with trailing spaces to `width` columns under one `style` —
/// used to paint a selection highlight across a whole row's background,
/// not just behind the characters it happens to contain.
fn pad_line(text: &str, width: usize, style: Style) -> Line<'static> {
    let used = ratatui::text::Span::raw(text).width();
    let padding = " ".repeat(width.saturating_sub(used));
    Line::from(Span::styled(format!("{text}{padding}"), style))
}

// --- Shared helpers ---------------------------------------------------------

fn render_footer(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(PANEL));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(10),
            Constraint::Length(version.len() as u16),
        ])
        .split(inner);
    frame.render_widget(
        Paragraph::new(footer(model)).wrap(Wrap { trim: true }),
        columns[0],
    );
    frame.render_widget(
        Paragraph::new(Span::styled(version, Style::default().fg(MUTED)))
            .alignment(ratatui::layout::Alignment::Right),
        columns[1],
    );
}

fn footer(model: &TuiModel) -> Text<'static> {
    let hint = if model.filtering {
        "type to filter · enter apply · esc clear"
    } else {
        match model.overlay {
            Overlay::None => match model.focus {
                Focus::Sidebar => "↑↓/jk select route · enter/tab open · ? help · q quit",
                _ => route_hint(model.route),
            },
            Overlay::ConfirmRemove { .. } => "tab switch · enter confirm · esc cancel · y/n",
            Overlay::ProtectedPlugin(_) => "esc/enter to dismiss",
            Overlay::AddMarketplace(_) => "type path/URL · enter add · esc cancel",
            _ => "enter/y confirm · esc/n cancel",
        }
    };
    match &model.status {
        model::Status::Idle => {
            Text::from(Line::from(Span::styled(hint, Style::default().fg(MUTED))))
        }
        model::Status::Working(value) => {
            let frame = SPINNER_FRAMES[model.tick % SPINNER_FRAMES.len()];
            Text::from(vec![
                Line::from(vec![
                    Span::styled(
                        format!("{frame} "),
                        Style::default().fg(WARNING).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        value.clone(),
                        Style::default().fg(WARNING).add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(Span::styled(hint, Style::default().fg(MUTED))),
            ])
        }
        model::Status::Success(value) => Text::from(vec![
            Line::from(Span::styled(
                value.clone(),
                Style::default().fg(SUCCESS).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(hint, Style::default().fg(MUTED))),
        ]),
        model::Status::Error(value) => Text::from(vec![
            Line::from(Span::styled(
                value.clone(),
                Style::default().fg(DANGER).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(hint, Style::default().fg(MUTED))),
        ]),
    }
}

fn route_hint(route: Route) -> &'static str {
    match route {
        Route::Overview => "tab sidebar · r/g refresh · ? help · q quit",
        Route::Plugins => {
            "↑↓/jk select · enter inspect · u update · r remove · tab sidebar · ? help"
        }
        Route::Marketplace => {
            "↑↓ select · enter inspect · i install · a add marketplace · / search · esc back"
        }
        Route::Context => "a analyze · p apply · tab sidebar · ? help",
        Route::Harnesses => "↑↓/jk select · s setup · tab sidebar · ? help",
        Route::Doctor => "tab sidebar · r/g refresh · ? help",
    }
}

fn panel_block(title: impl Into<Line<'static>>) -> Block<'static> {
    Block::default()
        .title(title)
        .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(PANEL))
        .padding(Padding::new(2, 2, 1, 0))
}

fn health_style(health: &str) -> Style {
    match health {
        "ready" => Style::default().fg(SUCCESS),
        "missing" | "unknown" => Style::default().fg(WARNING),
        _ => Style::default().fg(DANGER),
    }
}

fn setup_style(status: &str) -> Style {
    if status.contains("verified") && !status.contains("unverified") {
        Style::default().fg(SUCCESS)
    } else if status.contains("unverified") {
        Style::default().fg(WARNING)
    } else {
        Style::default().fg(MUTED)
    }
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
mod tests {
    use std::path::PathBuf;

    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use ratatui::layout::Rect;

    use crate::application::{DoctorReport, MarketplacePluginSummary, PluginSummary};

    use super::hit::Hit;
    use super::model::{Focus, Overlay, ROUTES, Route, TrustedRetry, TuiModel};
    use super::render;
    use super::view::doctor::{Severity, classify_doctor};
    use super::worker::{Intent, TrustGrant};

    fn plugin(id: &str) -> PluginSummary {
        PluginSummary {
            id: id.to_owned(),
            source: "embedded:example".to_owned(),
            store_path: PathBuf::from("/store/example"),
            capability_count: 2,
            update_available: None,
        }
    }

    fn model_with_plugins(ids: &[&str]) -> TuiModel {
        TuiModel {
            plugins: ids.iter().map(|id| plugin(id)).collect(),
            focus: Focus::Content,
            route: Route::Plugins,
            ..TuiModel::default()
        }
    }

    /// A model with every route's list populated (plugins, marketplace,
    /// harnesses) and a mixed-severity doctor report, so rendering each
    /// route exercises its non-empty branch rather than only the
    /// nothing-loaded-yet placeholder every other test leaves in place.
    fn model_with_data() -> TuiModel {
        use crate::application::{
            HarnessHealth, ManagedStateSummary, PackageManagedState, StoreHealth,
        };
        use uze_core::integration::{HarnessDetection, PublicationStatus};
        use uze_core::router::HarnessCapabilities;

        let mut model = model_with_plugins(&["one", "two"]);
        model.plugins[0].update_available = Some(true);
        model.marketplace_count = 1;
        model.marketplace_plugins = vec![MarketplacePluginSummary {
            marketplace: "uze-official".to_owned(),
            name: "flow".to_owned(),
            description: Some("A flow plugin".to_owned()),
            keywords: vec!["flow".to_owned()],
            installed: true,
            update_available: Some(false),
            is_default: true,
        }];
        model.doctor = Some(DoctorReport {
            uze_home: PathBuf::from("/home/uze"),
            store: StoreHealth::Ready,
            plugins: model.plugins.clone(),
            harnesses: vec![
                HarnessHealth {
                    integration: "claude".to_owned(),
                    detection: HarnessDetection {
                        present: true,
                        version: Some("1.0.0".to_owned()),
                    },
                    setup: "configured, verified".to_owned(),
                    strategy: Some("native-user-scope-skills".to_owned()),
                    provisioning: None,
                    publication: PublicationStatus::Published,
                    capabilities: HarnessCapabilities::default(),
                    native_instructions: true,
                },
                HarnessHealth {
                    integration: "codex".to_owned(),
                    detection: HarnessDetection::default(),
                    setup: "not configured".to_owned(),
                    strategy: None,
                    provisioning: None,
                    publication: PublicationStatus::NotApplicable,
                    capabilities: HarnessCapabilities::default(),
                    native_instructions: false,
                },
            ],
            attachments: vec![PackageManagedState {
                plugin: "one".to_owned(),
                state: ManagedStateSummary {
                    matched: 1,
                    missing: 1,
                    drifted: 1,
                    conflicts: 1,
                    blocked: 0,
                    ledger_error: None,
                },
            }],
            ledger_error: None,
            integration_state_error: None,
            provisioning_state_error: None,
        });
        model.harnesses_selected = 0;
        model
    }

    #[test]
    #[ignore]
    fn debug_dump_marketplace_header_pills() {
        use ratatui::{Terminal, backend::TestBackend};
        let mut terminal = Terminal::new(TestBackend::new(180, 20)).unwrap();
        let mut model = model_with_data();
        model.route = Route::Marketplace;
        model.marketplace_count = 1;
        model.marketplace_plugins = vec![MarketplacePluginSummary {
            marketplace: "uze-official".to_owned(),
            name: "uze".to_owned(),
            description: None,
            keywords: vec![],
            installed: true,
            update_available: Some(false),
            is_default: true,
        }];
        let mut hits = Vec::new();
        terminal
            .draw(|frame| render(frame, &model, &mut hits))
            .unwrap();
        let buffer = terminal.backend().buffer();
        for y in 0..20 {
            let row_text: String = (0..90).map(|x| buffer[(x, y)].symbol()).collect();
            if row_text.contains("uze-official") {
                for x in 28..75 {
                    let cell = &buffer[(x, y)];
                    eprintln!(
                        "x={x:2} sym={:?} bg={:?} fg={:?}",
                        cell.symbol(),
                        cell.bg,
                        cell.fg
                    );
                }
            }
        }
    }

    #[test]
    fn every_route_renders_without_panicking() {
        use ratatui::{Terminal, backend::TestBackend};

        let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
        let base = model_with_data();
        for route in ROUTES {
            let model = TuiModel {
                route,
                plugins: base.plugins.clone(),
                plugins_selected: base.plugins_selected,
                marketplace_count: base.marketplace_count,
                marketplace_plugins: base.marketplace_plugins.clone(),
                doctor: base.doctor.clone(),
                harnesses_selected: base.harnesses_selected,
                focus: Focus::Content,
                ..TuiModel::default()
            };
            let mut hits = Vec::new();
            terminal
                .draw(|frame| render(frame, &model, &mut hits))
                .unwrap();
        }
    }

    #[test]
    fn every_overlay_renders_without_panicking() {
        use ratatui::{Terminal, backend::TestBackend};

        let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
        let base = model_with_data();
        let overlays = [
            Overlay::Help,
            Overlay::ConfirmRemove {
                id: "one".to_owned(),
                focus: 1,
            },
            Overlay::ConfirmUpdate("one".to_owned()),
            Overlay::ConfirmInstall {
                name: "flow".to_owned(),
                marketplace: "uze-official".to_owned(),
            },
            Overlay::ConfirmContextApply,
            Overlay::ProtectedPlugin("one".to_owned()),
            Overlay::AddMarketplace("/home/user/marketplace".to_owned()),
            Overlay::TrustRequired {
                plugin: "one".to_owned(),
                detail: "one -> mcp-server".to_owned(),
                retry: TrustedRetry::Install {
                    name: "one".to_owned(),
                    marketplace: "uze-official".to_owned(),
                },
            },
        ];
        for overlay in overlays {
            let model = TuiModel {
                overlay,
                plugins: base.plugins.clone(),
                marketplace_plugins: base.marketplace_plugins.clone(),
                doctor: base.doctor.clone(),
                focus: Focus::Overlay,
                ..TuiModel::default()
            };
            let mut hits = Vec::new();
            terminal
                .draw(|frame| render(frame, &model, &mut hits))
                .unwrap();
        }
    }

    #[test]
    fn sidebar_keyboard_navigation_cycles_routes() {
        let mut model = TuiModel {
            focus: Focus::Sidebar,
            ..TuiModel::default()
        };
        assert_eq!(model.route, Route::Overview);
        model.apply_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(model.route, Route::Marketplace);
        model.apply_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
        assert_eq!(model.route, Route::Plugins);
        model.apply_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(model.route, Route::Marketplace);
    }

    #[test]
    fn tab_toggles_focus_between_sidebar_and_content() {
        let mut model = TuiModel::default();
        assert_eq!(model.focus, Focus::Sidebar);
        model.apply_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(model.focus, Focus::Content);
        model.apply_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(model.focus, Focus::Sidebar);
    }

    #[test]
    fn content_navigation_and_inspect_intent() {
        let mut model = model_with_plugins(&["one", "two"]);
        model.apply_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(model.plugins_selected, 1);
        assert_eq!(
            model.apply_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Intent::InspectPlugin("two".to_owned())
        );
    }

    #[test]
    fn remove_confirmation_flow() {
        let mut model = model_with_plugins(&["one"]);
        model.apply_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
        assert!(matches!(model.overlay, Overlay::ConfirmRemove { ref id, .. } if id == "one"));
        assert_eq!(model.focus, Focus::Overlay);
        let intent = model.apply_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        assert_eq!(intent, Intent::None);
        assert_eq!(model.overlay, Overlay::None);
        assert_eq!(model.focus, Focus::Content);
    }

    #[test]
    fn remove_confirmed_emits_remove_intent() {
        let mut model = model_with_plugins(&["one"]);
        model.apply_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
        let intent = model.apply_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(intent, Intent::Remove("one".to_owned()));
    }

    #[test]
    fn update_only_offered_when_available() {
        let mut model = model_with_plugins(&["one"]);
        model.apply_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE));
        assert_eq!(
            model.overlay,
            Overlay::None,
            "no update available, no overlay"
        );
        model.plugins[0].update_available = Some(true);
        model.apply_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::NONE));
        assert!(matches!(model.overlay, Overlay::ConfirmUpdate(ref id) if id == "one"));
    }

    #[test]
    fn trust_required_overlay_confirm_regrants_with_trust() {
        let mut model = TuiModel {
            overlay: Overlay::TrustRequired {
                plugin: "acme".to_owned(),
                detail: "acme -> mcp-server".to_owned(),
                retry: TrustedRetry::Install {
                    name: "acme".to_owned(),
                    marketplace: "uze-official".to_owned(),
                },
            },
            focus: Focus::Overlay,
            ..TuiModel::default()
        };
        let intent = model.apply_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
        assert_eq!(
            intent,
            Intent::Install {
                name: "acme".to_owned(),
                marketplace: "uze-official".to_owned(),
                grant: TrustGrant::Granted,
            }
        );
        assert_eq!(model.overlay, Overlay::None);
    }

    #[test]
    fn mouse_click_on_sidebar_route_switches_route_and_focus() {
        let mut model = TuiModel {
            hits: vec![(Rect::new(0, 1, 20, 1), Hit::Route(Route::Marketplace))],
            ..TuiModel::default()
        };
        let intent = model.apply_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: 1,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(intent, Intent::None);
        assert_eq!(model.route, Route::Marketplace);
        assert_eq!(model.focus, Focus::Content);
    }

    #[test]
    fn mouse_click_on_plugin_row_selects_and_inspects() {
        let mut model = model_with_plugins(&["one", "two"]);
        model.hits = vec![
            (Rect::new(0, 0, 20, 1), Hit::PluginRow(0)),
            (Rect::new(0, 1, 20, 1), Hit::PluginRow(1)),
        ];
        let intent = model.apply_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 2,
            row: 1,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(model.plugins_selected, 1);
        assert_eq!(intent, Intent::InspectPlugin("two".to_owned()));
    }

    #[test]
    fn scroll_moves_selection_without_mutating_anything() {
        let mut model = model_with_plugins(&["one", "two", "three"]);
        let intent = model.apply_mouse(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(intent, Intent::None);
        assert_eq!(model.plugins_selected, 1);
    }

    #[test]
    fn click_outside_overlay_dismisses_without_confirming() {
        let mut model = model_with_plugins(&["one"]);
        model.overlay = Overlay::ConfirmRemove {
            id: "one".to_owned(),
            focus: 1,
        };
        model.focus = Focus::Overlay;
        let intent = model.apply_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(
            intent,
            Intent::None,
            "a stray click must never confirm a destructive action"
        );
        assert_eq!(model.overlay, Overlay::None);
    }

    #[test]
    fn help_overlay_toggle_and_dismiss() {
        let mut model = TuiModel::default();
        model.apply_key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE));
        assert_eq!(model.overlay, Overlay::Help);
        model.apply_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(model.overlay, Overlay::None);
    }

    #[test]
    fn empty_marketplace_and_no_harness_states_do_not_panic_rendering() {
        let model = TuiModel {
            route: Route::Marketplace,
            ..TuiModel::default()
        };
        assert_eq!(model.list_len(), 0);
        assert!(model.selected_marketplace_plugin().is_none());
        let model = TuiModel {
            route: Route::Harnesses,
            ..TuiModel::default()
        };
        assert!(model.selected_harness().is_none());
    }

    #[test]
    fn read_only_navigation_never_produces_a_mutating_intent() {
        let mut model = model_with_plugins(&["one", "two"]);
        model.set_route(Route::Marketplace);
        model.marketplace_plugins = vec![MarketplacePluginSummary {
            marketplace: "uze-official".to_owned(),
            name: "uze".to_owned(),
            description: None,
            keywords: Vec::new(),
            installed: true,
            update_available: Some(false),
            is_default: true,
        }];
        for key in [
            KeyCode::Down,
            KeyCode::Up,
            KeyCode::Char('j'),
            KeyCode::Char('k'),
        ] {
            let intent = model.apply_key(KeyEvent::new(key, KeyModifiers::NONE));
            assert_eq!(intent, Intent::None);
        }
    }

    #[test]
    fn doctor_classifies_conflicts_as_high_and_missing_as_low() {
        use crate::application::{ManagedStateSummary, PackageManagedState};
        let doctor = DoctorReport {
            uze_home: PathBuf::from("/home"),
            store: crate::application::StoreHealth::Ready,
            plugins: Vec::new(),
            harnesses: Vec::new(),
            attachments: vec![PackageManagedState {
                plugin: "acme".to_owned(),
                state: ManagedStateSummary {
                    matched: 0,
                    missing: 1,
                    drifted: 0,
                    conflicts: 1,
                    blocked: 0,
                    ledger_error: None,
                },
            }],
            ledger_error: None,
            integration_state_error: None,
            provisioning_state_error: None,
        };
        let issues = classify_doctor(Some(&doctor));
        assert_eq!(issues[0].severity, Severity::High);
        assert!(issues.iter().any(|i| i.severity == Severity::Low));
    }

    fn marketplace_plugin(
        marketplace: &str,
        name: &str,
        installed: bool,
    ) -> MarketplacePluginSummary {
        MarketplacePluginSummary {
            marketplace: marketplace.to_owned(),
            name: name.to_owned(),
            description: None,
            keywords: Vec::new(),
            installed,
            update_available: None,
            is_default: false,
        }
    }

    #[test]
    fn marketplace_filter_narrows_visible_selection() {
        let mut model = TuiModel {
            route: Route::Marketplace,
            focus: Focus::Content,
            marketplace_plugins: vec![
                marketplace_plugin("ai", "std", false),
                marketplace_plugin("ai", "flow", true),
            ],
            ..TuiModel::default()
        };
        assert_eq!(model.marketplace_visible_indices(), vec![0, 1]);

        model.apply_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        assert!(model.filtering);
        for c in "flow".chars() {
            model.apply_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        assert_eq!(model.marketplace_visible_indices(), vec![1]);
        assert_eq!(model.selected_marketplace_plugin().unwrap().name, "flow");

        model.apply_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!model.filtering);
        assert!(model.marketplace_filter.is_empty());
        assert_eq!(model.marketplace_visible_indices(), vec![0, 1]);
    }

    #[test]
    fn marketplace_group_collapse_hides_its_plugins() {
        let mut model = TuiModel {
            route: Route::Marketplace,
            marketplace_plugins: vec![marketplace_plugin("ai", "std", false)],
            ..TuiModel::default()
        };
        assert_eq!(model.list_len(), 1);
        model.marketplace_toggle_group("ai");
        assert_eq!(model.list_len(), 0);
        assert!(model.selected_marketplace_plugin().is_none());
        model.marketplace_toggle_group("ai");
        assert_eq!(model.list_len(), 1);
    }

    #[test]
    fn add_marketplace_overlay_types_and_submits() {
        let mut model = TuiModel {
            focus: Focus::Content,
            ..TuiModel::default()
        };
        let intent = model.apply_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert_eq!(intent, Intent::None);
        assert!(matches!(model.overlay, Overlay::AddMarketplace(ref s) if s.is_empty()));

        for c in "/tmp/mp".chars() {
            model.apply_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        }
        assert!(matches!(model.overlay, Overlay::AddMarketplace(ref s) if s == "/tmp/mp"));

        let intent = model.apply_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(intent, Intent::AddMarketplace("/tmp/mp".to_owned()));
        assert_eq!(model.overlay, Overlay::None);
    }

    #[test]
    fn add_marketplace_overlay_esc_cancels_without_intent() {
        let mut model = TuiModel {
            overlay: Overlay::AddMarketplace("abc".to_owned()),
            focus: Focus::Overlay,
            ..TuiModel::default()
        };
        let intent = model.apply_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(intent, Intent::None);
        assert_eq!(model.overlay, Overlay::None);
    }

    #[test]
    fn r_refreshes_outside_plugins_but_still_removes_within_plugins() {
        let mut model = TuiModel {
            focus: Focus::Content,
            route: Route::Overview,
            ..TuiModel::default()
        };
        let intent = model.apply_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
        assert_eq!(intent, Intent::Refresh);

        let mut plugins_model = model_with_plugins(&["one"]);
        let intent = plugins_model.apply_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
        assert!(
            matches!(plugins_model.overlay, Overlay::ConfirmRemove { ref id, .. } if id == "one")
        );
        assert_eq!(intent, Intent::None);
    }
}
