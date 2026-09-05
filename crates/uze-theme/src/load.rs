//! Turning what an author wrote into what UZE draws.
//!
//! Everything expressive about the file format is spent here: a partial
//! theme is merged over the built-in default, aliases are followed,
//! translucent colours are composited over the theme's own background, and
//! anything left doubtful is reported. What comes out is a [`Theme`] with no
//! choices left in it.
//!
//! The split between an error and a warning is deliberate. A file UZE cannot
//! make sense of — a malformed colour, an alias that loops — is an error,
//! and the caller keeps the theme it already had. A file that names
//! something this build does not know is a *warning*: a theme written for a
//! newer UZE must still load on an older one, or every theme in the wild
//! breaks the first time the vocabulary grows.

use std::{
    collections::BTreeMap,
    fmt, fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use crate::{
    Rgb, Symbol, SymbolDef, Theme, Token,
    color::contrast_ratio,
    file::{CURRENT_VERSION, ColorValue, SymbolValue, ThemeFile},
};

/// The themes UZE carries with it. Their bytes are the worked examples for
/// the format — the same loader reads them and reads yours.
const BUILTIN_DEFAULT: &str = include_str!("../themes/default.json");
const BUILTIN_ASCII: &str = include_str!("../themes/ascii.json");

/// The syntax themes a theme file may name.
///
/// These are syntect's bundled set, listed here so this crate does not have
/// to depend on syntect to tell an author they made a typo.
/// `uze-extensions`, which does own syntect, holds the test that this list
/// still matches what syntect actually bundles.
pub const BUNDLED_SYNTAX_THEMES: &[&str] = &[
    "InspiredGitHub",
    "Solarized (dark)",
    "Solarized (light)",
    "base16-eighties.dark",
    "base16-mocha.dark",
    "base16-ocean.dark",
    "base16-ocean.light",
];

/// Contrast below this against the background is reported.
///
/// Far below WCAG AA (4.5) on purpose. UZE's design draws several
/// deliberately recessed levels — `text.faint`, the tree glyphs nobody reads
/// unless they went looking, sits at 1.9:1 — and a check that fires on the
/// shipped default is a check every author learns to ignore. This threshold
/// catches text that is effectively invisible, which is the only thing an
/// author cannot see for themselves.
const MIN_CONTRAST: f32 = 1.5;

/// A theme file UZE could not make sense of. The caller keeps whatever theme
/// was already active — a half-applied theme is worse than an unchanged one.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("could not read theme file {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("theme file {path} is not valid JSON: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("`{token}` is `{value}`, which is not a colour: {reason}")]
    Color {
        token: String,
        value: String,
        reason: &'static str,
    },
    #[error("`{}` alias loops back on itself", chain.join("` → `"))]
    AliasCycle { chain: Vec<String> },
    #[error(
        "`surface.background` is `{value}`, but the background is what every \
         other colour is composited over, so it has to be an opaque `#rrggbb` \
         of its own"
    )]
    BackgroundNotResolvable { value: String },
    #[error("`{symbol}` declares no glyph")]
    SymbolWithoutGlyph { symbol: String },
    #[error("syntax theme `{name}` is not one UZE bundles (available: {})", available.join(", "))]
    UnknownSyntaxTheme {
        name: String,
        available: &'static [&'static str],
    },
    #[error(
        "theme `{id}` leaves {} entries with nothing to draw: {}",
        missing.len(),
        missing.join(", ")
    )]
    Incomplete { id: String, missing: Vec<String> },
}

/// Something worth telling the operator that did not stop the theme from
/// loading.
#[derive(Clone, Debug, PartialEq)]
pub enum Warning {
    /// A name this build does not know — almost always a theme written for a
    /// newer UZE, occasionally a typo.
    UnknownName { kind: &'static str, name: String },
    /// Text that will be hard to read against this theme's own background.
    /// Reported, never corrected: the author's colour is the author's call.
    LowContrast { token: Token, ratio: f32 },
    /// A file written against a schema this build does not know.
    UnsupportedVersion { found: u32, supported: u32 },
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Warning::UnknownName { kind, name } => write!(
                f,
                "`{name}` is not a {kind} this version of UZE knows — it was ignored"
            ),
            Warning::LowContrast { token, ratio } => write!(
                f,
                "`{token}` has {ratio:.1}:1 contrast against the background, which will be hard to read"
            ),
            Warning::UnsupportedVersion { found, supported } => write!(
                f,
                "theme declares schema version {found}; this build understands {supported}"
            ),
        }
    }
}

