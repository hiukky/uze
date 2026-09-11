//! TUI — mouse hit-testing: mapping a clicked screen coordinate back to the
//! on-screen target it landed on.

use ratatui::layout::Rect;

use super::model::{Focus, Overlay, ResizablePanel, Route, TuiModel};
use super::worker::Intent;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Hit {
    Route(Route),
    /// The sidebar's "work" mode label — mirrors Ctrl+O, detaching from
    /// management back to the terminal workspace.
    SwitchToWorkspace,
    MarketplaceRow(usize),
    /// A marketplace group's header row — clicking it expands/collapses
    /// that group instead of selecting a plugin.
    MarketplaceGroupToggle(String),
    /// A plugin detail's Source card — jumps list selection to that
    /// marketplace's header, expanding it first if it's currently
    /// collapsed.
    JumpToMarketplace(String),
    /// The external-link glyph on that same card, by marketplace name:
    /// opens where the marketplace actually lives, in the reader's own
    /// browser. Carries the name rather than the address so the two can
    /// never disagree — the URL is resolved from the same read model the
    /// card printed it from.
    OpenLink(String),
    ExtensionRow(usize),
    HarnessRow(usize),
    NewProfile,
    DeleteSelectedProfile,
    ApplySelectedProfile,
    ProfileRow(usize),
    PreferenceRow(usize),
    /// Clicking a harness checkbox toggles it immediately — the click's
    /// obvious intent — rather than only selecting it the way `HarnessRow`
    /// does.
    ProfileHarnessRow(usize),
    /// The sidebar's right-border drag handle — mirrors the workspace TUI's
    /// `WorkspaceHit::ResizeSidebar`. Mousedown here only arms dragging; the
    /// actual width change happens in `apply_mouse` on the following `Drag`
    /// events, purely from the mouse's own column (see that arm's comment
    /// for why re-reading this hit's rect there was the wrong reference
    /// point).
    ResizeSidebar,
    /// A route-local divider between two content panels.
    ResizePanel(ResizablePanel),
    /// A row of the Overview's prompt history, by index into
    /// `TuiModel::prompt_history`.
    PromptHistory(usize),
    /// One row of the open index of everything, by position in it.
    ActionIndexEntry(usize),
    /// A detail view's button for one of the selected row's offers.
    OfferedAction(uze_keys::Action),
    /// One line of the Keys screen.
    KeyRow(usize),
    /// One line of the Appearance screen — a theme or a glyph set, by its
    /// place in the list. Headings are drawn but never registered: a label
    /// has nothing to activate.
    AppearanceRow(usize),
    /// The Keys list's scroll track, carrying its own rectangle: a click
    /// anywhere on it jumps there, and a drag keeps jumping while the
    /// button is held. The rect travels with the hit because the drag has
    /// to keep mapping rows to positions after the frame that drew it,
    /// and re-deriving that geometry from the model is how a drag comes to
    /// fight the mouse instead of tracking it.
    KeysTrack(Rect),
    /// The first-steps section's header, which folds it.
    ToggleFirstSteps,
    /// The mark on that header, which puts the section away for good.
    CloseFirstSteps,
    /// A list's search field. It is drawn on three screens and, until
    /// this, clicking it did nothing at all.
    FocusFilter,
}

impl TuiModel {
    /// The hit under the pointer. Hover and click must resolve the same
    /// rect for the same pixel, so both go through here.
    pub(crate) fn hit_at(&self, column: u16, row: u16) -> Option<&Hit> {
        self.hits
            .iter()
            .find(|(rect, _)| {
                rect.x <= column
                    && column < rect.x + rect.width
                    && rect.y <= row
                    && row < rect.y + rect.height
            })
            .map(|(_, hit)| hit)
    }

