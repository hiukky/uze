//! TUI — keyboard and mouse input dispatch: translating a terminal event
//! into a state transition and, where relevant, an [`Intent`] for a worker
//! to act on.
//!
//! The keyboard half is two steps and no more: say what is open
//! ([`TuiModel::scopes`]), then act on what the keymap says the keystroke
//! means ([`TuiModel::act`]). Which key that was is `uze-keys`'s business
//! and `super::keys`'s; nothing here names one.

use crossterm::event::{KeyEvent, MouseButton, MouseEvent, MouseEventKind};

use uze_application::application::ContextPlan;
use uze_keys::{Action, Resolution, Scope};

use super::hit::Hit;
use super::keys;
use super::model::{
    Focus, Overlay, ProfilePanel, ROUTES, ResizablePanel, Route, RowMenu, TuiModel,
};
use super::worker::Intent;

impl TuiModel {
    /// What is open, outermost first — the value that replaced an ordering
    /// of `match` arms. Everything about modality is here, and it is the
    /// only thing a test needs to construct to ask what a key does.
    pub(crate) fn scopes(&self) -> Vec<Scope> {
        let mut scopes = vec![Scope::Global, Scope::Management];
        scopes.push(match self.route {
            Route::Overview => Scope::Overview,
            Route::Plugins => Scope::Plugins,
            Route::Extensions => Scope::Extensions,
            Route::Harnesses => Scope::Harnesses,
            Route::Profiles => Scope::Profiles,
            Route::Keys => Scope::Keys,
        });
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
        if self.row_menu.is_some() {
            scopes.push(Scope::RowMenu);
        }
        match self.overlay {
            Overlay::None | Overlay::HarnessHelp => {}
            Overlay::ActionIndex { .. } => scopes.push(Scope::ActionIndex),
            Overlay::AddMarketplace(_) | Overlay::NewProfile(_) => scopes.push(Scope::TextPrompt),
            Overlay::ThemePicker { .. } => scopes.push(Scope::ThemePicker),
            _ => scopes.push(Scope::Confirm),
        }
        scopes
    }

