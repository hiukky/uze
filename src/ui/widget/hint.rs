//! The hint line: what can be done here, and the keys that do it.
//!
//! Read from the live keymap rather than written out, so a rebound key
//! says its new chord everywhere without anyone remembering to update a
//! caption. An action with no chord in these scopes is left out entirely —
//! a hint naming a key that does not exist is worse than no hint.

use ratatui::text::{Line, Span};
use uze_theme::Token;

use crate::ui::theme::{self, Symbol};

/// A hint line for `actions`, each printed with the key that reaches it
/// in `scopes`.
///
/// The one way a surface may name a key. An action with no chord in these
/// scopes is skipped rather than printed keyless: a hint is a list of
/// shortcuts, and what has none is offered somewhere a pointer can reach.
pub(crate) fn line(scopes: &[uze_keys::Scope], actions: &[uze_keys::Action]) -> Line<'static> {
    within(u16::MAX, scopes, actions)
}

/// The same line, holding only what fits in `width`.
///
/// A row of hints is a list, and a list cut at the edge ends in half a
/// word — `· m` where `m map` was meant, which reads as a key nobody can
/// press. So the last one that does not fit is left out instead, with its
/// separator: what is offered here is already a subset of what the
/// surface can do, and a pointer reaches the rest.
pub(crate) fn within(
    width: u16,
    scopes: &[uze_keys::Scope],
    actions: &[uze_keys::Action],
) -> Line<'static> {
    let keymap = uze_keys::active();
    let separator = theme::glyph(Symbol::HintSeparator);
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;
    for action in actions {
        let Some(chord) = keymap.chord_for(*action, scopes) else {
            continue;
        };
        let lead = match spans.is_empty() {
            true => String::new(),
            false => format!(" {separator} "),
        };
        let chord = chord.to_string();
        let label = format!(" {}", action.label().to_lowercase());
        let cost = lead.chars().count() + chord.chars().count() + label.chars().count();
        if used + cost > width as usize {
            break;
        }
        used += cost;
        if !lead.is_empty() {
            spans.push(Span::styled(lead, theme::fg(Token::TextDim)));
        }
        spans.push(Span::styled(chord, theme::fg_bold(Token::Accent)));
        spans.push(Span::styled(label, theme::fg(Token::TextMuted)));
    }
    Line::from(spans)
}
