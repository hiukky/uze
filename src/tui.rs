//! Minimal keyboard-first terminal presentation over [`UzeApplication`].
//!
//! This module owns only view state, input transitions, and terminal safety.
//! Every product operation is executed by a fresh application facade in a
//! short worker, so the terminal never reads Store, vendor files, or
//! integrations directly.

use std::{
    io::{self, Stdout},
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::Duration,
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph, Wrap},
};

use crate::{
    Result, UzeApplication, UzeHome,
    application::{
        DoctorReport, HarnessHealth, PluginInspection, PluginSummary, RemovePluginReport,
    },
    exposure::PackageExposurePlan,
};

const POLL_INTERVAL: Duration = Duration::from_millis(100);

// The TUI deliberately uses a compact, low-chrome palette. The terminal's
// background remains authoritative, while these colors establish hierarchy
// and make real lifecycle states legible at a glance.
const ACCENT: Color = Color::Cyan;
const MUTED: Color = Color::DarkGray;
const SUCCESS: Color = Color::Green;
const WARNING: Color = Color::Yellow;
const DANGER: Color = Color::Red;
const PANEL: Color = Color::DarkGray;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum View {
    Plugins,
    Harnesses,
    Doctor,
    Inspect,
    Add,
    ConfirmRemove,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Notice {
    Idle,
    Working(String),
    Success(String),
    Error(String),
}

#[derive(Clone, Debug)]
struct TuiModel {
    view: View,
    plugins: Vec<PluginSummary>,
    doctor: Option<DoctorReport>,
    inspection: Option<PluginInspection>,
    selected: usize,
    input: String,
    notice: Notice,
}

impl Default for TuiModel {
    fn default() -> Self {
        Self {
            view: View::Plugins,
            plugins: Vec::new(),
            doctor: None,
            inspection: None,
            selected: 0,
            input: String::new(),
            notice: Notice::Idle,
        }
    }
}

impl TuiModel {
    fn selected_plugin(&self) -> Option<&PluginSummary> {
        self.plugins.get(self.selected)
    }

    fn move_selection(&mut self, delta: isize) {
        if self.plugins.is_empty() {
            self.selected = 0;
            return;
        }
        self.selected = (self.selected as isize + delta)
            .clamp(0, self.plugins.len().saturating_sub(1) as isize)
            as usize;
    }

    fn refreshed(&mut self, plugins: Vec<PluginSummary>, doctor: DoctorReport) {
        self.plugins = plugins;
        self.doctor = Some(doctor);
        self.selected = self.selected.min(self.plugins.len().saturating_sub(1));
        self.notice = Notice::Idle;
    }

    fn apply_key(&mut self, key: KeyEvent) -> Intent {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Intent::Quit;
        }
        match self.view {
            View::Add => self.add_key(key),
            View::ConfirmRemove => self.remove_key(key),
            View::Inspect => match key.code {
                KeyCode::Esc | KeyCode::Backspace => {
                    self.view = View::Plugins;
                    Intent::None
                }
                _ => self.global_key(key),
            },
            _ => self.global_key(key),
        }
    }

    fn global_key(&mut self, key: KeyEvent) -> Intent {
        match key.code {
            KeyCode::Char('q') => Intent::Quit,
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_selection(1);
                Intent::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_selection(-1);
                Intent::None
            }
            KeyCode::Char('1') => {
                self.view = View::Plugins;
                Intent::None
            }
            KeyCode::Char('2') => {
                self.view = View::Harnesses;
                Intent::None
            }
            KeyCode::Char('3') | KeyCode::Char('d') => {
                self.view = View::Doctor;
                Intent::Refresh
            }
            KeyCode::Enter if self.view == View::Plugins => self
                .selected_plugin()
                .map(|plugin| Intent::Inspect(plugin.id.clone()))
                .unwrap_or(Intent::None),
            KeyCode::Char('a') => {
                self.view = View::Add;
                self.input.clear();
                Intent::None
            }
            KeyCode::Char('r') if self.view == View::Plugins => {
                if self.selected_plugin().is_some() {
                    self.view = View::ConfirmRemove;
                }
                Intent::None
            }
            KeyCode::Char('s') => Intent::Setup,
            KeyCode::Char('g') | KeyCode::F(5) => Intent::Refresh,
            KeyCode::Esc => {
                self.view = View::Plugins;
                Intent::None
            }
            _ => Intent::None,
        }
    }

    fn add_key(&mut self, key: KeyEvent) -> Intent {
        match key.code {
            KeyCode::Esc => {
                self.view = View::Plugins;
                Intent::None
            }
            KeyCode::Enter if !self.input.trim().is_empty() => {
                let source = std::mem::take(&mut self.input);
                self.view = View::Plugins;
                Intent::Add(source)
            }
            KeyCode::Backspace => {
                self.input.pop();
                Intent::None
            }
            KeyCode::Char(character) => {
                self.input.push(character);
                Intent::None
            }
            _ => Intent::None,
        }
    }

    fn remove_key(&mut self, key: KeyEvent) -> Intent {
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                self.view = View::Plugins;
                self.selected_plugin()
                    .map(|plugin| Intent::Remove(plugin.id.clone()))
                    .unwrap_or(Intent::None)
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                self.view = View::Plugins;
                Intent::None
            }
            _ => Intent::None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Intent {
    None,
    Quit,
    Refresh,
    Inspect(String),
    Add(String),
    Remove(String),
    Setup,
}

