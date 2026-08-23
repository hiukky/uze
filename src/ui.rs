//! Terminal presentation over [`UzeApplication`] — product surface, not a
//! debug console.
//!
//! This module owns only navigation/selection/overlay state and input
//! transitions. Every product operation runs in a short-lived worker against
//! a fresh application facade, so the terminal never reads Store, vendor
//! files, integrations, or `agents.json` directly — it calls
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
    widgets::{Block, Borders, Padding, Paragraph, Wrap},
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
const BLUE: Color = Color::Rgb(125, 151, 201); // #7d97c9 — "Default" tag only

/// Hairline dividers — solid approximations of the design's
/// `rgba(255,255,255,a)` borders, pre-blended over `BASE` since ratatui has
/// no alpha compositing. `BORDER_FAINT` (a≈0.05) separates list rows;
/// `BORDER` (a≈0.08) sits under the titlebar and around the sidebar/inputs.
const BORDER_FAINT: Color = Color::Rgb(22, 24, 25);
const BORDER: Color = Color::Rgb(30, 31, 32);
/// The Marketplace/Harnesses selected-row tint — `rgba(143,209,158,0.09)`
/// (the accent itself, barely-there) pre-blended over `BASE`.
const SELECTED_BG: Color = Color::Rgb(22, 30, 26);

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
    // Edge to edge horizontally (no left/right inset — matches the design's
    // `width:100%`), but with one blank row top and bottom: on a browser
    // canvas `height:100vh` reads as flush; in a real terminal, text
    // sitting on row 0 with nothing above it reads as clipped/cramped, not
    // "inset like a window". One flat backdrop for the entire frame — no
    // panel ever paints its own background; every division is a hairline
    // border or padding, never a filled slab.
    frame.render_widget(
        Block::default().style(Style::default().bg(BASE).fg(TEXT_PRIMARY)),
        frame.area(),
    );
    let area = Rect::new(
        frame.area().x,
        frame.area().y + 1,
        frame.area().width,
        frame.area().height.saturating_sub(2),
    );
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(3),
            Constraint::Length(2),
        ])
        .split(area);
    render_titlebar(frame, rows[0], model, hits);

    let narrow = rows[1].width < 90;
    let sidebar_width = if rows[1].width < 60 {
        16
    } else if narrow {
        18
    } else {
        27
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
        Overlay::HarnessHelp => overlay::render_harness_help(frame, frame.area()),
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

fn render_titlebar(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(BORDER));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // The titlebar is compact identity chrome: name, then *only* the health
    // indicator — never the in-flight operation message ("Installing …",
    // "Added plugin …"), which lives in the footer where long text has its
    // own line. Keeping operation text out of the header is what stops a
    // long install root/path from wrapping into the project path and
    // breaking the two-column layout.
    let issues = model.issues().len();
    let mut left = vec![
        Span::styled(
            " UZE",
            Style::default()
                .fg(TEXT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled("│", Style::default().fg(BORDER)),
        Span::raw("  "),
    ];
    if model.doctor.is_none() {
        let frame = SPINNER_FRAMES[model.tick % SPINNER_FRAMES.len()];
        left.push(Span::styled(
            format!("{frame} "),
            Style::default().fg(MUTED),
        ));
        left.push(Span::styled("checking…", Style::default().fg(MUTED)));
    } else if issues == 0 {
        left.push(Span::styled("● ", Style::default().fg(SUCCESS)));
        left.push(Span::styled("healthy", Style::default().fg(SUCCESS)));
    } else {
        left.push(Span::styled("● ", Style::default().fg(WARNING)));
        left.push(Span::styled(
            format!("{issues} issue(s)"),
            Style::default().fg(WARNING),
        ));
    }
    // The health block is clickable — anywhere on it jumps to the Doctor
    // screen, which is where the full problem + evidence + solution list
    // lives. The hit rect covers the indicator only, so "UZE" and the hair-
    // line stay inert.
    let prefix_width: usize = left.iter().take(4).map(|s| s.width()).sum();
    let health_width: usize = left.iter().skip(4).map(|s| s.width()).sum();
    let health_rect = Rect::new(
        inner
            .x
            .saturating_add(prefix_width as u16)
            .min(inner.right()),
        inner.y,
        (health_width.min(inner.width as usize) as u16).max(1),
        1,
    );
    hits.push((health_rect, Hit::Route(Route::Doctor)));

    // Path and branch, plain muted text with a faint dot separator — the
    // design colors neither with the accent; this is identity chrome, not
    // a call to action.
    let mut right = vec![Span::styled(
        display_project_path(&model.context_root),
        Style::default().fg(MUTED),
    )];
    if let Some(branch) = git_branch(&model.context_root) {
        right.push(Span::raw("  "));
        right.push(Span::styled("·", Style::default().fg(TEXT_FAINT)));
        right.push(Span::raw("  "));
        right.push(Span::styled(branch, Style::default().fg(MUTED)));
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
        Paragraph::new(Line::from(right))
            .alignment(ratatui::layout::Alignment::Right)
            .block(Block::default().padding(Padding::new(0, 1, 0, 0))),
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

/// The inactive nav label color (`#9a9892` in the design) — close to but
/// distinct from the other muted tones used elsewhere, so it's its own
/// constant rather than reusing `MUTED`/`TEXT_SECONDARY`.
const NAV_INACTIVE: Color = Color::Rgb(154, 152, 146);

fn render_sidebar(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    narrow: bool,
    hits: &mut Vec<(Rect, Hit)>,
) {
    // No fill, just a hairline right border — the sidebar sits on the same
    // backdrop as everything else; only a thin divider marks the edge.
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(BORDER_FAINT))
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

    for route in ROUTES {
        // Selection reads as a left border accent, not a filled bar — the
        // design never gives the sidebar a background tint at all.
        let selected = route == model.route;
        let border = if selected {
            // A full box-drawing "│", not the thin eighth-block "▏" — the
            // latter renders inconsistently (a sliver, sometimes
            // misaligned) across terminal fonts; "│" is universally
            // supported and reads as a clean solid line.
            Span::styled("│", Style::default().fg(ACCENT))
        } else {
            Span::raw(" ")
        };

        if narrow {
            let Some(rect) = row(1) else { break };
            let fg = if selected { TEXT_BRIGHT } else { NAV_INACTIVE };
            let mut style = Style::default().fg(fg);
            if selected {
                style = style.add_modifier(Modifier::BOLD);
            }
            let line = Line::from(vec![border, Span::styled(route.label(), style)]);
            frame.render_widget(Paragraph::new(line), rect);
            hits.push((rect, Hit::Route(route)));
            continue;
        }

        let Some(label_rect) = row(1) else { break };
        let subtitle_rect = row(1);
        row(1); // breathing room between items

        let label_fg = if selected { TEXT_BRIGHT } else { NAV_INACTIVE };
        let mut label_style = Style::default().fg(label_fg);
        if selected {
            label_style = label_style.add_modifier(Modifier::BOLD);
        }
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                border,
                Span::raw(" "),
                Span::styled(route.label(), label_style),
            ])),
            label_rect,
        );
        hits.push((label_rect, Hit::Route(route)));

        if let Some(subtitle_rect) = subtitle_rect {
            let line = Line::from(vec![
                Span::raw("  "),
                Span::styled(route_subtitle(route), Style::default().fg(TEXT_DIM)),
            ]);
            frame.render_widget(Paragraph::new(line), subtitle_rect);
            hits.push((subtitle_rect, Hit::Route(route)));
        }
    }
}

