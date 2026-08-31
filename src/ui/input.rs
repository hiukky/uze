//! TUI — keyboard and mouse input dispatch: translating a terminal event
//! into a state transition and, where relevant, an [`Intent`] for a worker
//! to act on.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::application::ContextPlan;

use super::model::{Focus, Overlay, ProfilePanel, ROUTES, Route, TuiModel};
use super::worker::Intent;

impl TuiModel {
    pub(crate) fn apply_key(&mut self, key: KeyEvent) -> Intent {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Intent::Quit;
        }
        if self.overlay != Overlay::None {
            return self.overlay_key(key);
        }
        if self.filtering {
            return self.filter_key(key);
        }
        match key.code {
            KeyCode::Char('?') => {
                self.overlay = if self.route == Route::Harnesses {
                    Overlay::HarnessHelp
                } else {
                    Overlay::Help
                };
                Intent::None
            }
            KeyCode::Char('q') => Intent::Quit,
            // Profiles cycles its three sub-panels on Tab while Content is
            // focused, instead of the generic Sidebar/Content toggle below —
            // scoped tightly to this route so every other screen's Tab
            // behavior is unchanged.
            KeyCode::Tab if self.route == Route::Profiles && self.focus == Focus::Content => {
                self.profile_panel = self.profile_panel.next();
                Intent::None
            }
            KeyCode::BackTab if self.route == Route::Profiles && self.focus == Focus::Content => {
                self.profile_panel = self.profile_panel.prev();
                Intent::None
            }
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
            // Left/Right only cycle a preference value in the Profiles
            // Editor panel — everywhere else (including the other two
            // Profiles panels) Left/`h` keeps its usual "back to sidebar"
            // meaning, matched below.
            KeyCode::Left
                if self.route == Route::Profiles && self.profile_panel == ProfilePanel::Editor =>
            {
                self.cycle_selected_preference(false)
            }
            KeyCode::Right
                if self.route == Route::Profiles && self.profile_panel == ProfilePanel::Editor =>
            {
                self.cycle_selected_preference(true)
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.focus = Focus::Sidebar;
                Intent::None
            }
            KeyCode::Char(' ') if self.route == Route::Profiles => {
                if self.profile_panel == ProfilePanel::Harnesses {
                    self.toggle_profile_harness_at(self.profile_harness_selected);
                }
                Intent::None
            }
            KeyCode::Char('j') | KeyCode::Down if self.route == Route::Profiles => {
                self.move_profile_selection(1);
                Intent::None
            }
            KeyCode::Char('k') | KeyCode::Up if self.route == Route::Profiles => {
                self.move_profile_selection(-1);
                Intent::None
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_selection(1);
                if self.route == Route::Plugins {
                    self.marketplace_inspect_intent()
                } else {
                    Intent::None
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_selection(-1);
                if self.route == Route::Plugins {
                    self.marketplace_inspect_intent()
                } else {
                    Intent::None
                }
            }
            KeyCode::Esc if self.route == Route::Profiles => {
                // Collapses back to the List panel first, mirroring every
                // other route's "Esc closes the drawer, doesn't touch
                // Sidebar/Content focus" convention.
                self.profile_panel = ProfilePanel::List;
                Intent::None
            }
            KeyCode::Esc => {
                // Slides the open drawer away — the fetched detail stays
                // cached, so reopening the same selection is instant.
                match self.route {
                    Route::Plugins => self.marketplace_drawer_open = false,
                    Route::Extensions => self.extension_drawer_open = false,
                    Route::Harnesses => self.harnesses_drawer_open = false,
                    _ => {}
                }
                Intent::None
            }
            KeyCode::Char('n') if self.route == Route::Profiles => {
                self.overlay = Overlay::NewProfile(String::new());
                self.focus = Focus::Overlay;
                Intent::None
            }
            KeyCode::Char('d')
                if self.route == Route::Profiles && self.profile_panel == ProfilePanel::List =>
            {
                if let Some(profile) = self.selected_profile() {
                    self.overlay = Overlay::ConfirmDeleteProfile {
                        id: profile.id.clone(),
                        focus: 1,
                    };
                    self.focus = Focus::Overlay;
                }
                Intent::None
            }
            KeyCode::Char('s')
                if self.route == Route::Profiles && self.profile_panel == ProfilePanel::List =>
            {
                self.selected_profile()
                    .map(|profile| Intent::SetActiveProfile(profile.id.clone()))
                    .unwrap_or(Intent::None)
            }
            KeyCode::Char('a' | 'A') if self.route == Route::Profiles => Intent::None,
            KeyCode::Enter => self.open_or_act(),
            KeyCode::Char('r') if self.route == Route::Plugins => {
                if let Some(plugin) = self.selected_marketplace_plugin().filter(|p| p.installed) {
                    let id = self.marketplace_plugin_id(&plugin);
                    self.overlay = if plugin.marketplace == "uze-official" {
                        // Anything from the embedded official snapshot is
                        // protected — remove is blocked with an explanation
                        // instead of silently offering a destructive (and
                        // pointless, it re-seeds) operation.
                        Overlay::ProtectedPlugin(id)
                    } else {
                        Overlay::ConfirmRemove { id, focus: 1 }
                    };
                    self.focus = Focus::Overlay;
                }
                Intent::None
            }
            // Global refresh alias outside Plugins, where `r` already means
            // remove — `g`/F5 keep working everywhere too.
            KeyCode::Char('r') => Intent::Refresh,
            KeyCode::Char('/') if self.route == Route::Plugins => {
                self.filtering = true;
                Intent::None
            }
            KeyCode::Char('u') if self.route == Route::Plugins => {
                if let Some(id) = self
                    .selected_marketplace_plugin()
                    .filter(|plugin| plugin.installed && plugin.update_available == Some(true))
                    .map(|plugin| self.marketplace_plugin_id(&plugin))
                {
                    self.overlay = Overlay::ConfirmUpdate(id);
                    self.focus = Focus::Overlay;
                }
                Intent::None
            }
            KeyCode::Char('i') if self.route == Route::Overview => {
                // `i install` on Overview is only offered when the consumer
                // lock declares plugins that aren't installed yet — the
                // intent carries the detected workspace root, and the
                // worker reproduces it through `install_project_environment`.
                self.overview_install_path()
                    .map(Intent::InstallProjectEnvironment)
                    .unwrap_or(Intent::None)
            }
            KeyCode::Char('i') if self.route == Route::Plugins => {
                if let Some((name, marketplace)) = self
                    .selected_marketplace_plugin()
                    .filter(|plugin| !plugin.installed)
                    .map(|plugin| (plugin.name.clone(), plugin.marketplace.clone()))
                {
                    self.overlay = Overlay::ConfirmInstall { name, marketplace };
                    self.focus = Focus::Overlay;
                }
                Intent::None
            }
            KeyCode::Char('s') if self.route == Route::Harnesses => {
                self.selected_harness().map_or(Intent::None, |harness| {
                    Intent::Setup(harness.integration.clone())
                })
            }
            KeyCode::Char('a') if self.route == Route::Harnesses => {
                Intent::ContextAnalyze(self.workspace_root())
            }
            // Global "add marketplace" everywhere else — Harnesses keeps `a`
            // for analyze above, since that arm is matched first.
            KeyCode::Char('a') => {
                self.overlay = Overlay::AddMarketplace(String::new());
                self.focus = Focus::Overlay;
                Intent::None
            }
            KeyCode::Char('p') if self.route == Route::Harnesses => {
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

    /// Enter's meaning depends on the route: open a plugin row's delivery
    /// detail (installed) or catalog detail (available), open an
    /// extension's catalog detail, or (Harnesses) nothing beyond the
    /// already-visible detail pane, since there is no deeper read model.
    pub(crate) fn open_or_act(&mut self) -> Intent {
        match self.route {
            Route::Plugins => {
                if self.selected_marketplace_plugin().is_none() {
                    return Intent::None;
                }
                self.marketplace_drawer_open = true;
                self.marketplace_inspect_intent()
            }
            Route::Extensions => {
                if self.selected_extension().is_none() {
                    return Intent::None;
                }
                self.extension_drawer_open = true;
                Intent::None
            }
            // List: jump straight into editing, the same way Enter opens a
            // drawer elsewhere. Editor: change the highlighted value (same
            // step as Right). Harnesses: no-op — toggling is Space's job,
            // deliberately not doubled onto Enter.
            Route::Profiles => match self.profile_panel {
                ProfilePanel::List => {
                    self.profile_panel = ProfilePanel::Editor;
                    Intent::None
                }
                ProfilePanel::Editor => self.cycle_selected_preference(true),
                ProfilePanel::Harnesses => Intent::None,
            },
            _ => Intent::None,
        }
    }

    /// `total_width` is the terminal's current column count — needed only
    /// for the sidebar-drag arm below (`clamp_sidebar_width`'s dynamic max
    /// shrinks as the terminal narrows), which is otherwise the one mouse
    /// gesture this method can't resolve from `self` alone.
    pub(crate) fn apply_mouse(&mut self, event: MouseEvent, total_width: u16) -> Intent {
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => self.click(event.column, event.row),
            // The sidebar always starts at column 0 — the frame this TUI
            // draws into is always the full terminal — the same fact
            // `orchestrator::compute_layout`'s drag arm relies on via its
            // own `layout.sidebar.x`. This used to read the *previous*
            // frame's `ResizeSidebar` hit rect instead (its right edge, not
            // its left) as that reference point: each drag step measured
            // from a stale, moving position, so the edge fought the mouse
            // instead of tracking it — hence no layout recomputation
            // needed here, just the mouse's own absolute column.
            MouseEventKind::Drag(MouseButton::Left) if self.dragging_sidebar => {
                let new_width = super::clamp_sidebar_width(event.column, total_width);
                if self.sidebar_width != Some(new_width) {
                    self.sidebar_width = Some(new_width);
                }
                Intent::None
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.dragging_sidebar = false;
                Intent::None
            }
            MouseEventKind::ScrollDown if self.overlay == Overlay::None => {
                self.focus = Focus::Content;
                self.move_selection(1);
                if self.route == Route::Plugins {
                    self.marketplace_inspect_intent()
                } else {
                    Intent::None
                }
            }
            MouseEventKind::ScrollUp if self.overlay == Overlay::None => {
                self.focus = Focus::Content;
                self.move_selection(-1);
                if self.route == Route::Plugins {
                    self.marketplace_inspect_intent()
                } else {
                    Intent::None
                }
            }
            _ => Intent::None,
        }
    }
}
