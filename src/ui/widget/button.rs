//! A button: a label the pointer can press, and the row they are laid out
//! in.
//!
//! The style was already shared before this module existed; the *layout*
//! was not, and the two places that lay buttons out — a dialog's answers
//! and a drawer's offers — each computed the same label padding and the
//! same two-column gap from scratch.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::Span,
    widgets::Paragraph,
};
use uze_theme::Token;

use crate::ui::theme;

/// The columns between one button and the next. Wide enough that two
/// labels never read as one, narrow enough that a row of them still reads
/// as a group.
const GAP: u16 = 2;

/// Which end of the row the buttons sit at.
///
/// It decides more than position. A right-aligned row has to know its full
/// width before it can place the first button, so a row too narrow for all
/// of them draws none — a dialog showing `Cancel` with its `Delete` cut off
/// would be answering a question the reader cannot see. A left-aligned row
/// places each button as it goes and simply stops at the edge, which is
/// what a drawer's offers want: the first few are the ones that matter, and
/// the rest are reachable by keyboard.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Align {
    Left,
    Right,
}

/// One pressable label.
#[derive(Clone, Debug)]
pub(crate) struct Button {
    label: String,
    hue: Token,
    /// Whether this is the button the eye should land on — the focused
    /// answer, the hovered offer. Filled rather than tinted.
    strong: bool,
    ground: Token,
}

impl Button {
    /// A button reading `label`, in the hue that says what pressing it
    /// does: the accent for what builds, the danger hue for what destroys,
    /// a neutral for everything else.
    pub(crate) fn new(label: impl Into<String>, hue: Token) -> Self {
        Self {
            label: label.into(),
            hue,
            strong: false,
            ground: Token::SurfaceBackground,
        }
    }

    pub(crate) fn strong(mut self, strong: bool) -> Self {
        self.strong = strong;
        self
    }

    /// The surface this button is drawn on, which its resting tint is
    /// mixed into — a button on a recessed drawer and the same button on
    /// the backdrop are not the same colour.
    pub(crate) fn ground(mut self, ground: Token) -> Self {
        self.ground = ground;
        self
    }

    /// The drawn text: the label with a column of air each side, so the
    /// filled state reads as a control rather than as highlighted text.
    fn text(&self) -> String {
        format!("  {}  ", self.label)
    }

    /// The columns this button occupies, for a caller sizing the slot it
    /// will go in before there is a frame to draw into.
    pub(crate) fn width(&self) -> u16 {
        self.text().chars().count() as u16
    }

    /// Draws this one button into `rect`, for a control that sits in a
    /// layout of its own rather than in a row of answers.
    pub(crate) fn render(&self, frame: &mut ratatui::Frame<'_>, rect: Rect) {
        frame.render_widget(
            Paragraph::new(Span::styled(self.text(), self.style())),
            rect,
        );
    }

    fn style(&self) -> Style {
        let style = if self.strong {
            theme::on(Token::SurfaceBackground, self.hue)
        } else {
            Style::default()
                .fg(theme::color(self.hue))
                .bg(theme::softened(self.hue, self.ground))
        };
        style.add_modifier(Modifier::BOLD)
    }
}

/// Lays `buttons` out along `row` and draws them, answering with the rect
/// each one took.
///
/// The caller registers those rects itself, because only it knows whether
/// they go on top of the hit list or beneath what is already there — a
/// dialog draws over a screen whose own targets are still registered, and
/// has to be asked first.
pub(crate) fn button_row<T: Clone>(
    frame: &mut ratatui::Frame<'_>,
    row: Rect,
    buttons: &[(Button, T)],
    align: Align,
) -> Vec<(Rect, T)> {
    if buttons.is_empty() || row.height == 0 {
        return Vec::new();
    }
    let total: u16 = buttons
        .iter()
        .map(|(button, _)| button.width())
        .sum::<u16>()
        + GAP * (buttons.len() as u16 - 1);
    let mut x = match align {
        Align::Right if row.width < total => return Vec::new(),
        Align::Right => row.right() - total,
        Align::Left => row.x,
    };
    let mut placed = Vec::with_capacity(buttons.len());
    for (button, target) in buttons {
        let width = button.width();
        if x + width > row.right() {
            break;
        }
        let rect = Rect::new(x, row.y, width, 1);
        button.render(frame, rect);
        placed.push((rect, target.clone()));
        x += width + GAP;
    }
    placed
}
