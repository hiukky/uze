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
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
};

use crate::{
    Result, UzeApplication, UzeHome,
    application::{
        DoctorReport, HarnessHealth, PluginInspection, PluginSummary, RemovePluginReport,
    },
    exposure::PackageExposurePlan,
};

const POLL_INTERVAL: Duration = Duration::from_millis(100);

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
                    app.add_plugin(PathBuf::from(source))
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
                "Preparing harnesses…".to_owned(),
                |app| {
                    app.setup(None).map(|results| {
                        let configured = results.iter().filter(|result| result.configured).count();
                        format!("Prepared {configured} harness(es)")
                    })
                },
            );
            model.notice = Notice::Working("Preparing harnesses…".to_owned());
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
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(2),
        ])
        .split(frame.area());
    let tabs = [
        (View::Plugins, "1 Plugins"),
        (View::Harnesses, "2 Harnesses"),
        (View::Doctor, "3 Doctor"),
    ]
    .into_iter()
    .map(|(view, label)| {
        let style = if model.view == view {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        Span::styled(format!("{label}    "), style)
    })
    .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(Line::from(tabs)), layout[0]);
    match model.view {
        View::Plugins | View::Add | View::ConfirmRemove => render_plugins(frame, layout[1], model),
        View::Harnesses => render_harnesses(frame, layout[1], model),
        View::Doctor => render_doctor(frame, layout[1], model),
        View::Inspect => render_inspect(frame, layout[1], model),
    }
    frame.render_widget(
        Paragraph::new(footer(model)).wrap(Wrap { trim: true }),
        layout[2],
    );
}

fn render_plugins(frame: &mut ratatui::Frame<'_>, area: ratatui::layout::Rect, model: &TuiModel) {
    let items = model
        .plugins
        .iter()
        .enumerate()
        .map(|(index, plugin)| {
            let marker = if index == model.selected { "> " } else { "  " };
            let health = plugin_health(model.doctor.as_ref(), &plugin.id);
            ListItem::new(format!(
                "{marker}{:<28} {:>2} capabilities  {:<12} {health}",
                plugin.id,
                plugin.capability_count,
                short_source(&plugin.source),
            ))
        })
        .collect::<Vec<_>>();
    let title = match model.view {
        View::Add => "Add plugin — enter a source path",
        View::ConfirmRemove => "Remove selected plugin? enter/y confirm · esc/n cancel",
        _ => "Plugins",
    };
    frame.render_widget(
        List::new(items).block(Block::default().title(title).borders(Borders::BOTTOM)),
        area,
    );
    if model.view == View::Add {
        let prompt = format!("source: {}", model.input);
        frame.render_widget(Paragraph::new(prompt), centered_line(area));
    }
}

fn render_harnesses(frame: &mut ratatui::Frame<'_>, area: ratatui::layout::Rect, model: &TuiModel) {
    let mut lines = vec![Line::from(
        "Harness                 Status                    Strategy",
    )];
    if let Some(doctor) = &model.doctor {
        for harness in &doctor.harnesses {
            lines.push(Line::from(format_harness(harness)));
        }
    } else {
        lines.push(Line::from("Refreshing harness state…"));
    }
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().title("Harnesses").borders(Borders::BOTTOM)),
        area,
    );
}

fn render_doctor(frame: &mut ratatui::Frame<'_>, area: ratatui::layout::Rect, model: &TuiModel) {
    let mut lines = Vec::new();
    if let Some(doctor) = &model.doctor {
        lines.push(Line::from(format!(
            "Store: {:?} · {} plugins",
            doctor.store,
            doctor.plugins.len()
        )));
        for package in &doctor.attachments {
            let state = &package.state;
            if state.drifted + state.conflicts + state.blocked + state.missing > 0 {
                lines.push(Line::from(format!(
                    "{}  {} missing · {} drifted · {} conflicts · {} blocked",
                    package.plugin, state.missing, state.drifted, state.conflicts, state.blocked
                )));
            }
        }
        if let Some(error) = &doctor.ledger_error {
            lines.push(Line::from(format!("Ledger blocked: {error}")));
        }
        if let Some(error) = &doctor.integration_state_error {
            lines.push(Line::from(format!("Integration state blocked: {error}")));
        }
        if lines.len() == 1 {
            lines.push(Line::from("No attachment problems detected."));
        }
    } else {
        lines.push(Line::from("Refreshing doctor report…"));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().title("Doctor").borders(Borders::BOTTOM))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_inspect(frame: &mut ratatui::Frame<'_>, area: ratatui::layout::Rect, model: &TuiModel) {
    let Some(report) = &model.inspection else {
        frame.render_widget(Paragraph::new("Inspecting plugin…"), area);
        return;
    };
    let mut lines = vec![
        Line::from(Span::styled(
            &report.plugin.id,
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("Source  {}", report.plugin.source.display())),
        Line::from(format!("Capabilities  {}", report.capabilities.len())),
        Line::from(""),
        Line::from("Delivery"),
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
        lines.push(Line::from(format!(
            "{:<12} {:<12} {capabilities}",
            delivery.integration, package
        )));
    }
    let state = &report.managed_state;
    lines.push(Line::from(""));
    lines.push(Line::from(format!(
        "Managed  {} matched · {} missing · {} drifted · {} conflicts · {} blocked",
        state.matched, state.missing, state.drifted, state.conflicts, state.blocked
    )));
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .title("Inspect · esc back")
                    .borders(Borders::BOTTOM),
            )
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
    let notice = match &model.notice {
        Notice::Idle => String::new(),
        Notice::Working(value) | Notice::Success(value) | Notice::Error(value) => {
            format!("\n{value}")
        }
    };
    Text::from(format!("{hint}{notice}"))
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

fn format_harness(harness: &HarnessHealth) -> String {
    format!(
        "{:<23} {:<25} {}",
        harness.integration,
        harness.setup,
        harness.strategy.as_deref().unwrap_or("not configured")
    )
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

fn short_source(source: &std::path::Path) -> String {
    source
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("local")
        .to_owned()
}

fn centered_line(area: ratatui::layout::Rect) -> ratatui::layout::Rect {
    ratatui::layout::Rect::new(
        area.x + 2,
        area.y + area.height.saturating_sub(2),
        area.width.saturating_sub(4),
        1,
    )
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
            source: PathBuf::from("/plugins/example"),
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
