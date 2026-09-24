//! TUI — keyboard and mouse input dispatch: translating a terminal event
//! into a state transition and, where relevant, an [`Intent`] for a worker
//! to act on.
//!
//! The keyboard half is two steps and no more: say what is open
//! ([`TuiModel::scopes`]), then act on what the keymap says the keystroke
//! means ([`TuiModel::act`]). Which key that was is `uze-keys`'s business
//! and `super::keys`'s; nothing here names one.

use crossterm::event::{KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use uze_application::application::ContextPlan;
use uze_keys::{Action, Resolution, Scope};

use super::hit::Hit;
use super::keys;
use super::model::{Confirmation, Focus, Overlay, ProfilePanel, ResizablePanel, Route, TuiModel};
use super::worker::Intent;

impl TuiModel {
    /// What is open, outermost first — the value that replaced an ordering
    /// of `match` arms. Everything about modality is here, and it is the
    /// only thing a test needs to construct to ask what a key does.
    pub(crate) fn scopes(&self) -> Vec<Scope> {
        let mut scopes = vec![Scope::Global, Scope::Management];
        scopes.push(self.route.scope());
        if self.keys_capture {
            // Every keystroke is the answer here, including ones bound
            // elsewhere — that is the point of a capture.
            scopes.push(Scope::KeyCapture);
        }
        if self.route == Route::Profiles && self.profile_panel == ProfilePanel::Editor {
            scopes.push(Scope::ProfileEditor);
        }
        if self.focus == Focus::Sidebar {
            scopes.push(Scope::ManagementSidebar);
        }
        if self.filtering {
            scopes.push(Scope::Filter);
        }
        match self.overlay {
            Overlay::None | Overlay::HarnessHelp => {}
            Overlay::ActionIndex { .. } => scopes.push(Scope::ActionIndex),
            Overlay::AddMarketplace(_) | Overlay::NewProfile(_) => scopes.push(Scope::TextPrompt),
            Overlay::ThemePicker { .. } => scopes.push(Scope::ThemePicker),
            Overlay::Confirm { .. } => scopes.push(Scope::Confirm),
            Overlay::ReleaseNotes(_) => scopes.push(Scope::ReleaseNotes),
        }
        scopes
    }

    pub(crate) fn apply_key(&mut self, key: KeyEvent) -> Intent {
        // A glossary has nothing to answer — it is read, and then gone —
        // so any keystroke closes it. That is a property of the surface,
        // not a binding, and so not the keymap's to hold.
        if self.overlay == Overlay::HarnessHelp {
            self.close_overlay();
            return Intent::None;
        }
        let Some(chord) = keys::chord_of(key) else {
            return Intent::None;
        };
        if self.keys_capture {
            // Every keystroke is the answer here, so it is resolved
            // against the capture alone — otherwise a chord bound
            // globally could never be rebound, since it would fire
            // instead of arriving.
            return match uze_keys::active().resolve(chord, &[Scope::KeyCapture]) {
                Resolution::Act(Action::Dismiss) => {
                    self.keys_capture = false;
                    self.keys_problem = None;
                    Intent::None
                }
                _ => self.capture_chord(chord),
            };
        }
        let scopes = self.scopes();
        match uze_keys::active().resolve(chord, &scopes) {
            Resolution::Act(action) => self.act(action),
            Resolution::Text => match keys::text_of(key) {
                Some(character) => self.type_character(character),
                None => Intent::None,
            },
            Resolution::Fallthrough => Intent::None,
        }
    }

    /// One action, performed, and noted if it was a first step that landed.
    ///
    /// Every action this client performs passes through here, whichever way
    /// it was reached — a key, the index, a button — so this
    /// is the one place the first-steps list can learn what has been done
    /// without every call site remembering to tell it. It asks *after*, and
    /// asks for evidence: a screen where the gesture does nothing would
    /// otherwise tick it off, and a list that says you have done what you
    /// have not is worse than no list.
    pub(crate) fn act(&mut self, action: Action) -> Intent {
        let intent = self.perform(action);
        if self.step_landed(action, &intent) {
            self.note_step(action);
        }
        intent
    }

    /// What a first step looks like once it has actually happened. Only
    /// the steps need an answer; everything else is never noted.
    ///
    /// Some of them leave their evidence on the model and some of them
    /// leave it in the intent they answer with — an action that closes the
    /// modal changes nothing here to look at.
    fn step_landed(&self, action: Action, intent: &Intent) -> bool {
        match action {
            Action::SwitchMode => *intent == Intent::CloseModal,
            // Wraps, so it always moves.
            Action::NextScreen | Action::PreviousScreen => true,
            Action::OpenThemePicker => *intent == Intent::OpenThemePicker,
            Action::Refresh => *intent == Intent::Refresh,
            Action::OpenActionIndex => matches!(self.overlay, Overlay::ActionIndex { .. }),
            _ => false,
        }
    }

    /// Performs one action. Every arm is a meaning, so this reads as what
    /// the product does rather than as what a keyboard is wired to.
    fn perform(&mut self, action: Action) -> Intent {
        if self.overlay != Overlay::None {
            return self.overlay_action(action);
        }
        match action {
            Action::OpenActionIndex => {
                self.overlay = Overlay::ActionIndex {
                    scopes: self.scopes(),
                    filter: String::new(),
                    selected: 0,
                };
                Intent::None
            }
            Action::OpenGlossary => {
                self.overlay = Overlay::HarnessHelp;
                Intent::None
            }
            Action::SwitchMode => Intent::CloseModal,
            Action::Quit => Intent::Quit,
            Action::Refresh => Intent::Refresh,
            // Settings is machine-wide, so it is not a route's own
            // action: every screen answers it the same way.
            Action::OpenThemePicker => Intent::OpenThemePicker,
            Action::SelectNext => self.move_by(1),
            Action::SelectPrevious => self.move_by(-1),
            Action::FocusNext => self.cycle_focus(true),
            Action::FocusPrevious => self.cycle_focus(false),
            // Walking the sidebar without being in it — the same gesture
            // the workspace uses for its own vertical list. It lands in
            // the screen rather than on its name, because choosing a
            // screen is wanting to be on it.
            Action::NextScreen => self.step_route(1),
            Action::PreviousScreen => self.step_route(-1),
            Action::FocusSidebar => {
                self.focus = Focus::Sidebar;
                Intent::None
            }
            Action::FocusContent => {
                self.focus = Focus::Content;
                Intent::None
            }
            Action::Activate => {
                if self.focus == Focus::Sidebar {
                    self.focus = Focus::Content;
                    return Intent::None;
                }
                if self.route == Route::Overview && !self.remembered.prompt_history.is_empty() {
                    return self.activate_selected_prompt();
                }
                if self.route == Route::Keys {
                    // The one screen where Enter asks for a key rather
                    // than opening something.
                    self.keys_capture = true;
                    self.keys_problem = None;
                    return Intent::None;
                }
                if self.route == Route::Settings {
                    return self.activate_settings();
                }
                self.open_or_act()
            }
            Action::ChangeKey => {
                // A second press puts the screen back, which is the only
                // way a pointer has of cancelling a capture it started.
                self.keys_capture = !self.keys_capture;
                self.keys_problem = None;
                self.focus = Focus::Content;
                Intent::None
            }
            Action::ResetKey => self.reset_selected_key(),
            Action::Dismiss => self.dismiss(),
            // Searching belongs to a screen with something to search. On
            // one without, the key says so rather than doing nothing: a
            // key that answers nothing at all is indistinguishable from a
            // key that is broken, which is what this one looked like.
            Action::StartFilter => {
                if self.has_filter() {
                    self.filtering = true;
                } else {
                    self.say("Nothing to search on this screen");
                }
                Intent::None
            }
            Action::EraseBack => self.erase_character(),
            Action::NextValue => self.cycle_selected_preference(true),
            Action::PreviousValue => self.cycle_selected_preference(false),
            Action::ToggleProfileHarness => {
                if self.profile_panel == ProfilePanel::Harnesses {
                    self.toggle_profile_harness_at(self.profile_harness_selected);
                }
                Intent::None
            }
            Action::InstallPlugin => {
                if let Some((name, marketplace)) = self
                    .selected_marketplace_plugin()
                    .filter(|plugin| !plugin.installed)
                    .map(|plugin| (plugin.name.clone(), plugin.marketplace.clone()))
                {
                    self.overlay = Overlay::Confirm {
                        kind: Confirmation::InstallPlugin { name, marketplace },
                        focus: None,
                    };
                }
                Intent::None
            }
            Action::UpdatePlugin => {
                if let Some(id) = self
                    .selected_marketplace_plugin()
                    .filter(|plugin| plugin.installed && plugin.freshness.behind())
                    .map(|plugin| self.marketplace_plugin_id(&plugin))
                {
                    self.overlay = Overlay::Confirm {
                        kind: Confirmation::UpdatePlugin(id),
                        focus: None,
                    };
                }
                Intent::None
            }
            Action::RemovePlugin => {
                if let Some(plugin) = self.selected_marketplace_plugin().filter(|p| p.installed) {
                    let id = self.marketplace_plugin_id(&plugin);
                    self.overlay = if plugin.marketplace == "uze-official" {
                        // Anything from the embedded official snapshot is
                        // protected — remove is blocked with an explanation
                        // instead of silently offering a destructive (and
                        // pointless, it re-seeds) operation.
                        Overlay::Confirm {
                            kind: Confirmation::ProtectedPlugin(id),
                            focus: None,
                        }
                    } else {
                        Overlay::Confirm {
                            kind: Confirmation::RemovePlugin(id),
                            focus: Some(1),
                        }
                    };
                }
                Intent::None
            }
            Action::AddMarketplace => {
                self.overlay = Overlay::AddMarketplace(String::new());
                Intent::None
            }
            Action::InstallProjectEnvironment => {
                // Only offered when the consumer lock declares plugins that
                // aren't installed yet — the intent carries the detected
                // workspace root, and the worker reproduces it through
                // `install_project_environment`.
                self.overview_install_path()
                    .map(Intent::InstallProjectEnvironment)
                    .unwrap_or(Intent::None)
            }
            Action::ClearPromptHistory => {
                if !self.remembered.prompt_history.is_empty() {
                    self.overlay = Overlay::Confirm {
                        kind: Confirmation::ClearPromptHistory,
                        focus: None,
                    };
                }
                Intent::None
            }
            Action::SetupHarness => self.selected_harness().map_or(Intent::None, |harness| {
                Intent::Setup(harness.integration.clone())
            }),
            Action::AnalyzeContext => Intent::ContextAnalyze(self.workspace_root()),
            Action::ApplyContextPlan => {
                if self
                    .remembered
                    .context_plan
                    .as_ref()
                    .is_some_and(ContextPlan::has_changes)
                {
                    self.overlay = Overlay::Confirm {
                        kind: Confirmation::ApplyContext,
                        focus: None,
                    };
                }
                Intent::None
            }
            Action::NewProfile => {
                self.overlay = Overlay::NewProfile(String::new());
                Intent::None
            }
            Action::DeleteProfile => {
                if self.profile_panel == ProfilePanel::List
                    && let Some(profile) = self.selected_profile()
                {
                    self.overlay = Overlay::Confirm {
                        kind: Confirmation::DeleteProfile(profile.id.clone()),
                        focus: Some(1),
                    };
                }
                Intent::None
            }
            Action::ApplyProfile => self.apply_selected_profile(),
            Action::PreviewProfile => {
                self.toggle_profile_preview();
                Intent::None
            }
            // Answered by the surfaces that own them; anywhere else they
            // are simply not offered.
            _ => Intent::None,
        }
    }

    /// One screen along the sidebar, wrapping, wherever the focus was.
    ///
    /// Answers with whatever arriving asks for. A screen that reads its own
    /// data on arrival is empty if it is reached this way and the ask is
    /// dropped — and stays empty, because arriving is the only moment it
    /// asks.
    fn step_route(&mut self, delta: isize) -> Intent {
        let entering = self.set_route(self.route.neighbour(delta));
        self.focus = Focus::Content;
        entering
    }

    /// Where the selection goes, which depends on what the screen is a
    /// list *of* — routes in the sidebar, prompts on the Overview, a
    /// profile's three panels, or the ordinary content rows.
    fn move_by(&mut self, delta: isize) -> Intent {
        if self.focus == Focus::Sidebar {
            return self.set_route(self.route.neighbour(delta));
        }
        match self.route {
            Route::Profiles if self.profile_preview_open => {
                self.move_profile_preview_cursor(delta);
                Intent::None
            }
            Route::Profiles => {
                self.move_profile_selection(delta);
                Intent::None
            }
            Route::Keys => {
                let last = self.key_rows().len().saturating_sub(1);
                self.key_screen.selected = self
                    .key_screen
                    .selected
                    .saturating_add_signed(delta)
                    .min(last);
                self.keys_capture = false;
                self.keys_problem = None;
                Intent::None
            }
            Route::Settings => {
                self.move_settings_selection(delta);
                Intent::None
            }
            // The Overview's only navigable list is its prompt history.
            Route::Overview if !self.remembered.prompt_history.is_empty() => {
                self.move_prompt_selection(delta);
                Intent::None
            }
            _ => {
                self.move_selection(delta);
                if self.route == Route::Plugins {
                    self.marketplace_inspect_intent()
                } else {
                    Intent::None
                }
            }
        }
    }

    /// Profiles cycles its three sub-panels while the content has focus,
    /// instead of the generic sidebar/content toggle — scoped to that
    /// route so every other screen's focus behaviour is unchanged.
    fn cycle_focus(&mut self, forward: bool) -> Intent {
        if self.route == Route::Profiles && self.focus == Focus::Content {
            self.profile_panel = if forward {
                self.profile_panel.next()
            } else {
                self.profile_panel.prev()
            };
            return Intent::None;
        }
        self.focus = match self.focus {
            Focus::Sidebar => Focus::Content,
            Focus::Content => Focus::Sidebar,
        };
        Intent::None
    }

    /// Closes the innermost thing that is open — a capture, a filter, a
    /// preview — and, when nothing is, the modal itself: the key that
    /// backs out of everything else backs out of the surface too, which
    /// is what makes it read as one.
    ///
    /// A screen's detail drawer is not one of those things. It is a column
    /// of the screen rather than a layer over it, so Esc on a screen
    /// showing one leaves the modal, and the next visit finds the drawer
    /// at the width it was dragged to.
    fn dismiss(&mut self) -> Intent {
        if self.keys_capture {
            self.keys_capture = false;
            self.keys_problem = None;
            return Intent::None;
        }
        if self.filtering {
            self.filtering = false;
            self.clear_filter();
            return Intent::None;
        }
        if self.route == Route::Profiles {
            // The preview closes first, then the panel collapses back to
            // the List: both are layers over the screen, unlike a drawer.
            if self.profile_preview_open {
                self.profile_preview_open = false;
                return Intent::None;
            }
            if self.profile_panel != ProfilePanel::List {
                self.profile_panel = ProfilePanel::List;
                return Intent::None;
            }
            return Intent::CloseModal;
        }
        Intent::CloseModal
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
                self.marketplace_inspect_intent()
            }
            Route::Extensions => Intent::None,
            // List: jump straight into editing, the same way Enter opens a
            // drawer elsewhere. Editor: change the highlighted value.
            // Harnesses: no-op — toggling is the toggle action's job,
            // deliberately not doubled onto Enter.
            Route::Profiles if self.profile_preview_open => {
                self.toggle_profile_preview_harness(self.profile_preview_cursor);
                Intent::None
            }
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

    /// One mouse event over the surface drawn in `surface` — the inside
    /// of the modal, or a whole test frame.
    ///
    /// Clicks and hovers resolve against the hit list, which is in frame
    /// coordinates like the event; only the drags measure a *width* from
    /// the pointer, and a width is measured from the surface's own left
    /// edge, not the frame's.
    pub(crate) fn apply_mouse(&mut self, event: MouseEvent, surface: Rect) -> Intent {
        let total_width = surface.width;
        let column = event.column.saturating_sub(surface.x);
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => self.click(event.column, event.row),
            // The sidebar starts at the surface's own left edge. This used
            // to read the *previous* frame's `ResizeSidebar` hit rect
            // instead (its right edge, not its left) as that reference
            // point: each drag step measured from a stale, moving
            // position, so the edge fought the mouse instead of tracking
            // it — hence no layout recomputation needed here, just the
            // pointer's own column.
            MouseEventKind::Drag(MouseButton::Left) if self.dragging_sidebar => {
                let new_width = super::clamp_sidebar_width(column, total_width);
                if self.sidebar_width != Some(new_width) {
                    self.sidebar_width = Some(new_width);
                }
                Intent::None
            }
            MouseEventKind::Drag(MouseButton::Left)
                if let Some(track) = self.dragging_keys_track =>
            {
                self.scroll_keys_to(track, event.row);
                Intent::None
            }
            MouseEventKind::Drag(MouseButton::Left) if let Some(panel) = self.dragging_panel => {
                let sidebar_width = self
                    .sidebar_width
                    .unwrap_or_else(|| super::sidebar_width_for(total_width));
                let content_width = total_width.saturating_sub(sidebar_width);
                let min_panel_width = 24;
                let max_panel_width = content_width.saturating_sub(min_panel_width);
                // A drawer grows leftwards from the right edge; the profile
                // columns' divider is measured from the content's left.
                let width = match panel {
                    ResizablePanel::ProfileColumns => column.saturating_sub(sidebar_width),
                    _ => total_width.saturating_sub(column),
                };
                *panel.width_mut(self) = Some(width.min(max_panel_width).max(min_panel_width));
                Intent::None
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.dragging_sidebar = false;
                self.dragging_panel = None;
                self.dragging_keys_track = None;
                Intent::None
            }
            // The wheel walks whatever the arrow keys walk. It used to
            // call a narrower mover that knew only three of the screens,
            // so on Keys — by far the longest list uze draws — and on
            // Profiles the wheel did nothing at all, and the only way down
            // the page was the keyboard. Which is the shape of thing this
            // whole mechanism exists to stop shipping.
            // An open list is still a list: the index scrolls under the
            // wheel the way it steps under the arrows. Guarding this on
            // "no overlay" left the one surface that is nothing *but* a
            // long list as the one the wheel did not reach.
            MouseEventKind::ScrollDown if matches!(self.overlay, Overlay::ActionIndex { .. }) => {
                self.overlay_action(Action::SelectNext)
            }
            MouseEventKind::ScrollUp if matches!(self.overlay, Overlay::ActionIndex { .. }) => {
                self.overlay_action(Action::SelectPrevious)
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp
                if matches!(self.overlay, Overlay::ReleaseNotes(_)) =>
            {
                if let Overlay::ReleaseNotes(modal) = &mut self.overlay {
                    modal.wheel(event.kind == MouseEventKind::ScrollDown);
                }
                Intent::None
            }
            MouseEventKind::ScrollDown if self.overlay == Overlay::None => {
                self.focus = Focus::Content;
                self.move_by(1)
            }
            MouseEventKind::ScrollUp if self.overlay == Overlay::None => {
                self.focus = Focus::Content;
                self.move_by(-1)
            }
            MouseEventKind::Moved if self.overlay == Overlay::None => {
                // One read of the hit list answers every hover the chrome
                // has: a target that lights up under the pointer is only
                // honest if it lights up for the same rect the click
                // resolves against.
                let hovered = self.hit_at(event.column, event.row).cloned();
                self.overview_prompt_hovered = match hovered {
                    Some(Hit::PromptHistory(index)) if self.route == Route::Overview => Some(index),
                    _ => None,
                };
                self.source_link_hovered = matches!(hovered, Some(Hit::OpenLink(_)));
                self.version_hovered = matches!(hovered, Some(Hit::RunningReleaseNotes));
                self.hovered_offer = match hovered {
                    Some(Hit::OfferedAction(action)) => Some(action),
                    _ => None,
                };
                Intent::None
            }
            _ => Intent::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

    use ratatui::layout::Rect;

    use super::super::keys::press;
    use super::super::model::{Confirmation, Overlay, ResizablePanel, Route, TuiModel};

    #[test]
    fn dragging_a_content_divider_records_its_route_local_width() {
        let mut model = TuiModel {
            dragging_panel: Some(ResizablePanel::HarnessDrawer),
            ..TuiModel::default()
        };
        model.apply_mouse(
            MouseEvent {
                kind: MouseEventKind::Drag(MouseButton::Left),
                column: 80,
                row: 4,
                modifiers: KeyModifiers::NONE,
            },
            Rect::new(0, 0, 120, 40),
        );

        assert_eq!(model.remembered.harness_screen.drawer_width, Some(40));
    }

    #[test]
    fn typing_a_search_never_performs_a_screen_action() {
        let mut model = TuiModel {
            route: Route::Plugins,
            filtering: true,
            ..TuiModel::default()
        };
        // `r` removes a plugin on this screen when nobody is typing.
        model.apply_key(press(KeyCode::Char('r'), KeyModifiers::NONE));
        assert_eq!(model.remembered.plugin_screen.filter, "r");
        assert_eq!(model.overlay, Overlay::None);
    }

    #[test]
    fn a_question_on_screen_answers_before_the_screen_does() {
        let mut model = TuiModel {
            route: Route::Plugins,
            overlay: Overlay::Confirm {
                kind: Confirmation::ClearPromptHistory,
                focus: None,
            },
            ..TuiModel::default()
        };
        // `r` reaches the confirmation, not the plugin list behind it.
        model.apply_key(press(KeyCode::Char('r'), KeyModifiers::NONE));
        assert!(matches!(
            model.overlay,
            Overlay::Confirm {
                kind: Confirmation::ClearPromptHistory,
                ..
            }
        ));
    }
}
