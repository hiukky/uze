//! TUI view — the catalog of cards Integrations and Extensions share.
//!
//! One grid and one card, so the two screens that list what UZE works
//! with cannot drift apart in how they list it: the Extensions copy had
//! already grown a one-column width, brighter text and a badge glued to
//! the name on a narrow card before the two were one.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

use crate::ui::theme::{self, Token};
use crate::ui::widget::RowState;

const GAP: u16 = 1;
const CARD_HEIGHT: u16 = 7;

fn columns(width: u16) -> u16 {
    if width >= 110 { 3 } else { 2 }
}

/// The rects of the first `count` cards laid out over `area`, row by row,
/// ending at the last card that fits whole.
pub(crate) fn cards(area: Rect, count: usize) -> impl Iterator<Item = Rect> {
    let columns = columns(area.width);
    let width = area.width.saturating_sub(GAP * (columns - 1)) / columns;
    (0..count as u16)
        .map(move |position| {
            Rect::new(
                area.x + position % columns * (width + GAP),
                area.y + position / columns * (CARD_HEIGHT + GAP),
                width,
                CARD_HEIGHT,
            )
        })
        .take_while(move |rect| rect.bottom() <= area.bottom())
}

/// The rows `count` cards take over `width`, gaps included.
pub(crate) fn height(width: u16, count: usize) -> u16 {
    (count as u16).div_ceil(columns(width)) * (CARD_HEIGHT + GAP)
}

/// What sits right of a card's name: the word with its mark where the
/// card is wide enough, the mark alone where it is not.
pub(crate) struct Badge {
    pub(crate) mark: String,
    pub(crate) label: &'static str,
    pub(crate) color: Color,
}

pub(crate) struct Card<'a> {
    pub(crate) name: &'a str,
    pub(crate) badge: Option<Badge>,
    pub(crate) description: &'a str,
    pub(crate) caption: Line<'a>,
}

pub(crate) fn render_card(
    frame: &mut ratatui::Frame<'_>,
    rect: Rect,
    card: Card<'_>,
    selected: bool,
) {
    let background = theme::color(
        RowState::of(selected, false)
            .ground()
            .unwrap_or(Token::SurfaceRecessed),
    );
    frame.render_widget(
        Paragraph::new("").style(Style::default().bg(background)),
        rect,
    );
    let inner = Rect::new(
        rect.x.saturating_add(2),
        rect.y.saturating_add(1),
        rect.width.saturating_sub(4),
        rect.height.saturating_sub(2),
    );
    let name = Span::styled(
        card.name,
        Style::default()
            .fg(if selected {
                theme::color(Token::TextBright)
            } else {
                theme::color(Token::TextSecondary)
            })
            .add_modifier(Modifier::BOLD),
    );
    frame.render_widget(
        Paragraph::new(title(name, card.badge, inner.width)),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    frame.render_widget(
        Paragraph::new(Span::styled(card.description, theme::fg(Token::TextDim)))
            .wrap(Wrap { trim: true }),
        Rect::new(inner.x, inner.y + 1, inner.width, 2),
    );
    frame.render_widget(
        Paragraph::new(card.caption),
        Rect::new(inner.x, inner.y + 4, inner.width, 1),
    );
}

/// The name, and right-aligned beside it only what fits with a gap
/// between the two: a card the drawer has narrowed wears the mark alone,
/// because a word clipped mid-way and glued to the name
/// ("Claude Code✓ Configur") says less than the mark on its own does.
fn title<'a>(name: Span<'a>, badge: Option<Badge>, width: u16) -> Line<'a> {
    let Some(badge) = badge else {
        return Line::from(name);
    };
    let fits = |text: &str| name.width() + 1 + Span::raw(text).width() <= usize::from(width);
    let full = format!("{} {}", badge.mark, badge.label);
    let worn = if fits(&full) {
        full
    } else if fits(&badge.mark) {
        badge.mark
    } else {
        return Line::from(name);
    };
    let worn = Span::styled(worn, Style::default().fg(badge.color));
    let gap = usize::from(width).saturating_sub(name.width() + worn.width());
    Line::from(vec![name, Span::raw(" ".repeat(gap)), worn])
}
