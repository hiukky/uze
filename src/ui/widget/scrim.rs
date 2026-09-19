//! The backdrop a modal is drawn on.
//!
//! A modal is the one surface that answers for the whole screen: until it
//! is answered or dismissed, nothing behind it responds. That was true
//! before this module existed and nothing said so — a dialog announced
//! itself with its own border, which on a busy screen is one hairline, and
//! the list underneath went on looking exactly as live as it had a frame
//! earlier.
//!
//! So the frame recedes instead. Everything already drawn is pushed toward
//! the backdrop (see [`theme::scrimmed`]), the modal is drawn over it at
//! full contrast, and the depth is visible rather than asserted.
//!
//! Deliberately not a widget an overlay renders for itself: what recedes is
//! whatever the client happened to draw *underneath*, which the overlay
//! knows nothing about. It goes between the two, in the client's own
//! `render`, which is the only place that ordering exists.

use ratatui::layout::Rect;
use ratatui::style::Modifier;

use crate::ui::theme::{self, Token};

/// Push everything drawn in `area` behind whatever is about to be drawn
/// over it.
///
/// Call it after the frame and before the modal; a modal paints its own
/// area over the result, so the scrim can safely cover the whole screen
/// rather than being cut around a rectangle it would have to be told.
pub(crate) fn render(frame: &mut ratatui::Frame<'_>, area: Rect) {
    let buffer = frame.buffer_mut();
    for row in area.top()..area.bottom() {
        for column in area.left()..area.right() {
            let Some(cell) = buffer.cell_mut((column, row)) else {
                continue;
            };
            cell.fg = theme::scrimmed(cell.fg, Token::TextPrimary);
            cell.bg = theme::scrimmed(cell.bg, Token::SurfaceBackground);
            // Weight is contrast too. A heading left bold behind a dialog
            // stays the loudest thing on a screen it is no longer part of.
            // Reverse video is left alone: it is not emphasis, it is which
            // of the two colours the cell shows, and dropping it would take
            // a highlighted row off the screen rather than push it back.
            cell.modifier.remove(Modifier::BOLD | Modifier::UNDERLINED);
        }
    }
}
