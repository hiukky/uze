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
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::Duration,
};

use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
        MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, Padding, Paragraph, Wrap},
};

use crate::{
    Result, UzeApplication, UzeError, UzeHome,
    application::{
        ContextPlan, ContextReconciliationReport, DoctorReport, HarnessHealth,
        MarketplacePluginDetail, MarketplacePluginSummary, PluginInspection, PluginSummary,
        ProjectContextStatus, RemovePluginReport, UpdatePluginReport,
    },
    provisioning::{ProcessOutput, ProcessResult, ProcessRunner, ProcessSpec, SystemProcessRunner},
};

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

// --- Routes -----------------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Route {
    Overview,
    Plugins,
    Marketplace,
    Context,
    Harnesses,
    Doctor,
}

const ROUTES: [Route; 6] = [
    Route::Overview,
    Route::Marketplace,
    Route::Plugins,
    Route::Context,
    Route::Harnesses,
    Route::Doctor,
];

impl Route {
    fn label(self) -> &'static str {
        match self {
            Route::Overview => "Overview",
            Route::Plugins => "Plugins",
            Route::Marketplace => "Marketplace",
            Route::Context => "Context",
            Route::Harnesses => "Harnesses",
            Route::Doctor => "Doctor",
        }
    }

    fn index(self) -> usize {
        ROUTES.iter().position(|route| *route == self).unwrap()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Focus {
    Sidebar,
    Content,
    Overlay,
}

#[derive(Clone, Debug, PartialEq)]
enum Overlay {
    None,
    Help,
    ConfirmRemove(String),
    ConfirmUpdate(String),
    ConfirmInstall(String),
    ConfirmContextApply,
    /// A mutation needs consent it wasn't given non-interactively. Confirming
    /// re-runs the *same* action with explicit trust — never a silent
    /// bypass; the operator sees exactly what would newly execute.
    TrustRequired {
        plugin: String,
        detail: String,
        retry: TrustedRetry,
    },
}

#[derive(Clone, Debug, PartialEq)]
enum TrustedRetry {
    Install(String),
    Update(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Status {
    Idle,
    Working(String),
    Success(String),
    Error(String),
}

// --- Model --------------------------------------------------------------

#[derive(Clone, Debug, Default)]
struct RefreshData {
    plugins: Vec<PluginSummary>,
    doctor: Option<DoctorReport>,
    marketplace_plugins: Vec<MarketplacePluginSummary>,
    marketplace_name: String,
    context_status: Option<ProjectContextStatus>,
}

struct TuiModel {
    route: Route,
    focus: Focus,
    overlay: Overlay,
    status: Status,

    plugins: Vec<PluginSummary>,
    plugins_selected: usize,
    plugin_detail: Option<PluginInspection>,

    marketplace_name: String,
    marketplace_plugins: Vec<MarketplacePluginSummary>,
    marketplace_selected: usize,
    marketplace_detail: Option<MarketplacePluginDetail>,

    harnesses_selected: usize,

    doctor: Option<DoctorReport>,

    context_root: PathBuf,
    context_status: Option<ProjectContextStatus>,
    context_plan: Option<ContextPlan>,

    /// Mouse hit targets for the frame just drawn, rebuilt every render.
    /// Kept in one place rather than recomputed ad hoc from coordinates
    /// scattered through render functions.
    hits: Vec<(Rect, Hit)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Hit {
    Route(Route),
    PluginRow(usize),
    MarketplaceRow(usize),
    HarnessRow(usize),
}

impl Default for TuiModel {
    fn default() -> Self {
        Self {
            route: Route::Overview,
            focus: Focus::Sidebar,
            overlay: Overlay::None,
            status: Status::Idle,
            plugins: Vec::new(),
            plugins_selected: 0,
            plugin_detail: None,
            marketplace_name: String::new(),
            marketplace_plugins: Vec::new(),
            marketplace_selected: 0,
            marketplace_detail: None,
            harnesses_selected: 0,
            doctor: None,
            context_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            context_status: None,
            context_plan: None,
            hits: Vec::new(),
        }
    }
}

impl TuiModel {
    fn selected_plugin(&self) -> Option<&PluginSummary> {
        self.plugins.get(self.plugins_selected)
    }

    fn selected_marketplace_plugin(&self) -> Option<&MarketplacePluginSummary> {
        self.marketplace_plugins.get(self.marketplace_selected)
    }

    fn selected_harness(&self) -> Option<&HarnessHealth> {
        self.doctor
            .as_ref()
            .and_then(|doctor| doctor.harnesses.get(self.harnesses_selected))
    }

    fn list_len(&self) -> usize {
        match self.route {
            Route::Plugins => self.plugins.len(),
            Route::Marketplace => self.marketplace_plugins.len(),
            Route::Harnesses => self.doctor.as_ref().map_or(0, |d| d.harnesses.len()),
            _ => 0,
        }
    }

    fn selected_mut(&mut self) -> Option<&mut usize> {
        match self.route {
            Route::Plugins => Some(&mut self.plugins_selected),
            Route::Marketplace => Some(&mut self.marketplace_selected),
            Route::Harnesses => Some(&mut self.harnesses_selected),
            _ => None,
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let len = self.list_len();
        let Some(selected) = self.selected_mut() else {
            return;
        };
        if len == 0 {
            *selected = 0;
            return;
        }
        *selected = (*selected as isize + delta).clamp(0, len as isize - 1) as usize;
    }

    fn refreshed(&mut self, data: RefreshData) {
        self.plugins = data.plugins;
        self.plugins_selected = self
            .plugins_selected
            .min(self.plugins.len().saturating_sub(1));
        self.doctor = data.doctor;
        self.harnesses_selected = self.harnesses_selected.min(
            self.doctor
                .as_ref()
                .map_or(0, |d| d.harnesses.len())
                .saturating_sub(1),
        );
        self.marketplace_plugins = data.marketplace_plugins;
        self.marketplace_name = data.marketplace_name;
        self.marketplace_selected = self
            .marketplace_selected
            .min(self.marketplace_plugins.len().saturating_sub(1));
        if data.context_status.is_some() {
            self.context_status = data.context_status;
        }
        self.status = Status::Idle;
    }

    fn issues(&self) -> Vec<Issue> {
        classify_doctor(self.doctor.as_ref())
    }

    // --- Input -----------------------------------------------------------

    fn apply_key(&mut self, key: KeyEvent) -> Intent {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Intent::Quit;
        }
        if self.overlay != Overlay::None {
            return self.overlay_key(key);
        }
        match key.code {
            KeyCode::Char('?') => {
                self.overlay = Overlay::Help;
                Intent::None
            }
            KeyCode::Char('q') => Intent::Quit,
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::Sidebar => Focus::Content,
                    _ => Focus::Sidebar,
                };
                Intent::None
            }
            KeyCode::BackTab => {
                self.focus = match self.focus {
                    Focus::Content => Focus::Sidebar,
                    _ => Focus::Content,
                };
                Intent::None
            }
            KeyCode::Char('g') | KeyCode::F(5) => Intent::Refresh,
            _ if self.focus == Focus::Sidebar => self.sidebar_key(key),
            _ => self.content_key(key),
        }
    }

    fn sidebar_key(&mut self, key: KeyEvent) -> Intent {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.set_route(ROUTES[(self.route.index() + 1) % ROUTES.len()]);
                Intent::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.set_route(ROUTES[(self.route.index() + ROUTES.len() - 1) % ROUTES.len()]);
                Intent::None
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                self.focus = Focus::Content;
                Intent::None
            }
            _ => Intent::None,
        }
    }

    fn content_key(&mut self, key: KeyEvent) -> Intent {
        match key.code {
            KeyCode::Left | KeyCode::Char('h') if self.route != Route::Context => {
                self.focus = Focus::Sidebar;
                Intent::None
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_selection(1);
                Intent::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_selection(-1);
                Intent::None
            }
            KeyCode::Esc => {
                self.plugin_detail = None;
                self.marketplace_detail = None;
                Intent::None
            }
            KeyCode::Enter => self.open_or_act(),
            KeyCode::Char('r') if self.route == Route::Plugins => {
                if let Some(id) = self.selected_plugin().map(|plugin| plugin.id.clone()) {
                    self.overlay = Overlay::ConfirmRemove(id);
                    self.focus = Focus::Overlay;
                }
                Intent::None
            }
            KeyCode::Char('u') if self.route == Route::Plugins => {
                if let Some(id) = self
                    .selected_plugin()
                    .filter(|plugin| plugin.update_available == Some(true))
                    .map(|plugin| plugin.id.clone())
                {
                    self.overlay = Overlay::ConfirmUpdate(id);
                    self.focus = Focus::Overlay;
                }
                Intent::None
            }
            KeyCode::Char('i') if self.route == Route::Marketplace => {
                if let Some(name) = self
                    .selected_marketplace_plugin()
                    .filter(|plugin| !plugin.installed)
                    .map(|plugin| plugin.name.clone())
                {
                    self.overlay = Overlay::ConfirmInstall(name);
                    self.focus = Focus::Overlay;
                }
                Intent::None
            }
            KeyCode::Char('s') if self.route == Route::Harnesses => {
                self.selected_harness().map_or(Intent::None, |harness| {
                    Intent::Setup(harness.integration.clone())
                })
            }
            KeyCode::Char('a') if self.route == Route::Context => {
                Intent::ContextAnalyze(self.context_root.clone())
            }
            KeyCode::Char('p') if self.route == Route::Context => {
                if self
                    .context_plan
                    .as_ref()
                    .is_some_and(ContextPlan::has_changes)
                {
                    self.overlay = Overlay::ConfirmContextApply;
                    self.focus = Focus::Overlay;
                }
                Intent::None
            }
            _ => Intent::None,
        }
    }

    /// Enter's meaning depends on the route: open plugin delivery detail,
    /// open marketplace plugin detail, or (Harnesses) nothing beyond the
    /// already-visible detail pane, since there is no deeper read model.
    fn open_or_act(&mut self) -> Intent {
        match self.route {
            Route::Plugins => self
                .selected_plugin()
                .map_or(Intent::None, |p| Intent::InspectPlugin(p.id.clone())),
            Route::Marketplace => self
                .selected_marketplace_plugin()
                .map_or(Intent::None, |p| {
                    Intent::InspectMarketplacePlugin(p.name.clone())
                }),
            _ => Intent::None,
        }
    }

    fn overlay_key(&mut self, key: KeyEvent) -> Intent {
        let overlay = self.overlay.clone();
        match (&overlay, key.code) {
            (Overlay::Help, _) => {
                self.close_overlay();
                Intent::None
            }
            (Overlay::ConfirmRemove(id), KeyCode::Char('y') | KeyCode::Enter) => {
                let id = id.clone();
                self.close_overlay();
                Intent::Remove(id)
            }
            (Overlay::ConfirmRemove(_), _) => {
                self.close_overlay();
                Intent::None
            }
            (Overlay::ConfirmUpdate(id), KeyCode::Char('y') | KeyCode::Enter) => {
                let id = id.clone();
                self.close_overlay();
                Intent::Update(id, TrustGrant::Ask)
            }
            (Overlay::ConfirmUpdate(_), _) => {
                self.close_overlay();
                Intent::None
            }
            (Overlay::ConfirmInstall(name), KeyCode::Char('y') | KeyCode::Enter) => {
                let name = name.clone();
                self.close_overlay();
                Intent::Install(name, TrustGrant::Ask)
            }
            (Overlay::ConfirmInstall(_), _) => {
                self.close_overlay();
                Intent::None
            }
            (Overlay::ConfirmContextApply, KeyCode::Char('y') | KeyCode::Enter) => {
                self.close_overlay();
                Intent::ContextApply(self.context_root.clone())
            }
            (Overlay::ConfirmContextApply, _) => {
                self.close_overlay();
                Intent::None
            }
            (Overlay::TrustRequired { retry, .. }, KeyCode::Char('y') | KeyCode::Enter) => {
                let intent = match retry {
                    TrustedRetry::Install(name) => {
                        Intent::Install(name.clone(), TrustGrant::Granted)
                    }
                    TrustedRetry::Update(id) => Intent::Update(id.clone(), TrustGrant::Granted),
                };
                self.close_overlay();
                intent
            }
            (Overlay::TrustRequired { .. }, _) => {
                self.close_overlay();
                Intent::None
            }
            (Overlay::None, _) => Intent::None,
        }
    }

    fn close_overlay(&mut self) {
        self.overlay = Overlay::None;
        self.focus = Focus::Content;
    }

    fn set_route(&mut self, route: Route) {
        self.route = route;
    }

    // --- Mouse -------------------------------------------------------------

    fn apply_mouse(&mut self, event: MouseEvent) -> Intent {
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => self.click(event.column, event.row),
            MouseEventKind::ScrollDown if self.overlay == Overlay::None => {
                self.focus = Focus::Content;
                self.move_selection(1);
                Intent::None
            }
            MouseEventKind::ScrollUp if self.overlay == Overlay::None => {
                self.focus = Focus::Content;
                self.move_selection(-1);
                Intent::None
            }
            _ => Intent::None,
        }
    }

    fn click(&mut self, column: u16, row: u16) -> Intent {
        if self.overlay != Overlay::None {
            // Any click dismisses/declines an overlay — a click outside a
            // dialog's actionable area should never silently confirm.
            self.close_overlay();
            return Intent::None;
        }
        let Some(hit) = self
            .hits
            .iter()
            .find(|(rect, _)| {
                rect.x <= column
                    && column < rect.x + rect.width
                    && rect.y <= row
                    && row < rect.y + rect.height
            })
            .map(|(_, hit)| hit.clone())
        else {
            return Intent::None;
        };
        match hit {
            Hit::Route(route) => {
                self.set_route(route);
                self.focus = Focus::Content;
                Intent::None
            }
            Hit::PluginRow(index) => {
                self.plugins_selected = index;
                self.focus = Focus::Content;
                self.open_or_act()
            }
            Hit::MarketplaceRow(index) => {
                self.marketplace_selected = index;
                self.focus = Focus::Content;
                self.open_or_act()
            }
            Hit::HarnessRow(index) => {
                self.harnesses_selected = index;
                self.focus = Focus::Content;
                Intent::None
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TrustGrant {
    Ask,
    Granted,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Intent {
    None,
    Quit,
    Refresh,
    InspectPlugin(String),
    InspectMarketplacePlugin(String),
    Remove(String),
    Update(String, TrustGrant),
    Install(String, TrustGrant),
    Setup(String),
    ContextAnalyze(PathBuf),
    ContextApply(PathBuf),
}

enum WorkerResult {
    Refreshed(std::result::Result<RefreshData, String>),
    PluginInspected(std::result::Result<PluginInspection, String>),
    MarketplaceInspected(std::result::Result<MarketplacePluginDetail, String>),
    Mutated(std::result::Result<(String, RefreshData), String>),
    TrustRequired {
        plugin: String,
        detail: String,
        retry: TrustedRetry,
    },
    ContextAnalyzed(std::result::Result<(ProjectContextStatus, ContextPlan), String>),
    ContextApplied(std::result::Result<(String, ContextReconciliationReport), String>),
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

fn dispatch(intent: Intent, home: &UzeHome, sender: &Sender<WorkerResult>, model: &mut TuiModel) {
    match intent {
        Intent::None | Intent::Quit => {}
        Intent::Refresh => {
            model.status = Status::Working("Refreshing environment…".to_owned());
            spawn_refresh(home.clone(), sender.clone(), model.context_root.clone());
        }
        Intent::InspectPlugin(id) => {
            model.status = Status::Working(format!("Inspecting {id}…"));
            let (home, sender) = (home.clone(), sender.clone());
            thread::spawn(move || {
                let result = tui_application(home)
                    .and_then(|app| app.inspect_plugin(&id))
                    .map_err(|error| error.to_string());
                let _ = sender.send(WorkerResult::PluginInspected(result));
            });
        }
        Intent::InspectMarketplacePlugin(name) => {
            model.status = Status::Working(format!("Inspecting {name}…"));
            let (home, sender) = (home.clone(), sender.clone());
            thread::spawn(move || {
                let result = tui_application(home)
                    .and_then(|app| app.inspect_marketplace_plugin(&name))
                    .map_err(|error| error.to_string());
                let _ = sender.send(WorkerResult::MarketplaceInspected(result));
            });
        }
        Intent::Remove(id) => {
            model.status = Status::Working(format!("Removing {id}…"));
            spawn_mutation(
                home.clone(),
                sender.clone(),
                model.context_root.clone(),
                move |app| app.remove_plugin(&id).map(remove_message),
            );
        }
        Intent::Update(id, grant) => {
            model.status = Status::Working(format!("Updating {id}…"));
            let retry_id = id.clone();
            spawn_trust_sensitive(
                home.clone(),
                sender.clone(),
                model.context_root.clone(),
                grant,
                id.clone(),
                move |app, authority| app.update_plugin(&id, authority).map(update_message),
                TrustedRetry::Update(retry_id),
            );
        }
        Intent::Install(name, grant) => {
            model.status = Status::Working(format!("Installing {name}…"));
            let retry_name = name.clone();
            spawn_trust_sensitive(
                home.clone(),
                sender.clone(),
                model.context_root.clone(),
                grant,
                name.clone(),
                move |app, authority| {
                    app.install_from_marketplace(&name, authority)
                        .map(|report| format!("Installed {}", report.plugin.id))
                },
                TrustedRetry::Install(retry_name),
            );
        }
        Intent::Setup(harness) => {
            model.status = Status::Working(format!("Setting up {harness}…"));
            spawn_mutation(
                home.clone(),
                sender.clone(),
                model.context_root.clone(),
                move |app| {
                    app.setup(Some(&harness)).map(|results| {
                        results
                            .into_iter()
                            .find(|r| r.integration == harness)
                            .map(|r| {
                                if r.configured {
                                    format!("{harness} ready")
                                } else {
                                    format!("{harness} setup {:?}", r.provisioning.status)
                                }
                            })
                            .unwrap_or_else(|| format!("{harness} setup attempted"))
                    })
                },
            );
        }
        Intent::ContextAnalyze(root) => {
            model.status = Status::Working("Analyzing project context…".to_owned());
            let (home, sender) = (home.clone(), sender.clone());
            thread::spawn(move || {
                let result = tui_application(home).and_then(|app| {
                    let status = app.context_inspect(&root)?;
                    let plan = app.context_plan(&root)?;
                    Ok((status, plan))
                });
                let _ = sender.send(WorkerResult::ContextAnalyzed(
                    result.map_err(|error| error.to_string()),
                ));
            });
        }
        Intent::ContextApply(root) => {
            model.status = Status::Working("Applying context reconciliation…".to_owned());
            let (home, sender) = (home.clone(), sender.clone());
            thread::spawn(move || {
                let result = tui_application(home)
                    .and_then(|app| app.context_reconcile(&root))
                    .map(|report| ("Context reconciled".to_owned(), report))
                    .map_err(|error| error.to_string());
                let _ = sender.send(WorkerResult::ContextApplied(result));
            });
        }
    }
}

fn spawn_refresh(home: UzeHome, sender: Sender<WorkerResult>, context_root: PathBuf) {
    thread::spawn(move || {
        let result = load_refresh_data(home, &context_root).map_err(|error| error.to_string());
        let _ = sender.send(WorkerResult::Refreshed(result));
    });
}

/// The one-time startup path, run in the background so the terminal takes
/// over instantly instead of sitting blank while default plugins are
/// seeded. Before this moved here, `main` ran `ensure_default_plugins`
/// synchronously — several harness-detection subprocess spawns — *before*
/// the alternate screen was even entered, so the terminal appeared frozen
/// for that whole stretch. Every subsequent refresh (`Intent::Refresh`)
/// goes through the plain `spawn_refresh` above; seeding defaults only
/// needs to happen once, at launch, not on every manual refresh.
fn spawn_startup(home: UzeHome, sender: Sender<WorkerResult>, context_root: PathBuf) {
    thread::spawn(move || {
        if let Ok(app) = tui_application(home.clone()) {
            let _ = app.ensure_default_plugins();
        }
        let result = load_refresh_data(home, &context_root).map_err(|error| error.to_string());
        let _ = sender.send(WorkerResult::Refreshed(result));
    });
}

fn load_refresh_data(home: UzeHome, context_root: &std::path::Path) -> Result<RefreshData> {
    let app = tui_application(home)?;
    let plugins = app.list_plugins()?;
    let doctor = app.doctor();
    let marketplaces = app.list_marketplaces()?;
    let marketplace_plugins = app.list_marketplace_plugins()?;
    let marketplace_name = marketplaces
        .first()
        .map(|m| m.name.clone())
        .unwrap_or_default();
    let context_status = app.context_inspect(context_root).ok();
    Ok(RefreshData {
        plugins,
        doctor: Some(doctor),
        marketplace_plugins,
        marketplace_name,
        context_status,
    })
}

fn spawn_mutation(
    home: UzeHome,
    sender: Sender<WorkerResult>,
    context_root: PathBuf,
    operation: impl FnOnce(&UzeApplication) -> Result<String> + Send + 'static,
) {
    thread::spawn(move || {
        let result = tui_application(home.clone()).and_then(|app| {
            let message = operation(&app)?;
            let data = load_refresh_data(home, &context_root)?;
            Ok((message, data))
        });
        let _ = sender.send(WorkerResult::Mutated(
            result.map_err(|error| error.to_string()),
        ));
    });
}

/// Like `spawn_mutation`, but the operation is one that can cross the trust
/// boundary. With `TrustGrant::Ask` it runs non-interactively
/// (`NoTrustAuthority`) and, on `TRUST_REQUIRED`, surfaces a dialog rather
/// than failing silently or granting on the operator's behalf.
/// `TrustGrant::Granted` is only ever reached by that dialog's own explicit
/// confirmation re-dispatching the same action.
fn spawn_trust_sensitive(
    home: UzeHome,
    sender: Sender<WorkerResult>,
    context_root: PathBuf,
    grant: TrustGrant,
    package_hint: String,
    operation: impl FnOnce(&UzeApplication, &dyn crate::trust::TrustAuthority) -> Result<String>
    + Send
    + 'static,
    retry: TrustedRetry,
) {
    thread::spawn(move || {
        let outcome = tui_application(home.clone()).and_then(|app| {
            let result = match grant {
                TrustGrant::Ask => operation(&app, &crate::trust::NoTrustAuthority),
                TrustGrant::Granted => operation(&app, &crate::trust::AlwaysTrust),
            };
            result.map(|message| (message, ()))
        });
        match outcome {
            Ok((message, ())) => match load_refresh_data(home, &context_root) {
                Ok(data) => {
                    let _ = sender.send(WorkerResult::Mutated(Ok((message, data))));
                }
                Err(error) => {
                    let _ = sender.send(WorkerResult::Mutated(Err(error.to_string())));
                }
            },
            Err(UzeError::TrustRequired { package, detail }) => {
                let _ = sender.send(WorkerResult::TrustRequired {
                    plugin: if package.is_empty() {
                        package_hint
                    } else {
                        package
                    },
                    detail,
                    retry,
                });
            }
            Err(error) => {
                let _ = sender.send(WorkerResult::Mutated(Err(error.to_string())));
            }
        }
    });
}

fn drain_worker_results(model: &mut TuiModel, receiver: &Receiver<WorkerResult>) {
    while let Ok(result) = receiver.try_recv() {
        match result {
            WorkerResult::Refreshed(Ok(data)) => model.refreshed(data),
            WorkerResult::PluginInspected(Ok(inspection)) => {
                model.plugin_detail = Some(inspection);
                model.status = Status::Idle;
            }
            WorkerResult::MarketplaceInspected(Ok(detail)) => {
                model.marketplace_detail = Some(detail);
                model.status = Status::Idle;
            }
            WorkerResult::Mutated(Ok((message, data))) => {
                model.refreshed(data);
                model.status = Status::Success(message);
            }
            WorkerResult::TrustRequired {
                plugin,
                detail,
                retry,
            } => {
                model.overlay = Overlay::TrustRequired {
                    plugin,
                    detail,
                    retry,
                };
                model.focus = Focus::Overlay;
                model.status = Status::Idle;
            }
            WorkerResult::ContextAnalyzed(Ok((status, plan))) => {
                model.context_status = Some(status);
                model.context_plan = Some(plan);
                model.status = Status::Idle;
            }
            WorkerResult::ContextApplied(Ok((message, report))) => {
                model.status = Status::Success(message);
                let _ = report;
            }
            WorkerResult::Refreshed(Err(error))
            | WorkerResult::PluginInspected(Err(error))
            | WorkerResult::MarketplaceInspected(Err(error))
            | WorkerResult::Mutated(Err(error))
            | WorkerResult::ContextAnalyzed(Err(error))
            | WorkerResult::ContextApplied(Err(error)) => model.status = Status::Error(error),
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

fn update_message(report: UpdatePluginReport) -> String {
    match report {
        UpdatePluginReport::Updated { plugin, .. } => format!("Updated {}", plugin.id),
        UpdatePluginReport::Blocked { report, .. } => {
            format!(
                "{} update blocked; managed state was preserved",
                report.package_id
            )
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
        Route::Overview => render_overview(frame, columns[1], model),
        Route::Plugins => render_plugins(frame, columns[1], model, hits),
        Route::Marketplace => render_marketplace(frame, columns[1], model, hits),
        Route::Context => render_context(frame, columns[1], model),
        Route::Harnesses => render_harnesses(frame, columns[1], model, hits),
        Route::Doctor => render_doctor(frame, columns[1], model),
    }

    frame.render_widget(
        Paragraph::new(footer(model))
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(PANEL)),
            )
            .wrap(Wrap { trim: true }),
        rows[2],
    );

    match &model.overlay {
        Overlay::None => {}
        Overlay::Help => render_help(frame, frame.area()),
        Overlay::ConfirmRemove(id) => render_confirm_remove(frame, frame.area(), id),
        Overlay::ConfirmUpdate(id) => render_confirm_update(frame, frame.area(), id),
        Overlay::ConfirmInstall(name) => render_confirm_install(frame, frame.area(), name),
        Overlay::ConfirmContextApply => render_confirm_context_apply(frame, frame.area()),
        Overlay::TrustRequired { plugin, detail, .. } => {
            render_trust_required(frame, frame.area(), plugin, detail)
        }
    }
}

fn render_titlebar(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let issues = model.issues().len();
    let health = if model.doctor.is_none() {
        Span::styled("checking…", Style::default().fg(MUTED))
    } else if issues == 0 {
        Span::styled("healthy", Style::default().fg(SUCCESS))
    } else {
        Span::styled(format!("{issues} issue(s)"), Style::default().fg(WARNING))
    };
    let line = Line::from(vec![
        Span::styled(
            " UZE",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        health,
        Span::styled(
            format!("  ·  v{} ", env!("CARGO_PKG_VERSION")),
            Style::default().fg(MUTED),
        ),
    ]);
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

fn render_overview(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let harness_total = model.doctor.as_ref().map_or(0, |d| d.harnesses.len());
    let harness_detected = model.doctor.as_ref().map_or(0, |d| {
        d.harnesses.iter().filter(|h| h.detection.present).count()
    });
    let portability = model
        .context_status
        .as_ref()
        .map(|status| portability_label(&status.portability))
        .unwrap_or("checking…");
    let issues = model.issues().len();

    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                format!("{harness_detected}"),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("/{harness_total} harnesses detected")),
        ]),
        Line::from(vec![
            Span::styled(
                format!("{}", model.plugins.len()),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw(if model.plugins.len() == 1 {
                " plugin installed"
            } else {
                " plugins installed"
            }),
        ]),
        Line::from(vec![Span::raw(if model.marketplace_name.is_empty() {
            "Marketplace loading…".to_owned()
        } else {
            format!(
                "{} marketplace ready ({} plugins)",
                model.marketplace_name,
                model.marketplace_plugins.len()
            )
        })]),
        Line::from(vec![
            Span::raw("Current project: "),
            Span::styled(
                portability,
                portability_style(model.context_status.as_ref()),
            ),
        ]),
        Line::from(vec![if issues == 0 {
            Span::styled("No issues", Style::default().fg(SUCCESS))
        } else {
            Span::styled(
                format!("{issues} issue(s) — see Doctor"),
                Style::default().fg(WARNING),
            )
        }]),
        Line::from(""),
        Line::from(Span::styled(
            "Suggested",
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )),
        Line::from("  Tab → Marketplace   browse and install plugins"),
        Line::from("  Tab → Harnesses     manage detected harnesses"),
        Line::from("  Tab → Context       analyze this project"),
    ];
    if model.plugins.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "No plugins installed yet — open Marketplace to install one.",
            Style::default().fg(MUTED),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Overview "))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn portability_label(portability: &crate::application::Portability) -> &'static str {
    use crate::application::Portability;
    match portability {
        Portability::NoContext => "no context",
        Portability::Portable => "portable",
        Portability::PartiallyPortable { .. } => "partially portable",
        Portability::VendorLocked { .. } => "vendor-locked",
    }
}

fn portability_style(status: Option<&ProjectContextStatus>) -> Style {
    use crate::application::Portability;
    match status.map(|s| &s.portability) {
        Some(Portability::Portable) => Style::default().fg(SUCCESS),
        Some(Portability::NoContext) => Style::default().fg(MUTED),
        Some(_) => Style::default().fg(WARNING),
        None => Style::default().fg(MUTED),
    }
}

fn render_plugins(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(46), Constraint::Percentage(54)])
        .split(area);

    let block = panel_block(format!(" Plugins  {} installed ", model.plugins.len()));
    let inner = block.inner(columns[0]);
    frame.render_widget(block, columns[0]);
    if model.plugins.is_empty() {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    "No plugins installed",
                    Style::default().add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    "Open Marketplace to install one.",
                    Style::default().fg(MUTED),
                )),
            ])
            .wrap(Wrap { trim: true }),
            inner,
        );
    } else {
        let items: Vec<ListItem> = model
            .plugins
            .iter()
            .enumerate()
            .map(|(index, plugin)| plugin_row(plugin, index == model.plugins_selected, model))
            .collect();
        frame.render_widget(List::new(items), inner);
        for index in 0..model.plugins.len() {
            let row = Rect::new(inner.x, inner.y + index as u16, inner.width, 1);
            if row.y < inner.y + inner.height {
                hits.push((row, Hit::PluginRow(index)));
            }
        }
    }
    render_plugin_detail(frame, columns[1], model);
}