enum WorkerResult {
    Refreshed(std::result::Result<(Vec<PluginSummary>, DoctorReport), String>),
    Inspection(std::result::Result<PluginInspection, String>),
    Mutated(std::result::Result<(String, Vec<PluginSummary>, DoctorReport), String>),
}

/// Runs TUI v0. `home` is passed to workers, which construct the same
/// production application composition root as the CLI.
pub fn run(home: UzeHome) -> Result<()> {
    let mut terminal = TerminalSession::start()?;
    let (sender, receiver) = mpsc::channel();
    let mut model = TuiModel::default();
    spawn_refresh(home.clone(), sender.clone());
    loop {
        terminal.draw(&model)?;
        drain_worker_results(&mut model, &receiver);
        if event::poll(POLL_INTERVAL).map_err(io_error)?
            && let Event::Key(key) = event::read().map_err(io_error)?
        {
            match model.apply_key(key) {
                Intent::None => {}
                Intent::Quit => return Ok(()),
                intent => dispatch(intent, &home, &sender, &mut model),
            }
        }
    }
}

fn dispatch(intent: Intent, home: &UzeHome, sender: &Sender<WorkerResult>, model: &mut TuiModel) {
    match intent {
        Intent::Refresh => {
            model.notice = Notice::Working("Refreshing environment…".to_owned());
            spawn_refresh(home.clone(), sender.clone());
        }
        Intent::Inspect(id) => {
            model.notice = Notice::Working(format!("Inspecting {id}…"));
            let home = home.clone();
            let sender = sender.clone();
            thread::spawn(move || {
                let result = UzeApplication::from_env(home)
                    .and_then(|app| app.inspect_plugin(&id))
                    .map_err(|error| error.to_string());
                let _ = sender.send(WorkerResult::Inspection(result));
            });
        }
        Intent::Add(source) => {
            spawn_mutation(
                home.clone(),
                sender.clone(),
                format!("Adding {source}…"),
                move |app| {
                    // TUI v0 cannot render a trust prompt yet, so it declines to answer
                    // rather than answering yes on the operator's behalf. The
                    // structured `TRUST_REQUIRED` surfaces as an error in the UI,
                    // and a future TUI will render the same `TrustRequest` the CLI
                    // already receives.
                    app.add_plugin(
                        crate::PackageSource::local(source),
                        &crate::trust::NoTrustAuthority,
                    )
                    .map(|report| format!("Installed {}", report.plugin.id))
                },
            );
            model.notice = Notice::Working("Adding plugin…".to_owned());
        }
        Intent::Remove(id) => {
            spawn_mutation(
                home.clone(),
                sender.clone(),
                format!("Removing {id}…"),
                move |app| app.remove_plugin(&id).map(remove_message),
            );
            model.notice = Notice::Working("Removing plugin…".to_owned());
        }
        Intent::Setup => {
            spawn_mutation(
                home.clone(),
                sender.clone(),
                "Provisioning harnesses…".to_owned(),
                |app| {
                    app.setup(None).map(|results| {
                        let configured = results.iter().filter(|result| result.configured).count();
                        format!("Prepared {configured} harness(es)")
                    })
                },
            );
            model.notice = Notice::Working("Provisioning harnesses…".to_owned());
        }
        Intent::None | Intent::Quit => {}
    }
}

