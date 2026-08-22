//! TUI — navigation, selection, and overlay state.

use std::path::PathBuf;

use ratatui::layout::Rect;

use crate::application::{
    ContextPlan, DoctorReport, HarnessHealth, MarketplacePluginDetail, MarketplacePluginSummary,
    PluginInspection, PluginSummary, ProjectContextStatus,
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
    ConfirmRemove {
        id: String,
        focus: usize,
    },
    ConfirmUpdate(String),
    ConfirmInstall(String),
    ConfirmContextApply,
    ProtectedPlugin(String),
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
    Install(String),
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
    pub(crate) marketplace_name: String,
    pub(crate) context_status: Option<ProjectContextStatus>,
}

pub(crate) struct TuiModel {
    pub(crate) route: Route,
    pub(crate) focus: Focus,
    pub(crate) overlay: Overlay,
    pub(crate) status: Status,

    pub(crate) plugins: Vec<PluginSummary>,
    pub(crate) plugins_selected: usize,
    pub(crate) plugin_detail: Option<PluginInspection>,

    pub(crate) marketplace_name: String,
    pub(crate) marketplace_plugins: Vec<MarketplacePluginSummary>,
    pub(crate) marketplace_selected: usize,
    pub(crate) marketplace_detail: Option<MarketplacePluginDetail>,

    pub(crate) harnesses_selected: usize,

    pub(crate) doctor: Option<DoctorReport>,

    pub(crate) context_root: PathBuf,
    pub(crate) context_status: Option<ProjectContextStatus>,
    pub(crate) context_plan: Option<ContextPlan>,

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
            marketplace_name: String::new(),
            marketplace_plugins: Vec::new(),
            marketplace_selected: 0,
            marketplace_detail: None,
            harnesses_selected: 0,
            doctor: None,
            context_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            context_status: None,
            context_plan: None,
            tick: 0,
            hits: Vec::new(),
        }
    }
}

impl TuiModel {
    pub(crate) fn selected_plugin(&self) -> Option<&PluginSummary> {
        self.plugins.get(self.plugins_selected)
    }

    pub(crate) fn selected_marketplace_plugin(&self) -> Option<&MarketplacePluginSummary> {
        self.marketplace_plugins.get(self.marketplace_selected)
    }

    pub(crate) fn selected_harness(&self) -> Option<&HarnessHealth> {
        self.doctor
            .as_ref()
            .and_then(|doctor| doctor.harnesses.get(self.harnesses_selected))
    }

    pub(crate) fn list_len(&self) -> usize {
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

    pub(crate) fn move_selection(&mut self, delta: isize) {
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

    pub(crate) fn refreshed(&mut self, data: RefreshData) {
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

    pub(crate) fn issues(&self) -> Vec<Issue> {
        classify_doctor(self.doctor.as_ref())
    }

    pub(crate) fn set_route(&mut self, route: Route) {
        self.route = route;
    }
}
