//! TUI — mouse hit-testing: mapping a clicked screen coordinate back to the
//! on-screen target it landed on.

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
    /// The external-link glyph on a plugin detail's Source card — jumps
    /// list selection to that marketplace's header, expanding it first if
    /// it's currently collapsed.
    JumpToMarketplace(String),
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
        if self.overlay != Overlay::None {
            // Any click dismisses/declines an overlay — a click outside a
            // dialog's actionable area should never silently confirm.
            self.close_overlay();
            return Intent::None;
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
            Hit::PromptHistory(index) => {
                self.focus = Focus::Content;
                self.overview_prompt_selected = index;
                self.activate_selected_prompt()
            }
        }
    }
}
