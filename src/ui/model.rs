//! TUI — navigation, selection, and overlay state.

use std::collections::BTreeSet;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use ratatui::layout::Rect;
use uze_application::{Autonomy, ManagementLayout, ModelPreference, Preferences, SandboxScope};
use uze_extensions::registry::BuiltinExtension;

use uze_application::application::offers::ActionOffer;
use uze_application::application::{
    ContextPlan, DoctorReport, HarnessHealth, HarnessPreview, MarketplacePluginDetail,
    MarketplacePluginSummary, MarketplaceSummary, OverviewWorkspaceSummary, PluginInspection,
    PluginSummary, ProfileApplyResult, ProfilePreview, ProfileSummary, ProjectContextStatus,
    ProjectEnvironmentState,
};

use super::hit::Hit;
use super::view::health::{Alert, actionable_alerts};

/// What a profile preview answers: these preferences, against these
/// harnesses. An answer that does not match the current question is stale.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PreviewQuestion {
    pub(crate) preferences: Preferences,
    pub(crate) harness_ids: Vec<String>,
    pub(crate) epoch: u64,
}

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
    /// whether this terminal can deliver that key at all. Called
    /// *Shortcuts* on screen — "keys" is what a harness authenticates with,
    /// and the product already says shortcut everywhere else.
    Keys,
    /// The choices `config.toml` holds: the palette, the glyph set — each
    /// shown drawn in its own marks, because the only way to answer "can
    /// this terminal render that?" is to look at it — and when finished
    /// agents ring.
    Settings,
}

/// Sidebar order, and it is an argument rather than a list: what UZE
/// delivers (plugins, then the preferences that travel with them), then who
/// receives it, then UZE's own surface. Profiles used to sit after
/// Integrations, among the screens about the app, where "preferences" read
/// as *uze's* preferences — they are an agent's, applied to harnesses, and
/// they belong beside the other thing UZE hands a harness.
pub(crate) const ROUTES: [Route; 7] = [
    Route::Overview,
    Route::Plugins,
    Route::Profiles,
    Route::Harnesses,
    Route::Extensions,
    Route::Keys,
    Route::Settings,
];

/// Whether the Shortcuts screen has anything to say about a surface.
///
/// The keymap is the whole vocabulary, screens this build hides included
/// — a keymap file written on one build has to keep meaning the same
/// thing on another. What a person reads is only the surfaces they can
/// reach, or the shortcuts screen would document a screen that is not
/// there.
///
/// Derived from the screen that owns the scope rather than stated again
/// here: a screen already says which feature it waits on, and a second
/// list saying it too is a list that can disagree with the first.
pub(crate) fn scope_is_offered(scope: uze_keys::Scope) -> bool {
    ROUTES
        .into_iter()
        .find(|route| route.scopes().contains(&scope))
        .is_none_or(|route| routes().contains(&route))
}

/// The screens this build offers, in sidebar order.
///
/// [`ROUTES`] is the vocabulary — every screen the code knows — and this
/// is what a person can reach. A screen still being decided names a
/// feature (`uze_application::Feature`), and a release build leaves it
/// out: what is downloaded offers nothing half-built, and the build that
/// does offer it says so with the screen's badge.
pub(crate) fn routes() -> Vec<Route> {
    ROUTES
        .into_iter()
        .filter(|route| route.feature().is_none_or(uze_application::feature_enabled))
        .collect()
}

impl Route {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Route::Overview => "Overview",
            Route::Plugins => "Plugins",
            Route::Extensions => "Extensions",
            Route::Harnesses => "Integrations",
            Route::Profiles => "Profiles",
            Route::Keys => "Shortcuts",
            Route::Settings => "Settings",
        }
    }

    /// What the route is about, in the few words under its name — few
    /// enough to fit the sidebar at its narrowest, which a test holds.
    pub(crate) fn subtitle(self) -> &'static str {
        match self {
            Route::Overview => "status & health",
            Route::Plugins => "skills · agents · MCP",
            Route::Extensions => "official extensions",
            Route::Harnesses => "detected agents",
            Route::Profiles => "agent preferences",
            Route::Keys => "what each key does",
            Route::Settings => "theme · notifications",
        }
    }

    /// The badge a route carries beside its name in the sidebar, or
    /// `None` for one that is finished. The sidebar is where someone
    /// decides which screen to open, so it is where "not settled yet" has
    /// to be said — a warning found only after arriving is a warning that
    /// came too late.
    ///
    /// A screen behind a feature is exactly the unsettled kind, so the
    /// two answers are one: the feature decides whether the screen is on
    /// offer at all, and the badge says so wherever it is.
    pub(crate) fn badge(self) -> Option<&'static str> {
        self.feature().map(|_| "Beta")
    }

    /// The keyboard surfaces this screen answers for: its own first, then
    /// any surface that exists only inside it.
    ///
    /// One place says which scopes belong to which screen — the stack a
    /// keystroke is resolved against reads it, and so does the question
    /// of whether this build offers the surface at all.
    pub(crate) fn scopes(self) -> &'static [uze_keys::Scope] {
        use uze_keys::Scope;
        match self {
            Route::Overview => &[Scope::Overview],
            Route::Plugins => &[Scope::Plugins],
            Route::Extensions => &[Scope::Extensions],
            Route::Harnesses => &[Scope::Harnesses],
            // The preference editor is a surface of this screen and of
            // nowhere else, which is why hiding the screen hides it too.
            Route::Profiles => &[Scope::Profiles, Scope::ProfileEditor],
            Route::Keys => &[Scope::Keys],
            Route::Settings => &[Scope::Settings],
        }
    }

    /// The scope this screen puts on the stack while it is the one open.
    pub(crate) fn scope(self) -> uze_keys::Scope {
        self.scopes()
            .first()
            .copied()
            .expect("every screen answers for a scope of its own")
    }

    /// The feature this screen waits on, or `None` for one this build
    /// offers unconditionally.
    pub(crate) fn feature(self) -> Option<uze_application::Feature> {
        match self {
            Route::Profiles => Some(uze_application::Feature::Profiles),
            _ => None,
        }
    }

    /// Where this screen sits among the ones on offer. A screen this
    /// build hides answers zero rather than panicking: the sidebar is
    /// drawn from `routes()`, so being asked about one at all is already
    /// a path that should not exist.
    pub(crate) fn index(self) -> usize {
        routes()
            .iter()
            .position(|route| *route == self)
            .unwrap_or(0)
    }

    /// The route one step along the sidebar, wrapping at either end. Only
    /// the direction of `delta` counts.
    pub(crate) fn neighbour(self, delta: isize) -> Self {
        let offered = routes();
        let count = offered.len();
        let step = if delta > 0 { 1 } else { count - 1 };
        offered[(self.index() + step) % count]
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
            Route::Settings => "settings",
        }
    }

    /// The screen a remembered id names, or `None` — including for one
    /// this build does not offer, so a layout written by a development
    /// build opens a downloaded one on its default screen.
    pub(crate) fn from_id(id: &str) -> Option<Self> {
        routes().into_iter().find(|route| route.id() == id)
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
    SettingsDrawer,
}

