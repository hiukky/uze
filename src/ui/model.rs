//! TUI — navigation, selection, and overlay state.

use std::collections::BTreeSet;
use std::{path::PathBuf, time::Instant};

use ratatui::layout::Rect;
use uze_core::preference::{Autonomy, ModelPreference, SandboxScope};
use uze_extensions::registry::BuiltinExtension;

use crate::application::{
    ContextPlan, DoctorReport, HarnessHealth, MarketplacePluginDetail, MarketplacePluginSummary,
    OverviewWorkspaceSummary, PluginInspection, PluginSummary, ProfileApplyResult, ProfileSummary,
    ProjectContextStatus, ProjectEnvironmentState,
};

use super::hit::Hit;
use super::view::health::{Alert, actionable_alerts};

// --- Routes -----------------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Route {
    Overview,
    /// The agentic side of the product: skills, agents, MCP — everything
    /// installable from a marketplace (the embedded `uze-official` snapshot
    /// included). Browse the catalog, install, update, remove.
    Plugins,
    /// The tool side: official uze extensions that extend the TUI/CLI
    /// itself (see `uze_extensions::BUILTIN_EXTENSIONS`) — as opposed to
    /// plugins, which are packages delivered *to* harnesses.
    Extensions,
    Harnesses,
    Profiles,
}

pub(crate) const ROUTES: [Route; 5] = [
    Route::Overview,
    Route::Plugins,
    Route::Extensions,
    Route::Harnesses,
    Route::Profiles,
];

impl Route {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Route::Overview => "Overview",
            Route::Plugins => "Plugins",
            Route::Extensions => "Extensions",
            Route::Harnesses => "Integrations",
            Route::Profiles => "Profiles",
        }
    }

    pub(crate) fn index(self) -> usize {
        ROUTES.iter().position(|route| *route == self).unwrap()
    }
}

/// Which of the Profiles screen's three panels currently has the arrow keys,
/// cycled by Tab/Shift+Tab while that route is focused — there is no
/// existing intra-content multi-panel focus mechanism elsewhere in the TUI
/// to reuse, since every other route is a single list (plus an optional
/// slide-in drawer).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProfilePanel {
    List,
    Editor,
    Harnesses,
}

/// A content-level divider in the Manage UI. These are deliberately kept
/// separate from the shared sidebar width: a resize only changes the panel
/// relationship within its current route.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ResizablePanel {
    MarketplaceDrawer,
    ExtensionDrawer,
    HarnessDrawer,
    ProfileColumns,
}

impl ProfilePanel {
    pub(crate) fn next(self) -> Self {
        match self {
            Self::List => Self::Editor,
            Self::Editor => Self::Harnesses,
            Self::Harnesses => Self::List,
        }
    }

    pub(crate) fn prev(self) -> Self {
        match self {
            Self::List => Self::Harnesses,
            Self::Editor => Self::List,
            Self::Harnesses => Self::Editor,
        }
    }
}

/// Number of rows in the Preferences editor panel (autonomy/sandbox/model) —
/// the v1 preference set is deliberately this small; see the domain model's
/// own doc comment for why `network`/`confirmations` aren't separate rows.
pub(crate) const PREFERENCE_ROW_COUNT: usize = 3;

fn cycle<T: Copy + PartialEq>(order: &[T], current: T, forward: bool) -> T {
    let index = order
        .iter()
        .position(|value| *value == current)
        .unwrap_or(0);
    let len = order.len();
    let next = if forward {
        (index + 1) % len
    } else {
        (index + len - 1) % len
    };
    order[next]
}

fn cycle_autonomy(current: Autonomy, forward: bool) -> Autonomy {
    const ORDER: [Autonomy; 4] = [
        Autonomy::Manual,
        Autonomy::Balanced,
        Autonomy::Auto,
        Autonomy::Unattended,
    ];
    cycle(&ORDER, current, forward)
}

