//! The open index of everything: every action reachable from where the
//! reader is, with the key that runs it.
//!
//! A composite rather than a primitive — it is assembled from
//! [`Surface`](super::Surface), [`mark`](super::mark) and a row per action
//! — and it is here for the same reason a primitive is: both clients draw
//! it, and until now both *built* it.
//!
//! The two copies had already drifted where it matters. One dimmed the key
//! column for an action that has no chord, so the blank reads as "nothing
//! to press"; the other left it in the accent, which says a key is there
//! and it is empty. Nobody chose that difference, and no reader of either
//! file could have seen it.
//!
//! What is *not* shared is which rows there are: the management client
//! folds in the offers of whatever its screen has selected, and the
//! workspace client has no such selection. Only the narrowing is common,
//! and it has to be — the same query answering differently in two places
//! is the bug a shared filter prevents.

use ratatui::{
    layout::{Constraint, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Clear, Paragraph},
};
use uze_theme::Token;

use super::{Surface, mark};
use crate::ui::theme::{self, Symbol};

/// One line of the index: an action, and the chord that runs it where one
/// is bound.
pub(crate) type Row = (uze_keys::Action, Option<uze_keys::Chord>);

/// The rows whose label or description contains `filter`, or all of them
/// when nothing is typed.
///
/// Both the label and the description, because the reader is searching for
/// what they want to *do* and the word for it is as often in one as in the
/// other.
pub(crate) fn narrowed(rows: Vec<Row>, filter: &str) -> Vec<Row> {
    let needle = filter.trim().to_lowercase();
    if needle.is_empty() {
        return rows;
    }
    rows.into_iter()
        .filter(|(action, _)| {
            action.label().to_lowercase().contains(&needle)
                || action.description().to_lowercase().contains(&needle)
        })
        .collect()
}

/// The narrowest and widest the index is drawn. Narrow enough to sit on a
/// small terminal, and capped so a long description does not stretch the
/// whole screen into one unreadable line.
const MIN_WIDTH: u16 = 30;
const MAX_WIDTH: u16 = 72;

/// Draws the index over `area`, answering with the rect each row took.
///
/// `entry` turns a row's position into the caller's own hit, because the
/// two clients have different hit enums and neither is this module's
/// business — the same arrangement [`button_row`](super::button_row) uses.
/// The caller registers the rects: they belong on *top* of whatever is
/// underneath, which is the caller's list to splice into.
pub(crate) fn render<H>(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    rows: &[Row],
    filter: &str,
    selected: usize,
    entry: impl Fn(usize) -> H,
) -> Vec<(Rect, H)> {
    let key_width = rows
        .iter()
        .filter_map(|(_, chord)| chord.map(|chord| chord.to_string().chars().count()))
        .max()
        .unwrap_or(0)
        .max(4);
    let width = area.width.saturating_sub(8).clamp(MIN_WIDTH, MAX_WIDTH);
    let height = (rows.len() as u16 + 5).min(area.height.saturating_sub(2));
    let rect = area.centered(Constraint::Length(width), Constraint::Length(height));
    frame.render_widget(Clear, rect);
    let inner = Surface::floating()
        .title(" Everything you can do ")
        .render(frame, rect);

    let typed = if filter.is_empty() {
        Line::from(Span::styled("type to narrow", theme::fg(Token::TextMuted)))
    } else {
        Line::from(vec![
            Span::styled(filter.to_owned(), theme::fg(Token::TextPrimary)),
            mark::caret(),
        ])
    };
    frame.render_widget(
        Paragraph::new(typed),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    let list = Rect::new(
        inner.x,
        inner.y + 2,
        inner.width,
        inner.height.saturating_sub(2),
    );
    let mut entries = Vec::with_capacity(rows.len());
    for (position, (action, chord)) in rows.iter().enumerate() {
        let y = list.y + position as u16;
        if y >= list.bottom() {
            break;
        }
        let row = Rect::new(list.x, y, list.width, 1);
        let chosen = position == selected;
        // An action with no key is a finished design, not a gap — it is
        // reached by pointer and from here — so the column it would have
        // filled is dimmed rather than left reading as an empty binding.
        let key = match chord {
            Some(chord) => chord.to_string(),
            None => String::new(),
        };
        let mut label = Style::default().fg(theme::color(if chosen {
            Token::TextBright
        } else if action.destructive() {
            Token::StateDanger
        } else {
            Token::TextPrimary
        }));
        if chosen {
            label = label.add_modifier(Modifier::BOLD);
        }
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!("{key:<key_width$}  "),
                    theme::fg(if chord.is_some() {
                        Token::Accent
                    } else {
                        Token::TextDim
                    }),
                ),
                Span::styled(action.label(), label),
                Span::styled(
                    format!(
                        "  {} {}",
                        theme::glyph(Symbol::EmDash),
                        action.description()
                    ),
                    theme::fg(Token::TextMuted),
                ),
            ])),
            row,
        );
        entries.push((row, entry(position)));
    }
    entries
}