impl ResizablePanel {
    /// Where this divider was last dragged to, or `None` for its default.
    pub(crate) fn width(self, model: &TuiModel) -> Option<u16> {
        match self {
            Self::MarketplaceDrawer => model.remembered.plugin_screen.drawer_width,
            Self::ExtensionDrawer => model.remembered.extension_screen.drawer_width,
            Self::HarnessDrawer => model.remembered.harness_screen.drawer_width,
            Self::KeysDrawer => model.key_screen.drawer_width,
            Self::SettingsDrawer => model.settings_drawer_width,
            Self::ProfileColumns => model.profile_columns_width,
        }
    }

    pub(crate) fn width_mut(self, model: &mut TuiModel) -> &mut Option<u16> {
        match self {
            Self::MarketplaceDrawer => &mut model.remembered.plugin_screen.drawer_width,
            Self::ExtensionDrawer => &mut model.remembered.extension_screen.drawer_width,
            Self::HarnessDrawer => &mut model.remembered.harness_screen.drawer_width,
            Self::KeysDrawer => &mut model.key_screen.drawer_width,
            Self::SettingsDrawer => &mut model.settings_drawer_width,
            Self::ProfileColumns => &mut model.profile_columns_width,
        }
    }
}

/// One list screen's own state: its search, where its selection is, and
/// its detail drawer.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ListScreen {
    /// Live substring filter, typed while `TuiModel::filtering` is true.
    pub(crate) filter: String,
    /// A position in the list as it is filtered now.
    pub(crate) selected: usize,
    /// Where the drawer's edge was dragged to; `None` for its default.
    pub(crate) drawer_width: Option<u16>,
}

