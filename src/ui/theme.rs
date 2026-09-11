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

/// One colour of a theme that is *not* in force, for the Appearance
/// screen's swatches.
///
/// The colour half of what `preview_spans` does for glyphs, and legitimate
/// for the same reason: the question that screen answers is "what does
/// *that* one look like", which no amount of resolving the active theme can
/// reach. Narrow on purpose — it takes a resolved [`uze_theme::Rgb`], so
/// the only thing it can draw is a colour some theme file already decided.
pub(crate) fn swatch(rgb: uze_theme::Rgb) -> Color {
    Color::Rgb(rgb.0, rgb.1, rgb.2)
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

/// A colour pushed most of the way toward the backdrop.
///
/// What a modal's scrim is made of. The frame underneath has to keep
/// reading as a place — the shape of the list, the row that was selected,
/// the panel the question came from — while nothing in it competes with
/// the question drawn on top. A colour blended toward
/// [`Token::SurfaceBackground`] keeps every one of those and drops the
/// contrast, which is exactly that.
///
/// Blended here rather than left to ratatui's `DIM`: that modifier is one
/// bit handed to the terminal, and terminals disagree about it — several
/// ignore it outright and the rest each choose their own intensity, so the
/// same frame would recede by a different amount per host. Doing the
/// arithmetic means how far back a backdrop sits is the theme's answer,
/// like every other colour here.
///
/// `absent` is the token to read a cell that names no colour of its own
/// as: a foreground asks for the body text, a background for the backdrop.
pub(crate) fn scrimmed(color: Color, absent: Token) -> Color {
    /// How far a scrimmed colour travels toward the backdrop, in percent.
    /// Enough that no word behind a modal competes with one in it; short of
    /// the flat wash that would make the screen underneath unreadable as a
    /// place, which is the thing a backdrop is for.
    const TOWARD_BACKDROP: u16 = 62;

    let ground = uze_theme::active().color(Token::SurfaceBackground);
    let (red, green, blue) = channels(color, absent);
    let toward = |from: u8, to: u8| {
        ((u16::from(from) * (100 - TOWARD_BACKDROP) + u16::from(to) * TOWARD_BACKDROP) / 100) as u8
    };
    Color::Rgb(
        toward(red, ground.0),
        toward(green, ground.1),
        toward(blue, ground.2),
    )
}

/// A token's colour, softened into the surface a control sits on.
///
/// What a button wears at rest: the hue of its meaning — accent, danger —
/// held back most of the way toward the panel behind it, so a row of them
/// reads as quiet controls rather than as alarms. The pointer brings the
/// full token back, and that step from soft to strong is what says the
/// thing under it can be clicked.
pub(crate) fn softened(token: Token, into: Token) -> Color {
    /// How much of the surface the resting colour keeps, in percent.
    const TOWARD_SURFACE: u16 = 70;

    let hue = uze_theme::active().color(token);
    let surface = uze_theme::active().color(into);
    let mix = |from: u8, to: u8| {
        ((u16::from(from) * (100 - TOWARD_SURFACE) + u16::from(to) * TOWARD_SURFACE) / 100) as u8
    };
    Color::Rgb(
        mix(hue.0, surface.0),
        mix(hue.1, surface.1),
        mix(hue.2, surface.2),
    )
}

/// The channels behind a drawn colour — for the one operation that has to
/// do arithmetic on one rather than pass it through.
fn channels(color: Color, absent: Token) -> (u8, u8, u8) {
    let rgb = match color {
        Color::Rgb(red, green, blue) => return (red, green, blue),
        // A pane's own output is the only thing that reaches a buffer as an
        // index: the themed 16 the active theme answers for, and above them
        // the 240 entries no theme defines, which every terminal builds by
        // the same arithmetic (a 6×6×6 cube, then a 24-step grey ramp).
        Color::Indexed(index) => match uze_theme::active().ansi(index) {
            Some(rgb) => rgb,
            None if index >= 232 => {
                let level = 8 + (index - 232) * 10;
                uze_theme::Rgb(level, level, level)
            }
            None => {
                let step = |value: u8| if value == 0 { 0 } else { 55 + value * 40 };
                let cube = index - 16;
                uze_theme::Rgb(step(cube / 36), step((cube % 36) / 6), step(cube % 6))
            }
        },
        // Nothing was asked for, so the cell shows whatever the surface it
        // sits on shows — which is what `absent` names.
        _ => uze_theme::active().color(absent),
    };
    (rgb.0, rgb.1, rgb.2)
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
    fn a_scrimmed_colour_sits_between_what_it_was_and_the_backdrop() {
        let ground = uze_theme::active().color(Token::SurfaceBackground);
        let Color::Rgb(red, ..) = scrimmed(color(Token::TextBright), Token::TextPrimary) else {
            panic!("a scrimmed colour is a resolved one");
        };
        let Color::Rgb(was, ..) = color(Token::TextBright) else {
            unreachable!("a token always resolves")
        };
        assert!(
            red.abs_diff(ground.0) < was.abs_diff(ground.0),
            "it moved toward the backdrop"
        );
        assert_ne!(red, ground.0, "and stopped short of it");
    }

    #[test]
    fn scrimming_the_backdrop_leaves_it_where_it_is() {
        // The cell nothing was drawn into is the one the eye reads the
        // scrim against, so a backdrop that shifted would be the screen
        // itself changing colour rather than its content receding.
        assert_eq!(
            scrimmed(color(Token::SurfaceBackground), Token::SurfaceBackground),
            color(Token::SurfaceBackground)
        );
        assert_eq!(
            scrimmed(Color::Reset, Token::SurfaceBackground),
            color(Token::SurfaceBackground),
            "and a cell that named no colour is read as the surface it sits on"
        );
    }

    #[test]
    fn a_scrimmed_pane_colour_is_resolved_before_it_is_blended() {
        // A pane's own output is the one thing that reaches the buffer as
        // an index. Blending needs channels, so the extended entries no
        // theme defines are built here by the arithmetic every terminal
        // uses — an index left unresolved would be the one thing on screen
        // that did not recede.
        assert!(matches!(
            scrimmed(Color::Indexed(208), Token::TextPrimary),
            Color::Rgb(..)
        ));
        assert!(matches!(
            scrimmed(Color::Indexed(240), Token::TextPrimary),
            Color::Rgb(..)
        ));
    }

    #[test]
    fn a_symbols_width_comes_from_the_theme_not_the_glyph_at_the_call_site() {
        assert_eq!(width(Symbol::StatusIdle), 1);
        assert_eq!(width(Symbol::TreeLast), 2);
        assert_eq!(width(Symbol::HintSeparator), 3);
    }
}
