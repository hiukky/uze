//! A resolved theme: every token and every symbol, already decided.
//!
//! Everything a theme file can express — a partial declaration, an alias, a
//! translucent colour — is gone by the time a `Theme` exists. Drawing is an
//! array read, and there is no `Option` on the path: a surface that asked
//! for a colour always gets one.

use crate::{Rgb, Symbol, SymbolDef, Token};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Theme {
    name: String,
    description: String,
    colors: Vec<Rgb>,
    symbols: Vec<SymbolDef>,
    syntax_theme: String,
}

impl Theme {
    /// Assembles a theme from complete vectors, indexed by
    /// [`Token::index`]/[`Symbol::index`]. Only the resolver calls this —
    /// everything else receives a theme already built, which is what keeps
    /// "complete" an invariant rather than a hope.
    pub(crate) fn new(
        name: String,
        description: String,
        colors: Vec<Rgb>,
        symbols: Vec<SymbolDef>,
        syntax_theme: String,
    ) -> Self {
        debug_assert_eq!(colors.len(), Token::ALL.len());
        debug_assert_eq!(symbols.len(), Symbol::ALL.len());
        Self {
            name,
            description,
            colors,
            symbols,
            syntax_theme,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn color(&self, token: Token) -> Rgb {
        self.colors[token.index()]
    }

    pub fn symbol(&self, symbol: Symbol) -> &SymbolDef {
        &self.symbols[symbol.index()]
    }

    /// The glyph alone, for the common case of drawing a still symbol.
    pub fn glyph(&self, symbol: Symbol) -> &str {
        self.symbol(symbol).glyph()
    }

    /// The frame an animated symbol shows at `tick`. A still symbol answers
    /// with its one glyph, so a caller that animates need not ask which kind
    /// it has.
    pub fn frame(&self, symbol: Symbol, tick: usize) -> &str {
        self.symbol(symbol).frame(tick)
    }

    /// One of the 16 colours a terminal pane can name by index. Anything
    /// above 15 is one of the 240 extended entries a theme does not define,
    /// and answers `None` so the caller can pass it through untouched.
    pub fn ansi(&self, index: u8) -> Option<Rgb> {
        Token::ANSI
            .get(usize::from(index))
            .map(|token| self.color(*token))
    }

    /// The syntect theme highlighted content is rendered with. Named by the
    /// theme so a light background does not leave a diff unreadable.
    pub fn syntax_theme(&self) -> &str {
        &self.syntax_theme
    }

    /// The background every other colour is read against — the contrast
    /// check's reference, and what a translucent declaration composites over.
    pub fn background(&self) -> Rgb {
        self.color(Token::SurfaceBackground)
    }
}