    pub(crate) fn click(&mut self, column: u16, row: u16) -> Intent {
        if let Overlay::ActionIndex { scopes, filter, .. } = self.overlay.clone() {
            // A click on a row performs it, the way choosing it with the
            // keyboard does; anywhere else closes without acting.
            let chosen = match self.hit_at(column, row) {
                Some(Hit::ActionIndexEntry(index)) => self
                    .action_index_rows(&scopes, &filter)
                    .get(*index)
                    .map(|(action, _)| *action),
                _ => None,
            };
            self.close_overlay();
            return match chosen {
                Some(action) => self.act(action),
                None => Intent::None,
            };
        }
        if self.overlay != Overlay::None {
            // A dialog's own buttons answer it; a click anywhere else
            // declines, because a click outside a dialog's actionable
            // area must never silently confirm.
            let answer = match self.hit_at(column, row) {
                Some(Hit::OfferedAction(action)) => Some(*action),
                _ => None,
            };
            return match answer {
                Some(action) => self.overlay_action(action),
                None => {
                    self.close_overlay();
                    Intent::None
                }
            };
        }
        let Some(hit) = self.hit_at(column, row).cloned() else {
            return Intent::None;
        };
        match hit {
            Hit::Route(route) => {
                let entering = self.set_route(route);
                self.focus = Focus::Content;
                entering
            }
            Hit::SwitchToWorkspace => Intent::SwitchToWorkspace,
            Hit::MarketplaceRow(index) => {
                self.marketplace_selected = index;
                self.marketplace_drawer_open = true;
                self.focus = Focus::Content;
                self.marketplace_inspect_intent()
            }
            Hit::MarketplaceGroupToggle(marketplace) => {
                self.marketplace_toggle_group(&marketplace);
                self.focus = Focus::Content;
                Intent::None
            }
            Hit::JumpToMarketplace(marketplace) => {
                self.collapsed_marketplaces.remove(&marketplace);
                if let Some(position) = self
                    .marketplace_visible_indices()
                    .iter()
                    .position(|&raw| self.marketplace_rows()[raw].marketplace == marketplace)
                {
                    self.marketplace_selected = position;
                }
                self.marketplace_drawer_open = true;
                let _ = self.set_route(Route::Plugins);
                self.focus = Focus::Content;
                self.marketplace_inspect_intent()
            }
            Hit::OpenLink(marketplace) => self
                .marketplaces
                .iter()
                .find(|entry| entry.name == marketplace)
                .and_then(|entry| entry.homepage.clone())
                .map_or(Intent::None, Intent::OpenLink),
            Hit::ExtensionRow(index) => {
                // Selection opens the drawer immediately — `move_selection`
                // does the same for keyboard navigation, so both input
                // paths agree. No intent: the drawer's content is static
                // catalog metadata, nothing to fetch.
                self.extensions_selected = index;
                self.extension_drawer_open = true;
                self.focus = Focus::Content;
                Intent::None
            }
            Hit::HarnessRow(index) => {
                self.harnesses_selected = index;
                self.harnesses_drawer_open = true;
                self.focus = Focus::Content;
                Intent::None
            }
            Hit::NewProfile => {
                self.overlay = Overlay::NewProfile(String::new());
                self.focus = Focus::Overlay;
                Intent::None
            }
            Hit::DeleteSelectedProfile => {
                if let Some(profile) = self.selected_profile() {
                    self.overlay = Overlay::ConfirmDeleteProfile {
                        id: profile.id.clone(),
                        focus: 1,
                    };
                    self.focus = Focus::Overlay;
                }
                Intent::None
            }
            Hit::ApplySelectedProfile => {
                let harness_ids: Vec<String> =
                    self.profile_harness_selection.iter().cloned().collect();
                self.selected_profile()
                    .filter(|_| !harness_ids.is_empty())
                    .map(|profile| Intent::ApplyProfile {
                        id: profile.id.clone(),
                        harness_ids,
                    })
                    .unwrap_or(Intent::None)
            }
            Hit::ProfileRow(index) => {
                self.profiles_selected = index;
                self.profile_panel = super::model::ProfilePanel::List;
                self.focus = Focus::Content;
                Intent::None
            }
            Hit::PreferenceRow(index) => {
                self.profile_editor_selected = index;
                self.profile_panel = super::model::ProfilePanel::Editor;
                self.focus = Focus::Content;
                Intent::None
            }
            Hit::ProfileHarnessRow(index) => {
                self.profile_harness_selected = index;
                self.profile_panel = super::model::ProfilePanel::Harnesses;
                self.focus = Focus::Content;
                self.toggle_profile_harness_at(index);
                Intent::None
            }
            // Kept for the next run the way every other shape this
            // client remembers is: written once on the way out (see
            // `run_management`), never on the input path.
            Hit::ToggleFirstSteps => {
                self.first_steps_collapsed = !self.first_steps_collapsed;
                Intent::None
            }
            Hit::CloseFirstSteps => {
                self.first_steps_closed = true;
                Intent::None
            }
            Hit::ResizeSidebar => {
                self.dragging_sidebar = true;
                Intent::None
            }
            Hit::ResizePanel(panel) => {
                self.dragging_panel = Some(panel);
                Intent::None
            }
            // Only reachable while the index is open, which the guarded
            // arm above already answered.
            Hit::ActionIndexEntry(_) => Intent::None,
            Hit::OfferedAction(action) => self.act(action),
            Hit::KeysTrack(track) => {
                self.dragging_keys_track = Some(track);
                self.focus = Focus::Content;
                self.scroll_keys_to(track, row);
                Intent::None
            }
            Hit::KeyRow(index) => {
                self.keys_selected = index;
                self.keys_capture = false;
                self.keys_problem = None;
                self.focus = Focus::Content;
                Intent::None
            }
            // A click on a choice is the choice. There is nothing to
            // inspect first here the way a plugin row has: what the row
            // does is drawn on the row.
            Hit::AppearanceRow(index) => {
                self.appearance_selected = index;
                self.focus = Focus::Content;
                self.activate_appearance()
            }
            Hit::FocusFilter => {
                self.filtering = true;
                self.focus = Focus::Content;
                Intent::None
            }
            Hit::PromptHistory(index) => {
                self.focus = Focus::Content;
                self.overview_prompt_selected = index;
                self.activate_selected_prompt()
            }
        }
    }
}