    pub(crate) fn apply_key(&mut self, key: KeyEvent) -> Intent {
        // Reference material closes on anything, which is a property of a
        // surface that has nothing to do but be read — not a binding, and
        // so not the keymap's to hold.
        // A glossary has nothing to answer — it is read, and then gone —
        // so any keystroke closes it. That is a property of the surface,
        // not a binding, and so not the keymap's to hold.
        if self.overlay == Overlay::HarnessHelp {
            self.overlay = Overlay::None;
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

    /// Performs one action. Every arm is a meaning, so this reads as what
    /// the product does rather than as what a keyboard is wired to.
    /// One action, performed, and noted if it was a first step that landed.
    ///
    /// Every action this client performs passes through here, whichever way
    /// it was reached — a key, a row's menu, the index, a button — so this
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
    /// leave it in the intent they answer with — an action that hands the
    /// screen over to the other mode changes nothing here to look at.
    fn step_landed(&self, action: Action, intent: &Intent) -> bool {
        match action {
            Action::SwitchMode => *intent == Intent::SwitchToWorkspace,
            // Wraps, so it always moves.
            Action::NextScreen | Action::PreviousScreen => true,
            Action::OpenThemePicker => *intent == Intent::OpenThemePicker,
            Action::Refresh => *intent == Intent::Refresh,
            Action::OpenActionIndex => matches!(self.overlay, Overlay::ActionIndex { .. }),
            _ => false,
        }
    }

    fn perform(&mut self, action: Action) -> Intent {
        if self.overlay != Overlay::None {
            return self.overlay_action(action);
        }
        if self.row_menu.is_some() {
            return self.row_menu_action(action);
        }
        match action {
            Action::OpenActionIndex => {
                self.overlay = Overlay::ActionIndex {
                    scopes: self.scopes(),
                    filter: String::new(),
                    selected: 0,
                };
                self.focus = Focus::Overlay;
                Intent::None
            }
            Action::OpenGlossary => {
                self.overlay = Overlay::HarnessHelp;
                self.focus = Focus::Overlay;
                Intent::None
            }
            Action::SwitchMode => Intent::SwitchToWorkspace,
            Action::Quit => Intent::Quit,
            Action::Refresh => Intent::Refresh,
            // Appearance is machine-wide, so it is not a route's own
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
                if self.route == Route::Overview && !self.prompt_history.is_empty() {
                    return self.activate_selected_prompt();
                }
                if self.route == Route::Keys {
                    // The one screen where Enter asks for a key rather
                    // than opening something.
                    self.keys_capture = true;
                    self.keys_problem = None;
                    return Intent::None;
                }
                self.open_or_act()
            }
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
            Action::OpenRowActions => self.open_row_actions(),
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
                    self.overlay = Overlay::ConfirmInstall { name, marketplace };
                    self.focus = Focus::Overlay;
                }
                Intent::None
            }
            Action::UpdatePlugin => {
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
            Action::RemovePlugin => {
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
            Action::AddMarketplace => {
                self.overlay = Overlay::AddMarketplace(String::new());
                self.focus = Focus::Overlay;
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
                if !self.prompt_history.is_empty() {
                    self.overlay = Overlay::ConfirmClearPromptHistory;
                    self.focus = Focus::Overlay;
                }
                Intent::None
            }
            Action::SetupHarness => self.selected_harness().map_or(Intent::None, |harness| {
                Intent::Setup(harness.integration.clone())
            }),
            Action::AnalyzeContext => Intent::ContextAnalyze(self.workspace_root()),
            Action::ApplyContextPlan => {
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
            Action::NewProfile => {
                self.overlay = Overlay::NewProfile(String::new());
                self.focus = Focus::Overlay;
                Intent::None
            }
            Action::DeleteProfile => {
                if self.profile_panel == ProfilePanel::List
                    && let Some(profile) = self.selected_profile()
                {
                    self.overlay = Overlay::ConfirmDeleteProfile {
                        id: profile.id.clone(),
                        focus: 1,
                    };
                    self.focus = Focus::Overlay;
                }
                Intent::None
            }
            Action::ActivateProfile => self
                .selected_profile()
                .map(|profile| Intent::SetActiveProfile(profile.id.clone()))
                .unwrap_or(Intent::None),
            // Answered by the surfaces that own them; anywhere else they
            // are simply not offered.
            _ => Intent::None,
        }
    }

    /// Raises the selected row's own actions. Only the available ones: a
    /// menu is what can be done now.
    pub(crate) fn open_row_actions(&mut self) -> Intent {
        let all = self.selected_offers();
        if all.is_empty() {
            // Same reason as `StartFilter` above: the Overview and the Keys
            // screen have no rows anything can be done *to*, and a gesture
            // that opened nothing there read exactly like a broken key.
            self.say("Nothing here to act on — pick a row on Plugins, Extensions, Integrations or Profiles");
            return Intent::None;
        }
        let available: Vec<_> = all
            .iter()
            .filter(|offer| offer.is_available())
            .cloned()
            .collect();
        // A menu is what can be done now — so normally it holds only the
        // available offers. When there is nothing at all, it holds the
        // unavailable ones instead, each with its reason: a gesture that
        // opened nothing would read exactly like the silent no-op this
        // mechanism replaced.
        let offers = if available.is_empty() { all } else { available };
        // A destructive entry is never the one the menu opens on — the
        // same rule the workspace client's own menu follows. When every
        // available action destroys something, nothing is highlighted at
        // all and reaching one costs a deliberate step.
        let selected = offers
            .iter()
            .position(|offer| offer.is_available() && !offer.action.destructive());
        self.row_menu = Some(RowMenu {
            offers,
            selected,
            anchor: self.selected_row_rect().unwrap_or_default(),
        });
        self.focus = Focus::Content;
        Intent::None
    }

    fn row_menu_action(&mut self, action: Action) -> Intent {
        match action {
            Action::SelectNext => {
                self.step_row_menu(1);
                Intent::None
            }
            Action::SelectPrevious => {
                self.step_row_menu(-1);
                Intent::None
            }
            Action::Activate => {
                let chosen = self.row_menu.take().and_then(|menu| {
                    menu.selected
                        .and_then(|index| menu.offers.get(index))
                        .filter(|offer| offer.is_available())
                        .map(|offer| offer.action)
                });
                match chosen {
                    Some(action) => self.act(action),
                    None => Intent::None,
                }
            }
            // Anything else, dismissal included, closes without acting.
            _ => {
                self.row_menu = None;
                Intent::None
            }
        }
    }

    /// Moves through a menu's *available* entries. An entry that only
    /// explains why it cannot run is read, never landed on.
    fn step_row_menu(&mut self, delta: isize) {
        let Some(menu) = self.row_menu.as_mut() else {
            return;
        };
        let reachable: Vec<usize> = menu
            .offers
            .iter()
            .enumerate()
            .filter(|(_, offer)| offer.is_available())
            .map(|(index, _)| index)
            .collect();
        if reachable.is_empty() {
            return;
        }
        let position = menu
            .selected
            .and_then(|selected| reachable.iter().position(|index| *index == selected));
        menu.selected = Some(match position {
            Some(position) => {
                reachable[position
                    .saturating_add_signed(delta)
                    .min(reachable.len() - 1)]
            }
            None if delta > 0 => reachable[0],
            None => reachable[reachable.len() - 1],
        });
    }

    /// One screen along the sidebar, wrapping, wherever the focus was.
    fn step_route(&mut self, delta: isize) -> Intent {
        let count = ROUTES.len();
        let step = if delta > 0 { 1 } else { count - 1 };
        self.set_route(ROUTES[(self.route.index() + step) % count]);
        self.focus = Focus::Content;
        Intent::None
    }

    /// Where the selection goes, which depends on what the screen is a
    /// list *of* — routes in the sidebar, prompts on the Overview, a
    /// profile's three panels, or the ordinary content rows.
    fn move_by(&mut self, delta: isize) -> Intent {
        if self.focus == Focus::Sidebar {
            let count = ROUTES.len();
            let step = if delta > 0 { 1 } else { count - 1 };
            self.set_route(ROUTES[(self.route.index() + step) % count]);
            return Intent::None;
        }
        match self.route {
            Route::Profiles => {
                self.move_profile_selection(delta);
                Intent::None
            }
            Route::Keys => {
                let last = self.key_rows().len().saturating_sub(1);
                self.keys_selected = self.keys_selected.saturating_add_signed(delta).min(last);
                self.keys_capture = false;
                self.keys_problem = None;
                Intent::None
            }
            // The Overview's only navigable list is its prompt history.
            Route::Overview if !self.prompt_history.is_empty() => {
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
        self.focus = match (self.focus, forward) {
            (Focus::Sidebar, true) => Focus::Content,
            (Focus::Content, false) => Focus::Sidebar,
            (Focus::Content, true) => Focus::Sidebar,
            (_, _) => Focus::Content,
        };
        Intent::None
    }

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
            // Collapses back to the List panel first, mirroring every other
            // route's "Esc closes the drawer, doesn't touch focus" rule.
            self.profile_panel = ProfilePanel::List;
            return Intent::None;
        }
        // Slides the open drawer away — the fetched detail stays cached, so
        // reopening the same selection is instant.
        match self.route {
            Route::Plugins => self.marketplace_drawer_open = false,
            Route::Extensions => self.extension_drawer_open = false,
            Route::Harnesses => self.harnesses_drawer_open = false,
            _ => {}
        }
        Intent::None
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
            // drawer elsewhere. Editor: change the highlighted value.
            // Harnesses: no-op — toggling is the toggle action's job,
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
            // Right-click is the other way to ask a row what can be done
            // to it — the gesture the workspace client already answers on
            // its tabs and spaces.
            MouseEventKind::Down(MouseButton::Right) => self.right_click(event.column, event.row),
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
            MouseEventKind::Drag(MouseButton::Left)
                if let Some(track) = self.dragging_keys_track =>
            {
                self.scroll_keys_to(track, event.row);
                Intent::None
            }
            MouseEventKind::Drag(MouseButton::Left) if self.dragging_panel.is_some() => {
                let sidebar_width = self
                    .sidebar_width
                    .unwrap_or_else(|| super::sidebar_width_for(total_width));
                let content_width = total_width.saturating_sub(sidebar_width);
                let min_panel_width = 24;
                let max_panel_width = content_width.saturating_sub(min_panel_width);
                let pointer_in_content = event.column.saturating_sub(sidebar_width);
                match self.dragging_panel {
                    Some(ResizablePanel::MarketplaceDrawer) => {
                        self.marketplace_drawer_width = Some(
                            total_width
                                .saturating_sub(event.column)
                                .clamp(min_panel_width, max_panel_width),
                        );
                    }
                    Some(ResizablePanel::ExtensionDrawer) => {
                        self.extension_drawer_width = Some(
                            total_width
                                .saturating_sub(event.column)
                                .clamp(min_panel_width, max_panel_width),
                        );
                    }
                    Some(ResizablePanel::HarnessDrawer) => {
                        self.harness_drawer_width = Some(
                            total_width
                                .saturating_sub(event.column)
                                .clamp(min_panel_width, max_panel_width),
                        );
                    }
                    Some(ResizablePanel::KeysDrawer) => {
                        self.keys_drawer_width = Some(
                            total_width
                                .saturating_sub(event.column)
                                .clamp(min_panel_width, max_panel_width),
                        );
                    }
                    Some(ResizablePanel::ProfileColumns) => {
                        self.profile_columns_width =
                            Some(pointer_in_content.clamp(min_panel_width, max_panel_width));
                    }
                    None => {}
                }
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
                // A row menu's highlight follows the pointer, the way the
                // workspace's agent picker and context menu do. Only
                // available entries carry a hit, so hovering never lands
                // on one that cannot run.
                if let Some(Hit::RowMenuEntry(index)) = hovered
                    && let Some(menu) = self.row_menu.as_mut()
                {
                    menu.selected = Some(index);
                }
                Intent::None
            }
            _ => Intent::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

    use super::super::keys::press;
    use super::super::model::{Overlay, ResizablePanel, Route, TuiModel};

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
            120,
        );

        assert_eq!(model.harness_drawer_width, Some(40));
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
        assert_eq!(model.marketplace_filter, "r");
        assert_eq!(model.overlay, Overlay::None);
    }

    #[test]
    fn a_question_on_screen_answers_before_the_screen_does() {
        let mut model = TuiModel {
            route: Route::Plugins,
            overlay: Overlay::ConfirmClearPromptHistory,
            ..TuiModel::default()
        };
        // `r` reaches the confirmation, not the plugin list behind it.
        model.apply_key(press(KeyCode::Char('r'), KeyModifiers::NONE));
        assert_eq!(model.overlay, Overlay::ConfirmClearPromptHistory);
    }
}
