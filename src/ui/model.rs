//! TUI — navigation, selection, and overlay state.

use std::collections::BTreeSet;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use ratatui::layout::Rect;
use uze_application::{Autonomy, ManagementLayout, ModelPreference, SandboxScope};
use uze_extensions::registry::BuiltinExtension;

use uze_application::application::offers::ActionOffer;
use uze_application::application::{
    ContextPlan, DoctorReport, HarnessHealth, MarketplacePluginDetail, MarketplacePluginSummary,
    MarketplaceSummary, OverviewWorkspaceSummary, PluginInspection, PluginSummary,
    ProfileApplyResult, ProfileSummary, ProjectContextStatus, ProjectEnvironmentState,
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
    /// The keyboard itself: every action, the key that reaches it, and
    /// whether this terminal can deliver that key at all.
    Keys,
}

pub(crate) const ROUTES: [Route; 6] = [
    Route::Overview,
    Route::Plugins,
    Route::Extensions,
    Route::Harnesses,
    Route::Profiles,
    Route::Keys,
];

impl Route {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Route::Overview => "Overview",
            Route::Plugins => "Plugins",
            Route::Extensions => "Extensions",
            Route::Harnesses => "Integrations",
            Route::Profiles => "Profiles",
            Route::Keys => "Keys",
        }
    }

    /// The badge a route carries beside its name in the sidebar, or
    /// `None` for one that is finished. The sidebar is where someone
    /// decides which screen to open, so it is where "not settled yet" has
    /// to be said — a warning found only after arriving is a warning that
    /// came too late.
    pub(crate) fn badge(self) -> Option<&'static str> {
        match self {
            Route::Profiles => Some("Beta"),
            _ => None,
        }
    }

    pub(crate) fn index(self) -> usize {
        ROUTES.iter().position(|route| *route == self).unwrap()
    }

    /// The name this route is remembered by between runs (see
    /// `ManagementLayout::route`). The user-facing one, so the file reads
    /// the way the sidebar does; stable, so a variant renamed in code
    /// does not forget where the operator was.
    pub(crate) fn id(self) -> &'static str {
        match self {
            Route::Overview => "overview",
            Route::Plugins => "plugins",
            Route::Extensions => "extensions",
            Route::Harnesses => "integrations",
            Route::Profiles => "profiles",
            Route::Keys => "keys",
        }
    }

    pub(crate) fn from_id(id: &str) -> Option<Self> {
        ROUTES.into_iter().find(|route| route.id() == id)
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
    KeysDrawer,
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

/// One rebindable line of the Keys screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct KeyRow {
    pub(crate) scope: uze_keys::Scope,
    pub(crate) action: uze_keys::Action,
    pub(crate) chord: Option<uze_keys::Chord>,
    pub(crate) default_chord: Option<uze_keys::Chord>,
}

impl KeyRow {
    /// Whether this line is the operator's own choice rather than what
    /// uze shipped with.
    pub(crate) fn custom(&self) -> bool {
        self.chord != self.default_chord
    }
}