fn cycle_sandbox(current: SandboxScope, forward: bool) -> SandboxScope {
    const ORDER: [SandboxScope; 3] = [
        SandboxScope::ReadOnly,
        SandboxScope::WorkspaceWrite,
        SandboxScope::FullAccess,
    ];
    cycle(&ORDER, current, forward)
}

fn cycle_model(current: ModelPreference, forward: bool) -> ModelPreference {
    const ORDER: [ModelPreference; 3] = [
        ModelPreference::Default,
        ModelPreference::Fast,
        ModelPreference::Capable,
    ];
    cycle(&ORDER, current, forward)
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
    /// A new profile's id, typed the same way as `AddMarketplace`.
    NewProfile(String),
    /// Mirrors `ConfirmRemove` exactly, as its own variant rather than an
    /// overload — `ConfirmRemove` is plugin-specific today.
    ConfirmDeleteProfile {
        id: String,
        focus: usize,
    },
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
    pub(crate) profiles: Vec<ProfileSummary>,
    pub(crate) context_status: Option<ProjectContextStatus>,
    /// The Overview's workspace-aware read model — present from the very
    /// first refresh onward (there is always a kind, even `NoWorkspace`).
    pub(crate) workspace: Option<OverviewWorkspaceSummary>,
}

pub(crate) struct TuiModel {
    pub(crate) route: Route,
    pub(crate) focus: Focus,
    pub(crate) overlay: Overlay,
    pub(crate) status: Status,
    pub(crate) status_expires_at: Option<Instant>,
    /// At most one health/maintenance worker is allowed at a time. Refresh
    /// intents while it runs are deliberately coalesced rather than spawning
    /// competing inspections against the same receipt ledger.
    pub(crate) maintenance_in_flight: bool,

    pub(crate) plugins: Vec<PluginSummary>,
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
    /// `filtering` is true (`/` in the Plugins route).
    pub(crate) marketplace_filter: String,
    pub(crate) filtering: bool,
    /// Marketplace group names currently collapsed in the tree — absence
    /// means expanded, so a freshly registered marketplace starts open.
    pub(crate) collapsed_marketplaces: BTreeSet<String>,

    /// The official uze extensions catalog, from
    /// `uze_extensions::registry::ExtensionRegistry`.
    pub(crate) extensions: Vec<BuiltinExtension>,
    /// Live substring filter over extension metadata, typed with `/` while
    /// the Extensions route is focused.
    pub(crate) extension_filter: String,
    /// Position within `extension_visible_indices`, rather than a raw catalog
    /// index, so filtered cards and keyboard navigation always agree.
    pub(crate) extensions_selected: usize,
    /// Whether the Extensions detail drawer is currently slid into view.
    pub(crate) extension_drawer_open: bool,

    pub(crate) harnesses_selected: usize,
    pub(crate) harnesses_drawer_open: bool,

    pub(crate) profiles: Vec<ProfileSummary>,
    pub(crate) profiles_selected: usize,
    pub(crate) profile_panel: ProfilePanel,
    pub(crate) profile_editor_selected: usize,
    pub(crate) profile_harness_selected: usize,
    /// Harness ids to apply the selected profile to. Session-only — never
    /// persisted as part of the `Profile` domain object (v1 scope: profiles
    /// hold only preferences).
    pub(crate) profile_harness_selection: BTreeSet<String>,
    /// Whether `profile_harness_selection` has received its one-time default
    /// (every currently detected harness) — set once real `doctor` data is
    /// available, so entering the route before the first refresh completes
    /// doesn't lock in an empty selection.
    pub(crate) profile_harness_defaulted: bool,
    /// The last `apply` action's per-harness outcomes, shown as a one-word
    /// badge next to each harness row. Empty (no badges) until an apply has
    /// actually run this session.
    pub(crate) profile_apply_results: Vec<ProfileApplyResult>,