// --- Shared helpers ---------------------------------------------------------

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

fn render_footer(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(BORDER_FAINT))
        .padding(Padding::new(1, 1, 0, 0));
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
    let mut text = footer(model);
    // Operation messages (install roots, marketplace paths) can exceed the
    // hint column; clip the status line to the column instead of letting it
    // wrap into a second row — the footer is exactly one row tall and the
    // second virtual line would be clipped mid-word, which is worse than an
    // ellipsis.
    if !matches!(model.status, model::Status::Idle)
        && let Some(line) = text.lines.first_mut()
    {
        clip_line(line, columns[0].width as usize);
    }
    frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), columns[0]);
    frame.render_widget(
        Paragraph::new(Span::styled(version, Style::default().fg(TEXT_DIM)))
            .alignment(ratatui::layout::Alignment::Right),
        columns[1],
    );
}

/// Truncates `line` in place to `max` columns, replacing whatever crosses
/// the limit with `…`. Spans are trimmed greedily left-to-right, so the
/// truncation point stays at the text that would have been visible anyway.
fn clip_line(line: &mut Line<'static>, max: usize) {
    let mut used = 0usize;
    let mut cut = None;
    for (i, span) in line.spans.iter().enumerate() {
        let width = span.width();
        if used + width <= max {
            used += width;
        } else {
            cut = Some(i);
            break;
        }
    }
    let Some(i) = cut else {
        return;
    };
    let keep = max.saturating_sub(used).saturating_sub(1); // room for "…"
    let mut truncated: String = line.spans[i].content.chars().take(keep).collect();
    truncated.push('…');
    line.spans[i].content = std::borrow::Cow::Owned(truncated);
    line.spans.truncate(i + 1);
}

