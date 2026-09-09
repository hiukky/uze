//! TUI view — shared helpers used by every route's render function.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::ui::hit::Hit;
use crate::ui::theme::{self, Symbol, Token};
use uze_application::application::PluginCapability;

pub mod extensions;
pub mod harnesses;
pub mod health;
pub mod keys;
pub mod overview;
pub mod plugins;
pub mod profiles;

pub(crate) const DRAWER_DEFAULT_WIDTH: u16 = 52;

/// The design's `selectedPackage.resources` field is a single flat string
/// ("README, CHANGELOG") — this mirrors that exactly: every capability's
/// own logical/file name, comma-joined, in the order the manifest declared
/// them. Not grouped by kind — the design doesn't, and a plugin rarely
/// declares enough resources for that grouping to earn its own visual
/// weight the way it would in a package-manager UI.
pub(crate) fn resource_summary(capabilities: &[PluginCapability]) -> String {
    if capabilities.is_empty() {
        return theme::glyph(Symbol::MarkUnsupported);
    }
    capabilities
        .iter()
        .map(|c| c.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The drawer's bottom status block: a `theme::color(Token::BorderDefault)`-colored top divider, then a
/// colored dot + bold status text, then a muted note beneath — exactly the
/// design's `border-top` + dot + text status footer, no card, no box.
pub(crate) fn render_status_line(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    color: Color,
    headline: &str,
    subtitle: &str,
) {
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(theme::fg(Token::BorderDefault));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let lines = vec![
        Line::from(vec![
            Span::styled(
                format!("{} ", theme::glyph(Symbol::StatusSelected)),
                Style::default().fg(color),
            ),
            Span::styled(
                headline.to_owned(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(Span::styled(
            subtitle.to_owned(),
            theme::fg(Token::TextMuted),
        )),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

/// Draws a row's "what can be done to this" affordance at its right edge,
/// and registers it.
///
/// Every row carries one, muted until the row is the selected one. The
/// point of it is that finding an action never requires knowing a letter —
/// so it has to be visible before anyone has learned anything, which is
/// exactly when a hover-only affordance is invisible.
pub(crate) fn render_row_actions(
    frame: &mut ratatui::Frame<'_>,
    row: Rect,
    index: usize,
    selected: bool,
    hits: &mut Vec<(Rect, Hit)>,
) {
    let width = theme::width(Symbol::Menu).max(1);
    if row.width <= width + 2 {
        return;
    }
    let rect = Rect::new(row.x + row.width - width - 1, row.y, width + 1, 1);
    frame.render_widget(
        Paragraph::new(Span::styled(
            theme::glyph(Symbol::Menu),
            theme::fg(if selected {
                Token::TextPrimary
            } else {
                Token::TextDim
            }),
        )),
        rect,
    );
    hits.push((rect, Hit::RowActions(index)));
}

/// A detail view's action bar: everything that can be done to the thing
/// on screen, including what cannot be done and why.
///
/// This is the other half of the row menu, and the split between them is
/// deliberate. A menu is a list of what can be done *now*, so an
/// unavailable action would only be noise in it. A detail view is where
/// someone asks why — so this is where "already up to date" is said,
/// instead of a keystroke appearing to do nothing.
pub(crate) fn render_offers(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    offers: &[uze_application::application::offers::ActionOffer],
    hits: &mut Vec<(Rect, Hit)>,
) {
    for (index, offer) in offers.iter().enumerate() {
        let y = area.y + index as u16;
        if y >= area.bottom() {
            return;
        }
        let row = Rect::new(area.x, y, area.width, 1);
        let mut spans = vec![Span::styled(
            offer.action.label(),
            Style::default()
                .fg(theme::color(if !offer.is_available() {
                    Token::TextDim
                } else if offer.action.destructive() {
                    Token::StateDanger
                } else {
                    Token::TextPrimary
                }))
                .add_modifier(if offer.is_available() {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        )];
        if let Some(reason) = offer.reason() {
            spans.push(Span::styled(
                format!("  {} {reason}", theme::glyph(Symbol::EmDash)),
                theme::fg(Token::TextDim),
            ));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), row);
        if offer.is_available() {
            hits.push((row, Hit::OfferedAction(offer.action)));
        }
    }
}