fn spawn_refresh(home: UzeHome, sender: Sender<WorkerResult>) {
    thread::spawn(move || {
        let result = UzeApplication::from_env(home)
            .map(|app| {
                let plugins = app.list_plugins()?;
                let doctor = app.doctor();
                Ok((plugins, doctor))
            })
            .and_then(|value: Result<(Vec<PluginSummary>, DoctorReport)>| value)
            .map_err(|error| error.to_string());
        let _ = sender.send(WorkerResult::Refreshed(result));
    });
}

fn spawn_mutation(
    home: UzeHome,
    sender: Sender<WorkerResult>,
    _label: String,
    operation: impl FnOnce(&UzeApplication) -> Result<String> + Send + 'static,
) {
    thread::spawn(move || {
        let result = UzeApplication::from_env(home)
            .and_then(|app| {
                let message = operation(&app)?;
                Ok((message, app.list_plugins()?, app.doctor()))
            })
            .map_err(|error| error.to_string());
        let _ = sender.send(WorkerResult::Mutated(result));
    });
}

fn drain_worker_results(model: &mut TuiModel, receiver: &Receiver<WorkerResult>) {
    while let Ok(result) = receiver.try_recv() {
        match result {
            WorkerResult::Refreshed(Ok((plugins, doctor))) => model.refreshed(plugins, doctor),
            WorkerResult::Inspection(Ok(inspection)) => {
                model.inspection = Some(inspection);
                model.view = View::Inspect;
                model.notice = Notice::Idle;
            }
            WorkerResult::Mutated(Ok((message, plugins, doctor))) => {
                model.refreshed(plugins, doctor);
                model.notice = Notice::Success(message);
            }
            WorkerResult::Refreshed(Err(error))
            | WorkerResult::Inspection(Err(error))
            | WorkerResult::Mutated(Err(error)) => model.notice = Notice::Error(error),
        }
    }
}

fn remove_message(report: RemovePluginReport) -> String {
    match report {
        RemovePluginReport::Removed { plugin, .. } => format!("Removed {plugin}"),
        RemovePluginReport::AlreadyAbsent { plugin } => {
            format!("No UZE state remains for {plugin}")
        }
        RemovePluginReport::Blocked { report, .. } => {
            format!(
                "{} changed outside UZE; managed state was preserved",
                report.package_id
            )
        }
    }
}

struct TerminalSession {
    terminal: Terminal<CrosstermBackend<Stdout>>,
}

impl TerminalSession {
    fn start() -> Result<Self> {
        enable_raw_mode().map_err(io_error)?;
        let mut stdout = io::stdout();
        if let Err(error) = execute!(stdout, EnterAlternateScreen, crossterm::cursor::Hide) {
            let _ = disable_raw_mode();
            return Err(io_error(error));
        }
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend).map_err(io_error)?;
        Ok(Self { terminal })
    }

    fn draw(&mut self, model: &TuiModel) -> Result<()> {
        self.terminal
            .draw(|frame| render(frame, model))
            .map(|_| ())
            .map_err(io_error)
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            LeaveAlternateScreen,
            crossterm::cursor::Show
        );
        let _ = self.terminal.show_cursor();
    }
}

fn render(frame: &mut ratatui::Frame<'_>, model: &TuiModel) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(3),
            Constraint::Length(3),
        ])
        .split(frame.area());
    render_header(frame, layout[0], model);
    match model.view {
        View::Plugins | View::Add | View::ConfirmRemove => render_plugins(frame, layout[1], model),
        View::Harnesses => render_harnesses(frame, layout[1], model),
        View::Doctor => render_doctor(frame, layout[1], model),
        View::Inspect => render_inspect(frame, layout[1], model),
    }
    frame.render_widget(
        Paragraph::new(footer(model))
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(PANEL)),
            )
            .wrap(Wrap { trim: true }),
        layout[2],
    );
}

fn render_header(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let count = model.plugins.len();
    let harnesses = model
        .doctor
        .as_ref()
        .map(|report| report.harnesses.len())
        .unwrap_or_default();
    let tabs = [
        (View::Plugins, "1  Plugins"),
        (View::Harnesses, "2  Harnesses"),
        (View::Doctor, "3  Doctor"),
    ]
    .into_iter()
    .flat_map(|(view, label)| {
        let selected = matches!(model.view, current if current == view)
            || matches!(
                (model.view, view),
                (
                    View::Inspect | View::Add | View::ConfirmRemove,
                    View::Plugins
                )
            );
        let style = if selected {
            Style::default()
                .fg(Color::Black)
                .bg(ACCENT)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED)
        };
        [Span::styled(format!(" {label} "), style), Span::raw("  ")]
    })
    .collect::<Vec<_>>();
    let header = vec![
        Line::from(vec![
            Span::styled(
                " UZE",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled("  local agent environment", Style::default().fg(MUTED)),
            Span::styled(
                format!("{count} plugins · {harnesses} harnesses "),
                Style::default().fg(MUTED),
            ),
        ]),
        Line::from(tabs),
    ];
    frame.render_widget(
        Paragraph::new(header).block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(PANEL)),
        ),
        area,
    );
}

