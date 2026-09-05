//! The one place the TUI turns a token into something ratatui can draw.
//!
//! Every colour the terminal puts on screen passes through here, which is
//! what makes the active theme reach all of it. Nothing else in `src/` names
//! a colour value — the architecture suite fails the build over it — so
//! there is exactly one answer to "what is this drawn in?", and it is a
//! [`Token`].
//!
//! The lookups take no theme argument on purpose. A theme cannot change in
//! the middle of a frame, so threading one through several hundred call
//! sites would only give every function a parameter it could never disagree
//! about; `uze_theme::active` holds it instead, and a theme switch is a swap
//! between frames.

use ratatui::style::{Color, Modifier, Style};

pub(crate) use uze_theme::{Symbol, Token};

/// The colour a token resolves to right now.
pub(crate) fn color(token: Token) -> Color {
    let rgb = uze_theme::active().color(token);
    Color::Rgb(rgb.0, rgb.1, rgb.2)
}

/// A colour that came from content rather than from the design system.
///
/// The one legitimate way anything outside this module puts a specific
/// colour on screen, and deliberately narrow: syntax highlighting comes from
/// a theme the extension ships, and a program inside a terminal pane emits
/// its own true colour. Flattening either into a token would throw away the
/// thing that makes it content. Chrome never comes through here — an
/// extension colouring its own borders is exactly the drift tokens exist to
/// prevent.
pub(crate) fn content(red: u8, green: u8, blue: u8) -> Color {
    Color::Rgb(red, green, blue)
}

/// Foreground only — by far the most common thing a span needs.
pub(crate) fn fg(token: Token) -> Style {
    Style::default().fg(color(token))
}

/// Foreground with weight, for a heading or an active row.
pub(crate) fn fg_bold(token: Token) -> Style {
    fg(token).add_modifier(Modifier::BOLD)
}

/// Background only — a surface behind whatever is drawn over it.
pub(crate) fn bg(token: Token) -> Style {
    Style::default().bg(color(token))
}

/// Both halves at once: content over a surface.
pub(crate) fn on(foreground: Token, background: Token) -> Style {
    Style::default().fg(color(foreground)).bg(color(background))
}

/// The glyph a symbol resolves to. Owned rather than borrowed because the
/// theme it came from is behind a lock — and every call site was building a
/// string for the span anyway.
pub(crate) fn glyph(symbol: Symbol) -> String {
    uze_theme::active().glyph(symbol).to_owned()
}

/// The frame an animated symbol shows at `tick`. A still symbol answers with
/// its one glyph, so a caller that animates need not ask which kind it has.
pub(crate) fn frame(symbol: Symbol, tick: usize) -> String {
    uze_theme::active().frame(symbol, tick).to_owned()
}

/// Every frame of an animated symbol, for the one consumer that hands an
/// animation to something else to run rather than ticking it itself.
pub(crate) fn frames(symbol: Symbol) -> Vec<String> {
    uze_theme::active().symbol(symbol).frames().to_vec()
}

/// Cells a symbol occupies. Lay a column out from this rather than from the
/// length of the glyph: a theme may have replaced it with a wider one.
pub(crate) fn width(symbol: Symbol) -> u16 {
    uze_theme::active().symbol(symbol).width()
}

/// The token a colour came from, or `None` if nothing in the active theme
/// resolves to it.
///
/// For tests only, and deliberately so: a render assertion means "this row
/// is drawn as a warning", not "this row is `#e0b567`". Naming the meaning
/// is what lets those assertions survive a change of theme — and what makes
/// them say what they were always trying to say.
#[cfg(test)]
pub(crate) fn token_of(color: Color) -> Option<Token> {
    let Color::Rgb(red, green, blue) = color else {
        return None;
    };
    let theme = uze_theme::active();
    Token::ALL
        .iter()
        .find(|token| theme.color(**token) == uze_theme::Rgb(red, green, blue))
        .copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_resolves_to_the_active_themes_colour() {
        let rgb = uze_theme::active().color(Token::Accent);
        assert_eq!(color(Token::Accent), Color::Rgb(rgb.0, rgb.1, rgb.2));
    }

    #[test]
    fn a_colour_reports_the_meaning_it_came_from() {
        assert_eq!(
            token_of(color(Token::StateWarning)),
            Some(Token::StateWarning)
        );
        assert_eq!(token_of(Color::Rgb(1, 2, 3)), None);
        assert_eq!(token_of(Color::Reset), None);
    }

    #[test]
    fn a_symbols_width_comes_from_the_theme_not_the_glyph_at_the_call_site() {
        assert_eq!(width(Symbol::StatusIdle), 1);
        assert_eq!(width(Symbol::TreeLast), 2);
        assert_eq!(width(Symbol::HintSeparator), 3);
    }
}