fn plugin_row<'a>(plugin: &'a PluginSummary, selected: bool, model: &TuiModel) -> ListItem<'a> {
    let marker = if selected { "› " } else { "  " };
    let style = if selected {
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let is_official = plugin.source.starts_with("embedded:");
    let mut spans = vec![Span::styled(marker, style), Span::styled(&plugin.id, style)];
    if is_official {
        spans.push(Span::styled("  Official", Style::default().fg(ACCENT)));
    }
    if plugin.update_available == Some(true) {
        spans.push(Span::styled(
            "  Update available",
            Style::default().fg(WARNING),
        ));
    }
    let health = plugin_health(model.doctor.as_ref(), &plugin.id);
    spans.push(Span::styled(format!("  {health}"), health_style(health)));
    ListItem::new(Line::from(spans))
}

fn render_plugin_detail(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let Some(plugin) = model.selected_plugin() else {
        frame.render_widget(Paragraph::new("").block(panel_block(" Plugin ")), area);
        return;
    };
    let is_official = plugin.source.starts_with("embedded:");
    let mut lines = vec![
        Line::from(Span::styled(
            &plugin.id,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            if is_official {
                "Official".to_owned()
            } else {
                format!("Source: {}", plugin.source)
            },
            Style::default().fg(MUTED),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("Capabilities  ", Style::default().fg(MUTED)),
            Span::raw(plugin.capability_count.to_string()),
        ]),
        Line::from(vec![
            Span::styled("Update        ", Style::default().fg(MUTED)),
            match plugin.update_available {
                Some(true) => Span::styled("Available", Style::default().fg(WARNING)),
                Some(false) => Span::styled("Up to date", Style::default().fg(SUCCESS)),
                None => Span::styled("Unknown", Style::default().fg(MUTED)),
            },
        ]),
    ];
    if let Some(inspection) = &model.plugin_detail
        && inspection.plugin.id == plugin.id
    {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Available in",
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )));
        for delivery in &inspection.deliveries {
            let route = delivery
                .package_plan
                .as_ref()
                .map(package_strategy)
                .unwrap_or_else(|| {
                    delivery
                        .capabilities
                        .first()
                        .and_then(|c| c.plan.as_ref())
                        .map(exposure_route_label)
                        .unwrap_or("unsupported")
                });
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:<12}", delivery.integration),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::styled(route, route_style(route)),
            ]));
        }
        let state = &inspection.managed_state;
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Managed  ", Style::default().fg(MUTED)),
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
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "enter  Inspect delivery",
        Style::default().fg(ACCENT),
    )));
    lines.push(Line::from(Span::styled(
        if plugin.update_available == Some(true) {
            "u      Update"
        } else {
            ""
        },
        Style::default().fg(if plugin.update_available == Some(true) {
            ACCENT
        } else {
            MUTED
        }),
    )));
    lines.push(Line::from(Span::styled(
        "r      Remove",
        Style::default().fg(MUTED),
    )));
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Selected plugin "))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn exposure_route_label(plan: &crate::exposure::ExposurePlan) -> &'static str {
    match plan.route {
        crate::router::CompatibilityRoute::Native => "native",
        crate::router::CompatibilityRoute::Adaptable => "adapted",
        crate::router::CompatibilityRoute::Degraded => "degraded",
        crate::router::CompatibilityRoute::Unsupported => "unsupported",
    }
}