impl ListScreen {
    /// The screen as a new visit finds it: its drawer at the width the
    /// layout left it, and no search — leaving a search puts the list
    /// back.
    fn reopen(&mut self, drawer_width: Option<u16>) {
        self.filter.clear();
        self.drawer_width = drawer_width;
    }
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

/// One line of the Settings screen. Two groups in one list, because the
/// two choices are one question — what this machine looks like — even
/// though neither decides the other.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum SettingsRow {
    /// A group's title. Never selectable: it is a label, not a choice.
    Heading(&'static str),
    Theme {
        id: String,
        active: bool,
        /// `None` for a theme UZE carries rather than one someone wrote.
        path: Option<std::path::PathBuf>,
    },
    GlyphSet {
        id: String,
        active: bool,
    },
    /// Which finished agent turns ring the bell. Here rather than on a
    /// screen of its own because it is one more thing about how this
    /// machine's client presents itself, and a screen with one choice on
    /// it is a screen nobody finds.
    Chime {
        chime: uze_application::Chime,
        active: bool,
    },
}

impl SettingsRow {
    pub(crate) fn selectable(&self) -> bool {
        !matches!(self, SettingsRow::Heading(_))
    }
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
    /// The notes of the release the sidebar's notice names, and of every
    /// other the changelog carries.
    ReleaseNotes(crate::ui::release_notes::ReleaseNotesModal),
    /// The Harnesses screen's own glossary — what each status/delivery/
    /// compatibility label actually means. Reference material about what
    /// the data *means*, which is a different question from what can be
    /// done here, so it is a surface of its own rather than a section of
    /// the index.
    HarnessHelp,
    /// A question the operator answers, or a notice they dismiss. `focus`
    /// is which answer the keyboard is on — 0 the way out, 1 the
    /// affirmative — for the destructive questions that let it move.
    Confirm {
        kind: Confirmation,
        focus: Option<usize>,
    },
    /// Free-text input, appended to on every character key and popped on
    /// backspace — see `TuiModel::overlay_key`'s `AddMarketplace` arms.
    AddMarketplace(String),
    /// A new profile's id, typed the same way as `AddMarketplace`.
    NewProfile(String),
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
}

/// What a confirmation is about, and so what agreeing to it does.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Confirmation {
    RemovePlugin(String),
    UpdatePlugin(String),
    InstallPlugin {
        name: String,
        marketplace: String,
    },
    ApplyContext,
    /// Deleting the workspace's recorded prompts. Destructive and not
    /// undoable, so it is confirmed like any other removal.
    ClearPromptHistory,
    /// Why a plugin from the embedded official snapshot cannot be removed.
    /// Nothing to agree to: it explains a refusal.
    ProtectedPlugin(String),
    DeleteProfile(String),
    /// A mutation needs consent it wasn't given non-interactively. Confirming
    /// re-runs the *same* action with explicit trust — never a silent
    /// bypass; the operator sees exactly what would newly execute.
    Trust {
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

    /// What the last visit resolved and where in it the operator was.
    pub(crate) remembered: Remembered,
    pub(crate) plugin_detail: Option<PluginInspection>,

    /// The Keys list. Its drawer is always open, so only its width is read.
    pub(crate) key_screen: ListScreen,
    /// The Settings screen's selected row, and the lists it is choosing
    /// from. Carried rather than read per frame for the reason
    /// [`Overlay::ThemePicker`] carries its own: the themes are a directory
    /// listing, and a list that changed between two frames would move the
    /// selection out from under the operator.
    pub(crate) settings_drawer_width: Option<u16>,
    pub(crate) settings_selected: usize,
    pub(crate) settings_themes: Vec<uze_application::application::ThemeSummary>,
    pub(crate) settings_glyph_sets: Vec<uze_application::application::GlyphSetSummary>,
    pub(crate) settings_chime: uze_application::Chime,
    /// Each theme's own colours, by id — resolved once with the list rather
    /// than per frame, because resolving one reads files. A theme absent
    /// from here resolved to nothing drawable and shows no swatches, which
    /// is the honest answer for a file with a typo in it.
    pub(crate) settings_palettes: std::collections::BTreeMap<String, Vec<uze_theme::Rgb>>,
    /// Whether the two lists have been read this visit — true even when a
    /// read found nothing, so an empty machine is not asked again every
    /// frame.
    pub(crate) settings_read: bool,
    /// Why the last rebinding was refused, in words — a conflict, a chord
    /// that is another key, or one this terminal cannot send.
    pub(crate) keys_problem: Option<String>,
    /// What the probe last saw. No table can enumerate every emulator,
    /// multiplexer and connection; pressing a key and being told what
    /// arrived is the answer for the machine in front of you.
    pub(crate) keys_probe: Option<String>,
    /// Whether a rebinding is waiting for its key. Its own state because it
    /// is the one moment the keyboard means nothing at all: every keystroke
    /// is the answer.
    pub(crate) keys_capture: bool,
    /// What this terminal turned out to be able to deliver.
    pub(crate) keyboard: super::keys::KeyboardSupport,
    pub(crate) marketplace_detail: Option<MarketplacePluginDetail>,
    /// The inspection the worker is currently answering for the drawer,
    /// so the per-frame `drawer_inspect_intent` check cannot queue the
    /// same fetch again while it is still running. Cleared when its
    /// answer, success or failure, lands.
    pub(crate) inspection_in_flight: Option<super::worker::Intent>,
    /// Whether the active list screen's filter is taking text.
    pub(crate) filtering: bool,
    /// Marketplace group names currently collapsed in the tree — absence
    /// means expanded, so a freshly registered marketplace starts open.
    pub(crate) collapsed_marketplaces: BTreeSet<String>,

    /// The official uze extensions catalog, from
    /// `uze_extensions::registry::ExtensionRegistry`.
    pub(crate) extensions: Vec<BuiltinExtension>,

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
    /// Whether the Profiles screen shows the selected profile's preview —
    /// what applying it writes into each harness — instead of the list.
    pub(crate) profile_preview_open: bool,
    /// The harness the preview's cursor is on, by its position in the
    /// preview.
    pub(crate) profile_preview_cursor: usize,
    /// Harnesses opened or closed by hand, against their default: open
    /// when applying would write something there, closed when not.
    pub(crate) profile_preview_toggled: BTreeSet<String>,
    /// The last preview answer, kept with the question it answered. Read
    /// through [`TuiModel::profile_preview_answer`], which refuses one that
    /// answers a question nobody is asking any more.
    pub(crate) profile_preview: Option<(PreviewQuestion, Result<ProfilePreview, String>)>,
    /// Bumped whenever a harness's configuration may have changed, so a
    /// read that started before the change can never pass for one after.
    pub(crate) profile_preview_epoch: u64,
    /// The question last sent to the worker, so a frame does not ask it
    /// again while the answer is on its way. Cleared whenever a harness's
    /// configuration may have changed underneath it.
    pub(crate) profile_preview_asked: Option<PreviewQuestion>,

    pub(crate) context_root: PathBuf,
    pub(crate) overview_prompt_hovered: Option<usize>,

    /// Whether the pointer is on the plugin drawer's source address.
    /// A link in a terminal has no cursor to change shape, so the colour
    /// is the only thing that can answer the pointer — see the address's
    /// own style in `view::plugins`.
    pub(crate) source_link_hovered: bool,
    /// Whether the pointer is on the footer's version, which opens this
    /// release's notes. Colour is the only answer a terminal has to hover.
    pub(crate) version_hovered: bool,
    /// The detail drawer's button under the pointer, if any.
    pub(crate) hovered_offer: Option<uze_keys::Action>,

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
    /// client, because it is one list drawn at the foot of both sidebars
    /// and a step taken in one surface is taken.
    pub(crate) steps_taken: std::collections::BTreeSet<String>,
    /// What the sidebar's foot says about releases, as of `release_revision`.
    pub(crate) release: Option<crate::self_update::Notice>,
    pub(crate) release_revision: u64,
}

impl Default for TuiModel {
    /// A model with nothing resolved, shaped as `ManagementLayout`'s own
    /// default — the widths a screen opens with are stated once, there.
    fn default() -> Self {
        Self::recall(None, &ManagementLayout::default())
    }
}

/// The fields of [`TuiModel`] that outlive one visit to the management
/// client — the machine state it resolved and where in it the operator
/// was. Everything not here belongs to one visit: the open overlay, the
/// status line, work in flight, and per-frame transients such as hit rects
/// and the spinner tick. The screen that was open and how its drawers were
/// left outlive the *process*, and every visit is shaped from the
/// `ManagementLayout` that keeps them (see [`TuiModel::recall`]).
///
/// The modal is opened and closed constantly, and rebuilding a default
/// model each time meant an empty screen — no plugins, no harnesses —
/// under a "Refreshing environment…" line, for as long as a full
/// resolution took. What the last visit resolved is still the truth about
/// the machine, so it is what the next one draws while a refresh confirms
/// it behind the frame.
#[derive(Default)]
pub(crate) struct Remembered {
    pub(crate) plugins: Vec<PluginSummary>,
    pub(crate) doctor: Option<DoctorReport>,
    /// When the state here was last resolved, or `None` while the
    /// session's first resolution is still on its way. Opening the modal
    /// reads it to decide whether it is looking at an answer or at
    /// nothing yet — see `management::RESOLUTION_STANDS_FOR`.
    pub(crate) resolved_at: Option<Instant>,
    /// Every registered marketplace, by the name its plugins carry —
    /// what the plugin drawer resolves a source link through.
    pub(crate) marketplaces: Vec<MarketplaceSummary>,
    pub(crate) marketplace_plugins: Vec<MarketplacePluginSummary>,
    pub(crate) profiles: Vec<ProfileSummary>,
    pub(crate) context_status: Option<ProjectContextStatus>,
    pub(crate) context_plan: Option<ContextPlan>,
    /// The detected UZE workspace (`agents.lock`/`marketplace.json`), loaded on
    /// the first refresh. `None` only before the startup worker returns.
    pub(crate) workspace: Option<OverviewWorkspaceSummary>,
    /// Recent prompts for the detected workspace, newest first. Read-only
    /// here: the workspace client owns writing them.
    pub(crate) prompt_history: Vec<uze_application::PromptEntry>,
    /// Plugins updated automatically this session, badged as "Updated" on
    /// the Plugins screen until [`UPDATE_BADGE_TTL`] after the operator has
    /// actually had that screen in front of them.
    pub(crate) update_badges: Vec<UpdateBadge>,
    /// The Plugins tree. Its selection indexes the *visible* (filtered,
    /// group-expanded) sequence — see `marketplace_visible_indices` — not
    /// `marketplace_plugins`; resolve through `selected_marketplace_plugin`.
    pub(crate) plugin_screen: ListScreen,
    /// The Extensions catalog. Its selection is a position within
    /// `extension_visible_indices`, rather than a raw catalog index, so
    /// filtered cards and keyboard navigation always agree.
    pub(crate) extension_screen: ListScreen,
    pub(crate) harness_screen: ListScreen,
    pub(crate) profiles_selected: usize,
    pub(crate) overview_prompt_selected: usize,
}

impl TuiModel {
    /// A model opening the management modal shaped as `layout` says —
    /// the screen, the drawers, the folds — with what the previous visit
    /// left behind. `None` is the first visit of the process, which has
    /// nothing resolved yet.
    pub(crate) fn recall(remembered: Option<Remembered>, layout: &ManagementLayout) -> Self {
        let mut model = Self {
            route: layout
                .route
                .as_deref()
                .and_then(Route::from_id)
                .unwrap_or(Route::Overview),
            focus: Focus::Sidebar,
            overlay: Overlay::None,
            key_screen: ListScreen::default(),
            settings_drawer_width: None,
            settings_selected: 0,
            settings_themes: Vec::new(),
            settings_glyph_sets: Vec::new(),
            settings_chime: uze_application::Chime::default(),
            settings_palettes: std::collections::BTreeMap::new(),
            settings_read: false,
            keys_capture: false,
            keys_problem: None,
            keys_probe: None,
            keyboard: super::keys::KeyboardSupport::default(),
            status: Status::Idle,
            status_expires_at: None,
            maintenance_in_flight: false,
            remembered: remembered.unwrap_or_default(),
            plugin_detail: None,
            marketplace_detail: None,
            inspection_in_flight: None,
            filtering: false,
            collapsed_marketplaces: layout.collapsed_marketplaces.clone(),
            extensions: uze_extensions::registry::ExtensionRegistry::builtin()
                .all()
                .to_vec(),
            profile_panel: ProfilePanel::List,
            profile_editor_selected: 0,
            profile_harness_selected: 0,
            profile_harness_selection: BTreeSet::new(),
            profile_harness_defaulted: false,
            profile_apply_results: Vec::new(),
            profile_preview_open: false,
            profile_preview_cursor: 0,
            profile_preview_toggled: BTreeSet::new(),
            profile_preview: None,
            profile_preview_epoch: 0,
            profile_preview_asked: None,
            context_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            overview_prompt_hovered: None,
            source_link_hovered: false,
            version_hovered: false,
            hovered_offer: None,
            tick: 0,
            hits: Vec::new(),
            sidebar_width: None,
            dragging_sidebar: false,
            profile_columns_width: layout.profile_columns_width,
            dragging_panel: None,
            dragging_keys_track: None,
            first_steps_collapsed: false,
            first_steps_closed: false,
            steps_taken: std::collections::BTreeSet::new(),
            release: None,
            release_revision: 0,
        };
        let remembered = &mut model.remembered;
        remembered
            .plugin_screen
            .reopen(layout.marketplace_drawer_width);
        remembered
            .extension_screen
            .reopen(layout.extension_drawer_width);
        remembered
            .harness_screen
            .reopen(layout.harness_drawer_width);
        model
    }

