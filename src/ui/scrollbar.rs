//! The one scrollbar the product draws.
//!
//! It arrived twice — the Keys list grew one, and the code surface grew
//! two more — and the second time is when a shared thing is worth making,
//! not the first. What that duplication produced in the meantime is
//! exactly what a shared control prevents: two grooves with different
//! glyphs and different hues, doing the same job on two screens of the
//! same product.
//!
//! It answers the question a long list cannot answer by itself — whether
//! there is more, and how much — and it takes the answer back: a click
//! jumps there and a drag keeps jumping, which is the gesture anyone who
//! has seen a scrollbar tries first. Drawing something that looks like a
//! control and does nothing is worse than not drawing it.
//!
//! # Two questions, one control
//!
//! A pointer on the track means different things to different lists, and
//! the difference is real rather than an inconsistency to iron out:
//!
//! - A list whose window is *derived from the selection* — the Keys
//!   screen — asks [`Scrollbar::item_at`]: the track is a picture of the
//!   whole list, so its top row is the first item and its bottom row the
//!   last.
//! - A list that keeps a scroll offset of its own — the code surface —
//!   asks [`Scrollbar::first_at`]: the track is a picture of where the
//!   *window* can go, so its bottom is the last screenful rather than the
//!   last line.
//!
//! Naming both keeps each screen honest about which it is, instead of one
//! of them quietly scrolling a line short.

use ratatui::{Frame, layout::Rect, text::Span, widgets::Paragraph};

use super::theme::{self, Symbol, Token};

/// A scrollbar: where it is drawn, and what it is measuring.
///
/// Constructed only when there is something to scroll — see
/// [`Scrollbar::measure`] — so holding one is itself the answer to "is
/// there a scrollbar here".
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Scrollbar {
    pub(crate) track: Rect,
    /// Rows the surface can show at once.
    pub(crate) visible: usize,
    /// Rows it has.
    pub(crate) total: usize,
}

impl Scrollbar {
    /// A scrollbar for a list of `total` rows showing `visible` of them,
    /// or `None` when everything fits.
    ///
    /// `None` rather than an empty groove: a scrollbar on a list that
    /// fits says the opposite of the truth, and the column it would take
    /// is better spent on the list.
    pub(crate) fn measure(track: Rect, visible: usize, total: usize) -> Option<Self> {
        (total > visible && track.height > 0 && visible > 0).then_some(Self {
            track,
            visible,
            total,
        })
    }

    /// How wide a groove is, from the theme rather than from a constant:
    /// a theme may replace the handle with a glyph two cells wide, and a
    /// column laid out from `1` would shear the row beside it.
    pub(crate) fn width() -> u16 {
        theme::width(Symbol::ScrollThumb).max(1)
    }

    /// The first row a pointer at `row` is asking to see.
    ///
    /// Measured from the middle of the handle, not its top, so the
    /// content does not jump the moment a drag begins — taking hold of a
    /// handle and not moving must scroll nothing.
    pub(crate) fn first_at(self, row: u16) -> usize {
        let scrollable = self.total.saturating_sub(self.visible);
        if scrollable == 0 {
            return 0;
        }
        let thumb = self.thumb_height();
        let travel = usize::from(self.track.height.saturating_sub(thumb)).max(1);
        let offset = usize::from(row.saturating_sub(self.track.y).saturating_sub(thumb / 2));
        (offset * scrollable).div_ceil(travel).min(scrollable)
    }

    /// The item a pointer at `row` is pointing at, for a list whose window
    /// follows its selection.
    ///
    /// The whole list, not the scrollable range: the top of the track is
    /// the first item and the bottom is the last, which is what a picture
    /// of the whole list means.
    pub(crate) fn item_at(self, row: u16) -> usize {
        let last = self.total.saturating_sub(1);
        let travel = usize::from(self.track.height.saturating_sub(1));
        if travel == 0 {
            return 0;
        }
        let offset = usize::from(row.saturating_sub(self.track.y)).min(travel);
        offset * last / travel
    }