fn render_marketplace(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(46), Constraint::Percentage(54)])
        .split(area);
    let title = if model.marketplace_name.is_empty() {
        " Marketplace ".to_owned()
    } else {
        format!(" Marketplace  ·  {} ", model.marketplace_name)
    };
    let block = panel_block(title);
    let inner = block.inner(columns[0]);
    frame.render_widget(block, columns[0]);
    if model.marketplace_plugins.is_empty() {
        frame.render_widget(
            Paragraph::new("No marketplace plugins available.").wrap(Wrap { trim: true }),
            inner,
        );
    } else {
        let items: Vec<ListItem> = model
            .marketplace_plugins
            .iter()
            .enumerate()
            .map(|(index, plugin)| {
                let selected = index == model.marketplace_selected;
                let marker = if selected { "› " } else { "  " };
                let style = if selected {
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                let status = if plugin.installed {
                    Span::styled("Installed", Style::default().fg(SUCCESS))
                } else {
                    Span::styled("Available", Style::default().fg(MUTED))
                };
                ListItem::new(Line::from(vec![
                    Span::styled(marker, style),
                    Span::styled(&plugin.name, style),
                    Span::raw("  "),
                    status,
                ]))
            })
            .collect();
        frame.render_widget(List::new(items), inner);
        for index in 0..model.marketplace_plugins.len() {
            let row = Rect::new(inner.x, inner.y + index as u16, inner.width, 1);
            if row.y < inner.y + inner.height {
                hits.push((row, Hit::MarketplaceRow(index)));
            }
        }
    }
    render_marketplace_detail(frame, columns[1], model);
}

