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
    /// A row's `⋯` — what can be done to that row, from the row itself.
    /// The whole point of it is that nobody has to know a letter.
    RowActions(usize),
    /// One entry of the open row menu, by index into its offers.
    RowMenuEntry(usize),
    /// One row of the open index of everything, by position in it.
    ActionIndexEntry(usize),
    /// An action offered by a detail view's own action bar.
    OfferedAction(uze_keys::Action),
    /// One line of the Keys screen.
    KeyRow(usize),
    /// The Keys list's scroll track, carrying its own rectangle: a click
    /// anywhere on it jumps there, and a drag keeps jumping while the
    /// button is held. The rect travels with the hit because the drag has
    /// to keep mapping rows to positions after the frame that drew it,
    /// and re-deriving that geometry from the model is how a drag comes to
    /// fight the mouse instead of tracking it.
    KeysTrack(Rect),
    /// The Keys screen's "change this key" target — the next keystroke
    /// becomes the binding.
    CaptureKey,
    /// Put back what uze ships with, for the selected line.
    ResetKey,
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

    /// Asks whatever is under the pointer what can be done to it.
    pub(crate) fn right_click(&mut self, column: u16, row: u16) -> Intent {
        if self.overlay != Overlay::None {
            return Intent::None;
        }
        self.row_menu = None;
        let row_index = match self.hit_at(column, row) {
            Some(Hit::MarketplaceRow(index) | Hit::RowActions(index)) => *index,
            Some(Hit::HarnessRow(index)) => *index,
            Some(Hit::ProfileRow(index)) => *index,
            _ => return Intent::None,
        };
        self.select_row(row_index);
        self.open_row_actions()
    }

    /// Moves the selection to `index` on whichever screen is open — the
    /// step a pointer gesture takes before acting on a row.
    pub(crate) fn select_row(&mut self, index: usize) {
        match self.route {
            Route::Plugins => self.marketplace_selected = index,
            Route::Harnesses => self.harnesses_selected = index,
            Route::Profiles => self.profiles_selected = index,
            _ => {}
        }
        self.focus = Focus::Content;
    }

    /// The rect the selected row was last drawn at, so a menu raised from
    /// the keyboard anchors where the pointer would have raised it.
    pub(crate) fn selected_row_rect(&self) -> Option<Rect> {
        let wanted = match self.route {
            Route::Plugins => Hit::MarketplaceRow(self.marketplace_selected),
            Route::Harnesses => Hit::HarnessRow(self.harnesses_selected),
            Route::Profiles => Hit::ProfileRow(self.profiles_selected),
            _ => return None,
        };
        self.hits
            .iter()
            .find(|(_, hit)| *hit == wanted)
            .map(|(rect, _)| *rect)
    }

    pub(crate) fn click(&mut self, column: u16, row: u16) -> Intent {
        if let Some(menu) = self.row_menu.clone() {
            // A click inside the menu chooses; anywhere else declines it,
            // the same way a click outside a dialog declines.
            let chosen = match self.hit_at(column, row) {
                Some(Hit::RowMenuEntry(index)) => menu.offers.get(*index).map(|offer| offer.action),
                _ => None,
            };
            self.row_menu = None;
            return match chosen {
                Some(action) => self.act(action),
                None => Intent::None,
            };
        }
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
                self.set_route(route);
                self.focus = Focus::Content;
                Intent::None
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
                self.set_route(Route::Plugins);
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
            Hit::ResizeSidebar => {
                self.dragging_sidebar = true;
                Intent::None
            }
            Hit::ResizePanel(panel) => {
                self.dragging_panel = Some(panel);
                Intent::None
            }
            Hit::RowActions(index) => {
                self.select_row(index);
                self.open_row_actions()
            }
            // Only reachable while the menu or the index is open, which
            // the guarded arms above already answered.
            Hit::RowMenuEntry(_) | Hit::ActionIndexEntry(_) => Intent::None,
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
            Hit::CaptureKey => {
                self.keys_capture = !self.keys_capture;
                self.keys_problem = None;
                self.focus = Focus::Content;
                Intent::None
            }
            Hit::ResetKey => self.reset_selected_key(),
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