    pub(crate) doctor: Option<DoctorReport>,

    pub(crate) context_root: PathBuf,
    pub(crate) context_status: Option<ProjectContextStatus>,
    pub(crate) context_plan: Option<ContextPlan>,

    /// The detected UZE workspace (`agents.lock`/`marketplace.json`), loaded on
    /// the first refresh. `None` only before the startup worker returns.
    pub(crate) workspace: Option<OverviewWorkspaceSummary>,

    /// Frame counter for spinner animation while background work is pending.
    pub(crate) tick: usize,

    /// Mouse hit targets for the frame just drawn, rebuilt every render.
    /// Kept in one place rather than recomputed ad hoc from coordinates
    /// scattered through render functions.
    pub(crate) hits: Vec<(Rect, Hit)>,

    /// User-dragged sidebar width; `None` falls back to the responsive
    /// default (see `super::sidebar_width_for`). Mirrors the workspace
    /// TUI's `WorkspaceModel::sidebar_width` — same field, same meaning,
    /// same resize bounds, so the two sidebars feel identical to drag.
    pub(crate) sidebar_width: Option<u16>,
    pub(crate) dragging_sidebar: bool,
    pub(crate) marketplace_drawer_width: Option<u16>,
    pub(crate) extension_drawer_width: Option<u16>,
    pub(crate) harness_drawer_width: Option<u16>,
    pub(crate) profile_columns_width: Option<u16>,
    pub(crate) dragging_panel: Option<ResizablePanel>,
}

impl Default for TuiModel {
    fn default() -> Self {
        Self {
            route: Route::Overview,
            focus: Focus::Sidebar,
            overlay: Overlay::None,
            status: Status::Idle,
            status_expires_at: None,
            maintenance_in_flight: false,
            plugins: Vec::new(),
            plugin_detail: None,
            marketplace_count: 0,
            marketplace_plugins: Vec::new(),
            marketplace_selected: 0,
            marketplace_detail: None,
            marketplace_drawer_open: false,
            marketplace_filter: String::new(),
            filtering: false,
            collapsed_marketplaces: BTreeSet::new(),
            extensions: uze_extensions::registry::ExtensionRegistry::builtin()
                .all()
                .to_vec(),
            extension_filter: String::new(),
            extensions_selected: 0,
            extension_drawer_open: false,
            harnesses_selected: 0,
            harnesses_drawer_open: false,
            profiles: Vec::new(),
            profiles_selected: 0,
            profile_panel: ProfilePanel::List,
            profile_editor_selected: 0,
            profile_harness_selected: 0,
            profile_harness_selection: BTreeSet::new(),
            profile_harness_defaulted: false,
            profile_apply_results: Vec::new(),
            doctor: None,
            context_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            context_status: None,
            context_plan: None,
            workspace: None,
            tick: 0,
            hits: Vec::new(),
            sidebar_width: None,
            dragging_sidebar: false,
            marketplace_drawer_width: None,
            extension_drawer_width: None,
            harness_drawer_width: None,
            profile_columns_width: None,
            dragging_panel: None,
        }
    }
}

impl TuiModel {
    pub(crate) fn expire_status(&mut self) {
        if self
            .status_expires_at
            .is_some_and(|expires| Instant::now() >= expires)
        {
            self.status = Status::Idle;
            self.status_expires_at = None;
        }
    }
    /// Installed plugins from `plugins` that no catalog entry knows about
    /// (ad-hoc `uze add`/git/local installs) — the merged Plugins tree's
    /// "local" group, so a direct install never disappears from the TUI
    /// when the catalog screen absorbs the old installed list.
    fn local_marketplace_rows(&self) -> Vec<MarketplacePluginSummary> {
        self.plugins
            .iter()
            .filter(|plugin| {
                !self
                    .marketplace_plugins
                    .iter()
                    .any(|m| format!("{}@{}", m.name, m.marketplace) == plugin.id)
            })
            .map(|plugin| MarketplacePluginSummary {
                marketplace: "local".to_owned(),
                name: plugin.active_name.clone(),
                description: None,
                keywords: Vec::new(),
                installed: true,
                update_available: plugin.update_available,
                is_default: false,
            })
            .collect()
    }

