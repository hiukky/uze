//! TUI — navigation, selection, and overlay state.

use std::collections::BTreeSet;
use std::path::PathBuf;

use ratatui::layout::Rect;

use crate::application::{
    ContextPlan, DoctorReport, HarnessHealth, MarketplacePluginDetail, MarketplacePluginSummary,
    OverviewWorkspaceSummary, PluginInspection, PluginSummary, ProjectContextStatus,
    ProjectEnvironmentState,
};

use super::hit::Hit;
use super::view::doctor::{Issue, classify_doctor};

// --- Routes -----------------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Route {
    Overview,
    Plugins,
    Marketplace,
    Context,
    Harnesses,
    Doctor,
}

pub(crate) const ROUTES: [Route; 6] = [
    Route::Overview,
    Route::Marketplace,
    Route::Plugins,
    Route::Context,
    Route::Harnesses,
    Route::Doctor,
];

impl Route {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Route::Overview => "Overview",
            Route::Plugins => "Plugins",
            Route::Marketplace => "Marketplace",
            Route::Context => "Context",
            Route::Harnesses => "Harnesses",
            Route::Doctor => "Doctor",
        }
    }

    pub(crate) fn index(self) -> usize {
        ROUTES.iter().position(|route| *route == self).unwrap()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Focus {
    Sidebar,
    Content,
    Overlay,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Overlay {
    None,
    Help,
    /// The Harnesses screen's own glossary — what each status/delivery/
    /// compatibility label actually means. Opened by `?` while on that
    /// route instead of the generic `Help` overlay, since the plain
    /// keybinding list has nothing to say about what "Adapted" means.
    HarnessHelp,
    ConfirmRemove {
        id: String,
        focus: usize,
    },
    ConfirmUpdate(String),
    ConfirmInstall {
        name: String,
        marketplace: String,
    },
    ConfirmContextApply,
    ProtectedPlugin(String),
    /// Free-text input, appended to on every character key and popped on
    /// backspace — see `TuiModel::overlay_key`'s `AddMarketplace` arms.
    AddMarketplace(String),
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
pub(crate) enum TrustedRetry {
    Install { name: String, marketplace: String },
    Update(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Status {
    Idle,
    Working(String),
    Success(String),
    Error(String),
}

// --- Model --------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub(crate) struct RefreshData {
    pub(crate) plugins: Vec<PluginSummary>,
    pub(crate) doctor: Option<DoctorReport>,
    pub(crate) marketplace_plugins: Vec<MarketplacePluginSummary>,
    pub(crate) marketplace_count: usize,
    pub(crate) context_status: Option<ProjectContextStatus>,
    /// The Overview's workspace-aware read model — present from the very
    /// first refresh onward (there is always a kind, even `NoWorkspace`).
    pub(crate) workspace: Option<OverviewWorkspaceSummary>,
    /// Whether `doctor` is the full per-receipt report (`true`) or the
    /// cheap dashboard health (`false`, attachments not inspected).
    pub(crate) deep_doctor: bool,
}

pub(crate) struct TuiModel {
    pub(crate) route: Route,
    pub(crate) focus: Focus,
    pub(crate) overlay: Overlay,
    pub(crate) status: Status,

    pub(crate) plugins: Vec<PluginSummary>,
    pub(crate) plugins_selected: usize,
    pub(crate) plugin_detail: Option<PluginInspection>,

    pub(crate) marketplace_count: usize,
    pub(crate) marketplace_plugins: Vec<MarketplacePluginSummary>,
    /// An index into the *visible* (filtered, group-expanded) sequence —
    /// see `marketplace_visible_indices` — not directly into
    /// `marketplace_plugins`. Resolve through `selected_marketplace_plugin`.
    pub(crate) marketplace_selected: usize,
    pub(crate) marketplace_detail: Option<MarketplacePluginDetail>,
    /// Whether the plugin-detail drawer is currently slid into view. Opens
    /// on selection, closes on `Esc` — the list panel reclaims the full
    /// width while it's closed, mirroring the design's slide-in drawer.
    pub(crate) marketplace_drawer_open: bool,
    /// Live substring filter over plugin/marketplace name, typed while
    /// `filtering` is true (`/` in the Marketplace route).
    pub(crate) marketplace_filter: String,
    pub(crate) filtering: bool,
    /// Marketplace group names currently collapsed in the tree — absence
    /// means expanded, so a freshly registered marketplace starts open.
    pub(crate) collapsed_marketplaces: BTreeSet<String>,

    pub(crate) harnesses_selected: usize,
    pub(crate) harnesses_drawer_open: bool,

    pub(crate) plugin_drawer_open: bool,

    pub(crate) doctor: Option<DoctorReport>,

    /// Whether `doctor` carries full per-receipt attachment inspection
    /// (`true`) or the cheap dashboard health (`false`). The Doctor route
    /// upgrades itself on entry; everything else stays fast.
    pub(crate) doctor_deep: bool,

    pub(crate) context_root: PathBuf,
    pub(crate) context_status: Option<ProjectContextStatus>,
    pub(crate) context_plan: Option<ContextPlan>,

    /// The detected UZE workspace (`agents.lock`/`agents.json`), loaded on
    /// the first refresh. `None` only before the startup worker returns.
    pub(crate) workspace: Option<OverviewWorkspaceSummary>,

    /// Frame counter for spinner animation while background work is pending.
    pub(crate) tick: usize,

    /// Mouse hit targets for the frame just drawn, rebuilt every render.
    /// Kept in one place rather than recomputed ad hoc from coordinates
    /// scattered through render functions.
    pub(crate) hits: Vec<(Rect, Hit)>,
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
            marketplace_count: 0,
            marketplace_plugins: Vec::new(),
            marketplace_selected: 0,
            marketplace_detail: None,
            marketplace_drawer_open: false,
            marketplace_filter: String::new(),
            filtering: false,
            collapsed_marketplaces: BTreeSet::new(),
            harnesses_selected: 0,
            harnesses_drawer_open: false,
            plugin_drawer_open: false,
            doctor: None,
            doctor_deep: false,
            context_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            context_status: None,
            context_plan: None,
            workspace: None,
            tick: 0,
            hits: Vec::new(),
        }
    }
}

impl TuiModel {
    pub(crate) fn selected_plugin(&self) -> Option<&PluginSummary> {
        self.plugins.get(self.plugins_selected)
    }

    /// Every `marketplace_plugins` index that currently passes the live
    /// filter (case-insensitive substring of plugin or marketplace name)
    /// and belongs to a group that isn't collapsed — the single source of
    /// truth both the list renderer and selection/navigation resolve
    /// through, so a hidden row is never selectable and vice versa.
    pub(crate) fn marketplace_visible_indices(&self) -> Vec<usize> {
        let needle = self.marketplace_filter.trim().to_lowercase();
        self.marketplace_plugins
            .iter()
            .enumerate()
            .filter(|(_, plugin)| !self.collapsed_marketplaces.contains(&plugin.marketplace))
            .filter(|(_, plugin)| {
                needle.is_empty()
                    || plugin.name.to_lowercase().contains(&needle)
                    || plugin.marketplace.to_lowercase().contains(&needle)
            })
            .map(|(index, _)| index)
            .collect()
    }

    /// Resolves `marketplace_selected` (a position in the visible sequence)
    /// back to the plugin it points at.
    pub(crate) fn selected_marketplace_plugin(&self) -> Option<&MarketplacePluginSummary> {
        let raw_index = *self
            .marketplace_visible_indices()
            .get(self.marketplace_selected)?;
        self.marketplace_plugins.get(raw_index)
    }

    /// An `Intent` that fetches the currently selected marketplace plugin's
    /// detail (the drawer's RESOURCES section), or `Intent::None` if it's
    /// already cached — arrow-key navigation and mouse clicks both open the
    /// drawer without going through `open_or_act`'s Enter path, so without
    /// this they'd leave RESOURCES stuck on "loading…" for any selection
    /// that was never explicitly Entered.
    pub(crate) fn marketplace_inspect_intent(&self) -> super::worker::Intent {
        let Some(plugin) = self.selected_marketplace_plugin() else {
            return super::worker::Intent::None;
        };
        if self.marketplace_detail.as_ref().is_some_and(|detail| {
            detail.summary.name == plugin.name && detail.summary.marketplace == plugin.marketplace
        }) {
            return super::worker::Intent::None;
        }
        super::worker::Intent::InspectMarketplacePlugin {
            name: plugin.name.clone(),
            marketplace: plugin.marketplace.clone(),
        }
    }

    /// Expands/collapses one marketplace group and re-clamps the selection
    /// so it never points past the now-shorter (or longer) visible list.
    pub(crate) fn marketplace_toggle_group(&mut self, marketplace: &str) {
        if !self.collapsed_marketplaces.remove(marketplace) {
            self.collapsed_marketplaces.insert(marketplace.to_owned());
        }
        self.clamp_marketplace_selection();
    }

    fn clamp_marketplace_selection(&mut self) {
        let visible = self.marketplace_visible_indices().len();
        self.marketplace_selected = self.marketplace_selected.min(visible.saturating_sub(1));
    }

    /// Consumes one key while `filtering` is true — every printable
    /// character is appended to `marketplace_filter` rather than
    /// interpreted as a shortcut. `Enter` keeps the filter and returns to
    /// normal navigation; `Esc` clears it too.
    pub(crate) fn filter_key(&mut self, key: crossterm::event::KeyEvent) -> super::worker::Intent {
        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Enter => self.filtering = false,
            KeyCode::Esc => {
                self.filtering = false;
                self.marketplace_filter.clear();
                self.clamp_marketplace_selection();
            }
            KeyCode::Backspace => {
                self.marketplace_filter.pop();
                self.clamp_marketplace_selection();
            }
            KeyCode::Char(c) => {
                self.marketplace_filter.push(c);
                self.clamp_marketplace_selection();
            }
            _ => {}
        }
        super::worker::Intent::None
    }

    pub(crate) fn selected_harness(&self) -> Option<&HarnessHealth> {
        self.doctor
            .as_ref()
            .and_then(|doctor| doctor.harnesses.get(self.harnesses_selected))
    }

    pub(crate) fn list_len(&self) -> usize {
        match self.route {
            Route::Plugins => self.plugins.len(),
            Route::Marketplace => self.marketplace_visible_indices().len(),
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

    pub(crate) fn move_selection(&mut self, delta: isize) {
        let len = self.list_len();
        let route = self.route;
        let Some(selected) = self.selected_mut() else {
            return;
        };
        if len == 0 {
            *selected = 0;
            return;
        }
        *selected = (*selected as isize + delta).clamp(0, len as isize - 1) as usize;
        // Marketplace/Harnesses reveal their drawer as soon as something is
        // selected — matches the design's click-to-select-and-open — while
        // Plugins keeps its drawer opt-in via Enter, since opening it there
        // also kicks off an async inspect fetch.
        match route {
            Route::Marketplace => self.marketplace_drawer_open = true,
            Route::Harnesses => self.harnesses_drawer_open = true,
            _ => {}
        }
    }

    pub(crate) fn refreshed(&mut self, data: RefreshData) {
        self.plugins = data.plugins;
        self.plugins_selected = self
            .plugins_selected
            .min(self.plugins.len().saturating_sub(1));
        self.doctor = data.doctor;
        self.doctor_deep = data.deep_doctor;
        self.harnesses_selected = self.harnesses_selected.min(
            self.doctor
                .as_ref()
                .map_or(0, |d| d.harnesses.len())
                .saturating_sub(1),
        );
        self.marketplace_plugins = data.marketplace_plugins;
        self.marketplace_count = data.marketplace_count;
        self.clamp_marketplace_selection();
        if data.context_status.is_some() {
            self.context_status = data.context_status;
        }
        if data.workspace.is_some() {
            self.workspace = data.workspace;
        }
        self.status = Status::Idle;
    }

    /// The path the workspace-aware read models resolve against: the
    /// detected workspace root when there is one, else the cwd (which
    /// `NoWorkspace`'s summary itself canonicalizes).
    pub(crate) fn workspace_root(&self) -> PathBuf {
        self.workspace
            .as_ref()
            .map(|workspace| workspace.root.clone())
            .unwrap_or_else(|| self.context_root.clone())
    }

    /// `Some(root)` exactly when the Application reports the project
    /// environment as `InstallRequired` — the only state the Overview may
    /// offer `i install` in. The state is the Application's verdict, never
    /// re-derived here from lock bytes.
    pub(crate) fn overview_install_path(&self) -> Option<PathBuf> {
        let workspace = self.workspace.as_ref()?;
        if workspace.project.environment == ProjectEnvironmentState::InstallRequired {
            Some(workspace.root.clone())
        } else {
            None
        }
    }

    pub(crate) fn issues(&self) -> Vec<Issue> {
        classify_doctor(self.doctor.as_ref())
    }

    pub(crate) fn set_route(&mut self, route: Route) {
        if route != Route::Marketplace {
            self.filtering = false;
        }
        // Harnesses opens straight onto its first entry's detail — the list
        // is short and every row *is* the point of the screen, unlike
        // Marketplace/Plugins, which need typing/browsing before a
        // selection means anything.
        if route == Route::Harnesses {
            self.harnesses_selected = 0;
            self.harnesses_drawer_open = true;
        }
        self.route = route;
    }

    /// Whether switching to `route` needs the full, vendor-inspecting
    /// `doctor()` — only the Doctor route, and only while the cached
    /// report is still the cheap dashboard health.
    pub(crate) fn route_change_needs_deep_health(&self, route: Route) -> bool {
        route == Route::Doctor && !self.doctor_deep
    }

    /// Depth of `doctor()` an ordinary refresh should request: deep only
    /// while the Doctor screen is the one being watched (it already shows
    /// `r refresh`), shallow everywhere else.
    pub(crate) fn refresh_depth(&self) -> bool {
        self.route == Route::Doctor
    }
}