/// An open row menu: the offers for one row, and which of them the
/// keyboard is on.
///
/// Only the available offers are here. A menu is a list of what can be
/// done now; the reason an action *cannot* be done belongs in the detail
/// view, where there is room to say it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RowMenu {
    pub(crate) offers: Vec<ActionOffer>,
    /// `None` when nothing is highlighted, which is how a menu whose only
    /// entry destroys something opens: reaching a destructive action is
    /// always a deliberate move, never the state the menu arrived in.
    pub(crate) selected: Option<usize>,
    /// The row's own rect — the popup anchors just under it, the same
    /// placement rule the workspace client's context menu uses.
    pub(crate) anchor: Rect,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Overlay {
    None,
    /// Everything that can be done here, each with the key that reaches
    /// it. Help and command palette are one surface because they answer
    /// the same question — building two would guarantee they disagree.
    ///
    /// Carries the scopes it was opened over: what is reachable is a
    /// question about the screen underneath, not about the index itself.
    ActionIndex {
        scopes: Vec<uze_keys::Scope>,
        filter: String,
        selected: usize,
    },
    /// The Harnesses screen's own glossary — what each status/delivery/
    /// compatibility label actually means. Reference material about what
    /// the data *means*, which is a different question from what can be
    /// done here, so it is a surface of its own rather than a section of
    /// the index.
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
    /// Deleting the workspace's recorded prompts. Destructive and not
    /// undoable, so it is confirmed like any other removal.
    ConfirmClearPromptHistory,
    /// Choosing what UZE looks like. Carries the list rather than reading
    /// it per frame: it is a directory listing, and a list that changed
    /// under the cursor between two frames would move the selection out
    /// from under the operator.
    ThemePicker {
        /// Each theme's id and whether it is the one in force. The id, not
        /// the theme's display name: a theme is selected by its file's own
        /// stem, and `dawn.json` is free to call itself anything.
        themes: Vec<(String, bool)>,
        selected: usize,
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
    pub(crate) marketplaces: Vec<MarketplaceSummary>,
    pub(crate) profiles: Vec<ProfileSummary>,
    pub(crate) context_status: Option<ProjectContextStatus>,
    /// The Overview's workspace-aware read model — present from the very
    /// first refresh onward (there is always a kind, even `NoWorkspace`).
    pub(crate) workspace: Option<OverviewWorkspaceSummary>,
    pub(crate) prompt_history: Vec<uze_application::PromptEntry>,
    /// Qualified ids of the plugins the startup worker updated on its own
    /// this session. Only ever non-empty on the one startup refresh; every
    /// later refresh reports nothing, so badges already raised are never
    /// disturbed by an ordinary reload.
    pub(crate) auto_updated: Vec<String>,
}

/// A "just updated" mark on one plugin row, raised by the startup
/// auto-update and dropped once it has had its moment on screen.
#[derive(Clone, Debug)]
pub(crate) struct UpdateBadge {
    /// The qualified plugin id (`name@marketplace`) the badge belongs to.
    pub(crate) plugin: String,
    /// When the operator first had the Plugins screen open in front of
    /// them with this badge on it. `None` until then: a badge that timed
    /// out while they were on another route would have told nobody
    /// anything, which is the whole point of raising it.
    pub(crate) seen_at: Option<Instant>,
}

/// How long an "Updated" badge stays up once it has actually been seen.
pub(crate) const UPDATE_BADGE_TTL: Duration = Duration::from_secs(10);

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

    /// Every registered marketplace, by the name its plugins carry —
    /// what the plugin drawer resolves a source link through.
    pub(crate) marketplaces: Vec<MarketplaceSummary>,
    pub(crate) marketplace_plugins: Vec<MarketplacePluginSummary>,
    /// An index into the *visible* (filtered, group-expanded) sequence —
    /// see `marketplace_visible_indices` — not directly into
    /// `marketplace_plugins`. Resolve through `selected_marketplace_plugin`.
    pub(crate) marketplace_selected: usize,
    /// The Keys screen's selected row, filter, and what it is waiting
    /// for. Capture is its own state because it is the one moment the
    /// keyboard means nothing at all: every keystroke is the answer.
    pub(crate) keys_drawer_width: Option<u16>,
    pub(crate) keys_selected: usize,
    pub(crate) keys_filter: String,
    pub(crate) keys_capture: bool,
    /// Why the last rebinding was refused, in words — a conflict, a chord
    /// that is another key, or one this terminal cannot send.
    pub(crate) keys_problem: Option<String>,
    /// What the probe last saw. No table can enumerate every emulator,
    /// multiplexer and connection; pressing a key and being told what
    /// arrived is the answer for the machine in front of you.
    pub(crate) keys_probe: Option<String>,
    /// What this terminal turned out to be able to deliver.
    pub(crate) keyboard: super::keys::KeyboardSupport,
    /// The action menu the selected row raised, if any — what can be done
    /// to that row, from the row itself. Built fresh each time it opens
    /// from the entity's own offers, never persisted.
    pub(crate) row_menu: Option<RowMenu>,
    pub(crate) marketplace_detail: Option<MarketplacePluginDetail>,
    /// Whether the plugin-detail drawer is currently slid into view. Opens
    /// on selection, closes on `Esc` — the list panel reclaims the full
    /// width while it's closed, mirroring the design's slide-in drawer.
    pub(crate) marketplace_drawer_open: bool,
    /// The inspection the worker is currently answering for the drawer,
    /// so the per-frame `drawer_inspect_intent` check cannot queue the
    /// same fetch again while it is still running. Cleared when its
    /// answer, success or failure, lands.
    pub(crate) inspection_in_flight: Option<super::worker::Intent>,
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
    pub(crate) harnesses_filter: String,

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

    /// When the state above was last resolved, or `None` while the
    /// session's first resolution is still on its way. Opening the
    /// management client reads it to decide whether it is looking at an
    /// answer or at nothing yet — see `management::RESOLUTION_STANDS_FOR`.
    pub(crate) resolved_at: Option<Instant>,

    /// Plugins updated automatically this session, badged as "Updated" on
    /// the Plugins screen until [`UPDATE_BADGE_TTL`] after the operator has
    /// actually had that screen in front of them.
    pub(crate) update_badges: Vec<UpdateBadge>,

    pub(crate) context_root: PathBuf,
    pub(crate) context_status: Option<ProjectContextStatus>,
    pub(crate) context_plan: Option<ContextPlan>,

    /// The detected UZE workspace (`agents.lock`/`marketplace.json`), loaded on
    /// the first refresh. `None` only before the startup worker returns.
    pub(crate) workspace: Option<OverviewWorkspaceSummary>,

    /// Recent prompts for the detected workspace, newest first. Read-only
    /// here: the workspace client owns writing them.
    pub(crate) prompt_history: Vec<uze_application::PromptEntry>,
    pub(crate) overview_prompt_selected: usize,
    pub(crate) overview_prompt_hovered: Option<usize>,

    /// Whether the pointer is on the plugin drawer's source address.
    /// A link in a terminal has no cursor to change shape, so the colour
    /// is the only thing that can answer the pointer — see the address's
    /// own style in `view::plugins`.
    pub(crate) source_link_hovered: bool,

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
    /// The Keys list's scroll track while it is being dragged, kept from
    /// the mousedown that armed it so every following drag maps against
    /// the geometry the gesture started on.
    pub(crate) dragging_keys_track: Option<Rect>,
    /// Whether the sidebar's first-steps section is folded to its header.
    pub(crate) first_steps_collapsed: bool,
    /// Whether it has been put away for good, which is offered only once
    /// every step has been taken.
    pub(crate) first_steps_closed: bool,
    /// The steps already taken, by action name — shared with the workspace
    /// client through `ClientLayout`, because it is one list drawn at the
    /// foot of both sidebars and a step taken in one mode is taken.
    pub(crate) steps_taken: std::collections::BTreeSet<String>,
}