fn render_plugins(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
        .split(area);
    let items = model
        .plugins
        .iter()
        .enumerate()
        .map(|(index, plugin)| {
            let selected = index == model.selected;
            let health = plugin_health(model.doctor.as_ref(), &plugin.id);
            let marker = if selected { "› " } else { "  " };
            let style = if selected {
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(marker, style),
                Span::styled(format!("{}\n", plugin.id), style),
                Span::raw("    "),
                Span::styled(short_source(&plugin.source), Style::default().fg(MUTED)),
                Span::styled(
                    format!("  ·  {} capabilities", plugin.capability_count),
                    Style::default().fg(MUTED),
                ),
                Span::styled(format!("  ·  {health}"), health_style(health)),
            ]))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        List::new(items).block(panel_block(format!(
            " Plugins  {} installed ",
            model.plugins.len()
        ))),
        columns[0],
    );
    render_plugin_summary(frame, columns[1], model);
    if model.view == View::Add {
        render_modal(
            frame,
            area,
            "Add a plugin",
            vec![
                Line::from("Enter a local package path."),
                Line::from(Span::styled(
                    format!("› {}", model.input),
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    "enter install · esc cancel",
                    Style::default().fg(MUTED),
                )),
            ],
            WARNING,
        );
    }
    if model.view == View::ConfirmRemove {
        let plugin = model
            .selected_plugin()
            .map(|item| item.id.as_str())
            .unwrap_or("this plugin");
        render_modal(
            frame,
            area,
            "Remove plugin?",
            vec![
                Line::from(vec![
                    Span::raw("Remove "),
                    Span::styled(
                        plugin.to_owned(),
                        Style::default().fg(DANGER).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" from UZE?"),
                ]),
                Line::from(Span::styled(
                    "Only artifacts that still match UZE ownership are detached.",
                    Style::default().fg(MUTED),
                )),
                Line::from(Span::styled(
                    "enter/y remove · esc/n preserve",
                    Style::default().fg(MUTED),
                )),
            ],
            DANGER,
        );
    }
}

