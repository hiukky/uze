//! TUI — keyboard and mouse input dispatch: translating a terminal event
//! into a state transition and, where relevant, an [`Intent`] for a worker
//! to act on.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::application::ContextPlan;

use super::is_protected_plugin;
use super::model::{Focus, Overlay, ROUTES, Route, TuiModel};
use super::worker::Intent;

impl TuiModel {
    pub(crate) fn apply_key(&mut self, key: KeyEvent) -> Intent {
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
                if let Some(plugin) = self.selected_plugin() {
                    if is_protected_plugin(plugin, &self.marketplace_plugins) {
                        self.overlay = Overlay::ProtectedPlugin(plugin.id.clone());
                        self.focus = Focus::Overlay;
                    } else {
                        self.overlay = Overlay::ConfirmRemove {
                            id: plugin.id.clone(),
                            focus: 1,
                        };
                        self.focus = Focus::Overlay;
                    }
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
    pub(crate) fn open_or_act(&mut self) -> Intent {
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

    pub(crate) fn apply_mouse(&mut self, event: MouseEvent) -> Intent {
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
}
