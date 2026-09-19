//! A chip: a short filled label that stands where a control stands.
//!
//! The header's controls are chips — a key hint, a request number, a
//! delivery button — and so are the reports drawn beside them. Both wear
//! the same shape, and [`ChipState::Static`] is what keeps that honest:
//! a chip that cannot be pressed sits on a recessed ground, so the shape
//! alone never promises a press.
//!
//! It lived in the workspace's own renderer, which meant the management
//! screens could not draw one — the second header that needed a control
//! of this shape would have built a third.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
use uze_theme::Token;

use crate::ui::{theme, widget::row};

/// The column of air each side of a chip's label. Part of the control: it
/// is filled, hovered and clicked exactly as the glyphs are, which is why
/// the rect is measured from it rather than around it.
pub(crate) const PAD: u16 = 1;

/// What the pointer is doing to a chip — or that it is not a control at
/// all.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ChipState {
    Resting,
    Hovered,
    Pressed,
    /// Not a control: drawn in a control's shape because it stands where
    /// one stands, and recessed rather than raised so the shape alone
    /// never promises a press ("⣾ delivering" is a report; "⇧6 #20" is a
    /// button).
    Static,
}

impl ChipState {
    /// The label colour and the ground under it, for a chip of `hue`.
    ///
    /// Both together rather than leaving the label to the caller: pressing
    /// inverts — the hue becomes the fill and the label drops to the
    /// backdrop — and that is the one state which overrules a label's own
    /// colour.
    pub(crate) fn skin(self, hue: Color) -> (Color, Color) {
        match self {
            Self::Resting => (hue, theme::color(Token::SurfaceRaised)),
            Self::Hovered => (hue, theme::color(Token::SurfaceHover)),
            Self::Pressed => (theme::color(Token::SurfaceBackground), hue),
            Self::Static => (hue, theme::color(Token::SurfaceRecessed)),
        }
    }
}

/// One header control, or one report wearing a control's shape.
#[derive(Clone, Debug)]
pub(crate) struct Chip {
    label: String,
    /// Resolved rather than a [`Token`]: a chip's hue is usually the
    /// answer of a state machine the caller owns — a delivery's ending, a
    /// slot's state — which has already resolved it.
    hue: Color,
    state: ChipState,
}

impl Chip {
    pub(crate) fn new(label: impl Into<String>, hue: Color, state: ChipState) -> Self {
        Self {
            label: label.into(),
            hue,
            state,
        }
    }

    /// The columns this chip occupies, label plus its padding.
    pub(crate) fn width(&self) -> u16 {
        Span::raw(&self.label).width() as u16 + 2 * PAD
    }

    /// Where this chip sits when its right edge is `right` — how a header
    /// lays its controls out, from the edge inward.
    pub(crate) fn rect_ending_at(&self, right: u16, row: u16) -> Rect {
        let width = self.width();
        Rect::new(right.saturating_sub(width), row, width, 1)
    }

    pub(crate) fn render(&self, frame: &mut ratatui::Frame<'_>, rect: Rect) {
        let (label, ground) = self.state.skin(self.hue);
        let mut spans = vec![
            Span::raw(" ".repeat(PAD as usize)),
            Span::styled(
                self.label.clone(),
                Style::default().fg(label).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" ".repeat(PAD as usize)),
        ];
        row::pad_to(&mut spans, rect.width, ground);
        frame.render_widget(Paragraph::new(Line::from(spans)), rect);
    }
}
