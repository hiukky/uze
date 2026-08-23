//! TUI — mouse hit-testing: mapping a clicked screen coordinate back to the
//! on-screen target it landed on.

use super::model::{Focus, Overlay, Route, TuiModel};
use super::worker::Intent;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Hit {
    Route(Route),
    PluginRow(usize),
    MarketplaceRow(usize),
    /// A marketplace group's header row — clicking it expands/collapses
    /// that group instead of selecting a plugin.
    MarketplaceGroupToggle(String),
    /// The external-link glyph on a plugin detail's Source card — jumps
    /// list selection to that marketplace's header, expanding it first if
    /// it's currently collapsed.
    JumpToMarketplace(String),
    HarnessRow(usize),
}

impl TuiModel {
    pub(crate) fn click(&mut self, column: u16, row: u16) -> Intent {
        if self.overlay != Overlay::None {
            // Any click dismisses/declines an overlay — a click outside a
            // dialog's actionable area should never silently confirm.
            self.close_overlay();
            return Intent::None;
        }
        let Some(hit) = self
            .hits
            .iter()
            .find(|(rect, _)| {
                rect.x <= column
                    && column < rect.x + rect.width
                    && rect.y <= row
                    && row < rect.y + rect.height
            })
            .map(|(_, hit)| hit.clone())
        else {
            return Intent::None;
        };
        match hit {
            Hit::Route(route) => {
                let deep = self.route_change_needs_deep_health(route);
                self.set_route(route);
                self.focus = Focus::Content;
                if deep {
                    Intent::RefreshDoctor
                } else {
                    Intent::None
                }
            }
            Hit::PluginRow(index) => {
                // Selecting only — same as arrow-key navigation. The
                // richer async inspect fetch (deliveries/managed state,
                // the "Inspecting…" status flash) is reserved for an
                // explicit Enter, so clicking through the list to browse
                // doesn't fire a fetch — and the noisy status line with
                // it — on every single click.
                self.plugins_selected = index;
                self.focus = Focus::Content;
                Intent::None
            }
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
                    .position(|&raw| self.marketplace_plugins[raw].marketplace == marketplace)
                {
                    self.marketplace_selected = position;
                }
                self.marketplace_drawer_open = true;
                self.set_route(Route::Marketplace);
                self.focus = Focus::Content;
                self.marketplace_inspect_intent()
            }
            Hit::HarnessRow(index) => {
                self.harnesses_selected = index;
                self.harnesses_drawer_open = true;
                self.focus = Focus::Content;
                Intent::None
            }
        }
    }
}
