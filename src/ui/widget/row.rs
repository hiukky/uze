//! A row: one line of a list, and the ground it is drawn on.
//!
//! Every list in the product answers the same two questions on every line
//! — is this the one the keyboard is on, and is the pointer over it — and
//! before this module each of them answered in its own spelling. The
//! ground a selected row takes is the most-copied decision in the UI.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use uze_theme::Token;

use super::{TRAILING_PAD, text};
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

/// Appends `text` pinned to the row's right edge, `TRAILING_PAD` off the
/// divider — the column the agent rows keep their alias in.
///
/// `text` is elided rather than allowed to overflow. It is the row's
/// caption, and a caption that does not fit used to run past the edge and
/// be cut there by the frame — which is how a long branch name on the Git
/// section header became an unreadable fragment with no "…" to say it had
/// been shortened.
/// The same, with the text slid sideways by `offset` columns when it is
/// too long for the room: a caption that cannot be shortened without
/// losing the end of it — a branch name is read from both ends — passes
/// through instead of stopping at an "…".
///
/// Only while something is asking to read it, which is the caller's
/// business: this draws the frame it is given an offset for and says
/// whether there was anything to slide. The run is the text, a gap, and
/// the text again, so it leaves on one side as it arrives on the other
/// rather than jumping back to the start.
pub(crate) fn push_trailing_marquee<'a>(
    spans: &mut Vec<Span<'a>>,
    width: u16,
    text: String,
    hue: Color,
    offset: usize,
) -> bool {
    let leading: u16 = spans.iter().map(|span| span.width() as u16).sum();
    let room = usize::from(width.saturating_sub(leading + TRAILING_PAD + 1).max(1));
    let length = text.chars().count();
    if length <= room {
        push_trailing(spans, width, text, hue);
        return false;
    }
    /// Blank columns between the end of the run and its start coming
    /// round again, so the two ends are never read as one word.
    const GAP: usize = 3;
    let cycle = length + GAP;
    let run: Vec<char> = text
        .chars()
        .chain(std::iter::repeat_n(' ', GAP))
        .chain(text.chars())
        .collect();
    let start = offset % cycle;
    let window: String = run.iter().skip(start).take(room).collect();
    spans.push(Span::raw(" ".repeat(usize::from(width).saturating_sub(
        usize::from(leading) + room + usize::from(TRAILING_PAD),
    ))));
    spans.push(Span::styled(window, Style::default().fg(hue)));
    spans.push(Span::raw(" ".repeat(TRAILING_PAD as usize)));
    true
}

pub(crate) fn push_trailing<'a>(spans: &mut Vec<Span<'a>>, width: u16, text: String, hue: Color) {
    let leading: u16 = spans.iter().map(|span| span.width() as u16).sum();
    // One column of gap between the leading spans and the caption, so the
    // two never read as one word.
    let room = width.saturating_sub(leading + TRAILING_PAD + 1).max(1);
    let text = text::elide(&text, room as usize);
    let used = leading + text.chars().count() as u16 + TRAILING_PAD;
    let gap = width.saturating_sub(used).max(1);
    spans.push(Span::raw(" ".repeat(gap as usize)));
    spans.push(Span::styled(text, Style::default().fg(hue)));
    spans.push(Span::raw(" ".repeat(TRAILING_PAD as usize)));
}

/// The first row of an informational popup: its name, and the key that
/// dismisses it pinned to the right.
pub(crate) fn title_row(name: &str, dismiss: &str, width: usize) -> ratatui::text::Line<'static> {
    let gap = width
        .saturating_sub(name.chars().count() + dismiss.chars().count())
        .max(1);
    Line::from(vec![
        Span::styled(
            name.to_owned(),
            Style::default()
                .fg(theme::color(Token::TextBright))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" ".repeat(gap)),
        Span::styled(dismiss.to_owned(), theme::fg(Token::TextMuted)),
    ])
}