/// A resolved theme plus everything worth saying about how it resolved.
#[derive(Debug)]
pub struct Loaded {
    pub theme: Theme,
    pub warnings: Vec<Warning>,
}

/// The built-in themes, by the id a user selects them with.
pub fn builtin_names() -> &'static [&'static str] {
    &["default", "ascii"]
}

/// A theme UZE carries. `default` is the one every other theme is resolved
/// on top of, so it is the only one required to be complete.
pub fn builtin(id: &str) -> Option<&'static Theme> {
    match id {
        "default" => Some(default_theme()),
        "ascii" => {
            static ASCII: OnceLock<Theme> = OnceLock::new();
            Some(ASCII.get_or_init(|| {
                load_str("ascii", BUILTIN_ASCII)
                    .expect("the bundled ascii theme resolves")
                    .theme
            }))
        }
        _ => None,
    }
}

/// The theme every partial theme is completed from, and the one UZE falls
/// back to whenever a selected theme cannot be loaded.
pub fn default_theme() -> &'static Theme {
    static DEFAULT: OnceLock<Theme> = OnceLock::new();
    DEFAULT.get_or_init(|| {
        resolve("default", default_file().clone(), None)
            .expect("the bundled default theme resolves and is complete")
            .theme
    })
}

/// The default theme *as written*, which is what a partial theme is completed
/// from.
///
/// Merging happens between declarations rather than between resolved colours,
/// and that is not an implementation detail: `state.success` is written
/// `@accent` in the default, so a theme that repaints the accent has to
/// repaint success with it. Completing from an already-resolved theme would
/// hand it the old accent's value and quietly break every alias the default
/// relies on.
fn default_file() -> &'static ThemeFile {
    static FILE: OnceLock<ThemeFile> = OnceLock::new();
    FILE.get_or_init(|| {
        serde_json::from_str(BUILTIN_DEFAULT).expect("the bundled default theme is valid JSON")
    })
}

/// Reads and resolves a theme file. Its id is the file's stem, which is what
/// a user selects it by.
pub fn load_file(path: &Path) -> Result<Loaded, LoadError> {
    let contents = fs::read_to_string(path).map_err(|source| LoadError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let id = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("theme");
    let file: ThemeFile = serde_json::from_str(&contents).map_err(|source| LoadError::Parse {
        path: path.to_path_buf(),
        source,
    })?;
    resolve(id, file, Some(default_file()))
}

/// Resolves a theme from its text, completing it from the built-in default.
pub fn load_str(id: &str, contents: &str) -> Result<Loaded, LoadError> {
    let file: ThemeFile = serde_json::from_str(contents).map_err(|source| LoadError::Parse {
        path: PathBuf::from(id),
        source,
    })?;
    resolve(id, file, Some(default_file()))
}

/// The resolver itself. `base` is the theme file unstated entries are
/// completed from; `None` demands a complete file, which is what the
/// built-in default is held to.
pub fn resolve(id: &str, file: ThemeFile, base: Option<&ThemeFile>) -> Result<Loaded, LoadError> {
    let mut warnings = Vec::new();

    if let Some(version) = file.version
        && version > CURRENT_VERSION
    {
        warnings.push(Warning::UnsupportedVersion {
            found: version,
            supported: CURRENT_VERSION,
        });
    }

    let colors_written = merged(base.map(|base| &base.colors), &file.colors);
    let symbols_written = merged(base.map(|base| &base.symbols), &file.symbols);
    // Only the author's own file earns a warning: a name the base uses is
    // one this build put there.
    let declared = known_by_name(&colors_written, &file.colors, "colour token", &mut warnings);
    let symbols_declared = known_by_name(&symbols_written, &file.symbols, "symbol", &mut warnings);

    let missing = missing_entries(&declared, &symbols_declared);
    if !missing.is_empty() {
        return Err(LoadError::Incomplete {
            id: id.to_owned(),
            missing,
        });
    }

    let background = resolve_background(&declared)?;
    let colors = resolve_colors(&declared, background)?;
    let symbols = resolve_symbols(&symbols_declared)?;
    let syntax_theme = resolve_syntax_theme(&file, base)?;

    for token in Token::ALL {
        // A surface is not read against itself, and a pane's own 16 belong
        // to the programs running inside it rather than to UZE's text.
        if token.is_surface() || Token::ANSI.contains(token) {
            continue;
        }
        let ratio = contrast_ratio(colors[token.index()], background);
        if ratio < MIN_CONTRAST {
            warnings.push(Warning::LowContrast {
                token: *token,
                ratio,
            });
        }
    }

    let name = file.name.unwrap_or_else(|| id.to_owned());
    let description = file.description.unwrap_or_default();
    Ok(Loaded {
        theme: Theme::new(name, description, colors, symbols, syntax_theme),
        warnings,
    })
}

/// What the author wrote, over what they inherited. Merging declarations
/// rather than resolved values is what keeps the base's aliases alive: a
/// theme that repaints `accent` repaints everything written `@accent`.
fn merged<'a, V: Clone>(
    base: Option<&'a BTreeMap<String, V>>,
    file: &'a BTreeMap<String, V>,
) -> BTreeMap<String, V> {
    let mut merged = base.cloned().unwrap_or_default();
    for (name, value) in file {
        merged.insert(name.clone(), value.clone());
    }
    merged
}