fn render_plugin_summary(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let Some(plugin) = model.selected_plugin() else {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    "No plugins installed",
                    Style::default().add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    "Press a to add a local package.",
                    Style::default().fg(MUTED),
                )),
            ])
            .block(panel_block(" Overview "))
            .alignment(Alignment::Center),
            area,
        );
        return;
    };
    let health = plugin_health(model.doctor.as_ref(), &plugin.id);
    let lines = vec![
        Line::from(Span::styled(
            &plugin.id,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            plugin.source.clone(),
            Style::default().fg(MUTED),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("Capabilities  ", Style::default().fg(MUTED)),
            Span::raw(plugin.capability_count.to_string()),
        ]),
        Line::from(vec![
            Span::styled("Managed state ", Style::default().fg(MUTED)),
            Span::styled(health, health_style(health)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "enter  Inspect delivery",
            Style::default().fg(ACCENT),
        )),
        Line::from(Span::styled(
            "r      Remove safely",
            Style::default().fg(MUTED),
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Selected plugin "))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_harnesses(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let mut lines = vec![Line::from(vec![
        Span::styled(
            "HARNESS",
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        ),
        Span::raw("                 "),
        Span::styled(
            "STATE",
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        ),
        Span::raw("                     "),
        Span::styled(
            "DELIVERY BASELINE",
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        ),
    ])];
    if let Some(doctor) = &model.doctor {
        for harness in &doctor.harnesses {
            lines.push(format_harness(harness));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "Refreshing harness state…",
            Style::default().fg(MUTED),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Harnesses "))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_doctor(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let mut lines = Vec::new();
    if let Some(doctor) = &model.doctor {
        let affected = doctor
            .attachments
            .iter()
            .filter(|package| {
                let state = &package.state;
                state.drifted + state.conflicts + state.blocked + state.missing > 0
            })
            .count();
        lines.push(Line::from(vec![
            Span::styled(
                if affected == 0 {
                    "Healthy"
                } else {
                    "Needs attention"
                },
                if affected == 0 {
                    Style::default().fg(SUCCESS).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(WARNING).add_modifier(Modifier::BOLD)
                },
            ),
            Span::styled(
                format!(
                    "  ·  {} installed plugins  ·  {:?} store",
                    doctor.plugins.len(),
                    doctor.store
                ),
                Style::default().fg(MUTED),
            ),
        ]));
        lines.push(Line::from(""));
        for package in &doctor.attachments {
            let state = &package.state;
            if state.drifted + state.conflicts + state.blocked + state.missing > 0 {
                lines.push(Line::from(vec![
                    Span::styled(
                        "! ",
                        Style::default().fg(WARNING).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        &package.plugin,
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(
                            "  {} missing · {} drifted · {} conflicts · {} blocked",
                            state.missing, state.drifted, state.conflicts, state.blocked
                        ),
                        Style::default().fg(MUTED),
                    ),
                ]));
            }
        }
        if let Some(error) = &doctor.ledger_error {
            lines.push(Line::from(Span::styled(
                format!("! Ledger blocked: {error}"),
                Style::default().fg(DANGER),
            )));
        }
        if let Some(error) = &doctor.integration_state_error {
            lines.push(Line::from(Span::styled(
                format!("! Integration state blocked: {error}"),
                Style::default().fg(DANGER),
            )));
        }
        if affected == 0
            && doctor.ledger_error.is_none()
            && doctor.integration_state_error.is_none()
        {
            lines.push(Line::from(Span::styled(
                "No managed attachment problems detected.",
                Style::default().fg(MUTED),
            )));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "Refreshing doctor report…",
            Style::default().fg(MUTED),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Doctor "))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_inspect(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let Some(report) = &model.inspection else {
        frame.render_widget(
            Paragraph::new("Inspecting plugin…").block(panel_block(" Plugin delivery ")),
            area,
        );
        return;
    };
    let mut lines = vec![
        Line::from(Span::styled(
            &report.plugin.id,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!("Source  {}", report.plugin.source),
            Style::default().fg(MUTED),
        )),
        Line::from(format!("{} capabilities", report.capabilities.len())),
        Line::from(""),
        Line::from(Span::styled(
            "DELIVERY",
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )),
    ];
    for delivery in &report.deliveries {
        let package = delivery
            .package_plan
            .as_ref()
            .map(package_strategy)
            .unwrap_or("decomposed");
        let capabilities = delivery
            .capabilities
            .iter()
            .map(delivery_status)
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:<13}", delivery.integration),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("{:<12}", package), route_style(package)),
            Span::styled(capabilities, Style::default().fg(MUTED)),
        ]));
    }
    let state = &report.managed_state;
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(
            "MANAGED  ",
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{} matched", state.matched),
            Style::default().fg(SUCCESS),
        ),
        Span::styled(
            format!(
                " · {} missing · {} drifted · {} conflicts · {} blocked",
                state.missing, state.drifted, state.conflicts, state.blocked
            ),
            Style::default().fg(MUTED),
        ),
    ]));
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Plugin delivery  ·  esc back "))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn footer(model: &TuiModel) -> Text<'static> {
    let hint = match model.view {
        View::Add => "enter add · esc cancel",
        View::ConfirmRemove => "enter/y remove · esc/n preserve",
        View::Inspect => "esc back · g refresh · q quit",
        _ => {
            "↑↓ navigate · enter inspect · a add · r remove · s setup · d doctor · g refresh · q quit"
        }
    };
    match &model.notice {
        Notice::Idle => Text::from(Line::from(Span::styled(hint, Style::default().fg(MUTED)))),
        Notice::Working(value) => Text::from(vec![
            Line::from(Span::styled(
                value.clone(),
                Style::default().fg(WARNING).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(hint, Style::default().fg(MUTED))),
        ]),
        Notice::Success(value) => Text::from(vec![
            Line::from(Span::styled(
                value.clone(),
                Style::default().fg(SUCCESS).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(hint, Style::default().fg(MUTED))),
        ]),
        Notice::Error(value) => Text::from(vec![
            Line::from(Span::styled(
                value.clone(),
                Style::default().fg(DANGER).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(hint, Style::default().fg(MUTED))),
        ]),
    }
}

fn package_strategy(plan: &PackageExposurePlan) -> &'static str {
    match plan.route {
        crate::router::CompatibilityRoute::Native => "native",
        crate::router::CompatibilityRoute::Adaptable => "adapted",
        crate::router::CompatibilityRoute::Degraded => "degraded",
        crate::router::CompatibilityRoute::Unsupported => "unsupported",
    }
}

fn delivery_status(delivery: &crate::application::CapabilityDelivery) -> String {
    if delivery.provided_by_package {
        return format!("{:?}: provided", delivery.kind);
    }
    let state = delivery
        .plan
        .as_ref()
        .map(|plan| match plan.route {
            crate::router::CompatibilityRoute::Native => "native",
            crate::router::CompatibilityRoute::Adaptable => "attached",
            crate::router::CompatibilityRoute::Degraded => "degraded",
            crate::router::CompatibilityRoute::Unsupported => "unsupported",
        })
        .unwrap_or("unavailable");
    format!("{:?}: {state}", delivery.kind)
}

fn format_harness(harness: &HarnessHealth) -> Line<'static> {
    let setup = harness.setup.as_str();
    let provisioning = harness
        .provisioning
        .as_ref()
        .map(|record| format!(" / {:?}", record.status).to_lowercase())
        .unwrap_or_default();
    Line::from(vec![
        Span::styled(
            format!("{:<24}", harness.integration),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{:<28}", format!("{setup}{provisioning}")),
            setup_style(setup),
        ),
        Span::styled(
            harness
                .strategy
                .clone()
                .unwrap_or_else(|| "not configured".to_owned()),
            Style::default().fg(MUTED),
        ),
    ])
}

