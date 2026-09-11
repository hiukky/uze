//! TUI view — shared helpers used by every route's render function.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::ui::hit::Hit;
use crate::ui::theme::{self, Symbol, Token};

pub mod extensions;
pub mod harnesses;
pub mod health;
pub mod keys;
pub mod overview;
pub mod plugins;
pub mod profiles;

pub(crate) const DRAWER_DEFAULT_WIDTH: u16 = 52;

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

/// Folds `text` to `width` the way the drawer's paragraph would, but
/// *before* it is authored — so every line the drawer pushes is one drawn
/// row, and a row index is a screen row.
///
/// The drawer renders through `Wrap`, and its two clickable rows (the
/// marketplace name, the address under it) were anchored by counting
/// authored lines. A description long enough to fold pushed the drawn rows
/// down and left the targets sitting above them: the address read as a
/// link and answered nothing. Wrapping the free text here keeps the two
/// counts the same number by construction, with no measurement of what
/// ratatui did afterwards.
pub(crate) fn fold(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_owned()];
    }
    let mut rows: Vec<String> = Vec::new();
    let mut row = String::new();
    for word in text.split_whitespace() {
        // A word wider than the row is broken across rows rather than
        // left to overflow — the paragraph's own wrapper does the same,
        // and a bare URL in a description is exactly that word.
        let mut word = word;
        while word.chars().count() > width {
            if !row.is_empty() {
                rows.push(std::mem::take(&mut row));
            }
            let split = word
                .char_indices()
                .nth(width)
                .map_or(word.len(), |(index, _)| index);
            let (head, tail) = word.split_at(split);
            rows.push(head.to_owned());
            word = tail;
        }
        let projected = row.chars().count() + usize::from(!row.is_empty()) + word.chars().count();
        if projected > width && !row.is_empty() {
            rows.push(std::mem::take(&mut row));
        }
        if !row.is_empty() {
            row.push(' ');
        }
        row.push_str(word);
    }
    if !row.is_empty() || rows.is_empty() {
        rows.push(row);
    }
    rows
}