/// Drops the entries this build has no vocabulary for, warning about each
/// one the author wrote themselves, so nothing downstream carries the
/// possibility of a name that means nothing.
fn known_by_name<K: Ord + Copy + Named, V: Clone>(
    written: &BTreeMap<String, V>,
    authored: &BTreeMap<String, V>,
    kind: &'static str,
    warnings: &mut Vec<Warning>,
) -> BTreeMap<K, V> {
    let mut known = BTreeMap::new();
    for (name, value) in written {
        match K::from_written_name(name) {
            Some(key) => {
                known.insert(key, value.clone());
            }
            None if authored.contains_key(name) => warnings.push(Warning::UnknownName {
                kind,
                name: name.clone(),
            }),
            None => {}
        }
    }
    known
}

/// Lets one lookup serve both vocabularies without either of them learning
/// about the loader.
trait Named: Sized {
    fn from_written_name(name: &str) -> Option<Self>;
}

impl Named for Token {
    fn from_written_name(name: &str) -> Option<Self> {
        Token::from_name(name)
    }
}

impl Named for Symbol {
    fn from_written_name(name: &str) -> Option<Self> {
        Symbol::from_name(name)
    }
}

/// The background has to be settled before anything else, because it is what
/// a translucent declaration composites over — so it cannot itself be
/// translucent, and following an alias to find it would mean resolving
/// aliases before the thing they are composited against exists.
fn resolve_background(declared: &BTreeMap<Token, ColorValue>) -> Result<Rgb, LoadError> {
    let value = declared
        .get(&Token::SurfaceBackground)
        .expect("completeness was checked first");
    match parse_color(&value.0) {
        Some(Declaration::Opaque(rgb)) => Ok(rgb),
        _ => Err(LoadError::BackgroundNotResolvable {
            value: value.0.clone(),
        }),
    }
}

fn resolve_colors(
    declared: &BTreeMap<Token, ColorValue>,
    background: Rgb,
) -> Result<Vec<Rgb>, LoadError> {
    let mut colors = Vec::with_capacity(Token::ALL.len());
    for token in Token::ALL {
        colors.push(resolve_one(*token, declared, background, &mut Vec::new())?);
    }
    Ok(colors)
}

/// Resolves one token, following aliases. `chain` is the path taken so far,
/// which is both the cycle check and what the error prints — a loop the
/// author cannot see the shape of is a loop they cannot fix.
fn resolve_one(
    token: Token,
    declared: &BTreeMap<Token, ColorValue>,
    background: Rgb,
    chain: &mut Vec<Token>,
) -> Result<Rgb, LoadError> {
    if chain.contains(&token) {
        let mut names: Vec<String> = chain.iter().map(|token| token.name().to_owned()).collect();
        names.push(token.name().to_owned());
        return Err(LoadError::AliasCycle { chain: names });
    }

    let value = &declared
        .get(&token)
        .expect("completeness was checked first")
        .0;
    match parse_color(value) {
        Some(Declaration::Opaque(rgb)) => Ok(rgb),
        Some(Declaration::Translucent(rgb, alpha)) => Ok(rgb.over(background, alpha)),
        Some(Declaration::Alias(target)) => {
            let Some(target) = Token::from_name(&target) else {
                return Err(LoadError::Color {
                    token: token.name().to_owned(),
                    value: value.clone(),
                    reason: "it aliases a token that does not exist",
                });
            };
            chain.push(token);
            let resolved = resolve_one(target, declared, background, chain);
            chain.pop();
            resolved
        }
        None => Err(LoadError::Color {
            token: token.name().to_owned(),
            value: value.clone(),
            reason: "expected `#rrggbb`, `#rrggbbaa`, or `@another.token`",
        }),
    }
}

