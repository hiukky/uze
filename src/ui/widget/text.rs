//! Fitting text to the room there is.
//!
//! A terminal has no reflow and no tooltip: text that does not fit is text
//! the reader loses, so every column in the product cuts, and cutting is
//! one decision — where, and with what mark — not a dozen.
//!
//! The two halves were split by accident rather than by design. [`elide`]
//! sat in `ui.rs` and nine files reached for it; [`clip`] sat in
//! `management.rs`, a *screen*, and four other screens reached across for
//! it as `super::super::management::clip_line`. A primitive a screen
//! exports is a primitive in the wrong place: nothing about cutting a line
//! belongs to the screen that happened to need it first.

use ratatui::text::Line;

use crate::ui::theme::{self, Symbol};

/// `text`, cut to `width` columns with the ellipsis mark when it does not
/// fit.
///
/// The mark's own width comes from the theme: a glyph set that spells the
/// ellipsis with three periods takes three columns where `…` takes one,
/// and a cut measured against the wrong one overflows the column it was
/// meant to fit.
pub(crate) fn elide(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    let Some(kept) = width.checked_sub(theme::width(Symbol::Ellipsis) as usize) else {
        return String::new();
    };
    let mut kept: String = text.chars().take(kept).collect();
    kept.push_str(&theme::glyph(Symbol::Ellipsis));
    kept
}

/// Cuts `line` to `max` columns in place, keeping every span that fits
/// whole and eliding the one that straddles the edge.
///
/// Spans rather than characters because a line is styled: cutting the
/// string would lose which part of it was the key and which the
/// description, and the last visible span has to keep its own style right
/// up to the mark.
pub(crate) fn clip(line: &mut Line<'static>, max: usize) {
    let mut used = 0usize;
    let mut cut = None;
    for (index, span) in line.spans.iter().enumerate() {
        let width = span.width();
        if used + width <= max {
            used += width;
        } else {
            cut = Some(index);
            break;
        }
    }
    let Some(index) = cut else {
        return;
    };
    line.spans[index].content =
        std::borrow::Cow::Owned(elide(&line.spans[index].content, max - used));
    line.spans.truncate(index + 1);
}