fn footer(model: &TuiModel) -> Text<'static> {
    let hint = if model.filtering {
        "type to filter · enter apply · esc clear"
    } else {
        match model.overlay {
            Overlay::None => match model.focus {
                Focus::Sidebar => "↑↓/jk select route · enter/tab open · ? help · q quit",
                _ => route_hint(model),
            },
            Overlay::ConfirmRemove { .. } => "tab switch · enter confirm · esc cancel · y/n",
            Overlay::ProtectedPlugin(_) => "esc/enter to dismiss",
            Overlay::AddMarketplace(_) => "type path/URL · enter add · esc cancel",
            _ => "enter/y confirm · esc/n cancel",
        }
    };
    match &model.status {
        model::Status::Idle => Text::from(Line::from(hint_spans(hint))),
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
                Line::from(hint_spans(hint)),
            ])
        }
        model::Status::Success(value) => Text::from(vec![
            Line::from(Span::styled(
                value.clone(),
                Style::default().fg(SUCCESS).add_modifier(Modifier::BOLD),
            )),
            Line::from(hint_spans(hint)),
        ]),
        model::Status::Error(value) => Text::from(vec![
            Line::from(Span::styled(
                value.clone(),
                Style::default().fg(DANGER).add_modifier(Modifier::BOLD),
            )),
            Line::from(hint_spans(hint)),
        ]),
    }
}

fn route_hint(model: &TuiModel) -> &'static str {
    match model.route {
        Route::Overview => {
            if model.overview_install_path().is_some() {
                "i install · r refresh · ? help"
            } else {
                "r refresh · ? help"
            }
        }
        Route::Plugins => "↑↓ select · enter details · u update · r remove",
        Route::Marketplace => {
            "↑↓ select · enter inspect · i install · a add marketplace · / search · esc close"
        }
        Route::Context => "a analyze · p apply",
        Route::Harnesses => "↑↓ select · s setup · ? status · esc close",
        Route::Doctor => "r refresh · ? help",
    }
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
    let title_row = Rect::new(area.x, area.y, area.width, 1);
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

/// Draws one content row and, when there's room for it, a hairline
/// `BORDER_FAINT` divider directly beneath — the design's
/// `border-bottom: 1px solid rgba(255,255,255,0.05)` under every list row
/// (Overview activity, Plugins, Context, Doctor checks, compat rows).
/// Returns the y position immediately after the divider.
pub(crate) fn render_divided_row(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    y: u16,
    line: Line<'_>,
) -> u16 {
    if y >= area.y + area.height {
        return y;
    }
    frame.render_widget(Paragraph::new(line), Rect::new(area.x, y, area.width, 1));
    let divider_y = y + 1;
    if divider_y < area.y + area.height {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "─".repeat(area.width as usize),
                Style::default().fg(BORDER_FAINT),
            )),
            Rect::new(area.x, divider_y, area.width, 1),
        );
    }
    divider_y + 1
}