enum Declaration {
    Opaque(Rgb),
    Translucent(Rgb, u8),
    Alias(String),
}

fn parse_color(value: &str) -> Option<Declaration> {
    if let Some(target) = value.strip_prefix('@') {
        return (!target.is_empty()).then(|| Declaration::Alias(target.to_owned()));
    }
    let digits = value.strip_prefix('#')?;
    let byte = |index: usize| u8::from_str_radix(digits.get(index..index + 2)?, 16).ok();
    let rgb = Rgb(byte(0)?, byte(2)?, byte(4)?);
    match digits.len() {
        6 => Some(Declaration::Opaque(rgb)),
        8 => Some(Declaration::Translucent(rgb, byte(6)?)),
        _ => None,
    }
}

fn resolve_symbols(declared: &BTreeMap<Symbol, SymbolValue>) -> Result<Vec<SymbolDef>, LoadError> {
    let mut symbols = Vec::with_capacity(Symbol::ALL.len());
    for symbol in Symbol::ALL {
        let value = declared
            .get(symbol)
            .expect("completeness was checked first");
        symbols.push(build_symbol(symbol.name(), value)?);
    }
    Ok(symbols)
}

fn build_symbol(name: &str, value: &SymbolValue) -> Result<SymbolDef, LoadError> {
    let missing = || LoadError::SymbolWithoutGlyph {
        symbol: name.to_owned(),
    };
    let def = match value {
        SymbolValue::Glyph(glyph) => SymbolDef::new(glyph.clone()),
        SymbolValue::Frames(frames) => {
            if frames.is_empty() {
                return Err(missing());
            }
            SymbolDef::animated(frames.clone())
        }
        SymbolValue::Detailed {
            glyph,
            frames,
            width,
        } => {
            let def = match (glyph, frames.is_empty()) {
                (Some(glyph), _) => SymbolDef::new(glyph.clone()),
                (None, false) => SymbolDef::animated(frames.clone()),
                (None, true) => return Err(missing()),
            };
            match width {
                Some(width) => def.with_width(*width),
                None => def,
            }
        }
    };
    Ok(def)
}

fn resolve_syntax_theme(file: &ThemeFile, base: Option<&ThemeFile>) -> Result<String, LoadError> {
    let theme_of = |file: &ThemeFile| file.syntax.as_ref().and_then(|syntax| syntax.theme.clone());
    let Some(name) = theme_of(file).or_else(|| base.and_then(theme_of)) else {
        return Ok(BUNDLED_SYNTAX_THEMES[0].to_owned());
    };
    if BUNDLED_SYNTAX_THEMES.contains(&name.as_str()) {
        Ok(name)
    } else {
        Err(LoadError::UnknownSyntaxTheme {
            name,
            available: BUNDLED_SYNTAX_THEMES,
        })
    }
}

