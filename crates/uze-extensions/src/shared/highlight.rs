//! Syntax highlighting, for the extensions that show a file's own text.
//!
//! Its own module because two surfaces colour source code — the diff
//! [`crate::code::diff`] draws and the file [`crate::code::editor`]
//! opens — and the alternative was a second copy of the syntect
//! plumbing, which is
//! the shape where a fallback theme is fixed in one place and left wrong
//! in the other.
//!
//! Everything here is loaded once per process and shared: `syntect`'s
//! default syntax and theme sets are megabytes of parsed data, and
//! rebuilding them per file is what makes a highlighter feel slow.
//!
//! Colour produced here travels to the host as [`Rgb`] rather than as a
//! [`crate::view::Role`], because it comes from the syntax theme the way
//! an image's colour comes from its pixels — see [`crate::view::Rgb`].

use std::{path::Path, sync::OnceLock};

use syntect::{
    easy::HighlightLines,
    highlighting::{Theme, ThemeSet},
    parsing::SyntaxSet,
};

use crate::view::Rgb;

/// The palette highlighting falls back to when the host names one syntect
/// does not bundle.
pub const FALLBACK_SYNTAX_THEME: &str = "base16-ocean.dark";

pub(crate) fn syntax_set() -> &'static SyntaxSet {
    static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme_set() -> &'static ThemeSet {
    static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();
    THEME_SET.get_or_init(ThemeSet::load_defaults)
}

/// The named theme, or the fallback.
///
/// A name the host gave us that syntect does not know would otherwise be
/// a panic on the first line drawn, which is a poor way to learn about a
/// typo — fall back and keep rendering.
pub(crate) fn theme(name: &str) -> &'static Theme {
    let themes = &theme_set().themes;
    themes
        .get(name)
        .unwrap_or_else(|| &themes[FALLBACK_SYNTAX_THEME])
}

/// A highlighter for `path`'s language, drawn from `theme_name`.
///
/// One per stream of lines, never one per line: syntect's highlighter
/// carries state across calls — an open block comment, an unterminated
/// string — and a fresh one at every line is how a doc comment stops
/// being one halfway down a file.
pub(crate) fn highlighter(path: &Path, theme_name: &str) -> HighlightLines<'static> {
    let syntax_set = syntax_set();
    let syntax = path
        .extension()
        .and_then(|extension| extension.to_str())
        .and_then(|extension| syntax_set.find_syntax_by_extension(extension))
        .unwrap_or_else(|| syntax_set.find_syntax_plain_text());
    HighlightLines::new(syntax, theme(theme_name))
}

/// A highlighter for a language named the way a markdown fence names it
/// — `rust`, `js`, `sh` — rather than by a file's extension.
///
/// Its own entry point because the two names genuinely differ: a fence
/// says `rust` where a file says `.rs`, and looking one up as the other
/// silently produces plain text, which is a block of code that renders
/// but is not coloured. Falls back to plain text for a language syntect
/// does not know, and for an unfenced block, which names none.
pub(crate) fn highlighter_for_language(
    language: &str,
    theme_name: &str,
) -> HighlightLines<'static> {
    let syntax_set = syntax_set();
    let syntax = syntax_set
        .find_syntax_by_token(language.trim())
        .unwrap_or_else(|| syntax_set.find_syntax_plain_text());
    HighlightLines::new(syntax, theme(theme_name))
}

/// One line, highlighted in `highlighter`'s ongoing state.
pub(crate) fn line(highlighter: &mut HighlightLines<'_>, text: &str) -> Vec<(Rgb, String)> {
    // syntect's line-oriented highlighter expects a trailing newline
    // (matches `load_defaults_newlines` above) to track multi-line
    // constructs correctly across calls.
    let newline_terminated = format!("{text}\n");
    let ranges = highlighter
        .highlight_line(&newline_terminated, syntax_set())
        .unwrap_or_default();
    ranges
        .into_iter()
        .map(|(style, piece)| {
            let foreground = style.foreground;
            (
                Rgb(foreground.r, foreground.g, foreground.b),
                piece.trim_end_matches('\n').to_owned(),
            )
        })
        .collect()
}

/// Every line of `text`, highlighted as one continuous stream.
pub(crate) fn lines(text: &str, path: &Path, theme_name: &str) -> Vec<Vec<(Rgb, String)>> {
    let mut highlighter = highlighter(path, theme_name);
    text.lines()
        .map(|text| line(&mut highlighter, text))
        .collect()
}