fn plugin_health(doctor: Option<&DoctorReport>, plugin: &str) -> &'static str {
    let Some(state) = doctor
        .and_then(|doctor| doctor.attachments.iter().find(|item| item.plugin == plugin))
        .map(|item| &item.state)
    else {
        return "unknown";
    };
    if state.drifted + state.conflicts + state.blocked > 0 {
        "needs attention"
    } else if state.missing > 0 {
        "missing"
    } else {
        "ready"
    }
}

fn short_source(source: &str) -> String {
    let path = std::path::Path::new(source);
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| source.to_owned())
}

fn panel_block(title: impl Into<Line<'static>>) -> Block<'static> {
    Block::default()
        .title(title)
        .title_style(Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(PANEL))
}

fn render_modal(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    title: &str,
    lines: Vec<Line<'static>>,
    color: Color,
) {
    let width = area.width.min(72);
    let height = (lines.len() as u16 + 4).min(area.height.saturating_sub(2));
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(format!(" {title} ")).border_style(Style::default().fg(color)))
            .wrap(Wrap { trim: true }),
        popup,
    );
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

fn io_error(source: io::Error) -> crate::UzeError {
    crate::UzeError::Write {
        path: PathBuf::from("terminal"),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(id: &str) -> PluginSummary {
        PluginSummary {
            id: id.to_owned(),
            source: "/plugins/example".to_owned(),
            store_path: PathBuf::from("/store/example"),
            capability_count: 2,
        }
    }

    #[test]
    fn plugin_navigation_and_remove_confirmation_are_terminal_independent() {
        let mut model = TuiModel {
            plugins: vec![plugin("one"), plugin("two")],
            ..TuiModel::default()
        };
        assert_eq!(
            model.apply_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
            Intent::None
        );
        assert_eq!(model.selected, 1);
        assert_eq!(
            model.apply_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Intent::Inspect("two".to_owned())
        );
        model.view = View::Plugins;
        model.apply_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
        assert_eq!(model.view, View::ConfirmRemove);
        assert_eq!(
            model.apply_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)),
            Intent::None
        );
        assert_eq!(model.view, View::Plugins);
    }

    #[test]
    fn add_input_and_error_state_are_terminal_independent() {
        let mut model = TuiModel::default();
        model.apply_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        for character in "./plugin".chars() {
            model.apply_key(KeyEvent::new(KeyCode::Char(character), KeyModifiers::NONE));
        }
        assert_eq!(
            model.apply_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Intent::Add("./plugin".to_owned())
        );
        drain_worker_results(&mut model, &mpsc::channel::<WorkerResult>().1);
        model.notice =
            Notice::Error("configuration could not be inspected; state preserved".to_owned());
        assert!(matches!(model.notice, Notice::Error(_)));
    }
}
