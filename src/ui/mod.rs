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

// Compact, low-chrome palette. The terminal's own background stays
// authoritative; these colors establish hierarchy and make real lifecycle
// states legible at a glance — not a "dashboard 2019" full-color treatment.
const ACCENT: Color = Color::Cyan;
const MUTED: Color = Color::DarkGray;
const SUCCESS: Color = Color::Green;
const WARNING: Color = Color::Yellow;
const DANGER: Color = Color::Red;
const PANEL: Color = Color::DarkGray;

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
    if marketplace_plugins.iter().any(|m| m.name == plugin.id) {
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

    let narrow = rows[1].width < 80;
    let sidebar_width = if rows[1].width < 60 {
        16
    } else if narrow {
        18
    } else {
        22
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
        Overlay::ConfirmInstall(name) => overlay::render_confirm_install(frame, frame.area(), name),
        Overlay::ConfirmContextApply => overlay::render_confirm_context_apply(frame, frame.area()),
        Overlay::ProtectedPlugin(id) => overlay::render_protected_plugin(frame, frame.area(), id),
        Overlay::TrustRequired { plugin, detail, .. } => {
            overlay::render_trust_required(frame, frame.area(), plugin, detail)
        }
    }
}

fn render_titlebar(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let issues = model.issues().len();
    let mut line_spans = vec![
        Span::styled(
            " UZE",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
    ];
    if model.doctor.is_none() {
        let frame = SPINNER_FRAMES[model.tick % SPINNER_FRAMES.len()];
        line_spans.push(Span::styled(
            format!("{frame} "),
            Style::default().fg(MUTED),
        ));
        line_spans.push(Span::styled("checking…", Style::default().fg(MUTED)));
    } else if issues == 0 {
        line_spans.push(Span::styled("healthy", Style::default().fg(SUCCESS)));
    } else {
        line_spans.push(Span::styled(
            format!("{issues} issue(s)"),
            Style::default().fg(WARNING),
        ));
    }
    let line = Line::from(line_spans);
    frame.render_widget(
        Paragraph::new(line).block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(PANEL)),
        ),
        area,
    );
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
    // A little vertical air between routes reads far less cramped than a
    // solid stack — but only when the terminal is actually tall enough to
    // afford it; a short terminal falls back to one row each so every
    // route stays reachable by mouse, not just by cycling with the keys.
    let stride: u16 = if inner.height as usize >= ROUTES.len() * 2 {
        2
    } else {
        1
    };
    for (index, route) in ROUTES.iter().enumerate() {
        let row = Rect::new(inner.x, inner.y + index as u16 * stride, inner.width, 1);
        if row.y >= inner.y + inner.height {
            break;
        }
        let selected = *route == model.route;
        let style = if selected && model.focus == Focus::Sidebar {
            Style::default()
                .fg(Color::Black)
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD)
        } else if selected {
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED)
        };
        let label = if narrow {
            &route.label()[..route.label().len().min(inner.width as usize)]
        } else {
            route.label()
        };
        frame.render_widget(Paragraph::new(Span::styled(label, style)), row);
        hits.push((row, Hit::Route(*route)));
    }
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
    let hint = match model.overlay {
        Overlay::None => match model.focus {
            Focus::Sidebar => "↑↓/jk select route · enter/tab open · ? help · q quit",
            _ => route_hint(model.route),
        },
        Overlay::ConfirmRemove { .. } => "tab switch · enter confirm · esc cancel · y/n",
        Overlay::ProtectedPlugin(_) => "esc/enter to dismiss",
        _ => "enter/y confirm · esc/n cancel",
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
        Route::Overview => "tab sidebar · ? help · q quit",
        Route::Plugins => {
            "↑↓/jk select · enter inspect · u update · r remove · tab sidebar · ? help"
        }
        Route::Marketplace => "↑↓/jk select · enter inspect · i install · tab sidebar · ? help",
        Route::Context => "a analyze · p apply · tab sidebar · ? help",
        Route::Harnesses => "↑↓/jk select · s setup · tab sidebar · ? help",
        Route::Doctor => "tab sidebar · g refresh · ? help",
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
    use super::model::{Focus, Overlay, Route, TrustedRetry, TuiModel};
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
                retry: TrustedRetry::Install("acme".to_owned()),
            },
            focus: Focus::Overlay,
            ..TuiModel::default()
        };
        let intent = model.apply_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
        assert_eq!(
            intent,
            Intent::Install("acme".to_owned(), TrustGrant::Granted)
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
}
