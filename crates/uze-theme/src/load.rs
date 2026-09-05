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
    collections::{BTreeMap, BTreeSet},
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
/// Below WCAG AA (4.5) on purpose: UZE's design draws several deliberately
/// recessed levels, and a check that fires on the shipped default is a check
/// every author learns to ignore. Every text token in the built-in theme
/// clears 3.0 except one, and that one is exempt by name below.
///
/// What this actually catches is the failure mode a partial theme makes
/// easy: repaint the background and the state hues you did not declare stay
/// where they were, so an amber tuned for near-black lands at 1.8:1 on a
/// light page. The author cannot see that from their own file — every
/// colour in it looks fine — which is exactly when a warning earns its keep.
const MIN_CONTRAST: f32 = 3.0;

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

/// A bundled theme *as written*, for a caller assembling a stack that ends
/// on one. `default` is the bottom of every stack already, so what this is
/// really for is `ascii`, whose glyphs are the reason to extend it.
pub fn builtin_file(id: &str) -> &'static ThemeFile {
    static ASCII: OnceLock<ThemeFile> = OnceLock::new();
    match id {
        "ascii" => ASCII.get_or_init(|| {
            serde_json::from_str(BUILTIN_ASCII).expect("the bundled ascii theme is valid JSON")
        }),
        _ => default_file(),
    }
}

/// The theme every partial theme is completed from, and the one UZE falls
/// back to whenever a selected theme cannot be loaded.
pub fn default_theme() -> &'static Theme {
    static DEFAULT: OnceLock<Theme> = OnceLock::new();
    DEFAULT.get_or_init(|| {
        resolve_stack(
            &Identity::from_file("default", default_file()),
            &[default_file()],
        )
        .expect("the bundled default theme resolves and is complete")
        .theme
    })
}

/// The default theme *as written*, which is the bottom of every stack.
///
/// Merging happens between declarations rather than between resolved colours,
/// and that is not an implementation detail: `state.success` is written
/// `@accent` in the default, so a theme that repaints the accent has to
/// repaint success with it. Completing from an already-resolved theme would
/// hand it the old accent's value and quietly break every alias the default
/// relies on.
pub fn default_file() -> &'static ThemeFile {
    static FILE: OnceLock<ThemeFile> = OnceLock::new();
    FILE.get_or_init(|| {
        serde_json::from_str(BUILTIN_DEFAULT).expect("the bundled default theme is valid JSON")
    })
}

/// Reads a theme file, without resolving it.
///
/// Parsing and resolving are separate because a theme can name a parent, and
/// only the caller knows where a theme id lives — this crate resolves no
/// path. Read the file, follow its [`ThemeFile::extends`], and hand the
/// whole stack to [`resolve_stack`].
pub fn parse_file(path: &Path) -> Result<ThemeFile, LoadError> {
    let contents = fs::read_to_string(path).map_err(|source| LoadError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    parse_str(&path.to_string_lossy(), &contents)
}

pub fn parse_str(id: &str, contents: &str) -> Result<ThemeFile, LoadError> {
    serde_json::from_str(contents).map_err(|source| LoadError::Parse {
        path: PathBuf::from(id),
        source,
    })
}

/// A theme with no ancestry of its own, resolved over the built-in default.
/// The common case, and what the bundled themes and the tests use.
pub fn load_str(id: &str, contents: &str) -> Result<Loaded, LoadError> {
    let file = parse_str(id, contents)?;
    resolve_stack(&Identity::from_file(id, &file), &[default_file(), &file])
}

/// Who a resolved theme says it is.
///
/// Explicit rather than inferred from the layers, because the layers cannot
/// answer it: an ancestor naming itself must not name its variation, and the
/// operator's overrides — the outermost layer of all — must not rename the
/// theme they are applied over. Only the caller knows which file *is* the
/// theme, so the caller says.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Identity {
    pub id: String,
    pub name: Option<String>,
    pub description: Option<String>,
}