fn render_marketplace_detail(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let Some(plugin) = model.selected_marketplace_plugin() else {
        frame.render_widget(Paragraph::new("").block(panel_block(" Plugin ")), area);
        return;
    };
    let mut lines = vec![
        Line::from(Span::styled(
            &plugin.name,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            plugin.description.clone().unwrap_or_default(),
            Style::default().fg(MUTED),
        )),
    ];
    if !plugin.keywords.is_empty() {
        lines.push(Line::from(Span::styled(
            plugin.keywords.join(", "),
            Style::default().fg(MUTED),
        )));
    }
    lines.push(Line::from(""));
    if let Some(detail) = &model.marketplace_detail
        && detail.summary.name == plugin.name
    {
        lines.push(Line::from(Span::styled(
            "Capabilities",
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )));
        for capability in &detail.capabilities {
            lines.push(Line::from(format!(
                "  {:?}  {}",
                capability.kind, capability.name
            )));
        }
        lines.push(Line::from(""));
    }
    lines.push(Line::from(vec![
        Span::styled("Status  ", Style::default().fg(MUTED)),
        if plugin.installed {
            Span::styled("Installed", Style::default().fg(SUCCESS))
        } else {
            Span::styled("Not installed", Style::default().fg(MUTED))
        },
    ]));
    if plugin.is_default {
        lines.push(Line::from(Span::styled(
            "Installed by default on a fresh setup",
            Style::default().fg(MUTED),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "enter  Inspect capabilities",
        Style::default().fg(ACCENT),
    )));
    if !plugin.installed {
        lines.push(Line::from(Span::styled(
            "i      Install",
            Style::default().fg(ACCENT),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Plugin "))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_context(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let mut lines = vec![
        Line::from(Span::styled(
            "Project",
            Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
        )),
        Line::from(format!("  {}", model.context_root.display())),
        Line::from(""),
    ];
    match &model.context_status {
        None => lines.push(Line::from(Span::styled(
            "Press a to analyze this project.",
            Style::default().fg(MUTED),
        ))),
        Some(status) => {
            lines.push(Line::from(vec![
                Span::styled("Portability  ", Style::default().fg(MUTED)),
                Span::styled(
                    portability_label(&status.portability),
                    portability_style(Some(status)),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("Canonical    ", Style::default().fg(MUTED)),
                Span::raw("AGENTS.md"),
            ]));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Harnesses",
                Style::default().fg(MUTED).add_modifier(Modifier::BOLD),
            )));
            for harness in &status.harnesses {
                let delivery = match &harness.delivery {
                    crate::application::HarnessContextDelivery::Native => "native".to_owned(),
                    crate::application::HarnessContextDelivery::Bridge { state, .. } => {
                        format!("{state:?}").to_lowercase()
                    }
                    crate::application::HarnessContextDelivery::NotDetected => {
                        "not detected".to_owned()
                    }
                };
                lines.push(Line::from(format!(
                    "  {:<12} {delivery}",
                    harness.integration
                )));
            }
            if !status.warnings.is_empty() {
                lines.push(Line::from(""));
                for warning in &status.warnings {
                    lines.push(Line::from(Span::styled(
                        format!("! {warning}"),
                        Style::default().fg(WARNING),
                    )));
                }
            }
        }
    }
    if let Some(plan) = &model.context_plan {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            if plan.has_changes() {
                "Plan: changes pending"
            } else {
                "Plan: nothing to apply"
            },
            Style::default().fg(if plan.has_changes() { WARNING } else { SUCCESS }),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "a  Analyze (read-only)",
        Style::default().fg(ACCENT),
    )));
    if model
        .context_plan
        .as_ref()
        .is_some_and(ContextPlan::has_changes)
    {
        lines.push(Line::from(Span::styled(
            "p  Apply",
            Style::default().fg(ACCENT),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Context "))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_harnesses(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    model: &TuiModel,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    let block = panel_block(" Harnesses ");
    let inner = block.inner(columns[0]);
    frame.render_widget(block, columns[0]);
    match &model.doctor {
        None => {
            frame.render_widget(Paragraph::new("Loading…").wrap(Wrap { trim: true }), inner);
        }
        Some(doctor) => {
            let items: Vec<ListItem> = doctor
                .harnesses
                .iter()
                .enumerate()
                .map(|(index, harness)| {
                    let selected = index == model.harnesses_selected;
                    let marker = if selected { "› " } else { "  " };
                    let style = if selected {
                        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                    };
                    let status = if harness.detection.present {
                        Span::styled("Installed", Style::default().fg(SUCCESS))
                    } else {
                        Span::styled("Not installed", Style::default().fg(MUTED))
                    };
                    ListItem::new(Line::from(vec![
                        Span::styled(marker, style),
                        Span::styled(format!("{:<14}", harness.integration), style),
                        status,
                    ]))
                })
                .collect();
            frame.render_widget(List::new(items), inner);
            for index in 0..doctor.harnesses.len() {
                let row = Rect::new(inner.x, inner.y + index as u16, inner.width, 1);
                if row.y < inner.y + inner.height {
                    hits.push((row, Hit::HarnessRow(index)));
                }
            }
        }
    }
    render_harness_detail(frame, columns[1], model);
}

fn render_harness_detail(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let Some(harness) = model.selected_harness() else {
        frame.render_widget(Paragraph::new("").block(panel_block(" Harness ")), area);
        return;
    };
    let action_label = if harness.detection.present {
        "Update"
    } else {
        "Install"
    };
    let mut lines = vec![
        Line::from(Span::styled(
            &harness.integration,
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("Version   ", Style::default().fg(MUTED)),
            Span::raw(
                harness
                    .detection
                    .version
                    .clone()
                    .unwrap_or_else(|| "unknown".to_owned()),
            ),
        ]),
        Line::from(vec![
            Span::styled("Status    ", Style::default().fg(MUTED)),
            Span::styled(harness.setup.clone(), setup_style(&harness.setup)),
        ]),
        Line::from(vec![
            Span::styled("Delivery  ", Style::default().fg(MUTED)),
            Span::raw(
                harness
                    .strategy
                    .clone()
                    .unwrap_or_else(|| "not configured".to_owned()),
            ),
        ]),
    ];
    if let Some(provisioning) = &harness.provisioning {
        lines.push(Line::from(vec![
            Span::styled("Provisioning  ", Style::default().fg(MUTED)),
            Span::raw(format!(
                "{:?} ({:?})",
                provisioning.status, provisioning.action
            )),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!("s  {action_label}"),
        Style::default().fg(ACCENT),
    )));
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" Harness "))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_doctor(frame: &mut ratatui::Frame<'_>, area: Rect, model: &TuiModel) {
    let mut lines = Vec::new();
    let issues = model.issues();
    if issues.is_empty() {
        lines.push(Line::from(Span::styled(
            "Healthy",
            Style::default().fg(SUCCESS).add_modifier(Modifier::BOLD),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            format!("{} issue(s)", issues.len()),
            Style::default().fg(WARNING).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
        for severity in [Severity::High, Severity::Medium, Severity::Low] {
            let matching: Vec<&Issue> = issues.iter().filter(|i| i.severity == severity).collect();
            if matching.is_empty() {
                continue;
            }
            lines.push(Line::from(Span::styled(
                severity.label(),
                severity.style().add_modifier(Modifier::BOLD),
            )));
            for issue in matching {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::raw(issue.message.clone()),
                ]));
            }
        }
    }
    if let Some(doctor) = &model.doctor {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!(
                "{} plugins  ·  {} harnesses  ·  {:?} store",
                doctor.plugins.len(),
                doctor.harnesses.len(),
                doctor.store
            ),
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

// --- Overlays -------------------------------------------------------------

fn render_help(frame: &mut ratatui::Frame<'_>, area: Rect) {
    render_modal(
        frame,
        area,
        "Help",
        vec![
            Line::from("↑↓ / j k     Navigate"),
            Line::from("Tab          Switch focus (sidebar ↔ content)"),
            Line::from("Enter        Open / Inspect"),
            Line::from("Mouse click  Select sidebar route or list row"),
            Line::from("Scroll       Move selection"),
            Line::from("r            Remove plugin (Plugins)"),
            Line::from("u            Update plugin (Plugins, when available)"),
            Line::from("i            Install plugin (Marketplace)"),
            Line::from("a / p        Analyze / Apply (Context)"),
            Line::from("s            Setup harness (Harnesses)"),
            Line::from("g            Refresh"),
            Line::from("q            Quit"),
            Line::from(""),
            Line::from(Span::styled("any key to close", Style::default().fg(MUTED))),
        ],
        ACCENT,
    );
}

fn render_confirm_remove(frame: &mut ratatui::Frame<'_>, area: Rect, id: &str) {
    render_modal(
        frame,
        area,
        "Remove plugin?",
        vec![
            Line::from(vec![
                Span::raw("Remove "),
                Span::styled(
                    id.to_owned(),
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

fn render_confirm_update(frame: &mut ratatui::Frame<'_>, area: Rect, id: &str) {
    render_modal(
        frame,
        area,
        "Update plugin?",
        vec![
            Line::from(vec![
                Span::raw("Update "),
                Span::styled(
                    id.to_owned(),
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" to the latest marketplace revision?"),
            ]),
            Line::from(Span::styled(
                "enter/y update · esc/n cancel",
                Style::default().fg(MUTED),
            )),
        ],
        WARNING,
    );
}

fn render_confirm_install(frame: &mut ratatui::Frame<'_>, area: Rect, name: &str) {
    render_modal(
        frame,
        area,
        "Install plugin?",
        vec![
            Line::from(vec![
                Span::raw("Install "),
                Span::styled(
                    name.to_owned(),
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" from the official marketplace?"),
            ]),
            Line::from(Span::styled(
                "enter/y install · esc/n cancel",
                Style::default().fg(MUTED),
            )),
        ],
        ACCENT,
    );
}

fn render_confirm_context_apply(frame: &mut ratatui::Frame<'_>, area: Rect) {
    render_modal(
        frame,
        area,
        "Apply context changes?",
        vec![
            Line::from("This reconciles AGENTS.md and its harness bridges."),
            Line::from(Span::styled(
                "enter/y apply · esc/n cancel",
                Style::default().fg(MUTED),
            )),
        ],
        WARNING,
    );
}

fn render_trust_required(frame: &mut ratatui::Frame<'_>, area: Rect, plugin: &str, detail: &str) {
    render_modal(
        frame,
        area,
        "Trust required",
        vec![
            Line::from(vec![
                Span::styled(
                    plugin.to_owned(),
                    Style::default().fg(WARNING).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" declares an executable capability that was not previously trusted:"),
            ]),
            Line::from(Span::styled(detail.to_owned(), Style::default().fg(MUTED))),
            Line::from(""),
            Line::from(Span::styled(
                "enter/y trust and continue · esc/n cancel",
                Style::default().fg(MUTED),
            )),
        ],
        WARNING,
    );
}

fn render_modal(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    title: &str,
    lines: Vec<Line<'static>>,
    color: Color,
) {
    let width = area.width.min(76);
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

// --- Shared helpers ---------------------------------------------------------

fn footer(model: &TuiModel) -> Text<'static> {
    let hint = match model.overlay {
        Overlay::None => match model.focus {
            Focus::Sidebar => "↑↓/jk select route · enter/tab open · ? help · q quit",
            _ => route_hint(model.route),
        },
        _ => "enter/y confirm · esc/n cancel",
    };
    match &model.status {
        Status::Idle => Text::from(Line::from(Span::styled(hint, Style::default().fg(MUTED)))),
        Status::Working(value) => Text::from(vec![
            Line::from(Span::styled(
                value.clone(),
                Style::default().fg(WARNING).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(hint, Style::default().fg(MUTED))),
        ]),
        Status::Success(value) => Text::from(vec![
            Line::from(Span::styled(
                value.clone(),
                Style::default().fg(SUCCESS).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(hint, Style::default().fg(MUTED))),
        ]),
        Status::Error(value) => Text::from(vec![
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

fn package_strategy(plan: &crate::exposure::PackageExposurePlan) -> &'static str {
    match plan.route {
        crate::router::CompatibilityRoute::Native => "native",
        crate::router::CompatibilityRoute::Adaptable => "adapted",
        crate::router::CompatibilityRoute::Degraded => "degraded",
        crate::router::CompatibilityRoute::Unsupported => "unsupported",
    }
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

// --- Doctor severity classification -----------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
enum Severity {
    High,
    Medium,
    Low,
}

impl Severity {
    fn label(self) -> &'static str {
        match self {
            Severity::High => "High",
            Severity::Medium => "Medium",
            Severity::Low => "Low",
        }
    }

    fn style(self) -> Style {
        match self {
            Severity::High => Style::default().fg(DANGER),
            Severity::Medium => Style::default().fg(WARNING),
            Severity::Low => Style::default().fg(MUTED),
        }
    }
}

#[derive(Clone, Debug)]
struct Issue {
    severity: Severity,
    message: String,
}

/// Classifies `DoctorReport` findings into actionable severities. Deliberately
/// conservative about what counts as "verified": a matched receipt means
/// configuration agrees with what UZE expects, not that the harness has been
/// observed to actually work — see `docs/architecture/invariants.md`.
fn classify_doctor(doctor: Option<&DoctorReport>) -> Vec<Issue> {
    let Some(doctor) = doctor else {
        return Vec::new();
    };
    let mut issues = Vec::new();
    if let Some(error) = &doctor.ledger_error {
        issues.push(Issue {
            severity: Severity::High,
            message: format!("Attachment ledger is unreadable: {error}"),
        });
    }
    if let Some(error) = &doctor.integration_state_error {
        issues.push(Issue {
            severity: Severity::High,
            message: format!("Integration state is unreadable: {error}"),
        });
    }
    if let Some(error) = &doctor.provisioning_state_error {
        issues.push(Issue {
            severity: Severity::High,
            message: format!("Provisioning state is unreadable: {error}"),
        });
    }
    for package in &doctor.attachments {
        let state = &package.state;
        if state.conflicts > 0 || state.blocked > 0 {
            issues.push(Issue {
                severity: Severity::High,
                message: format!(
                    "{}: {} conflict(s), {} blocked — needs manual resolution",
                    package.plugin, state.conflicts, state.blocked
                ),
            });
        }
        if state.drifted > 0 {
            issues.push(Issue {
                severity: Severity::Medium,
                message: format!(
                    "{}: {} attachment(s) drifted from what UZE expects",
                    package.plugin, state.drifted
                ),
            });
        }
        if state.missing > 0 {
            issues.push(Issue {
                severity: Severity::Low,
                message: format!(
                    "{}: {} attachment(s) missing",
                    package.plugin, state.missing
                ),
            });
        }
    }
    for harness in &doctor.harnesses {
        if harness.detection.present && harness.setup.contains("not configured") {
            issues.push(Issue {
                severity: Severity::Medium,
                message: format!(
                    "{} is installed but not configured — run setup",
                    harness.integration
                ),
            });
        }
    }
    for plugin in &doctor.plugins {
        if plugin.update_available == Some(true) {
            issues.push(Issue {
                severity: Severity::Low,
                message: format!("{}: update available", plugin.id),
            });
        }
    }
    issues.sort_by_key(|issue| issue.severity);
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(matches!(model.overlay, Overlay::ConfirmRemove(ref id) if id == "one"));
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
        model.overlay = Overlay::ConfirmRemove("one".to_owned());
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