impl Default for TuiModel {
    /// A model with nothing resolved, shaped as `ManagementLayout`'s own
    /// default — the drawers a screen opens with are stated once, there.
    fn default() -> Self {
        let layout = ManagementLayout::default();
        Self {
            route: Route::Overview,
            focus: Focus::Sidebar,
            overlay: Overlay::None,
            keys_drawer_width: None,
            keys_selected: 0,
            keys_filter: String::new(),
            keys_capture: false,
            keys_problem: None,
            keys_probe: None,
            keyboard: super::keys::KeyboardSupport::default(),
            row_menu: None,
            status: Status::Idle,
            status_expires_at: None,
            maintenance_in_flight: false,
            plugins: Vec::new(),
            plugin_detail: None,
            marketplaces: Vec::new(),
            marketplace_plugins: Vec::new(),
            marketplace_selected: 0,
            marketplace_detail: None,
            marketplace_drawer_open: layout.marketplace_drawer_open,
            inspection_in_flight: None,
            marketplace_filter: String::new(),
            filtering: false,
            collapsed_marketplaces: layout.collapsed_marketplaces,
            extensions: uze_extensions::registry::ExtensionRegistry::builtin()
                .all()
                .to_vec(),
            extension_filter: String::new(),
            extensions_selected: 0,
            extension_drawer_open: layout.extension_drawer_open,
            harnesses_selected: 0,
            harnesses_drawer_open: layout.harnesses_drawer_open,
            harnesses_filter: String::new(),
            profiles: Vec::new(),
            profiles_selected: 0,
            profile_panel: ProfilePanel::List,
            profile_editor_selected: 0,
            profile_harness_selected: 0,
            profile_harness_selection: BTreeSet::new(),
            profile_harness_defaulted: false,
            profile_apply_results: Vec::new(),
            doctor: None,
            resolved_at: None,
            update_badges: Vec::new(),
            context_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            context_status: None,
            context_plan: None,
            workspace: None,
            prompt_history: Vec::new(),
            overview_prompt_selected: 0,
            overview_prompt_hovered: None,
            source_link_hovered: false,
            tick: 0,
            hits: Vec::new(),
            sidebar_width: None,
            dragging_sidebar: false,
            marketplace_drawer_width: layout.marketplace_drawer_width,
            extension_drawer_width: layout.extension_drawer_width,
            harness_drawer_width: layout.harness_drawer_width,
            profile_columns_width: layout.profile_columns_width,
            dragging_panel: None,
            dragging_keys_track: None,
            first_steps_collapsed: false,
            first_steps_closed: false,
            steps_taken: std::collections::BTreeSet::new(),
        }
    }
}