impl Identity {
    /// A theme identified only by the id it is selected with.
    pub fn of(id: &str) -> Self {
        Self {
            id: id.to_owned(),
            ..Self::default()
        }
    }

    /// The identity a theme file declares for itself, falling back to its id.
    pub fn from_file(id: &str, file: &ThemeFile) -> Self {
        Self {
            id: id.to_owned(),
            name: file.name.clone(),
            description: file.description.clone(),
        }
    }
}

/// Resolves a theme from the layers it is made of, nearest last.
///
/// A stack rather than a file and a base, because appearance arrives in
/// layers and always did: the built-in default underneath, a theme that may
/// itself be a variation of another theme, and — last, so it wins — whatever
/// the operator overrode for themselves. Merging happens between
/// *declarations* at every level, which is what keeps an ancestor's aliases
/// alive: repaint `accent` in a variation and everything written `@accent`
/// two layers down follows.
///
/// `layers[0]` has to be complete; every other layer may declare as little
/// as one token.
pub fn resolve_stack(identity: &Identity, layers: &[&ThemeFile]) -> Result<Loaded, LoadError> {
    let id = identity.id.as_str();
    let mut warnings = Vec::new();
    let Some((base, authored)) = layers.split_first() else {
        return Err(LoadError::Incomplete {
            id: id.to_owned(),
            missing: vec!["everything: no layers were given".to_owned()],
        });
    };

    for layer in authored {
        if let Some(version) = layer.version
            && version > CURRENT_VERSION
        {
            warnings.push(Warning::UnsupportedVersion {
                found: version,
                supported: CURRENT_VERSION,
            });
        }
    }

    let mut colors_written = base.colors.clone();
    let mut symbols_written = base.symbols.clone();
    for layer in authored {
        for (name, value) in &layer.colors {
            colors_written.insert(name.clone(), value.clone());
        }
        for (name, value) in &layer.symbols {
            symbols_written.insert(name.clone(), value.clone());
        }
    }

    // A name only the built-in layer uses is one this build put there; a
    // name someone wrote is worth telling them about.
    let authored_names = |pick: fn(&ThemeFile) -> Vec<&String>| -> BTreeSet<String> {
        authored
            .iter()
            .flat_map(|layer| pick(layer))
            .cloned()
            .collect()
    };
    let declared = known_by_name(
        &colors_written,
        &authored_names(|layer| layer.colors.keys().collect()),
        "colour token",
        &mut warnings,
    );
    let symbols_declared = known_by_name(
        &symbols_written,
        &authored_names(|layer| layer.symbols.keys().collect()),
        "symbol",
        &mut warnings,
    );

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
    let syntax_theme = resolve_syntax_theme(layers)?;

    for token in Token::ALL {
        // A surface is not read against itself, and a pane's own 16 belong
        // to the programs running inside it rather than to UZE's text.
        //
        // `text.faint` is exempt because being barely legible is its
        // definition — the tree glyphs and age columns nobody reads unless
        // they went looking. Warning about a token whose whole job is to sit
        // at the edge would fire on every theme ever written.
        if token.is_surface() || Token::ANSI.contains(token) || *token == Token::TextFaint {
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

    let name = identity.name.clone().unwrap_or_else(|| id.to_owned());
    let description = identity.description.clone().unwrap_or_default();
    Ok(Loaded {
        theme: Theme::new(name, description, colors, symbols, syntax_theme),
        warnings,
    })
}

/// Drops the entries this build has no vocabulary for, warning about each
/// one the author wrote themselves, so nothing downstream carries the
/// possibility of a name that means nothing.
fn known_by_name<K: Ord + Copy + Named, V: Clone>(
    written: &BTreeMap<String, V>,
    authored: &BTreeSet<String>,
    kind: &'static str,
    warnings: &mut Vec<Warning>,
) -> BTreeMap<K, V> {
    let mut known = BTreeMap::new();
    for (name, value) in written {
        match K::from_written_name(name) {
            Some(key) => {
                known.insert(key, value.clone());
            }
            None if authored.contains(name) => warnings.push(Warning::UnknownName {
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
        Some(Declaration::Contrast(alpha)) => {
            let separator = if background.is_light() {
                Rgb(0, 0, 0)
            } else {
                Rgb(255, 255, 255)
            };
            Ok(separator.over(background, alpha))
        }
        Some(Declaration::Alias(target)) => {
            follow(token, &target, value, declared, background, chain)
        }
        Some(Declaration::TintedAlias(target, alpha)) => {
            follow(token, &target, value, declared, background, chain)
                .map(|rgb| rgb.over(background, alpha))
        }
        None => Err(LoadError::Color {
            token: token.name().to_owned(),
            value: value.clone(),
            reason: "expected `#rrggbb`, `#rrggbbaa`, `~aa`, `@another.token`, \
                     or `@another.token/aa`",
        }),
    }
}

/// Resolves the token an alias points at, keeping the chain so a loop is
/// reported as the loop it is.
fn follow(
    token: Token,
    target: &str,
    value: &str,
    declared: &BTreeMap<Token, ColorValue>,
    background: Rgb,
    chain: &mut Vec<Token>,
) -> Result<Rgb, LoadError> {
    let Some(target) = Token::from_name(target) else {
        return Err(LoadError::Color {
            token: token.name().to_owned(),
            value: value.to_owned(),
            reason: "it aliases a token that does not exist",
        });
    };
    chain.push(token);
    let resolved = resolve_one(target, declared, background, chain);
    chain.pop();
    resolved
}

enum Declaration {
    Opaque(Rgb),
    Translucent(Rgb, u8),
    /// Separate from the background by this much, in whichever direction is
    /// visible against it.
    Contrast(u8),
    Alias(String),
    /// Another token's colour, at this alpha, over the background.
    ///
    /// Without this a tint had to be written as a literal — which is how
    /// the selected row and the diff washes ended up carrying UZE's own
    /// sage green into every theme that repainted the accent.
    TintedAlias(String, u8),
}

fn parse_color(value: &str) -> Option<Declaration> {
    if let Some(target) = value.strip_prefix('@') {
        if let Some((target, alpha)) = target.split_once('/') {
            return (!target.is_empty() && alpha.len() == 2)
                .then(|| u8::from_str_radix(alpha, 16).ok())
                .flatten()
                .map(|alpha| Declaration::TintedAlias(target.to_owned(), alpha));
        }
        return (!target.is_empty()).then(|| Declaration::Alias(target.to_owned()));
    }
    if let Some(alpha) = value.strip_prefix('~') {
        return (alpha.len() == 2)
            .then(|| u8::from_str_radix(alpha, 16).ok())
            .flatten()
            .map(Declaration::Contrast);
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

/// The nearest layer that named a syntax palette wins, so a variation
/// inherits its parent's unless it says otherwise.
fn resolve_syntax_theme(layers: &[&ThemeFile]) -> Result<String, LoadError> {
    let named = layers.iter().rev().find_map(|layer| {
        layer
            .syntax
            .as_ref()
            .and_then(|syntax| syntax.theme.clone())
    });
    let Some(name) = named else {
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
    fn the_default_carries_the_palette_that_shipped() {
        // The premise of the migration: replacing 676 colour constants with
        // token lookups changes no pixel. Each pair below is the constant
        // `src/ui.rs` used to hold, against the token that replaced it.
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
            // Derived, and landing exactly where the hand-blended constants
            // did — see `derived_surfaces_are_composited_rather_than_transcribed`.
            (Token::BorderFaint, Rgb(22, 24, 25)),
            (Token::SurfaceSelected, Rgb(22, 30, 26)),
            (Token::SurfaceRaised, Rgb(32, 34, 35)),
            (Token::SurfaceRaisedSubtle, Rgb(27, 29, 30)),
            (Token::SurfaceRecessed, Rgb(16, 18, 19)),
        ];
        for (token, rgb) in expected {
            assert_eq!(theme.color(token), rgb, "`{token}` drifted from the design");
        }
    }

    #[test]
    fn derived_surfaces_are_composited_rather_than_transcribed() {
        // Every surface and border is declared as a translucent colour over
        // the theme's own background rather than as the opaque value it
        // resolves to on the dark palette. It has to be: a theme that
        // repaints the background and inherits an opaque shade computed
        // against the old one gets dark slabs on a light page — which is
        // exactly what a five-line light theme produced before this.
        //
        // Four of them no longer land on the byte the design shipped. Two
        // are off by one in one channel, because those constants had been
        // nudged a step off their own stated alpha by hand. The two diff
        // washes moved further: they were mixed by eye rather than derived
        // from the state colours at all, and no single alpha reproduces
        // them. A background tint behind a line whose gutter and text
        // already say what happened to it is the right thing to spend that
        // on.
        let theme = default_theme();
        assert_eq!(theme.color(Token::BorderDefault), Rgb(29, 31, 32)); // was (30, 31, 32)
        assert_eq!(theme.color(Token::SurfaceRaisedBright), Rgb(45, 46, 47)); // was (44, 46, 47)
        assert_eq!(theme.color(Token::StateDiffAdded), Rgb(24, 32, 28)); // was (18, 32, 23)
        assert_eq!(theme.color(Token::StateDiffRemoved), Rgb(32, 23, 21)); // was (38, 22, 20)
    }

    #[test]
    fn a_tint_follows_the_token_it_tints() {
        // The defect a real third-party palette exposed: the selected row
        // and the diff washes were written as literal alpha values of UZE's
        // own sage, so a theme that repainted the accent got a green
        // selection anyway.
        let loaded = load(r##"{ "colors": { "accent": "#bd93f9" } }"##);
        let background = loaded.theme.background();
        assert_eq!(
            loaded.theme.color(Token::SurfaceSelected),
            Rgb(0xbd, 0x93, 0xf9).over(background, 0x17)
        );
        let repainted = load(r##"{ "colors": { "state.danger": "#ff5555" } }"##);
        assert_eq!(
            repainted.theme.color(Token::StateDiffRemoved),
            Rgb(0xff, 0x55, 0x55).over(repainted.theme.background(), 0x1a)
        );
    }

    #[test]
    fn the_terminals_own_sixteen_keep_their_hues_when_a_theme_repaints_a_meaning() {
        // `ansi.2` is the *green* a program asks for by index. It used to
        // alias `accent`, which is green in UZE's own palette and purple in
        // Dracula's — so a pane printing green came out purple.
        let loaded = load(r##"{ "colors": { "accent": "#bd93f9", "state.info": "#ff79c6" } }"##);
        assert_eq!(loaded.theme.color(Token::Accent), Rgb(0xbd, 0x93, 0xf9));
        assert_eq!(
            loaded.theme.ansi(2),
            default_theme().ansi(2),
            "the terminal's green followed a meaning instead of staying green"
        );
        assert_eq!(loaded.theme.ansi(4), default_theme().ansi(4));
        // The four that genuinely are a role still follow it.
        let dark = load(r##"{ "colors": { "surface.background": "#282a36" } }"##);
        assert_eq!(dark.theme.ansi(0), Some(Rgb(0x28, 0x2a, 0x36)));
    }

    #[test]
    fn a_tinted_alias_that_names_nothing_is_refused() {
        let error = load_str(
            "test",
            r##"{ "colors": { "surface.selected": "@nope/17" } }"##,
        )
        .expect_err("an alias to nothing cannot resolve");
        assert!(error.to_string().contains("surface.selected"));
    }

    #[test]
    fn an_ancestors_name_never_becomes_its_variations_name() {
        // A variation that does not name itself is called by its id. Letting
        // the layer underneath answer would have every unnamed variation of
        // Dracula reporting as "Dracula".
        let parent = parse_str("parent", r##"{ "name": "Parent" }"##).expect("parent parses");
        let child = parse_str("child", r##"{ "colors": {} }"##).expect("child parses");
        let loaded = resolve_stack(
            &Identity::from_file("child", &child),
            &[default_file(), &parent, &child],
        )
        .expect("resolves");
        assert_eq!(loaded.theme.name(), "child");
    }

    #[test]
    fn a_variation_inherits_its_parents_declarations_and_overrides_a_few() {
        // What `extends` buys, resolved here as the stack it becomes: the
        // parent's aliases stay alive, so repainting the accent in the child
        // moves everything written `@accent` in the parent.
        let parent = parse_str(
            "parent",
            r##"{ "name": "parent", "colors": { "accent": "#112233", "text.muted": "#445566" } }"##,
        )
        .expect("parent parses");
        let child = parse_str(
            "child",
            r##"{ "name": "child", "colors": { "accent": "#aabbcc" } }"##,
        )
        .expect("child parses");

        let loaded = resolve_stack(
            &Identity::from_file("child", &child),
            &[default_file(), &parent, &child],
        )
        .expect("resolves");
        assert_eq!(loaded.theme.name(), "child");
        assert_eq!(loaded.theme.color(Token::Accent), Rgb(0xaa, 0xbb, 0xcc));
        // Inherited from the parent, which the child never mentioned.
        assert_eq!(loaded.theme.color(Token::TextMuted), Rgb(0x44, 0x55, 0x66));
        // Written `@accent` two layers down, and it followed the child.
        assert_eq!(
            loaded.theme.color(Token::StateSuccess),
            Rgb(0xaa, 0xbb, 0xcc)
        );
    }

    #[test]
    fn the_last_layer_wins_whatever_the_theme_underneath_it_says() {
        // The operator's own overrides: a Nerd Font's glyphs, kept across
        // every theme they switch to.
        let theme = parse_str(
            "theme",
            r##"{ "name": "theme", "symbols": { "status.idle": "o" } }"##,
        )
        .expect("theme parses");
        let nerd_font_dot = "\u{f111}";
        let overrides = parse_str(
            "overrides",
            &format!(r##"{{ "symbols": {{ "status.idle": "{nerd_font_dot}" }} }}"##),
        )
        .expect("overrides parse");

        // The identity is the theme's own — the overrides layer sits on top
        // of it without renaming it.
        let loaded = resolve_stack(
            &Identity::from_file("theme", &theme),
            &[default_file(), &theme, &overrides],
        )
        .expect("resolves");
        assert_eq!(loaded.theme.glyph(Symbol::StatusIdle), nerd_font_dot);
        // And the overrides did not rename the theme out from under it.
        assert_eq!(loaded.theme.name(), "theme");
    }

    #[test]
    fn a_contrast_overlay_turns_around_with_the_background() {
        let dark = load(r##"{ "colors": { "surface.background": "#000000" } }"##);
        let light = load(r##"{ "colors": { "surface.background": "#ffffff" } }"##);
        // `surface.raised` is `~17` in the built-in default: separate from
        // the background by that much, whichever way is visible.
        assert_eq!(dark.theme.color(Token::SurfaceRaised), Rgb(23, 23, 23));
        assert_eq!(light.theme.color(Token::SurfaceRaised), Rgb(232, 232, 232));
    }

    #[test]
    fn a_light_theme_gets_light_surfaces_from_the_background_alone() {
        // The property the derived form buys, and the reason it is worth
        // four moved bytes: declaring the background is enough.
        let loaded = load(r##"{ "colors": { "surface.background": "#faf7f2" } }"##);
        for surface in [
            Token::SurfaceRaised,
            Token::SurfaceRaisedSubtle,
            Token::SurfaceRaisedBright,
            Token::SurfaceRecessed,
            Token::BorderDefault,
            Token::BorderFaint,
            Token::StateDiffAdded,
            Token::StateDiffRemoved,
        ] {
            let rgb = loaded.theme.color(surface);
            let brightness = u16::from(rgb.0) + u16::from(rgb.1) + u16::from(rgb.2);
            assert!(
                brightness > 500,
                "`{surface}` resolved to {rgb}, a dark slab on a light page"
            );
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
    fn the_bundled_syntax_theme_list_matches_what_is_actually_bundled() {
        // This crate names no highlighting library in production — it only
        // has to be able to tell an author their theme name is a typo. That
        // makes the list a copy, and a copy needs the test that catches it
        // drifting.
        let mut actual: Vec<String> = syntect::highlighting::ThemeSet::load_defaults()
            .themes
            .keys()
            .cloned()
            .collect();
        actual.sort();
        assert_eq!(actual, BUNDLED_SYNTAX_THEMES);
    }

    /// No glyph UZE ships is one a terminal draws from its emoji font.
    ///
    /// An emoji is a different family, a width that varies by terminal, and
    /// a picture that ignores the hue carrying the meaning — three reasons a
    /// status mark cannot be one. This is a rule about *UZE's own* themes:
    /// a theme someone writes is theirs, and may use whatever their terminal
    /// renders.
    ///
    /// The ranges below are the pictographic ones — where a codepoint has an
    /// emoji presentation or is drawn from the emoji font in practice.
    /// Dingbats and Geometric Shapes are deliberately *not* banned wholesale:
    /// `✓`, `✕`, `✦`, `❯`, `●` and `○` live there and are text, which is the
    /// whole distinction. The handful of Dingbats that do carry an emoji
    /// presentation are named individually.
    #[test]
    fn no_bundled_glyph_is_an_emoji() {
        const PICTOGRAPHIC: &[(u32, u32)] = &[
            (0x2600, 0x26FF),   // Miscellaneous Symbols — ⚠ and its neighbours
            (0x2B00, 0x2BFF),   // Miscellaneous Symbols and Arrows
            (0xFE00, 0xFE0F),   // variation selectors, including VS16
            (0x1F000, 0x1FAFF), // the emoji planes
        ];
        // Dingbats is mixed; these are the members with an emoji presentation.
        const EMOJI_DINGBATS: &[u32] = &[
            0x2702, 0x2705, 0x2708, 0x2709, 0x270A, 0x270B, 0x270C, 0x270D, 0x270F, 0x2712, 0x2714,
            0x2716, 0x271D, 0x2721, 0x2728, 0x2733, 0x2734, 0x2744, 0x2747, 0x274C, 0x274E, 0x2753,
            0x2754, 0x2755, 0x2757, 0x2763, 0x2764, 0x2795, 0x2796, 0x2797, 0x27B0, 0x27BF,
        ];

        for id in builtin_names() {
            let theme = builtin(id).expect("bundled");
            for symbol in Symbol::ALL {
                for frame in theme.symbol(*symbol).frames() {
                    for character in frame.chars() {
                        let point = character as u32;
                        let pictographic = PICTOGRAPHIC
                            .iter()
                            .any(|(low, high)| (*low..=*high).contains(&point));
                        assert!(
                            !pictographic && !EMOJI_DINGBATS.contains(&point),
                            "theme `{id}` draws `{symbol}` as `{character}` \
                             (U+{point:04X}), which terminals render from the \
                             emoji font"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn every_builtin_name_resolves() {
        for name in builtin_names() {
            assert!(builtin(name).is_some(), "`{name}` is listed but absent");
        }
        assert!(builtin("nocturne").is_none());
    }
}
