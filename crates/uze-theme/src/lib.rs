//! UZE's design vocabulary — the one place that says what a colour or a
//! glyph *means*, and the schema anyone can write to change what it looks
//! like.
//!
//! Every surface UZE draws — the workspace client, the management TUI, the
//! CLI's own output, the colours a pane reports to the program inside it —
//! selects appearance by naming a [`Token`] or a [`Symbol`] and resolving it
//! against the active [`Theme`]. Nothing outside this crate holds a colour
//! value, which is what keeps those surfaces from drifting apart the way
//! four hand-maintained copies of one palette always do.
//!
//! What belongs here: the vocabulary ([`Token`], [`Symbol`]), the file
//! format ([`ThemeFile`]), and the resolver that turns a partial theme file
//! into a complete [`Theme`]. What does not: anything that knows the machine
//! (this crate resolves no path and reads no environment — a caller hands it
//! a file), anything that knows UZE's domain, and anything that knows how to
//! draw. A rendering library's colour type is the caller's business; this
//! crate speaks [`Rgb`].
//!
//! ```
//! use uze_theme::{Token, active};
//!
//! // Works before any theme is loaded — the built-in default needs no I/O.
//! let accent = active().color(Token::Accent);
//! assert_eq!((accent.0, accent.1, accent.2), (143, 209, 158));
//! ```

mod active;
mod color;
mod file;
mod load;
mod schema;
mod symbol;
mod theme;
mod token;
mod vocab;

pub use active::{active, set_active};
pub use color::{Rgb, contrast_ratio};
pub use file::{CURRENT_VERSION, ColorValue, SymbolValue, SyntaxSection, ThemeFile};
pub use load::{
    BUNDLED_SYNTAX_THEMES, LoadError, Loaded, Warning, builtin, builtin_names, default_theme,
    load_file, load_str, resolve,
};
pub use schema::{SCHEMA_ID, json_schema};
pub use symbol::{Symbol, SymbolDef};
pub use theme::Theme;
pub use token::Token;