    /// Draws the handle, with `first` the row at the top of the window.
    ///
    /// The handle only — the groove it runs in is left blank. A drawn
    /// groove is a second full-height line, and where one of these sits
    /// beside a panel divider that is exactly what it looks like: two
    /// bars arguing about which is the edge. Undrawn, the divider is the
    /// line and the handle is a mark on it saying where you are, which is
    /// the thing the control is actually for.
    ///
    /// The whole track stays the drag target either way — see
    /// [`Scrollbar::track`]. A groove you cannot see is still a groove
    /// you can grab.
    ///
    /// The handle is a *heavier* vertical rule rather than a block, so
    /// that where it sits on a panel divider the line thickens instead of
    /// stepping sideways: a block element is flush left in its cell and a
    /// box-drawing rule is centred, and mixing the two is a line with a
    /// jog in it.
    pub(crate) fn render(self, frame: &mut Frame<'_>, first: usize) {
        let thumb = self.thumb_height();
        let top = self.thumb_y(first);
        for row in 0..thumb {
            let y = top.saturating_add(row);
            if y >= self.track.bottom() {
                break;
            }
            frame.render_widget(
                Paragraph::new(Span::styled(
                    theme::glyph(Symbol::ScrollThumb),
                    theme::fg(Token::BorderDefault),
                )),
                Rect::new(self.track.x, y, self.track.width, 1),
            );
        }
    }

    /// How tall the handle is: the share of the whole that is on screen,
    /// floored at one row so it never vanishes on a list long enough to
    /// make the fraction round to nothing — which is exactly the list that
    /// most needs it.
    fn thumb_height(self) -> u16 {
        let share = usize::from(self.track.height) * self.visible / self.total.max(1);
        (share as u16).clamp(1, self.track.height.max(1))
    }

    fn thumb_y(self, first: usize) -> u16 {
        let scrollable = self.total.saturating_sub(self.visible);
        if scrollable == 0 {
            return self.track.y;
        }
        let travel = self.track.height.saturating_sub(self.thumb_height());
        self.track.y + (usize::from(travel) * first.min(scrollable) / scrollable) as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_that_fits_has_no_scrollbar() {
        assert!(Scrollbar::measure(Rect::new(0, 0, 1, 10), 10, 10).is_none());
        assert!(Scrollbar::measure(Rect::new(0, 0, 1, 10), 10, 3).is_none());
        assert!(Scrollbar::measure(Rect::new(0, 0, 1, 10), 10, 11).is_some());
    }

    /// The window's bottom is the last *screenful*, not the last line:
    /// dragging to the end must not leave a page of blank rows below the
    /// content.
    #[test]
    fn dragging_a_window_stops_at_the_last_screenful() {
        let bar = Scrollbar::measure(Rect::new(80, 4, 1, 20), 20, 100).expect("it scrolls");

        assert_eq!(bar.first_at(bar.track.y), 0, "the top is the top");
        assert_eq!(bar.first_at(bar.track.bottom()), 80);
        let middle = bar.first_at(bar.track.y + bar.track.height / 2);
        assert!(
            (30..=50).contains(&middle),
            "halfway down is about halfway through, was {middle}"
        );
    }

    /// A list whose window follows its selection asks the other question:
    /// the bottom of the track is the last *item*.
    #[test]
    fn dragging_a_selection_reaches_the_last_item() {
        let bar = Scrollbar::measure(Rect::new(0, 0, 1, 10), 10, 100).expect("it scrolls");

        assert_eq!(bar.item_at(bar.track.y), 0);
        assert_eq!(bar.item_at(bar.track.bottom()), 99, "the last item, not 90");
    }

    /// The handle is the share of the whole on screen, and never nothing.
    #[test]
    fn the_handle_never_vanishes_on_the_list_that_most_needs_it() {
        let bar = Scrollbar::measure(Rect::new(0, 0, 1, 10), 10, 100_000).expect("it scrolls");
        assert_eq!(bar.thumb_height(), 1);

        let half = Scrollbar::measure(Rect::new(0, 0, 1, 10), 5, 10).expect("it scrolls");
        assert_eq!(half.thumb_height(), 5);
    }
}
