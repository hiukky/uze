//! The theme file format.
//!
//! Everything here is what an author *wrote*, not what UZE will draw:
//! declarations are kept as text until the resolver can report a bad one
//! against the token it was written for. Token and symbol names are strings
//! rather than the enums for the same reason — a theme written for a newer
//! UZE names entries this build has never heard of, and that has to be a
//! warning rather than a file that will not load.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The schema version this build writes and understands.
pub const CURRENT_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ThemeFile {
    /// The schema this file was written against. Absent means
    /// [`CURRENT_VERSION`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    /// Display name. Defaults to the file's own stem, so a theme need not
    /// repeat its filename.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Token name → declaration. Any token left out keeps the built-in
    /// default's value.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub colors: BTreeMap<String, ColorValue>,
    /// Symbol name → glyph. Any symbol left out keeps the built-in
    /// default's glyph.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub symbols: BTreeMap<String, SymbolValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub syntax: Option<SyntaxSection>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SyntaxSection {
    /// The syntect theme highlighted content is rendered with.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
}

/// A colour as written: `#rrggbb`, `#rrggbbaa`, or `@another.token`.
/// Kept as text so a malformed value is reported against the token that
/// carries it rather than as a parse error at some byte offset.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(transparent)]
pub struct ColorValue(pub String);

/// A symbol as written. The short forms cover almost every case; the long
/// one exists for a glyph whose width the terminal disagrees with.
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(untagged)]
pub enum SymbolValue {
    /// `"mark.official": "✓"`
    Glyph(String),
    /// `"status.working": ["⠋", "⠙", "⠹"]`
    Frames(Vec<String>),
    /// `"tree.branch": { "glyph": "├─", "width": 2 }`
    Detailed {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        glyph: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        frames: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        width: Option<u16>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_five_line_theme_parses() {
        let file: ThemeFile =
            serde_json::from_str(r##"{ "name": "warm", "colors": { "accent": "#ff8800" } }"##)
                .expect("a partial theme is a valid theme");
        assert_eq!(file.name.as_deref(), Some("warm"));
        assert_eq!(file.colors["accent"], ColorValue("#ff8800".to_owned()));
        assert!(file.symbols.is_empty());
    }

    #[test]
    fn every_symbol_form_parses() {
        let file: ThemeFile = serde_json::from_str(
            r##"{ "symbols": {
                   "mark.official": "OK",
                   "status.working": ["-", "\\", "|", "/"],
                   "tree.branch": { "glyph": "|-", "width": 2 }
                 } }"##,
        )
        .expect("all three symbol forms are valid");
        assert_eq!(
            file.symbols["mark.official"],
            SymbolValue::Glyph("OK".to_owned())
        );
        assert!(matches!(
            file.symbols["status.working"],
            SymbolValue::Frames(ref frames) if frames.len() == 4
        ));
        assert!(matches!(
            file.symbols["tree.branch"],
            SymbolValue::Detailed { width: Some(2), .. }
        ));
    }

    #[test]
    fn a_misspelled_top_level_field_is_refused_rather_than_ignored() {
        // `colours` silently doing nothing is worse than a file that will
        // not load: the author would be left staring at an unchanged UI.
        let error = serde_json::from_str::<ThemeFile>(r##"{ "colours": {} }"##)
            .expect_err("an unknown field is refused");
        assert!(error.to_string().contains("colours"), "{error}");
    }
}