/// What a theme with nothing to fall back on failed to declare. Only the
/// built-in default is held to this, and the test that runs it is what makes
/// "every surface always has a colour" true rather than hopeful.
fn missing_entries(
    colors: &BTreeMap<Token, ColorValue>,
    symbols: &BTreeMap<Symbol, SymbolValue>,
) -> Vec<String> {
    Token::ALL
        .iter()
        .filter(|token| !colors.contains_key(token))
        .map(|token| token.name().to_owned())
        .chain(
            Symbol::ALL
                .iter()
                .filter(|symbol| !symbols.contains_key(symbol))
                .map(|symbol| symbol.name().to_owned()),
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(json: &str) -> Loaded {
        load_str("test", json).expect("theme resolves")
    }

    #[test]
    fn the_builtin_default_is_complete_and_resolves() {
        let theme = default_theme();
        // Every token and symbol answers — the property every other
        // surface's "always has a colour" rests on.
        for token in Token::ALL {
            let _ = theme.color(*token);
        }
        for symbol in Symbol::ALL {
            assert!(
                !theme.glyph(*symbol).is_empty(),
                "`{symbol}` resolved to nothing"
            );
        }
        assert_eq!(theme.name(), "uze");
    }

    #[test]
    fn the_default_carries_the_exact_palette_that_shipped() {
        // The premise of the whole migration: replacing 676 colour constants
        // with token lookups changes no pixel. Each pair below is the
        // constant `src/ui.rs` used to hold, against the token that replaced
        // it.
        let theme = default_theme();
        let expected = [
            (Token::SurfaceBackground, Rgb(10, 12, 13)),
            (Token::TextBright, Rgb(242, 240, 234)),
            (Token::TextPrimary, Rgb(230, 228, 222)),
            (Token::TextSecondary, Rgb(168, 166, 160)),
            (Token::TextTertiary, Rgb(201, 199, 192)),
            (Token::TextMuted, Rgb(107, 113, 118)),
            (Token::TextDim, Rgb(91, 96, 101)),
            (Token::TextFaint, Rgb(61, 66, 71)),
            (Token::TextInactive, Rgb(154, 152, 146)),
            (Token::Accent, Rgb(143, 209, 158)),
            (Token::StateSuccess, Rgb(143, 209, 158)),
            (Token::StateWarning, Rgb(224, 181, 103)),
            (Token::StateDanger, Rgb(224, 118, 95)),
            (Token::StateInfo, Rgb(125, 151, 201)),
            (Token::StateInFlight, Rgb(125, 190, 194)),
            (Token::StateLanded, Rgb(163, 143, 201)),
            (Token::BorderFaint, Rgb(22, 24, 25)),
            (Token::BorderDefault, Rgb(30, 31, 32)),
            (Token::SurfaceSelected, Rgb(22, 30, 26)),
            (Token::SurfaceRaised, Rgb(32, 34, 35)),
            (Token::SurfaceRaisedSubtle, Rgb(27, 29, 30)),
            (Token::SurfaceRaisedBright, Rgb(44, 46, 47)),
            (Token::SurfaceRecessed, Rgb(16, 18, 19)),
            (Token::StateDiffAdded, Rgb(18, 32, 23)),
            (Token::StateDiffRemoved, Rgb(38, 22, 20)),
        ];
        for (token, rgb) in expected {
            assert_eq!(theme.color(token), rgb, "`{token}` drifted from the design");
        }
    }

    #[test]
    fn the_default_carries_the_exact_glyphs_that_shipped() {
        let theme = default_theme();
        assert_eq!(theme.glyph(Symbol::MarkNative), "√");
        assert_eq!(theme.glyph(Symbol::MarkOfficial), "✓");
        assert_eq!(theme.glyph(Symbol::StatusSelected), "●");
        assert_eq!(theme.glyph(Symbol::StatusIdle), "○");
        assert_eq!(theme.glyph(Symbol::TreeLast), "└─");
        assert_eq!(theme.glyph(Symbol::HintSeparator), " · ");
        assert_eq!(theme.symbol(Symbol::StatusWorking).frames().len(), 10);
        assert_eq!(theme.frame(Symbol::StatusWorking, 0), "⠋");
        assert_eq!(theme.frame(Symbol::StatusWorking, 10), "⠋");
    }

    #[test]
    fn a_theme_declaring_one_token_changes_only_that_token() {
        let loaded = load(r##"{ "colors": { "accent": "#ff8800" } }"##);
        assert_eq!(loaded.theme.color(Token::Accent), Rgb(255, 136, 0));
        assert_eq!(
            loaded.theme.color(Token::TextMuted),
            default_theme().color(Token::TextMuted)
        );
        assert_eq!(
            loaded.theme.glyph(Symbol::MarkNative),
            default_theme().glyph(Symbol::MarkNative)
        );
    }

    #[test]
    fn an_alias_follows_through_the_overriding_theme() {
        // `state.success` aliases `accent` in the default, so a theme that
        // repaints the accent repaints success too — which is what aliasing
        // is for, and what the two colours meant when they were one const.
        let loaded = load(r##"{ "colors": { "accent": "#ff8800" } }"##);
        assert_eq!(loaded.theme.color(Token::StateSuccess), Rgb(255, 136, 0));
    }

    #[test]
    fn a_translucent_colour_composites_over_this_themes_own_background() {
        let loaded = load(
            r##"{ "colors": {
                   "surface.background": "#000000",
                   "surface.raised": "#ffffff80"
                 } }"##,
        );
        // Half of white over black, not over the default's near-black.
        assert_eq!(loaded.theme.color(Token::SurfaceRaised), Rgb(128, 128, 128));
    }

    #[test]
    fn an_alias_loop_is_refused_with_the_loop_written_out() {
        let error = load_str(
            "test",
            r##"{ "colors": { "accent": "@state.info", "state.info": "@accent" } }"##,
        )
        .expect_err("a loop cannot resolve");
        let message = error.to_string();
        assert!(message.contains("accent"), "{message}");
        assert!(message.contains("state.info"), "{message}");
    }

    #[test]
    fn a_malformed_colour_names_the_token_that_carries_it() {
        let error = load_str("test", r##"{ "colors": { "text.muted": "grey" } }"##)
            .expect_err("`grey` is not a colour");
        let message = error.to_string();
        assert!(message.contains("text.muted"), "{message}");
        assert!(message.contains("grey"), "{message}");
    }

    #[test]
    fn a_translucent_background_is_refused_by_name() {
        let error = load_str(
            "test",
            r##"{ "colors": { "surface.background": "#ffffff80" } }"##,
        )
        .expect_err("the background cannot be composited over itself");
        assert!(error.to_string().contains("surface.background"));
    }

    #[test]
    fn an_unknown_name_warns_and_the_theme_still_loads() {
        let loaded = load(
            r##"{ "colors": { "text.chartreuse": "#00ff00" },
                 "symbols": { "mark.hologram": "H" } }"##,
        );
        assert_eq!(loaded.warnings.len(), 2);
        assert!(
            loaded
                .warnings
                .iter()
                .any(|warning| warning.to_string().contains("text.chartreuse"))
        );
        assert_eq!(
            loaded.theme.color(Token::Accent),
            default_theme().color(Token::Accent)
        );
    }

    #[test]
    fn unreadable_text_is_reported_and_left_alone() {
        let loaded = load(
            r##"{ "colors": {
                   "surface.background": "#0a0c0d",
                   "text.primary": "#0b0d0e"
                 } }"##,
        );
        assert!(matches!(
            loaded.warnings.as_slice(),
            [Warning::LowContrast {
                token: Token::TextPrimary,
                ..
            }]
        ));
        // Reported, not corrected.
        assert_eq!(loaded.theme.color(Token::TextPrimary), Rgb(11, 13, 14));
    }

    #[test]
    fn a_surface_is_not_measured_against_itself() {
        // Every surface sits at low contrast against the background by
        // design — warning about them is what would make the warning useless.
        let loaded = load(r##"{ "colors": { "accent": "#8fd19e" } }"##);
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    }

    #[test]
    fn a_future_schema_version_warns_rather_than_refusing() {
        let loaded = load(r##"{ "version": 99, "colors": {} }"##);
        assert!(matches!(
            loaded.warnings.as_slice(),
            [Warning::UnsupportedVersion { found: 99, .. }]
        ));
    }

    #[test]
    fn a_typo_in_the_syntax_theme_is_refused_rather_than_panicking_at_first_diff() {
        let error = load_str(
            "test",
            r##"{ "syntax": { "theme": "base16-ocean-dark" } }"##,
        )
        .expect_err("an unknown syntax theme is refused");
        assert!(error.to_string().contains("base16-ocean-dark"));
    }

    #[test]
    fn the_ascii_theme_is_entirely_ascii_and_keeps_the_colours() {
        let theme = builtin("ascii").expect("bundled");
        for symbol in Symbol::ALL {
            for frame in theme.symbol(*symbol).frames() {
                assert!(
                    frame.is_ascii(),
                    "`{symbol}` is `{frame}`, which is not ASCII"
                );
            }
        }
        assert_eq!(
            theme.color(Token::Accent),
            default_theme().color(Token::Accent)
        );
    }

    #[test]
    fn every_builtin_name_resolves() {
        for name in builtin_names() {
            assert!(builtin(name).is_some(), "`{name}` is listed but absent");
        }
        assert!(builtin("nocturne").is_none());
    }
}