pub(crate) fn health_style(health: &str) -> Style {
    match health {
        "ready" => Style::default().fg(SUCCESS),
        "missing" | "unknown" => Style::default().fg(WARNING),
        _ => Style::default().fg(DANGER),
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
    use ratatui::{Terminal, backend::TestBackend, layout::Rect};

    use crate::UzeHome;
    use crate::application::{DoctorReport, MarketplacePluginSummary, PluginSummary};

    use super::hit::Hit;
    use super::model::{Focus, Overlay, ROUTES, RefreshData, Route, TrustedRetry, TuiModel};
    use super::render;
    use super::view::doctor::{Severity, classify_doctor};
    use super::worker::{Intent, TrustGrant};
    use super::{ACCENT, MUTED, clip_line, hint_spans};

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
                    display_name: "claude".to_owned(),
                    detection: HarnessDetection {
                        present: true,
                        version: Some("1.0.0".to_owned()),
                    },
                    setup: "configured, verified".to_owned(),
                    strategy: Some("managed-user-scope-skills-dir".to_owned()),
                    provisioning: None,
                    publication: PublicationStatus::Published,
                    capabilities: HarnessCapabilities::default(),
                    native_instructions: true,
                },
                HarnessHealth {
                    integration: "codex".to_owned(),
                    display_name: "codex".to_owned(),
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
            Overlay::HarnessHelp,
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
    fn mouse_click_on_plugin_row_only_selects_no_fetch() {
        // Clicking a row must behave like arrow-key navigation — select
        // only, no async inspect fetch (and the "Inspecting…" status flash
        // that comes with it) on every single click. Enter still fetches
        // explicitly — see `content_navigation_and_inspect_intent`.
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
        assert_eq!(intent, Intent::None);
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
            // Marketplace navigation may dispatch a read-only inspect fetch
            // (keeps the drawer's RESOURCES section populated as selection
            // moves) — that's not a mutation, so only reject the intents
            // that actually write something.
            assert!(
                matches!(
                    intent,
                    Intent::None | Intent::InspectMarketplacePlugin { .. }
                ),
                "navigation must never produce a mutating intent, got {intent:?}"
            );
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

    #[test]
    fn entering_doctor_route_requests_deep_health_once() {
        // The dashboard starts with shallow health (no per-receipt vendor
        // inspection); walking the sidebar into Doctor must upgrade it —
        // exactly once, and never again after the deep report arrives.
        let mut model = TuiModel {
            focus: Focus::Sidebar,
            ..TuiModel::default()
        };
        model.refreshed(RefreshData {
            doctor: Some(DoctorReport {
                uze_home: PathBuf::from("/home"),
                store: crate::application::StoreHealth::Ready,
                plugins: Vec::new(),
                harnesses: Vec::new(),
                attachments: Vec::new(),
                ledger_error: None,
                integration_state_error: None,
                provisioning_state_error: None,
            }),
            deep_doctor: false,
            ..RefreshData::default()
        });
        assert!(!model.doctor_deep);

        // Sidebar order: Overview → Marketplace → Plugins → Context →
        // Harnesses → Doctor.
        let mut last_intent = Intent::None;
        for _ in 0..5 {
            last_intent = model.apply_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }
        assert_eq!(model.route, Route::Doctor);
        assert_eq!(
            last_intent,
            Intent::RefreshDoctor,
            "entering Doctor with shallow health must request the full report"
        );

        // Deep report arrives; leaving and re-entering Doctor is free.
        model.refreshed(RefreshData {
            doctor: model.doctor.clone(),
            deep_doctor: true,
            ..RefreshData::default()
        });
        assert!(model.doctor_deep);
        let _ = model.apply_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        let intent = model.apply_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(
            intent,
            Intent::None,
            "a deep report must not be re-requested on every Doctor visit"
        );
        assert_eq!(model.route, Route::Doctor);
    }

    #[test]
    fn doctor_route_refresh_is_deep_but_other_routes_stay_shallow() {
        // The depth decision is a pure routing rule: `r` on Doctor
        // refreshes the full report; everywhere else the cheap health.
        let doctor_route = TuiModel {
            route: Route::Doctor,
            ..TuiModel::default()
        };
        assert!(doctor_route.refresh_depth());

        let mut other = TuiModel::default();
        assert!(!other.refresh_depth());
        other.set_route(Route::Marketplace);
        assert!(!other.refresh_depth());
        // Even a shallow report present on the dashboard never claims to
        // carry attachment checks.
        other.refreshed(RefreshData {
            doctor: Some(DoctorReport {
                uze_home: PathBuf::from("/home"),
                store: crate::application::StoreHealth::Ready,
                plugins: Vec::new(),
                harnesses: Vec::new(),
                attachments: Vec::new(),
                ledger_error: None,
                integration_state_error: None,
                provisioning_state_error: None,
            }),
            deep_doctor: false,
            ..RefreshData::default()
        });
        assert!(!other.doctor_deep);
    }

    #[test]
    fn footer_hint_styles_commands_with_accent_and_descriptions_muted() {
        use ratatui::{
            style::{Modifier, Style},
            text::Line,
        };

        let line = Line::from(hint_spans("↑↓ select · enter inspect · esc back"));
        let content: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(content, "↑↓ select · enter inspect · esc back");
        // Chunks split as key/description: command accent+bold, verb muted,
        // with raw " · " separators between chunks.
        assert_eq!(line.spans.len(), 8);
        assert_eq!(
            line.spans[0].style,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        );
        assert_eq!(line.spans[0].content.as_ref(), "↑↓");
        assert_eq!(line.spans[1].style, Style::default().fg(MUTED));
        assert_eq!(line.spans[1].content.as_ref(), " select");
        assert_eq!(line.spans[2].content.as_ref(), " · ");
        assert_eq!(line.spans[6].content.as_ref(), "esc");
        assert_eq!(line.spans[6].style.fg, Some(ACCENT));

        // A command-only chunk (no verb) still carries the accent.
        let line = Line::from(hint_spans("tab switch · y/n"));
        assert_eq!(line.spans.len(), 4);
        assert_eq!(line.spans[3].content.as_ref(), "y/n");
        assert_eq!(line.spans[3].style.fg, Some(ACCENT));
    }

    #[test]
    fn titlebar_health_click_opens_doctor_and_focuses_content() {
        use ratatui::{Terminal, backend::TestBackend};

        let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
        let mut model = TuiModel::default(); // doctor not loaded → "checking…"
        let mut hits = Vec::new();
        terminal
            .draw(|frame| render(frame, &model, &mut hits))
            .unwrap();
        model.hits = hits;
        let intent = model.apply_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 10,
            row: 1,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(
            intent,
            Intent::RefreshDoctor,
            "entering Doctor (titlebar health click) must request the full report"
        );
        assert_eq!(
            model.route,
            Route::Doctor,
            "clicking the titlebar's health indicator must open the Doctor screen"
        );
        assert_eq!(model.focus, Focus::Content);
    }

    #[test]
    fn clip_line_truncates_long_status_with_ellipsis() {
        use ratatui::text::Line;

        let mut line =
            Line::from("Installed plugin root: /home/user/.codex/plugins/cache/very/long/path");
        clip_line(&mut line, 20);
        let content: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(content, "Installed plugin ro…");
        assert_eq!(ratatui::text::Span::raw(&content).width(), 20);

        let mut line = Line::from("Installed uze");
        clip_line(&mut line, 20);
        let content: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(content, "Installed uze");
    }

    // --- Overview workspace awareness ---------------------------------------

    use crate::application::{
        MarketplaceState, MemoryState, OverviewMarketplace, OverviewWorkspaceSummary,
        ProjectEnvironmentState, ProjectOverview, WorkspaceKind,
    };
    fn consumer_workspace(
        state: ProjectEnvironmentState,
        declared: usize,
        installed: usize,
        missing: &[&str],
        root: &std::path::Path,
    ) -> OverviewWorkspaceSummary {
        OverviewWorkspaceSummary {
            cwd: root.to_path_buf(),
            root: root.to_path_buf(),
            kind: WorkspaceKind::Consumer,
            project: ProjectOverview {
                environment: state,
                memory: MemoryState::Ready,
                declared_plugins: declared,
                installed_plugins: installed,
                missing_plugins: missing.iter().map(|s| s.to_string()).collect(),
            },
            marketplace: None,
        }
    }

    fn marketplace_workspace(root: &std::path::Path) -> OverviewWorkspaceSummary {
        OverviewWorkspaceSummary {
            cwd: root.to_path_buf(),
            root: root.to_path_buf(),
            kind: WorkspaceKind::Marketplace,
            project: ProjectOverview {
                environment: ProjectEnvironmentState::NotConfigured,
                memory: MemoryState::None,
                declared_plugins: 0,
                installed_plugins: 0,
                missing_plugins: Vec::new(),
            },
            marketplace: Some(OverviewMarketplace {
                name: Some("acme".to_owned()),
                package_count: 1,
                invalid_packages: 0,
                state: MarketplaceState::Valid,
            }),
        }
    }

    #[test]
    fn overview_install_key_emits_install_intent_with_workspace_root() {
        let root = std::path::PathBuf::from("/tmp/project");
        let mut model = TuiModel {
            route: Route::Overview,
            focus: Focus::Content,
            workspace: Some(consumer_workspace(
                ProjectEnvironmentState::InstallRequired,
                4,
                3,
                &["flow"],
                &root,
            )),
            ..TuiModel::default()
        };
        let intent = model.apply_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        assert_eq!(intent, Intent::InstallProjectEnvironment(root.clone()));
    }

    #[test]
    fn overview_install_key_is_inert_when_environment_is_ready() {
        let root = std::path::PathBuf::from("/tmp/project");
        let mut model = TuiModel {
            route: Route::Overview,
            focus: Focus::Content,
            workspace: Some(consumer_workspace(
                ProjectEnvironmentState::Ready,
                2,
                2,
                &[],
                &root,
            )),
            ..TuiModel::default()
        };
        let intent = model.apply_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        assert_eq!(intent, Intent::None);
    }

    #[test]
    fn overview_install_key_is_inert_outside_consumer_workspaces() {
        let root = std::path::PathBuf::from("/tmp/market");
        let mut model = TuiModel {
            route: Route::Overview,
            focus: Focus::Content,
            workspace: Some(marketplace_workspace(&root)),
            ..TuiModel::default()
        };
        let intent = model.apply_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        assert_eq!(
            intent,
            Intent::None,
            "marketplace health must not offer `uze install`"
        );
    }

    #[test]
    fn refreshed_updates_workspace_state() {
        let root = std::path::PathBuf::from("/tmp/project");
        let mut model = TuiModel {
            route: Route::Overview,
            ..TuiModel::default()
        };
        assert!(model.workspace.is_none());
        model.refreshed(RefreshData {
            workspace: Some(consumer_workspace(
                ProjectEnvironmentState::InstallRequired,
                4,
                3,
                &["flow"],
                &root,
            )),
            ..RefreshData::default()
        });
        assert_eq!(model.overview_install_path(), Some(root.clone()));

        model.refreshed(RefreshData {
            workspace: Some(consumer_workspace(
                ProjectEnvironmentState::Ready,
                4,
                4,
                &[],
                &root,
            )),
            ..RefreshData::default()
        });
        assert_eq!(
            model.overview_install_path(),
            None,
            "refresh must reflect a completed install"
        );
    }

    /// All rows of the rendered buffer, right-trimmed — the cheap,
    /// snapshot-free way to assert on what the TUI actually drew.
    fn buffer_rows(terminal: &Terminal<TestBackend>) -> Vec<String> {
        let buffer = terminal.backend().buffer();
        let area = buffer.area;
        (area.y..area.y + area.height)
            .map(|row| {
                let mut line = String::new();
                for column in area.x..area.x + area.width {
                    line.push_str(buffer[(column, row)].symbol());
                }
                line.trim_end().to_string()
            })
            .collect()
    }

    fn overview_model(root: &std::path::Path) -> TuiModel {
        // Hybrid workspace: both columns exist, so layout tests can assert
        // on PROJECT vs MARKETPLACE placement.
        let mut workspace = consumer_workspace(
            ProjectEnvironmentState::InstallRequired,
            4,
            3,
            &["flow"],
            root,
        );
        workspace.kind = WorkspaceKind::Hybrid;
        workspace.marketplace = Some(OverviewMarketplace {
            name: Some("acme".to_owned()),
            package_count: 2,
            invalid_packages: 0,
            state: MarketplaceState::Valid,
        });
        TuiModel {
            route: Route::Overview,
            focus: Focus::Content,
            workspace: Some(workspace),
            ..TuiModel::default()
        }
    }

    #[test]
    fn wide_terminal_stacks_workspace_rows() {
        let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
        let model = overview_model(std::path::Path::new("/tmp/project"));
        let mut hits = Vec::new();
        terminal
            .draw(|frame| render(frame, &model, &mut hits))
            .unwrap();
        let rows = buffer_rows(&terminal);
        let project_row = rows
            .iter()
            .position(|row| row.contains("PROJECT"))
            .expect("PROJECT heading");
        let marketplace_row = rows
            .iter()
            .position(|row| row.contains("MARKETPLACE"))
            .expect("MARKETPLACE heading");
        assert!(
            project_row < marketplace_row,
            "workspace blocks always stack: PROJECT above MARKETPLACE, on wide terminals too"
        );
    }

    #[test]
    fn narrow_terminal_stacks_workspace_rows() {
        let mut terminal = Terminal::new(TestBackend::new(60, 40)).unwrap();
        let model = overview_model(std::path::Path::new("/tmp/project"));
        let mut hits = Vec::new();
        terminal
            .draw(|frame| render(frame, &model, &mut hits))
            .unwrap();
        let rows = buffer_rows(&terminal);
        let project_row = rows
            .iter()
            .position(|row| row.contains("PROJECT"))
            .expect("PROJECT heading");
        let marketplace_row = rows
            .iter()
            .position(|row| row.contains("MARKETPLACE"))
            .expect("MARKETPLACE heading");
        assert!(
            project_row < marketplace_row,
            "narrow layout stacks PROJECT above MARKETPLACE"
        );
    }

    #[test]
    fn install_action_only_rendered_when_dependencies_are_missing() {
        let root = std::path::Path::new("/tmp/project");
        let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
        let model = TuiModel {
            route: Route::Overview,
            focus: Focus::Content,
            workspace: Some(consumer_workspace(
                ProjectEnvironmentState::InstallRequired,
                4,
                3,
                &["flow"],
                root,
            )),
            ..TuiModel::default()
        };
        let mut hits = Vec::new();
        terminal
            .draw(|frame| render(frame, &model, &mut hits))
            .unwrap();
        let rows = buffer_rows(&terminal);
        assert!(rows.iter().any(|row| row.contains("i install")));
        assert!(rows.iter().any(|row| row.contains("! install required")));
        assert!(rows.iter().any(|row| row.contains("! 3/4 installed")));

        let model = TuiModel {
            route: Route::Overview,
            focus: Focus::Content,
            workspace: Some(consumer_workspace(
                ProjectEnvironmentState::Ready,
                4,
                4,
                &[],
                root,
            )),
            ..TuiModel::default()
        };
        let mut hits = Vec::new();
        terminal
            .draw(|frame| render(frame, &model, &mut hits))
            .unwrap();
        let rows = buffer_rows(&terminal);
        assert!(
            !rows.iter().any(|row| row.contains("i install")),
            "a ready environment must not offer a useless install action"
        );
        assert!(rows.iter().any(|row| row.contains("✓ ready")));
        assert!(rows.iter().any(|row| row.contains("4 installed")));
    }

    #[test]
    fn overview_renders_application_verdicts_verbatim() {
        let root = std::path::Path::new("/tmp/project");
        let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();

        // Invalid lock: the Application says Invalid, the view renders it —
        // and there is no install action for a state the system cannot read.
        let model = TuiModel {
            route: Route::Overview,
            focus: Focus::Content,
            workspace: Some(consumer_workspace(
                ProjectEnvironmentState::Invalid,
                0,
                0,
                &[],
                root,
            )),
            ..TuiModel::default()
        };
        let mut hits = Vec::new();
        terminal
            .draw(|frame| render(frame, &model, &mut hits))
            .unwrap();
        let rows = buffer_rows(&terminal);
        assert!(rows.iter().any(|row| row.contains("× invalid")));
        assert!(rows.iter().any(|row| row.contains("— unknown")));
        assert!(!rows.iter().any(|row| row.contains("i install")));

        // Marketplace with an invalid manifest: state rendered verbatim.
        let mut bad_market = marketplace_workspace(root);
        bad_market.marketplace = Some(OverviewMarketplace {
            name: None,
            package_count: 0,
            invalid_packages: 0,
            state: MarketplaceState::InvalidManifest,
        });
        let model = TuiModel {
            route: Route::Overview,
            focus: Focus::Content,
            workspace: Some(bad_market),
            ..TuiModel::default()
        };
        let mut hits = Vec::new();
        terminal
            .draw(|frame| render(frame, &model, &mut hits))
            .unwrap();
        let rows = buffer_rows(&terminal);
        assert!(rows.iter().any(|row| row.contains("× invalid manifest")));
        assert!(
            rows.iter().any(|row| row.contains("— unknown")),
            "an unparseable manifest cannot name itself"
        );
        assert!(
            !rows.iter().any(|row| row.contains("PROJECT")),
            "a pure marketplace workspace must not show a PROJECT column"
        );
    }

    #[test]
    fn consumer_only_workspace_hides_marketplace_column() {
        let root = std::path::Path::new("/tmp/project");
        let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
        let model = TuiModel {
            route: Route::Overview,
            focus: Focus::Content,
            workspace: Some(consumer_workspace(
                ProjectEnvironmentState::Ready,
                2,
                2,
                &[],
                root,
            )),
            ..TuiModel::default()
        };
        let mut hits = Vec::new();
        terminal
            .draw(|frame| render(frame, &model, &mut hits))
            .unwrap();
        let rows = buffer_rows(&terminal);
        assert!(rows.iter().any(|row| row.contains("PROJECT")));
        assert!(
            !rows.iter().any(|row| row.contains("MARKETPLACE")),
            "no agents.json → no MARKETPLACE column, even in a consumer"
        );
    }

    #[test]
    fn overview_render_does_not_mutate_project_state() {
        use ratatui::{Terminal, backend::TestBackend};

        let base = std::env::temp_dir().join(format!(
            "uze-ui-overview-immutable-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let root = base.join("project");
        std::fs::create_dir_all(&root).unwrap();
        let lock_path = root.join("agents.lock");
        let lock_bytes = b"version: 1\nplugins: {}\n";
        std::fs::write(&lock_path, lock_bytes).unwrap();
        let manifest_bytes = br#"{"name":"m","plugins":[]}"#;
        std::fs::write(root.join("agents.json"), manifest_bytes).unwrap();
        let agents_md = b"# hi\n";
        std::fs::write(root.join("AGENTS.md"), agents_md).unwrap();

        let model = TuiModel {
            route: Route::Overview,
            context_root: root.clone(),
            workspace: Some(consumer_workspace(
                ProjectEnvironmentState::Ready,
                2,
                2,
                &[],
                &root,
            )),
            ..TuiModel::default()
        };
        let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
        let mut hits = Vec::new();
        terminal
            .draw(|frame| render(frame, &model, &mut hits))
            .unwrap();
        let rows = buffer_rows(&terminal);

        // The render must leave the workspace exactly as found.
        assert_eq!(std::fs::read(&lock_path).unwrap(), lock_bytes);
        assert_eq!(
            std::fs::read(root.join("agents.json")).unwrap(),
            manifest_bytes
        );
        assert_eq!(std::fs::read(root.join("AGENTS.md")).unwrap(), agents_md);
        // And the semantic rows actually rendered (sanity, not just no-op).
        assert!(rows.iter().any(|row| row.contains("Environment")));
        assert!(rows.iter().any(|row| row.contains("Memory")));
        assert!(rows.iter().any(|row| row.contains("Plugins")));
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn no_workspace_render_creates_nothing() {
        use ratatui::{Terminal, backend::TestBackend};

        let base = std::env::temp_dir().join(format!(
            "uze-ui-noworkspace-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let root = base.join("random");
        std::fs::create_dir_all(&root).unwrap();

        let model = TuiModel {
            route: Route::Overview,
            context_root: root.clone(),
            workspace: Some(OverviewWorkspaceSummary {
                cwd: root.clone(),
                root: root.clone(),
                kind: WorkspaceKind::NoWorkspace,
                project: ProjectOverview {
                    environment: ProjectEnvironmentState::NotConfigured,
                    memory: MemoryState::None,
                    declared_plugins: 0,
                    installed_plugins: 0,
                    missing_plugins: Vec::new(),
                },
                marketplace: None,
            }),
            ..TuiModel::default()
        };
        let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
        let mut hits = Vec::new();
        terminal
            .draw(|frame| render(frame, &model, &mut hits))
            .unwrap();
        let rows = buffer_rows(&terminal);
        assert!(rows.iter().any(|row| row.contains("— not configured")));
        assert!(rows.iter().any(|row| row.contains("PROJECT")));
        assert!(
            !rows.iter().any(|row| row.contains("MARKETPLACE")),
            "no agents.json in a plain directory → no MARKETPLACE column"
        );

        assert!(
            !root.join("agents.lock").exists(),
            "rendering must never create a project lock"
        );
        assert!(
            !root.join("agents.json").exists(),
            "rendering must never create a marketplace manifest"
        );
        let entries: Vec<_> = std::fs::read_dir(&root).unwrap().collect();
        assert!(
            entries.is_empty(),
            "a NoWorkspace render must leave the directory untouched"
        );
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn overview_install_intent_reaches_install_project_environment() {
        use std::sync::mpsc;
        use std::time::Duration;

        // `dispatch` builds its application through
        // `UzeApplication::from_env_with_runner`, whose integrations read
        // `HOME`; serialize + restore it so the test is hermetic.
        use std::sync::Mutex;
        static HOME_LOCK: Mutex<()> = Mutex::new(());
        let _guard = HOME_LOCK.lock().unwrap();

        let base = std::env::temp_dir().join(format!(
            "uze-ui-install-dispatch-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let home = base.join("home");
        let project = base.join("project");
        let market = base.join("market");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(market.join("flow/skills/uze-test")).unwrap();
        std::fs::write(
            market.join("agents.json"),
            r#"{"name":"test","plugins":[{"name":"flow","source":"flow"}]}"#,
        )
        .unwrap();
        std::fs::write(market.join("flow/plugin.json"), r#"{"name":"flow"}"#).unwrap();
        std::fs::write(market.join("flow/skills/uze-test/SKILL.md"), "# s\n").unwrap();
        let lock = uze_core::project_lock::ProjectLock {
            version: 1,
            marketplaces: [(
                "test".to_owned(),
                uze_core::project_lock::LockedMarketplace {
                    source: uze_core::project_lock::MarketplaceSource::Path {
                        path: market.clone(),
                    },
                    resolved: uze_core::project_lock::ResolvedMarketplace::default(),
                },
            )]
            .into_iter()
            .collect(),
            plugins: [(
                "flow".to_owned(),
                uze_core::project_lock::LockedPlugin {
                    source: uze_core::project_lock::PluginSource::Marketplace {
                        marketplace: "test".to_owned(),
                        plugin: "flow".to_owned(),
                    },
                    resolved: uze_core::project_lock::ResolvedPlugin {
                        revision: None,
                        version: None,
                        integrity: None,
                    },
                },
            )]
            .into_iter()
            .collect(),
        };
        uze_core::project_lock::save_lock(&project, &lock).unwrap();

        let original_home = std::env::var_os("HOME");
        // SAFETY: single-threaded access to HOME guarded by HOME_LOCK; none
        // of the other tests in this binary read HOME.
        unsafe { std::env::set_var("HOME", &base) };
        unsafe { std::env::set_var("UZE_HOME", &home) };

        let uze_home = UzeHome::at(&home);
        let mut model = TuiModel {
            route: Route::Overview,
            context_root: project.clone(),
            workspace: Some(consumer_workspace(
                ProjectEnvironmentState::InstallRequired,
                1,
                0,
                &["flow"],
                &project,
            )),
            ..TuiModel::default()
        };
        let (sender, receiver) = mpsc::channel();
        super::worker::dispatch(
            Intent::InstallProjectEnvironment(project.clone()),
            &uze_home,
            &sender,
            &mut model,
        );
        let result = receiver.recv_timeout(Duration::from_secs(30)).unwrap();
        match result {
            super::worker::WorkerResult::Mutated(Ok((message, data))) => {
                assert!(
                    message.contains("Installed"),
                    "install must report success, got {message}"
                );
                let workspace = data.workspace.expect("refresh carries workspace state");
                let project = &workspace.project;
                assert_eq!(
                    project.environment,
                    ProjectEnvironmentState::Ready,
                    "after install the Application must report Ready"
                );
                assert_eq!(
                    (project.declared_plugins, project.installed_plugins),
                    (1, 1)
                );
                assert!(project.missing_plugins.is_empty());
            }
            _ => panic!("expected Mutated(Ok(..))"),
        }

        if let Some(home) = original_home {
            // SAFETY: same guard as above.
            unsafe { std::env::set_var("HOME", home) };
        } else {
            unsafe { std::env::remove_var("HOME") };
        }
        unsafe { std::env::remove_var("UZE_HOME") };
        std::fs::remove_dir_all(&base).ok();
    }
}