/// The fields of [`TuiModel`] that outlive one visit to the management
/// client — the machine state it resolved and where in it the operator
/// was. Everything not listed here belongs to one visit: the open
/// overlay, the status line, work in flight, and per-frame transients
/// such as hit rects and the spinner tick. The screen that was open and
/// how its drawers were left are not here either: those outlive the
/// *process*, and live in the `ManagementLayout` every visit is shaped
/// from (see [`TuiModel::recall`]).
///
/// Management is entered and left every time the operator presses Ctrl+O,
/// and rebuilding a default model each time meant an empty screen — no
/// plugins, no harnesses — under a "Refreshing environment…" line, for as
/// long as a full resolution took. What the last visit resolved is still
/// the truth about the machine, so it is what the next one draws while a
/// refresh confirms it behind the frame.
pub(crate) struct Remembered {
    plugins: Vec<PluginSummary>,
    doctor: Option<DoctorReport>,
    resolved_at: Option<Instant>,
    marketplaces: Vec<MarketplaceSummary>,
    marketplace_plugins: Vec<MarketplacePluginSummary>,
    profiles: Vec<ProfileSummary>,
    context_status: Option<ProjectContextStatus>,
    context_plan: Option<ContextPlan>,
    workspace: Option<OverviewWorkspaceSummary>,
    prompt_history: Vec<uze_application::PromptEntry>,
    update_badges: Vec<UpdateBadge>,
    marketplace_selected: usize,
    extensions_selected: usize,
    harnesses_selected: usize,
    profiles_selected: usize,
    overview_prompt_selected: usize,
}

impl TuiModel {
    /// A model opening the management client shaped as `layout` says —
    /// the screen, the drawers, the folds — with what the previous visit
    /// left behind. `None` is the first visit of the process, which has
    /// nothing resolved yet and starts from the default model.
    pub(crate) fn recall(remembered: Option<Remembered>, layout: &ManagementLayout) -> Self {
        let mut model = remembered.map_or_else(Self::default, |remembered| {
            let Remembered {
                plugins,
                doctor,
                resolved_at,
                marketplaces,
                marketplace_plugins,
                profiles,
                context_status,
                context_plan,
                workspace,
                prompt_history,
                update_badges,
                marketplace_selected,
                extensions_selected,
                harnesses_selected,
                profiles_selected,
                overview_prompt_selected,
            } = remembered;
            Self {
                plugins,
                doctor,
                resolved_at,
                marketplaces,
                marketplace_plugins,
                profiles,
                context_status,
                context_plan,
                workspace,
                prompt_history,
                update_badges,
                marketplace_selected,
                extensions_selected,
                harnesses_selected,
                profiles_selected,
                overview_prompt_selected,
                ..Self::default()
            }
        });
        model.route = layout
            .route
            .as_deref()
            .and_then(Route::from_id)
            .unwrap_or(Route::Overview);
        model.marketplace_drawer_open = layout.marketplace_drawer_open;
        model.extension_drawer_open = layout.extension_drawer_open;
        model.harnesses_drawer_open = layout.harnesses_drawer_open;
        model.marketplace_drawer_width = layout.marketplace_drawer_width;
        model.extension_drawer_width = layout.extension_drawer_width;
        model.harness_drawer_width = layout.harness_drawer_width;
        model.profile_columns_width = layout.profile_columns_width;
        model.collapsed_marketplaces = layout.collapsed_marketplaces.clone();
        model
    }

    /// The shape this visit leaves the management client in, for the
    /// next visit and the next run alike.
    pub(crate) fn management_layout(&self) -> ManagementLayout {
        ManagementLayout {
            route: Some(self.route.id().to_owned()),
            marketplace_drawer_open: self.marketplace_drawer_open,
            extension_drawer_open: self.extension_drawer_open,
            harnesses_drawer_open: self.harnesses_drawer_open,
            marketplace_drawer_width: self.marketplace_drawer_width,
            extension_drawer_width: self.extension_drawer_width,
            harness_drawer_width: self.harness_drawer_width,
            profile_columns_width: self.profile_columns_width,
            collapsed_marketplaces: self.collapsed_marketplaces.clone(),
        }
    }