    /// The shape this opening leaves the management modal in, for the
    /// next visit and the next run alike.
    pub(crate) fn management_layout(&self) -> ManagementLayout {
        let remembered = &self.remembered;
        ManagementLayout {
            route: Some(self.route.id().to_owned()),
            marketplace_drawer_width: remembered.plugin_screen.drawer_width,
            extension_drawer_width: remembered.extension_screen.drawer_width,
            harness_drawer_width: remembered.harness_screen.drawer_width,
            profile_columns_width: self.profile_columns_width,
            collapsed_marketplaces: self.collapsed_marketplaces.clone(),
        }
    }

    /// What this visit leaves for the next one.
    pub(crate) fn remember(self) -> Remembered {
        self.remembered
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
        if self.remembered.update_badges.is_empty() {
            return;
        }
        let now = Instant::now();
        if self.route == Route::Plugins {
            for badge in &mut self.remembered.update_badges {
                badge.seen_at.get_or_insert(now);
            }
        }
        self.remembered.update_badges.retain(|badge| {
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
        self.remembered
            .update_badges
            .iter()
            .any(|badge| badge.plugin == plugin_id)
    }

    /// Installed plugins from `plugins` that no catalog entry knows about
    /// (ad-hoc `uze add`/git/local installs) — the merged Plugins tree's
    /// "local" group, so a direct install never disappears from the TUI
    /// when the catalog screen absorbs the old installed list.
    fn local_marketplace_rows(&self) -> Vec<MarketplacePluginSummary> {
        self.remembered
            .plugins
            .iter()
            .filter(|plugin| {
                !self
                    .remembered
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
                freshness: plugin.freshness.clone(),
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
        let mut rows = self.remembered.marketplace_plugins.clone();
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
        self.remembered
            .plugins
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
        let needle = self.remembered.plugin_screen.filter.trim().to_lowercase();
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

    /// Resolves the Plugins selection (a position in the visible sequence)
    /// back to the plugin it points at — an owned clone, since the merged
    /// row list is computed on demand (`marketplace_rows`).
    pub(crate) fn selected_marketplace_plugin(&self) -> Option<MarketplacePluginSummary> {
        let raw_index = *self
            .marketplace_visible_indices()
            .get(self.remembered.plugin_screen.selected)?;
        self.marketplace_rows().get(raw_index).cloned()
    }

    pub(crate) fn selected_extension(&self) -> Option<&BuiltinExtension> {
        self.extensions.get(
            *self
                .extension_visible_indices()
                .get(self.remembered.extension_screen.selected)?,
        )
    }

    pub(crate) fn extension_visible_indices(&self) -> Vec<usize> {
        let needle = self
            .remembered
            .extension_screen
            .filter
            .trim()
            .to_lowercase();
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

    /// The fetch the Plugins drawer is missing right now, or
    /// `Intent::None`. The list arrives from a background refresh, so a
    /// row can be selected without any navigation event having asked for
    /// its detail — this is checked every frame so the drawer never sits
    /// on "loading…" waiting for a click that would only re-request what
    /// it already needs.
    pub(crate) fn drawer_inspect_intent(&self) -> super::worker::Intent {
        if self.route != Route::Plugins {
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
        self.clamp_list_selection(Route::Plugins);
    }

    pub(crate) fn harness_visible_indices(&self) -> Vec<usize> {
        let needle = self.remembered.harness_screen.filter.trim().to_lowercase();
        let Some(doctor) = &self.remembered.doctor else {
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
            _ => self.edit_filter(|filter| filter.push(character)),
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
            _ => self.edit_filter(|filter| {
                filter.pop();
            }),
        }
        super::worker::Intent::None
    }

    /// Forgets the active route's filter. Leaving a search puts the list
    /// back the way it was found.
    pub(crate) fn clear_filter(&mut self) {
        self.edit_filter(String::clear);
    }

    /// Changes the active list screen's filter, and keeps its selection on
    /// the list the filter leaves — a narrowed list can be shorter than
    /// wherever the selection was.
    fn edit_filter(&mut self, edit: impl FnOnce(&mut String)) {
        let route = self.route;
        let Some(screen) = self.list_mut(route) else {
            return;
        };
        edit(&mut screen.filter);
        self.clamp_list_selection(route);
    }

    fn clamp_list_selection(&mut self, route: Route) {
        let len = self.list_len(route);
        if let Some(screen) = self.list_mut(route) {
            screen.selected = screen.selected.min(len.saturating_sub(1));
        }
    }

    /// The list screen `route` is, if it is one: Plugins, Extensions,
    /// Integrations and Keys. The Overview is a report, Profiles is three
    /// panels and Settings a catalogue with headings.
    pub(crate) fn list(&self, route: Route) -> Option<&ListScreen> {
        match route {
            Route::Plugins => Some(&self.remembered.plugin_screen),
            Route::Extensions => Some(&self.remembered.extension_screen),
            Route::Harnesses => Some(&self.remembered.harness_screen),
            Route::Keys => Some(&self.key_screen),
            Route::Overview | Route::Profiles | Route::Settings => None,
        }
    }

    pub(crate) fn list_mut(&mut self, route: Route) -> Option<&mut ListScreen> {
        match route {
            Route::Plugins => Some(&mut self.remembered.plugin_screen),
            Route::Extensions => Some(&mut self.remembered.extension_screen),
            Route::Harnesses => Some(&mut self.remembered.harness_screen),
            Route::Keys => Some(&mut self.key_screen),
            Route::Overview | Route::Profiles | Route::Settings => None,
        }
    }

    /// How many rows `route`'s list shows as it is filtered now.
    pub(crate) fn list_len(&self, route: Route) -> usize {
        match route {
            Route::Plugins => self.marketplace_visible_indices().len(),
            Route::Extensions => self.extension_visible_indices().len(),
            Route::Harnesses => self.harness_visible_indices().len(),
            Route::Keys => self.key_rows().len(),
            Route::Overview | Route::Profiles | Route::Settings => 0,
        }
    }

    /// Whether this screen has a search field.
    pub(crate) fn has_filter(&self) -> bool {
        self.list(self.route).is_some()
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

    /// Where in the Keys list a point on its scroll track lands.
    ///
    /// The track is a picture of the whole list, so a position on it is a
    /// position in the list — the top row is the first key, the bottom row
    /// the last. The window itself is derived from the selection rather
    /// than stored, so moving the selection is how the track moves the
    /// page; there is no second notion of "where the page is" that could
    /// disagree with the first.
    pub(crate) fn scroll_keys_to(&mut self, track: Rect, row: u16) {
        let rows = self.key_rows().len();
        let Some(bar) = super::widget::Scrollbar::measure(track, usize::from(track.height), rows)
        else {
            return;
        };
        // `item_at`, not `first_at`: this list's window is derived from
        // its selection, so a position on the track is a position in the
        // whole list — see `super::scrollbar`.
        self.key_screen.selected = bar.item_at(row);
        self.keys_capture = false;
        self.keys_problem = None;
    }

    /// Takes a keystroke as the new binding for the selected line.
    ///
    /// Everything that could be wrong with it is said before anything is
    /// written: a chord that is another key on a terminal, one this
    /// terminal cannot send, and one that already means something else in
    /// the same keyboard. A screen that let you lock yourself out would be
    /// worse than one that had no rebinding at all.
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

    /// The Settings screen's lines, both groups in reading order.
    pub(crate) fn settings_rows(&self) -> Vec<SettingsRow> {
        let mut rows = vec![SettingsRow::Heading("Theme")];
        rows.extend(self.settings_themes.iter().map(|theme| SettingsRow::Theme {
            id: theme.id.clone(),
            active: theme.active,
            path: theme.path.clone(),
        }));
        rows.push(SettingsRow::Heading("Glyphs"));
        rows.extend(
            self.settings_glyph_sets
                .iter()
                .map(|set| SettingsRow::GlyphSet {
                    id: set.id.clone(),
                    active: set.active,
                }),
        );
        rows.push(SettingsRow::Heading("Notifications"));
        rows.extend(
            uze_application::Chime::ALL
                .into_iter()
                .map(|chime| SettingsRow::Chime {
                    chime,
                    active: chime == self.settings_chime,
                }),
        );
        rows
    }

    /// The row the keyboard is on, skipping headings — a selection that
    /// landed on a label would have nothing to activate.
    pub(crate) fn selected_settings_row(&self) -> Option<SettingsRow> {
        let rows = self.settings_rows();
        rows.get(self.settings_selected)
            .filter(|row| row.selectable())
            .cloned()
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
        pairs.retain(|(scope, _)| scope_is_offered(*scope));
        let chord_in = |keymap: &uze_keys::Keymap, scope, action| {
            keymap
                .bindings()
                .iter()
                .find(|binding| binding.scope == scope && binding.action == action)
                .map(|binding| binding.chord)
        };
        let needle = self.key_screen.filter.trim().to_lowercase();
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
        self.key_rows().get(self.key_screen.selected).copied()
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
        crate::ui::widget::action_index::narrowed(rows, filter)
    }

    /// What can be done to whatever is selected on this screen.
    ///
    /// Read from the entity itself, never decided here: the drawer's
    /// buttons and the index both ask this, which is what keeps them from
    /// disagreeing about whether a plugin can be updated.
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
            Route::Keys => self
                .selected_key_row()
                .map(|row| uze_application::application::offers::key_offers(row.custom()))
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    pub(crate) fn selected_harness(&self) -> Option<&HarnessHealth> {
        let index = *self
            .harness_visible_indices()
            .get(self.remembered.harness_screen.selected)?;
        self.remembered.doctor.as_ref()?.harnesses.get(index)
    }

    pub(crate) fn selected_profile(&self) -> Option<&ProfileSummary> {
        self.remembered
            .profiles
            .get(self.remembered.profiles_selected)
    }

    /// Every harness on this machine, in the order the screen lists them.
    /// The preview covers the unchecked ones too, so checking a box never
    /// has to wait on a read.
    pub(crate) fn detected_harness_ids(&self) -> Vec<String> {
        self.remembered
            .doctor
            .as_ref()
            .map(|doctor| {
                doctor
                    .harnesses
                    .iter()
                    .filter(|harness| harness.detection.present)
                    .map(|harness| harness.integration.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn profile_preview_question(&self) -> Option<PreviewQuestion> {
        let profile = self.selected_profile()?;
        let harness_ids = self.detected_harness_ids();
        (!harness_ids.is_empty()).then_some(PreviewQuestion {
            preferences: profile.preferences,
            harness_ids,
            epoch: self.profile_preview_epoch,
        })
    }

    /// The read the Settings screen is missing, or `Intent::None`.
    ///
    /// Arriving asks for it (see [`Self::set_route`]), but arriving is not
    /// the only way onto the screen: the management modal reopens on the
    /// screen it was left on, restored without passing through a route
    /// change, and that screen used to stay empty until clicked again.
    pub(crate) fn settings_intent(&self) -> super::worker::Intent {
        if self.route == Route::Settings && !self.settings_read {
            super::worker::Intent::LoadSettings
        } else {
            super::worker::Intent::None
        }
    }

    /// The preview read the Profiles screen is missing right now, or
    /// `Intent::None`. Asked every frame, like the plugin drawer's detail:
    /// the selection, an edited preference and a finished apply all change
    /// the answer, and none of them should have to remember to ask.
    pub(crate) fn profile_preview_intent(&self) -> super::worker::Intent {
        if self.route != Route::Profiles {
            return super::worker::Intent::None;
        }
        match self.profile_preview_question() {
            Some(question) if self.profile_preview_asked.as_ref() != Some(&question) => {
                super::worker::Intent::PreviewProfile(question)
            }
            _ => super::worker::Intent::None,
        }
    }

    /// The answer to the question the screen is asking now — `None` while it
    /// has not arrived. An `Err` stays until something changes the question:
    /// asking again unchanged would repeat the failure forever.
    pub(crate) fn profile_preview_answer(&self) -> Option<Result<&ProfilePreview, &str>> {
        let question = self.profile_preview_question()?;
        let (answered, result) = self.profile_preview.as_ref()?;
        (*answered == question).then(|| result.as_ref().map_err(String::as_str))
    }

    /// The preview of the selected profile as it is now, when it was read.
    pub(crate) fn current_profile_preview(&self) -> Option<&ProfilePreview> {
        self.profile_preview_answer()?.ok()
    }

    pub(crate) fn profile_previewed(
        &mut self,
        question: PreviewQuestion,
        result: Result<ProfilePreview, String>,
    ) {
        self.profile_preview = Some((question, result));
    }

    /// Writes the selected profile into the checked harnesses and makes it
    /// the active one. Refuses, saying why, with nothing checked: marking a
    /// profile active while writing it nowhere is how "active" stopped
    /// meaning "in effect".
    pub(crate) fn apply_selected_profile(&mut self) -> super::worker::Intent {
        let Some((id, preferences)) = self
            .selected_profile()
            .map(|profile| (profile.id.clone(), profile.preferences))
        else {
            return super::worker::Intent::None;
        };
        let harness_ids: Vec<String> = self
            .detected_harness_ids()
            .into_iter()
            .filter(|harness| self.profile_harness_selection.contains(harness))
            .collect();
        if harness_ids.is_empty() {
            self.say("Check at least one harness to apply the profile to");
            return super::worker::Intent::None;
        }
        super::worker::Intent::ApplyProfile {
            id,
            preferences,
            harness_ids,
        }
    }

    pub(crate) fn toggle_profile_preview(&mut self) {
        self.profile_preview_open = !self.profile_preview_open;
        self.profile_preview_cursor = 0;
        self.profile_preview_toggled.clear();
    }

    /// Whether the preview shows a harness's settings, not just its row.
    pub(crate) fn profile_preview_expanded(&self, harness: &HarnessPreview) -> bool {
        let pending = harness
            .plan
            .as_ref()
            .map_or(true, |plan| plan.pending() > 0);
        pending != self.profile_preview_toggled.contains(&harness.integration)
    }

    pub(crate) fn move_profile_preview_cursor(&mut self, delta: isize) {
        let count = self
            .current_profile_preview()
            .map_or(0, |preview| preview.harnesses.len());
        self.profile_preview_cursor = self
            .profile_preview_cursor
            .saturating_add_signed(delta)
            .min(count.saturating_sub(1));
    }

    /// Opens or closes the harness at `index` in the preview.
    pub(crate) fn toggle_profile_preview_harness(&mut self, index: usize) {
        let Some(id) = self
            .current_profile_preview()
            .and_then(|preview| preview.harnesses.get(index))
            .map(|harness| harness.integration.clone())
        else {
            return;
        };
        self.profile_preview_cursor = index;
        if !self.profile_preview_toggled.remove(&id) {
            self.profile_preview_toggled.insert(id);
        }
    }

    /// Starts a new epoch, so the next frame asks again and anything read
    /// before now is refused — for when a harness's configuration may have
    /// changed.
    pub(crate) fn invalidate_profile_preview(&mut self) {
        self.profile_preview_epoch = self.profile_preview_epoch.wrapping_add(1);
    }

    /// Walks the Settings list, stepping over headings rather than
    /// landing on them: a selection sitting on a label has nothing to
    /// activate, and pressing Enter there would do nothing with no reason
    /// visible on screen.
    pub(crate) fn move_settings_selection(&mut self, delta: isize) {
        let rows = self.settings_rows();
        if rows.is_empty() || delta == 0 {
            return;
        }
        let step = delta.signum();
        let mut index = self.settings_selected.min(rows.len() - 1);
        for _ in 0..delta.unsigned_abs() {
            let next_choice = std::iter::successors(Some(index), |row| {
                row.checked_add_signed(step)
                    .filter(|next| *next < rows.len())
            })
            .skip(1)
            .find(|row| rows[*row].selectable());
            match next_choice {
                Some(row) => index = row,
                None => break,
            }
        }
        self.settings_selected = index;
    }

    /// Puts the selection on the theme in force, or on the first thing that
    /// can be chosen when none is. The list opens with a heading, so
    /// starting at zero would start on a label.
    ///
    /// The theme in force rather than the first card, because the client
    /// forgets this screen's selection between visits: landing on `default`
    /// every time read as the chosen theme having been lost.
    pub(crate) fn settle_settings_selection(&mut self) {
        let rows = self.settings_rows();
        if rows
            .get(self.settings_selected)
            .is_some_and(|row| row.selectable())
        {
            return;
        }
        self.settings_selected = rows
            .iter()
            .position(|row| matches!(row, SettingsRow::Theme { active: true, .. }))
            .or_else(|| rows.iter().position(SettingsRow::selectable))
            .unwrap_or(0);
    }

    /// Chooses whatever the selection is on. Which axis it belongs to is
    /// the row's own answer, so there is no mode to be in.
    pub(crate) fn activate_settings(&mut self) -> crate::ui::worker::Intent {
        match self.selected_settings_row() {
            Some(SettingsRow::Theme { id, .. }) => crate::ui::worker::Intent::SelectTheme(id),
            Some(SettingsRow::GlyphSet { id, .. }) => crate::ui::worker::Intent::SelectGlyphSet(id),
            Some(SettingsRow::Chime { chime, .. }) => crate::ui::worker::Intent::SelectChime(chime),
            _ => crate::ui::worker::Intent::None,
        }
    }

    /// Profiles has three independently-scrolled sub-panels rather than one
    /// list, so it is no [`ListScreen`] and clamps whichever panel is
    /// currently focused.
    pub(crate) fn move_profile_selection(&mut self, delta: isize) {
        let clamp = |current: usize, len: usize| step_within(current, delta, len);
        match self.profile_panel {
            ProfilePanel::List => {
                self.remembered.profiles_selected = clamp(
                    self.remembered.profiles_selected,
                    self.remembered.profiles.len(),
                );
            }
            ProfilePanel::Editor => {
                self.profile_editor_selected =
                    clamp(self.profile_editor_selected, PREFERENCE_ROW_COUNT);
            }
            ProfilePanel::Harnesses => {
                let len = self
                    .remembered
                    .doctor
                    .as_ref()
                    .map_or(0, |d| d.harnesses.len());
                self.profile_harness_selected = clamp(self.profile_harness_selected, len);
            }
        }
    }

    /// Cycles the Editor panel's currently-highlighted preference value and
    /// returns the `Intent` that persists it. Mutates `self.remembered.profiles`
    /// optimistically so the row reflects the new value immediately, without
    /// waiting on the (silent, fire-and-forget) background write.
    pub(crate) fn cycle_selected_preference(&mut self, forward: bool) -> super::worker::Intent {
        let Some(profile) = self
            .remembered
            .profiles
            .get_mut(self.remembered.profiles_selected)
        else {
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
            .remembered
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

    pub(crate) fn move_selection(&mut self, delta: isize) {
        let route = self.route;
        let len = self.list_len(route);
        let Some(screen) = self.list_mut(route) else {
            return;
        };
        screen.selected = step_within(screen.selected, delta, len);
    }

    fn clamp_prompt_selection(&mut self) {
        self.remembered.overview_prompt_selected = self
            .remembered
            .overview_prompt_selected
            .min(self.remembered.prompt_history.len().saturating_sub(1));
        self.overview_prompt_hovered = self
            .overview_prompt_hovered
            .filter(|index| *index < self.remembered.prompt_history.len());
    }

    pub(crate) fn move_prompt_selection(&mut self, delta: isize) {
        let len = self.remembered.prompt_history.len();
        if len == 0 {
            return;
        }
        self.remembered.overview_prompt_selected =
            step_within(self.remembered.overview_prompt_selected, delta, len);
    }

    /// Leaves management for the tab the selected prompt was typed into.
    pub(crate) fn activate_selected_prompt(&mut self) -> super::worker::Intent {
        self.remembered
            .prompt_history
            .get(self.remembered.overview_prompt_selected)
            .map(|entry| super::worker::Intent::CloseToTab(entry.tab_id))
            .unwrap_or(super::worker::Intent::None)
    }

    pub(crate) fn refreshed(&mut self, data: RefreshData) {
        self.remembered.plugins = data.plugins;
        self.remembered.doctor = data.doctor;
        self.remembered.resolved_at = Some(Instant::now());
        self.clamp_list_selection(Route::Harnesses);
        self.remembered.marketplace_plugins = data.marketplace_plugins;
        self.remembered.marketplaces = data.marketplaces;
        self.clamp_list_selection(Route::Plugins);
        self.clamp_list_selection(Route::Extensions);
        self.remembered.profiles = data.profiles;
        // A refresh is the operator asking to see the machine as it is;
        // a harness configuration edited by hand is part of that.
        self.invalidate_profile_preview();
        self.remembered.profiles_selected = self
            .remembered
            .profiles_selected
            .min(self.remembered.profiles.len().saturating_sub(1));
        self.profile_harness_selected = self.profile_harness_selected.min(
            self.remembered
                .doctor
                .as_ref()
                .map_or(0, |d| d.harnesses.len())
                .saturating_sub(1),
        );
        if !self.profile_harness_defaulted
            && let Some(doctor) = &self.remembered.doctor
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
            self.remembered.context_status = data.context_status;
        }
        if data.workspace.is_some() {
            self.remembered.workspace = data.workspace;
        }
        self.remembered.prompt_history = data.prompt_history;
        self.clamp_prompt_selection();
        // Additive, never a replacement: only the startup refresh carries
        // auto-updates, so an ordinary reload (or a mutation's own refresh)
        // must leave badges already raised exactly where they are.
        for plugin in data.auto_updated {
            if !self.was_just_updated(&plugin) {
                self.remembered.update_badges.push(UpdateBadge {
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
        self.remembered
            .workspace
            .as_ref()
            .map(|workspace| workspace.root.clone())
            .unwrap_or_else(|| uze_application::workspace_root_or_self(&self.context_root))
    }

    /// `Some(root)` exactly when the Application reports the project
    /// environment as `InstallRequired` — the only state the Overview may
    /// offer `i install` in. The state is the Application's verdict, never
    /// re-derived here from lock bytes.
    pub(crate) fn overview_install_path(&self) -> Option<PathBuf> {
        let workspace = self.remembered.workspace.as_ref()?;
        if workspace.project.environment == ProjectEnvironmentState::InstallRequired {
            Some(workspace.root.clone())
        } else {
            None
        }
    }

    pub(crate) fn alerts(&self) -> Vec<Alert> {
        actionable_alerts(self.remembered.doctor.as_ref())
    }

    pub(crate) fn set_route(&mut self, route: Route) -> crate::ui::worker::Intent {
        self.filtering = false;
        // Harnesses opens straight onto its first entry's detail — the list
        // is short and every row *is* the point of the screen, unlike
        // Marketplace/Plugins, which need typing/browsing before a
        // selection means anything.
        if route == Route::Harnesses {
            self.remembered.harness_screen.selected = 0;
        }
        if route == Route::Profiles {
            self.profile_panel = ProfilePanel::List;
        }
        if route != Route::Overview {
            self.overview_prompt_hovered = None;
        }
        self.route = route;
        // Settings reads its two lists on arrival rather than per frame:
        // a list that changed between two frames would move the selection
        // out from under the operator, which is the same reason the theme
        // picker carries its own.
        if route == Route::Settings {
            return crate::ui::worker::Intent::LoadSettings;
        }
        crate::ui::worker::Intent::None
    }
}

/// `current` moved by `delta` and held inside a list of `len` rows; `0` for
/// an empty list.
fn step_within(current: usize, delta: isize, len: usize) -> usize {
    len.checked_sub(1)
        .map_or(0, |last| current.saturating_add_signed(delta).min(last))
}