    /// Every row the Plugins tree renders: the marketplace catalog
    /// (official snapshot first, then registered marketplaces) followed by
    /// the "local" group of ad-hoc installed plugins. All tree logic
    /// (visible indices, selection, rendering) resolves through this one
    /// list so the local group is a group like any other.
    pub(crate) fn marketplace_rows(&self) -> Vec<MarketplacePluginSummary> {
        let mut rows = self.marketplace_plugins.clone();
        rows.extend(self.local_marketplace_rows());
        rows
    }

    /// The qualified remove/inspect id for a merged-tree row: `name@market`
    /// as usual, except local rows, whose real identity lives on the
    /// installed `PluginSummary` (a path or Git URL), never a
    /// marketplace-qualified string.
    pub(crate) fn marketplace_plugin_id(&self, plugin: &MarketplacePluginSummary) -> String {
        if plugin.marketplace != "local" {
            return format!("{}@{}", plugin.name, plugin.marketplace);
        }
        self.plugins
            .iter()
            .find(|p| p.active_name == plugin.name)
            .map(|p| p.id.clone())
            .unwrap_or_else(|| plugin.name.clone())
    }

    /// Every `marketplace_rows` index that currently passes the live
    /// filter (case-insensitive substring of plugin or marketplace name)
    /// and belongs to a group that isn't collapsed — the single source of
    /// truth both the list renderer and selection/navigation resolve
    /// through, so a hidden row is never selectable and vice versa.
    pub(crate) fn marketplace_visible_indices(&self) -> Vec<usize> {
        let needle = self.marketplace_filter.trim().to_lowercase();
        self.marketplace_rows()
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
    /// back to the plugin it points at — an owned clone, since the merged
    /// row list is computed on demand (`marketplace_rows`).
    pub(crate) fn selected_marketplace_plugin(&self) -> Option<MarketplacePluginSummary> {
        let raw_index = *self
            .marketplace_visible_indices()
            .get(self.marketplace_selected)?;
        self.marketplace_rows().get(raw_index).cloned()
    }

    pub(crate) fn selected_extension(&self) -> Option<&BuiltinExtension> {
        self.extensions.get(
            *self
                .extension_visible_indices()
                .get(self.extensions_selected)?,
        )
    }

    pub(crate) fn extension_visible_indices(&self) -> Vec<usize> {
        let needle = self.extension_filter.trim().to_lowercase();
        self.extensions
            .iter()
            .enumerate()
            .filter(|(_, extension)| {
                needle.is_empty()
                    || extension.id.to_lowercase().contains(&needle)
                    || extension.name.to_lowercase().contains(&needle)
                    || extension.description.to_lowercase().contains(&needle)
                    || extension.surface.to_lowercase().contains(&needle)
            })
            .map(|(index, _)| index)
            .collect()
    }

    /// An `Intent` that fetches the currently selected row's detail (the
    /// drawer's RESOURCES/deliveries sections), or `Intent::None` if it's
    /// already cached — arrow-key navigation and mouse clicks both open the
    /// drawer without going through `open_or_act`'s Enter path, so without
    /// this they'd leave the drawer's body stuck on "loading…" for any
    /// selection that was never explicitly Entered.
    pub(crate) fn marketplace_inspect_intent(&self) -> super::worker::Intent {
        let Some(plugin) = self.selected_marketplace_plugin() else {
            return super::worker::Intent::None;
        };
        let id = self.marketplace_plugin_id(&plugin);
        if plugin.installed {
            // Installed rows read deliveries/managed state — that model
            // comes from the installed-package inspection, not the catalog.
            if self
                .plugin_detail
                .as_ref()
                .is_some_and(|detail| detail.plugin.id == id)
            {
                return super::worker::Intent::None;
            }
            return super::worker::Intent::InspectPlugin(id);
        }
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

    fn clamp_extension_selection(&mut self) {
        self.extensions_selected = self
            .extensions_selected
            .min(self.extension_visible_indices().len().saturating_sub(1));
    }

    /// Consumes one key while `filtering` is true — every printable
    /// character is appended to the active route's filter rather than
    /// interpreted as a shortcut. `Enter` keeps the filter and returns to
    /// normal navigation; `Esc` clears it too.
    pub(crate) fn filter_key(&mut self, key: crossterm::event::KeyEvent) -> super::worker::Intent {
        use crossterm::event::KeyCode;
        match key.code {
            KeyCode::Enter => self.filtering = false,
            KeyCode::Esc => {
                self.filtering = false;
                match self.route {
                    Route::Plugins => {
                        self.marketplace_filter.clear();
                        self.clamp_marketplace_selection();
                    }
                    Route::Extensions => {
                        self.extension_filter.clear();
                        self.clamp_extension_selection();
                    }
                    _ => {}
                }
            }
            KeyCode::Backspace => match self.route {
                Route::Plugins => {
                    self.marketplace_filter.pop();
                    self.clamp_marketplace_selection();
                }
                Route::Extensions => {
                    self.extension_filter.pop();
                    self.clamp_extension_selection();
                }
                _ => {}
            },
            KeyCode::Char(c) => match self.route {
                Route::Plugins => {
                    self.marketplace_filter.push(c);
                    self.clamp_marketplace_selection();
                }
                Route::Extensions => {
                    self.extension_filter.push(c);
                    self.clamp_extension_selection();
                }
                _ => {}
            },
            _ => {}
        }
        super::worker::Intent::None
    }

    pub(crate) fn selected_harness(&self) -> Option<&HarnessHealth> {
        self.doctor
            .as_ref()
            .and_then(|doctor| doctor.harnesses.get(self.harnesses_selected))
    }

    pub(crate) fn selected_profile(&self) -> Option<&ProfileSummary> {
        self.profiles.get(self.profiles_selected)
    }

    /// Profiles has three independently-scrolled sub-panels rather than one
    /// list, so it bypasses the generic `move_selection`/`list_len`/
    /// `selected_mut` dispatch (designed for exactly one selection per
    /// route) and clamps whichever panel is currently focused.
    pub(crate) fn move_profile_selection(&mut self, delta: isize) {
        let clamp = |current: usize, len: usize| -> usize {
            if len == 0 {
                0
            } else {
                (current as isize + delta).clamp(0, len as isize - 1) as usize
            }
        };
        match self.profile_panel {
            ProfilePanel::List => {
                self.profiles_selected = clamp(self.profiles_selected, self.profiles.len());
            }
            ProfilePanel::Editor => {
                self.profile_editor_selected =
                    clamp(self.profile_editor_selected, PREFERENCE_ROW_COUNT);
            }
            ProfilePanel::Harnesses => {
                let len = self.doctor.as_ref().map_or(0, |d| d.harnesses.len());
                self.profile_harness_selected = clamp(self.profile_harness_selected, len);
            }
        }
    }

    /// Cycles the Editor panel's currently-highlighted preference value and
    /// returns the `Intent` that persists it. Mutates `self.profiles`
    /// optimistically so the row reflects the new value immediately, without
    /// waiting on the (silent, fire-and-forget) background write.
    pub(crate) fn cycle_selected_preference(&mut self, forward: bool) -> super::worker::Intent {
        let Some(profile) = self.profiles.get_mut(self.profiles_selected) else {
            return super::worker::Intent::None;
        };
        match self.profile_editor_selected {
            0 => {
                profile.preferences.autonomy = cycle_autonomy(profile.preferences.autonomy, forward)
            }
            1 => profile.preferences.sandbox = cycle_sandbox(profile.preferences.sandbox, forward),
            2 => profile.preferences.model = cycle_model(profile.preferences.model, forward),
            _ => return super::worker::Intent::None,
        }
        super::worker::Intent::UpdatePreferences {
            id: profile.id.clone(),
            preferences: profile.preferences,
        }
    }

    /// Toggles one harness's inclusion in the apply target set, by its
    /// position in `doctor.harnesses` (the Harnesses panel's row index).
    pub(crate) fn toggle_profile_harness_at(&mut self, index: usize) {
        let Some(id) = self
            .doctor
            .as_ref()
            .and_then(|doctor| doctor.harnesses.get(index))
            .map(|harness| harness.integration.clone())
        else {
            return;
        };
        if !self.profile_harness_selection.remove(&id) {
            self.profile_harness_selection.insert(id);
        }
    }

    pub(crate) fn list_len(&self) -> usize {
        match self.route {
            Route::Plugins => self.marketplace_visible_indices().len(),
            Route::Extensions => self.extension_visible_indices().len(),
            Route::Harnesses => self.doctor.as_ref().map_or(0, |d| d.harnesses.len()),
            _ => 0,
        }
    }

    fn selected_mut(&mut self) -> Option<&mut usize> {
        match self.route {
            Route::Plugins => Some(&mut self.marketplace_selected),
            Route::Extensions => Some(&mut self.extensions_selected),
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
        // Plugins/Harnesses reveal their drawer as soon as something is
        // selected — matches the design's click-to-select-and-open (the
        // Plugins drawer is bookended by install/update/remove, so a
        // selection there always has an action in reach). Extensions'
        // drawer is static catalog detail, opened the same way.
        match route {
            Route::Plugins => self.marketplace_drawer_open = true,
            Route::Extensions => self.extension_drawer_open = true,
            Route::Harnesses => self.harnesses_drawer_open = true,
            _ => {}
        }
    }

    pub(crate) fn refreshed(&mut self, data: RefreshData) {
        self.plugins = data.plugins;
        self.doctor = data.doctor;
        self.harnesses_selected = self.harnesses_selected.min(
            self.doctor
                .as_ref()
                .map_or(0, |d| d.harnesses.len())
                .saturating_sub(1),
        );
        self.marketplace_plugins = data.marketplace_plugins;
        self.marketplace_count = data.marketplace_count;
        self.clamp_marketplace_selection();
        self.clamp_extension_selection();
        self.profiles = data.profiles;
        self.profiles_selected = self
            .profiles_selected
            .min(self.profiles.len().saturating_sub(1));
        self.profile_harness_selected = self.profile_harness_selected.min(
            self.doctor
                .as_ref()
                .map_or(0, |d| d.harnesses.len())
                .saturating_sub(1),
        );
        if !self.profile_harness_defaulted
            && let Some(doctor) = &self.doctor
        {
            self.profile_harness_selection = doctor
                .harnesses
                .iter()
                .filter(|harness| harness.detection.present)
                .map(|harness| harness.integration.clone())
                .collect();
            self.profile_harness_defaulted = true;
        }
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

    pub(crate) fn alerts(&self) -> Vec<Alert> {
        actionable_alerts(self.doctor.as_ref())
    }

    pub(crate) fn set_route(&mut self, route: Route) {
        self.filtering = false;
        // Harnesses opens straight onto its first entry's detail — the list
        // is short and every row *is* the point of the screen, unlike
        // Marketplace/Plugins, which need typing/browsing before a
        // selection means anything.
        if route == Route::Harnesses {
            self.harnesses_selected = 0;
            self.harnesses_drawer_open = true;
        }
        if route == Route::Profiles {
            self.profile_panel = ProfilePanel::List;
        }
        self.route = route;
    }
}
