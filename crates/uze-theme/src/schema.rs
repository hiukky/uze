//! The published JSON Schema for a theme file.
//!
//! Generated from the vocabulary rather than written beside it: the useful
//! half of a schema here is the list of every token and symbol name, and a
//! hand-maintained copy of that list would be wrong the first time the
//! vocabulary grew. An editor pointed at this file gives an author
//! completion over the real names and tells them about a typo before UZE
//! does.
//!
//! `themes/theme.schema.json` is the checked-in copy consumers point at; the
//! test below fails when it drifts from what this module generates, the same
//! way `CREDITS.md` fails when it drifts from the lockfile.

use serde_json::{Value, json};

use crate::{Symbol, Token, file::CURRENT_VERSION};

/// The schema's own identity. Versioned by path so a theme written against
/// today's vocabulary keeps pointing at a schema that describes it.
pub const SCHEMA_ID: &str = "https://uze.dev/schema/theme/v1.json";

/// The JSON Schema describing a theme file.
pub fn json_schema() -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": SCHEMA_ID,
        "title": "UZE theme",
        "description":
            "A UZE theme. Every field is optional: whatever a theme does not \
             declare keeps the built-in default's value, so a usable theme can \
             be a handful of lines.",
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "version": {
                "description": "Schema version this file was written against.",
                "type": "integer",
                "minimum": 1,
                "default": CURRENT_VERSION,
            },
            "extends": {
                "description":
                    "The theme this one is a variation of, by id. Absent means the \
                     built-in default. Anything this file does not declare comes \
                     from the nearest ancestor that does.",
                "type": "string",
            },
            "name": { "type": "string", "description": "Display name. Defaults to the file's stem." },
            "description": { "type": "string" },
            "colors": {
                "description":
                    "Token name to colour. A colour is `#rrggbb`; `#rrggbbaa`, \
                     composited over this theme's own `surface.background`; \
                     `~aa`, separated from that background by `aa` in whichever \
                     direction is visible against it (white on a dark theme, \
                     black on a light one); `@another.token`, taking another \
                     token's value; or `@another.token/aa`, that token's value at \
                     that alpha over the background.",
                "type": "object",
                "additionalProperties": false,
                "properties": color_properties(),
            },
            "symbols": {
                "description":
                    "Symbol name to glyph: a string, a list of frames for an \
                     animation, or an object with an explicit display width for a \
                     glyph the terminal measures differently than Unicode does.",
                "type": "object",
                "additionalProperties": false,
                "properties": symbol_properties(),
            },
            "syntax": {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "theme": {
                        "description": "Palette for syntax-highlighted content, such as a diff.",
                        "type": "string",
                        "enum": crate::BUNDLED_SYNTAX_THEMES,
                    },
                },
            },
        },
    })
}

fn color_properties() -> Value {
    let mut properties = serde_json::Map::new();
    for token in Token::ALL {
        properties.insert(
            token.name().to_owned(),
            json!({
                "type": "string",
                "pattern": "^(#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?|~[0-9a-fA-F]{2}|@[a-z0-9.-]+(/[0-9a-fA-F]{2})?)$",
            }),
        );
    }
    Value::Object(properties)
}

fn symbol_properties() -> Value {
    let glyph = json!({
        "oneOf": [
            { "type": "string" },
            { "type": "array", "items": { "type": "string" }, "minItems": 1 },
            {
                "type": "object",
                "additionalProperties": false,
                "properties": {
                    "glyph": { "type": "string" },
                    "frames": { "type": "array", "items": { "type": "string" }, "minItems": 1 },
                    "width": { "type": "integer", "minimum": 0 },
                },
            },
        ],
    });
    let mut properties = serde_json::Map::new();
    for symbol in Symbol::ALL {
        properties.insert(symbol.name().to_owned(), glyph.clone());
    }
    Value::Object(properties)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::ThemeFile;

    const PUBLISHED: &str = include_str!("../themes/theme.schema.json");

    #[test]
    fn the_published_schema_matches_the_vocabulary() {
        let published: Value =
            serde_json::from_str(PUBLISHED).expect("the published schema is valid JSON");
        assert_eq!(
            published,
            json_schema(),
            "crates/uze-theme/themes/theme.schema.json is out of date — \
             regenerate it with `cargo run -p uze-theme --example schema`"
        );
    }

    #[test]
    fn the_bundled_themes_only_name_things_the_schema_allows() {
        // The schema's whole job is naming the vocabulary, and the loader
        // already enforces everything else about a theme far more strictly
        // than a schema could. So what is worth checking is that UZE's own
        // themes stay inside what it tells authors is allowed.
        for source in [
            include_str!("../themes/default.json"),
            include_str!("../themes/ascii.json"),
        ] {
            let file: ThemeFile = serde_json::from_str(source).expect("bundled theme parses");
            for name in file.colors.keys() {
                assert!(
                    Token::from_name(name).is_some(),
                    "bundled theme declares `{name}`, which the schema does not allow"
                );
            }
            for name in file.symbols.keys() {
                assert!(
                    Symbol::from_name(name).is_some(),
                    "bundled theme declares `{name}`, which the schema does not allow"
                );
            }
        }
    }
}
