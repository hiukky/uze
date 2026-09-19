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
    let keymap = uze_keys::active();
    let separator = theme::glyph(Symbol::HintSeparator);
    let mut spans: Vec<Span<'static>> = Vec::new();
    for action in actions {
        let Some(chord) = keymap.chord_for(*action, scopes) else {
            continue;
        };
        if !spans.is_empty() {
            spans.push(Span::styled(
                format!(" {separator} "),
                theme::fg(Token::TextDim),
            ));
        }
        spans.push(Span::styled(
            chord.to_string(),
            theme::fg_bold(Token::Accent),
        ));
        spans.push(Span::styled(
            format!(" {}", action.label().to_lowercase()),
            theme::fg(Token::TextMuted),
        ));
    }
    Line::from(spans)
}
