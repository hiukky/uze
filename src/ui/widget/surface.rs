//! A surface: a bordered box with a ground, and the rect inside it.
//!
//! Every floating box in the UI — a dialog, a popup, a detail card, an
//! extension's frame — is this one thing. They differ in what they say
//! (a title, a hint along the bottom) and in how much room they leave
//! around it, never in what they are made of.

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, BorderType, Borders, Padding},
};
use uze_theme::Token;

use super::{POPUP_H_PAD, POPUP_V_PAD};
use crate::ui::theme;

/// The inset a surface gives its content when it is floating over the
/// screen rather than sitting in a column.
///
/// [`POPUP_H_PAD`] and [`POPUP_V_PAD`] rather than a number of its own:
/// that pair was already the answer, named in `ui.rs` and used by the
/// popups that compute their own width from it. What the hand-built
/// surfaces disagreed about was never which value was right — four of the
/// five spellings simply did not reach for the constant.
const FLOATING_PADDING: Padding = Padding {
    left: POPUP_H_PAD,
    right: POPUP_H_PAD,
    top: POPUP_V_PAD,
    bottom: 0,
};

/// A bordered box. Built, then drawn once with [`Surface::render`], which
/// answers with the rect inside the border — what the caller actually has
/// to fill.
#[derive(Clone, Debug)]
pub(crate) struct Surface {
    borders: Borders,
    border_type: BorderType,
    /// The hairline's colour. A token rather than a `Color` so a surface
    /// that answers the pointer — a sidebar's divider under a drag —
    /// states the meaning of the change rather than resolving it early.
    tone: Token,
    ground: Option<Token>,
    title: Option<Line<'static>>,
    hint: Option<Line<'static>>,
    padding: Padding,
}

impl Surface {
    /// A box floating over the rest of the screen: the full hairline, the
    /// backdrop's own ground so it reads as still part of this app rather
    /// than a different layer, and room around its content.
    ///
    /// The caller must have cleared what is underneath — a surface draws
    /// over, it does not erase.
    pub(crate) fn floating() -> Self {
        Self {
            borders: Borders::ALL,
            border_type: BorderType::Plain,
            tone: Token::BorderDefault,
            ground: Some(Token::SurfaceBackground),
            title: None,
            hint: None,
            padding: FLOATING_PADDING,
        }
    }

    /// A box inside a column, framing content that is already on screen —
    /// a detail card, a list's frame. The same hairline as
    /// [`floating`](Self::floating) and none of its inset: a card is
    /// already inside something that gave it room.
    pub(crate) fn card() -> Self {
        Self {
            padding: Padding::ZERO,
            ..Self::floating()
        }
    }

    /// A card the keyboard can land on, whose border is what says so. The
    /// rounded corner is the one place the UI uses one, and it is load
    /// bearing: these cards are chosen from rather than read, and the
    /// corner is what separates "pick one of these" from every framed
    /// region that is merely showing something.
    pub(crate) fn selectable(selected: bool) -> Self {
        Self {
            border_type: BorderType::Rounded,
            tone: if selected {
                Token::Accent
            } else {
                Token::BorderFaint
            },
            ground: None,
            ..Self::card()
        }
    }

    /// The heading along the top. Drawn in the accent, bold — a surface's
    /// title names the surface, and there is only one weight for that.
    pub(crate) fn title(mut self, title: impl Into<Line<'static>>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// A note along the bottom edge, right-aligned: what this surface
    /// answers to, or that its content is longer than the box.
    pub(crate) fn hint(mut self, hint: impl Into<Line<'static>>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Room around the content, when this surface needs other than its
    /// kind's own. Reach for it only with a reason worth a comment: the
    /// paddings that disagreed before this module existed all looked
    /// exactly this deliberate.
    pub(crate) fn padding(mut self, padding: Padding) -> Self {
        self.padding = padding;
        self
    }

    /// Draws into `area` and answers with the rect inside the border.
    pub(crate) fn render(self, frame: &mut ratatui::Frame<'_>, area: Rect) -> Rect {
        let block = self.block();
        let inner = block.inner(area);
        frame.render_widget(block, area);
        inner
    }

    /// The surface as a ratatui [`Block`], for the caller that hands it to
    /// a widget which draws its own frame — a `Paragraph` that scrolls
    /// inside it, say — rather than drawing into the rect itself.
    pub(crate) fn into_block(self) -> Block<'static> {
        self.block()
    }

    fn block(self) -> Block<'static> {
        let mut block = Block::default()
            .borders(self.borders)
            .border_type(self.border_type)
            .border_style(theme::fg(self.tone))
            .padding(self.padding);
        if let Some(ground) = self.ground {
            block = block.style(theme::bg(ground));
        }
        if let Some(title) = self.title {
            block = block.title(title).title_style(
                Style::default()
                    .fg(theme::color(Token::Accent))
                    .add_modifier(Modifier::BOLD),
            );
        }
        if let Some(hint) = self.hint {
            block = block.title_bottom(hint.right_aligned());
        }
        block
    }
}

/// Paints `area` in one ground and nothing else — a selected row's tint, a
/// pane's backdrop, the wash under a scrim.
///
/// Its own function because a fill is not a surface with the borders
/// turned off: it says "this region is this colour", carries no hairline
/// to align with anything, and leaves no inner rect worth answering with.
pub(crate) fn fill(frame: &mut ratatui::Frame<'_>, area: Rect, ground: Token) {
    frame.render_widget(Block::default().style(theme::bg(ground)), area);
}

/// The frame's own ground: the backdrop everything is drawn on, and the
/// ink any text that names no colour of its own inherits.
///
/// Distinct from [`fill`] because it is the one surface with nothing
/// behind it. Every other ground is a region *on* this one, and says so by
/// naming a token; this one is what "no colour stated" resolves to.
pub(crate) fn root(frame: &mut ratatui::Frame<'_>, area: Rect) {
    frame.render_widget(
        Block::default().style(theme::on(Token::TextPrimary, Token::SurfaceBackground)),
        area,
    );
}
