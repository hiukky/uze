//! A row: one line of a list, and the ground it is drawn on.
//!
//! Every list in the product answers the same two questions on every line
//! — is this the one the keyboard is on, and is the pointer over it — and
//! before this module each of them answered in its own spelling. The
//! ground a selected row takes is the most-copied decision in the UI.

use ratatui::{
    style::{Color, Style},
    text::Span,
};
use uze_theme::Token;

use crate::ui::theme;

/// Where a row stands relative to the reader.
///
/// Selection and hover are separate questions, not two points on one
/// scale: the keyboard is on exactly one row and the pointer may be on
/// another, and a list that collapses them loses track of where a click
/// would land.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum RowState {
    /// Neither the keyboard nor the pointer.
    #[default]
    Resting,
    /// The pointer is over it.
    Hovered,
    /// The keyboard is on it.
    Selected,
}

impl RowState {
    /// The two questions a caller already has the answers to, as the one
    /// value the row is drawn from. Selection wins: a row under the
    /// pointer *and* under the keyboard is where a press would land, and
    /// that is what the stronger ground says.
    pub(crate) fn of(selected: bool, hovered: bool) -> Self {
        match (selected, hovered) {
            (true, _) => Self::Selected,
            (false, true) => Self::Hovered,
            (false, false) => Self::Resting,
        }
    }

    /// The ground this row is drawn on, or `None` where it takes the
    /// surface it already sits on.
    pub(crate) fn ground(self) -> Option<Token> {
        match self {
            Self::Resting => None,
            Self::Hovered => Some(Token::SurfaceHover),
            Self::Selected => Some(Token::SurfaceSelected),
        }
    }
}

/// Gives every span in `spans` the ground `bg`, then extends the line to
/// `width` with it.
///
/// A row's ground has to reach the full width or the fill stops where the
/// text does, which reads as a ragged highlight rather than as a selected
/// row. Spans keep their own foreground; only the ground is imposed.
pub(crate) fn pad_to(spans: &mut Vec<Span<'_>>, width: u16, bg: Color) {
    for span in spans.iter_mut() {
        span.style = span.style.bg(bg);
    }
    let used: usize = spans.iter().map(Span::width).sum();
    let gap = (width as usize).saturating_sub(used);
    spans.push(Span::styled(" ".repeat(gap), Style::default().bg(bg)));
}

/// Fills the row to `width` on the ground its state calls for, and leaves
/// a resting row untouched.
///
/// Untouched rather than padded with the surface behind it: a list draws
/// on whatever ground its column already has, and a resting row that
/// painted its own would have to know which — the coupling that had every
/// list spelling this out for itself.
pub(crate) fn fill(spans: &mut Vec<Span<'_>>, width: u16, state: RowState) {
    let Some(ground) = state.ground() else {
        return;
    };
    pad_to(spans, width, theme::color(ground));
}