    /// What this visit leaves for the next one.
    pub(crate) fn remember(self) -> Remembered {
        Remembered {
            plugins: self.plugins,
            doctor: self.doctor,
            resolved_at: self.resolved_at,
            marketplaces: self.marketplaces,
            marketplace_plugins: self.marketplace_plugins,
            profiles: self.profiles,
            context_status: self.context_status,
            context_plan: self.context_plan,
            workspace: self.workspace,
            prompt_history: self.prompt_history,
            update_badges: self.update_badges,
            marketplace_selected: self.marketplace_selected,
            extensions_selected: self.extensions_selected,
            harnesses_selected: self.harnesses_selected,
            profiles_selected: self.profiles_selected,
            overview_prompt_selected: self.overview_prompt_selected,
        }
    }

    /// Says something for a few seconds and then goes quiet. For the
    /// answers that are not a result — "there is nothing here to do that
    /// to" — which are what a key must give when the screen it was pressed
    /// on has nothing for it. A key that answers nothing at all is
    /// indistinguishable from a key that is broken.
    pub(crate) fn say(&mut self, message: impl Into<String>) {
        self.status = Status::Success(message.into());
        self.status_expires_at = Some(Instant::now() + Duration::from_secs(3));
    }

    pub(crate) fn expire_status(&mut self) {
        if self
            .status_expires_at
            .is_some_and(|expires| Instant::now() >= expires)
        {
            self.status = Status::Idle;
            self.status_expires_at = None;
        }
    }

    /// Ages the "Updated" badges by one frame: any badge on screen right
    /// now starts (or continues) its countdown, and one whose time is up
    /// comes down.
    ///
    /// The countdown starts on first *sight*, not on the update itself —
    /// auto-updates land during startup, while the operator is usually
    /// still on Overview, so a badge timed from the update would routinely
    /// expire before the screen carrying it was ever opened.
    pub(crate) fn expire_update_badges(&mut self) {
        if self.update_badges.is_empty() {
            return;
        }
        let now = Instant::now();
        if self.route == Route::Plugins {
            for badge in &mut self.update_badges {
                badge.seen_at.get_or_insert(now);
            }
        }
        self.update_badges.retain(|badge| {
            badge
                .seen_at
                .is_none_or(|seen| now - seen < UPDATE_BADGE_TTL)
        });
    }

    /// Whether this plugin carries a live "Updated" badge. Takes the
    /// qualified id the tree already resolves for every row
    /// (`marketplace_plugin_id`), so a local-group row and a catalog row
    /// for the same package answer identically.
    pub(crate) fn was_just_updated(&self, plugin_id: &str) -> bool {
        self.update_badges
            .iter()
            .any(|badge| badge.plugin == plugin_id)
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

    /// The fetch the open Plugins drawer is missing right now, or
    /// `Intent::None`. The drawer opens by default and the list arrives
    /// from a background refresh, so a row can be selected without any
    /// navigation event having asked for its detail — this is checked
    /// every frame so the drawer never sits on "loading…" waiting for a
    /// click that would only re-request what it already needs.
    pub(crate) fn drawer_inspect_intent(&self) -> super::worker::Intent {
        if self.route != Route::Plugins || !self.marketplace_drawer_open {
            return super::worker::Intent::None;
        }
        let intent = self.marketplace_inspect_intent();
        if self.inspection_in_flight.as_ref() == Some(&intent) {
            return super::worker::Intent::None;
        }
        intent
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

    fn clamp_harness_selection(&mut self) {
        self.harnesses_selected = self
            .harnesses_selected
            .min(self.harness_visible_indices().len().saturating_sub(1));
    }

    pub(crate) fn harness_visible_indices(&self) -> Vec<usize> {
        let needle = self.harnesses_filter.trim().to_lowercase();
        let Some(doctor) = &self.doctor else {
            return Vec::new();
        };
        doctor
            .harnesses
            .iter()
            .enumerate()
            .filter(|(_, harness)| {
                needle.is_empty()
                    || harness.display_name.to_lowercase().contains(&needle)
                    || harness.integration.to_lowercase().contains(&needle)
                    || harness.description.to_lowercase().contains(&needle)
            })
            .map(|(index, _)| index)
            .collect()
    }

    /// Types one character into whatever is taking text — a text prompt
    /// first, then the active route's live filter. Which surface that is
    /// was already decided by the keymap (`Scope::consumes_text`); this
    /// only says where the character lands.
    pub(crate) fn type_character(&mut self, character: char) -> super::worker::Intent {
        match &mut self.overlay {
            Overlay::ActionIndex {
                filter, selected, ..
            } => {
                filter.push(character);
                *selected = 0;
            }
            Overlay::AddMarketplace(input) | Overlay::NewProfile(input) => input.push(character),
            _ => {
                match self.route {
                    Route::Plugins => self.marketplace_filter.push(character),
                    Route::Extensions => self.extension_filter.push(character),
                    Route::Harnesses => self.harnesses_filter.push(character),
                    Route::Keys => {
                        self.keys_filter.push(character);
                        self.keys_selected = 0;
                    }
                    _ => return super::worker::Intent::None,
                }
                self.clamp_filtered_selection();
            }
        }
        super::worker::Intent::None
    }

    /// Erases the character before the cursor of whatever is taking text.
    pub(crate) fn erase_character(&mut self) -> super::worker::Intent {
        match &mut self.overlay {
            Overlay::ActionIndex {
                filter, selected, ..
            } => {
                filter.pop();
                *selected = 0;
            }
            Overlay::AddMarketplace(input) | Overlay::NewProfile(input) => {
                input.pop();
            }
            _ => {
                match self.route {
                    Route::Plugins => self.marketplace_filter.pop(),
                    Route::Extensions => self.extension_filter.pop(),
                    Route::Harnesses => self.harnesses_filter.pop(),
                    Route::Keys => {
                        self.keys_filter.pop();
                        self.keys_selected = 0;
                        return super::worker::Intent::None;
                    }
                    _ => return super::worker::Intent::None,
                };
                self.clamp_filtered_selection();
            }
        }
        super::worker::Intent::None
    }

    /// Forgets the active route's filter. Leaving a search puts the list
    /// back the way it was found.
    pub(crate) fn clear_filter(&mut self) {
        match self.route {
            Route::Plugins => self.marketplace_filter.clear(),
            Route::Extensions => self.extension_filter.clear(),
            Route::Harnesses => self.harnesses_filter.clear(),
            Route::Keys => {
                self.keys_filter.clear();
                self.keys_selected = 0;
                return;
            }
            _ => return,
        }
        self.clamp_filtered_selection();
    }

    /// A narrowed list can be shorter than wherever the selection was.
    fn clamp_filtered_selection(&mut self) {
        match self.route {
            Route::Plugins => self.clamp_marketplace_selection(),
            Route::Extensions => self.clamp_extension_selection(),
            Route::Harnesses => self.clamp_harness_selection(),
            _ => {}
        }
    }

    /// Takes a keystroke as the new binding for the selected line.
    ///
    /// Everything that could be wrong with it is said before anything is
    /// written: a chord that is another key on a terminal, one this
    /// terminal cannot send, and one that already means something else in
    /// the same keyboard. A screen that let you lock yourself out would be
    /// worse than one that had no rebinding at all.
    /// Where in the Keys list a point on its scroll track lands.
    ///
    /// The track is a picture of the whole list, so a position on it is a
    /// position in the list — the top row is the first key, the bottom row
    /// the last. The window itself is derived from the selection rather
    /// than stored, so moving the selection is how the track moves the
    /// page; there is no second notion of "where the page is" that could
    /// disagree with the first.
    /// Whether this screen has a search field. Plugins, Extensions and
    /// Integrations filter their lists; Keys filters its own; the Overview
    /// is a report and Profiles is three panels rather than a list.
    pub(crate) fn has_filter(&self) -> bool {
        matches!(
            self.route,
            Route::Plugins | Route::Extensions | Route::Harnesses | Route::Keys
        )
    }

    /// The list at the foot of the sidebar, as it stands.
    pub(crate) fn first_steps(&self) -> super::FirstSteps<'_> {
        super::FirstSteps {
            steps: &super::management::FIRST_STEPS,
            taken: &self.steps_taken,
            collapsed: self.first_steps_collapsed,
            closed: self.first_steps_closed,
            scopes: super::management::FIRST_STEP_SCOPES,
        }
    }

    /// Records that a step was taken, whichever way it was reached. Called
    /// from the one place every action passes through, so a step cannot be
    /// performed without the list noticing.
    pub(crate) fn note_step(&mut self, action: uze_keys::Action) {
        if super::management::FIRST_STEPS.contains(&action) {
            self.steps_taken.insert(action.name());
        }
    }

    pub(crate) fn scroll_keys_to(&mut self, track: Rect, row: u16) {
        let last = self.key_rows().len().saturating_sub(1);
        let travel = usize::from(track.height.saturating_sub(1));
        if travel == 0 {
            return;
        }
        let offset = usize::from(row.saturating_sub(track.y)).min(travel);
        self.keys_selected = offset * last / travel;
        self.keys_capture = false;
        self.keys_problem = None;
    }

    pub(crate) fn capture_chord(&mut self, chord: uze_keys::Chord) -> super::worker::Intent {
        let Some(row) = self.selected_key_row() else {
            self.keys_capture = false;
            return super::worker::Intent::None;
        };
        // The probe, and it costs nothing: capturing a key is already
        // asking the terminal what it sends, so saying what arrived is
        // the honest answer no compatibility table can give.
        self.keys_probe = Some(format!("`{chord}` arrived here — {}", chord.tier().label()));
        // The grammar's own refusals, run against what arrived: under an
        // enhanced protocol a terminal really can report `ctrl+i`, and
        // binding it would take Tab away everywhere.
        if let Err(problem) = uze_keys::Chord::parse(&chord.to_string()) {
            self.keys_problem = Some(problem.to_string());
            return super::worker::Intent::None;
        }
        if !self.keyboard.can_deliver(chord.tier()) {
            self.keys_problem = Some(format!(
                "`{chord}` {} — this terminal cannot send it",
                chord.tier().label()
            ));
            return super::worker::Intent::None;
        }
        match uze_keys::active().rebind(row.action, row.scope, Some(chord)) {
            Ok(keymap) => {
                uze_keys::set_active(keymap);
                self.keys_capture = false;
                self.keys_problem = None;
                super::worker::Intent::PersistKeymap
            }
            Err(conflicts) => {
                self.keys_problem = conflicts.first().map(ToString::to_string);
                super::worker::Intent::None
            }
        }
    }

    /// Puts back what uze ships with, for the selected line.
    pub(crate) fn reset_selected_key(&mut self) -> super::worker::Intent {
        let Some(row) = self.selected_key_row().filter(KeyRow::custom) else {
            return super::worker::Intent::None;
        };
        match uze_keys::active().rebind(row.action, row.scope, row.default_chord) {
            Ok(keymap) => {
                uze_keys::set_active(keymap);
                self.keys_capture = false;
                self.keys_problem = None;
                super::worker::Intent::PersistKeymap
            }
            Err(conflicts) => {
                // Reaching this means the operator moved uze's own default
                // onto something else. Say so rather than silently
                // refusing: the way out is to free that key first.
                self.keys_problem = conflicts.first().map(ToString::to_string);
                super::worker::Intent::None
            }
        }
    }

    /// One line of the Keys screen: an action, where it is live, and the
    /// key that reaches it there.
    ///
    /// Built from the keymap in force rather than from the default, so an
    /// unbinding leaves the row rather than the row disappearing with the
    /// key — you have to be able to see what you turned off.
    pub(crate) fn key_rows(&self) -> Vec<KeyRow> {
        let active = uze_keys::active();
        let default = uze_keys::default_keymap();
        let mut pairs: Vec<(uze_keys::Scope, uze_keys::Action)> = default
            .bindings()
            .iter()
            .chain(active.bindings())
            .map(|binding| (binding.scope, binding.action))
            .collect();
        pairs.sort_by_key(|(scope, action)| {
            (
                uze_keys::ALL_SCOPES
                    .iter()
                    .position(|candidate| candidate == scope)
                    .unwrap_or(usize::MAX),
                uze_keys::ALL_ACTIONS
                    .iter()
                    .position(|candidate| candidate == action)
                    .unwrap_or(usize::MAX),
            )
        });
        pairs.dedup();
        let chord_in = |keymap: &uze_keys::Keymap, scope, action| {
            keymap
                .bindings()
                .iter()
                .find(|binding| binding.scope == scope && binding.action == action)
                .map(|binding| binding.chord)
        };
        let needle = self.keys_filter.trim().to_lowercase();
        pairs
            .into_iter()
            .map(|(scope, action)| KeyRow {
                scope,
                action,
                chord: chord_in(&active, scope, action),
                default_chord: chord_in(default, scope, action),
            })
            .filter(|row| {
                needle.is_empty()
                    || row.action.label().to_lowercase().contains(&needle)
                    || row.action.description().to_lowercase().contains(&needle)
                    || row.scope.heading().to_lowercase().contains(&needle)
                    || row
                        .chord
                        .is_some_and(|chord| chord.to_string().contains(&needle))
            })
            .collect()
    }

    pub(crate) fn selected_key_row(&self) -> Option<KeyRow> {
        self.key_rows().get(self.keys_selected).copied()
    }

    /// Everything reachable from here, each with the key that reaches it.
    ///
    /// Two sources, because "reachable" has two halves: what the keymap
    /// binds in the scopes underneath the index, and what the selected row
    /// offers — which includes actions that deliberately hold no chord at
    /// all. An action with no key is a finished design, and the index is
    /// where someone finds it.
    pub(crate) fn action_index_rows(
        &self,
        scopes: &[uze_keys::Scope],
        filter: &str,
    ) -> Vec<(uze_keys::Action, Option<uze_keys::Chord>)> {
        let keymap = uze_keys::active();
        let mut rows = keymap.available(scopes);
        for offer in self.selected_offers() {
            if offer.is_available() && !rows.iter().any(|(action, _)| *action == offer.action) {
                rows.push((offer.action, keymap.chord_for(offer.action, scopes)));
            }
        }
        let needle = filter.trim().to_lowercase();
        if needle.is_empty() {
            return rows;
        }
        rows.retain(|(action, _)| {
            action.label().to_lowercase().contains(&needle)
                || action.description().to_lowercase().contains(&needle)
        });
        rows
    }

    /// What can be done to whatever is selected on this screen.
    ///
    /// Read from the entity itself, never decided here: the row menu, the
    /// detail view and the index all ask this, which is what keeps them
    /// from disagreeing about whether a plugin can be updated.
    pub(crate) fn selected_offers(&self) -> Vec<ActionOffer> {
        match self.route {
            Route::Plugins => self
                .selected_marketplace_plugin()
                .map(|plugin| plugin.offers())
                .unwrap_or_default(),
            Route::Extensions => self
                .selected_extension()
                .map(|_| uze_application::application::offers::extension_offers())
                .unwrap_or_default(),
            Route::Harnesses => self
                .selected_harness()
                .map(HarnessHealth::offers)
                .unwrap_or_default(),
            Route::Profiles => self
                .selected_profile()
                .map(ProfileSummary::offers)
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    pub(crate) fn selected_harness(&self) -> Option<&HarnessHealth> {
        let index = *self
            .harness_visible_indices()
            .get(self.harnesses_selected)?;
        self.doctor.as_ref()?.harnesses.get(index)
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
            Route::Harnesses => self.harness_visible_indices().len(),
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

    fn clamp_prompt_selection(&mut self) {
        self.overview_prompt_selected = self
            .overview_prompt_selected
            .min(self.prompt_history.len().saturating_sub(1));
        self.overview_prompt_hovered = self
            .overview_prompt_hovered
            .filter(|index| *index < self.prompt_history.len());
    }

    pub(crate) fn move_prompt_selection(&mut self, delta: isize) {
        let len = self.prompt_history.len();
        if len == 0 {
            return;
        }
        self.overview_prompt_selected =
            (self.overview_prompt_selected as isize + delta).clamp(0, len as isize - 1) as usize;
    }

    /// Leaves management for the tab the selected prompt was typed into.
    pub(crate) fn activate_selected_prompt(&mut self) -> super::worker::Intent {
        self.prompt_history
            .get(self.overview_prompt_selected)
            .map(|entry| super::worker::Intent::SwitchToWorkspaceTab(entry.tab_id))
            .unwrap_or(super::worker::Intent::None)
    }

    pub(crate) fn refreshed(&mut self, data: RefreshData) {
        self.plugins = data.plugins;
        self.doctor = data.doctor;
        self.resolved_at = Some(Instant::now());
        self.clamp_harness_selection();
        self.marketplace_plugins = data.marketplace_plugins;
        self.marketplaces = data.marketplaces;
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
        self.prompt_history = data.prompt_history;
        self.clamp_prompt_selection();
        // Additive, never a replacement: only the startup refresh carries
        // auto-updates, so an ordinary reload (or a mutation's own refresh)
        // must leave badges already raised exactly where they are.
        for plugin in data.auto_updated {
            if !self.was_just_updated(&plugin) {
                self.update_badges.push(UpdateBadge {
                    plugin,
                    seen_at: None,
                });
            }
        }
        self.status = Status::Idle;
    }

    /// The path the workspace-aware read models resolve against: the
    /// detected workspace root when there is one, else the same answer
    /// resolved directly.
    ///
    /// The fallback resolves rather than handing back the raw cwd, because
    /// this keys UZE-owned state — the prompt history a screen clears is
    /// the one it is listing, and that listing is seeded before the
    /// workspace summary lands (see `worker::recent_prompts`). Reached
    /// only from a key press, never from a frame.
    pub(crate) fn workspace_root(&self) -> PathBuf {
        self.workspace
            .as_ref()
            .map(|workspace| workspace.root.clone())
            .unwrap_or_else(|| uze_application::workspace_root_or_self(&self.context_root))
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
        if route != Route::Overview {
            self.overview_prompt_hovered = None;
        }
        self.route = route;
    }
}
