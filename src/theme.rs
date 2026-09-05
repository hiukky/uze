//! Assembling the theme in force, and putting it there.
//!
//! One place resolves a theme id into something drawable, because appearance
//! arrives in layers and every caller wants the same stack:
//!
//! ```text
//! built-in default          every theme completes from it
//!   → ancestors             a theme may be a variation of another (`extends`)
//!     → the theme itself
//!       → the operator's overrides    applied last, whatever theme is on
//! ```
//!
//! Walking that stack lives here rather than in `uze-theme` because finding
//! a theme by id means knowing where themes live, and the design system
//! deliberately resolves no path — it is handed files. `uze-application`
//! already owns the directory, so this is the one seam where the two meet.
//!
//! Nothing here can stop UZE from running. A theme is decoration; a theme
//! file with a typo in it must not be the reason a machine cannot install a
//! plugin. So every failure resolves to "keep the built-in default and say
//! why" — and *says why*, by name, because a theme that silently does
//! nothing leaves the author staring at an unchanged screen with no idea
//! which of the two possible mistakes they made.

use uze_application::{Result, UzeApplication, UzeError, UzeHome};
use uze_theme::{Loaded, ThemeFile};

/// How deep a chain of variations may go before UZE stops following it.
///
/// A loop is already caught by name; this catches the other shape, an
/// ancestry long enough that nobody could reason about what a token
/// resolves to.
const MAX_ANCESTRY: usize = 8;

/// Resolves a theme by id into the stack it is made of.
///
/// The id may name a theme the operator wrote or one UZE carries; theirs
/// wins, the same way `uze theme list` shows it.
pub fn resolve(app: &UzeApplication, home: &UzeHome, id: &str) -> Result<Loaded> {
    resolve_with_layers(app, home, id).map(|(loaded, _)| loaded)
}

/// The same, and what it was assembled from, bottom-up — for `uze theme
/// show`, where "which file said this" is the first thing an author asks.
pub fn resolve_with_layers(
    app: &UzeApplication,
    home: &UzeHome,
    id: &str,
) -> Result<(Loaded, Vec<String>)> {
    let mut ancestry: Vec<ThemeFile> = Vec::new();
    let mut chain: Vec<String> = Vec::new();
    let mut next = Some(id.to_owned());

    while let Some(current) = next {
        if chain.contains(&current) {
            chain.push(current);
            return Err(unusable(format!(
                "theme `{id}` extends itself, round and round: {}",
                chain.join(" → ")
            )));
        }
        if chain.len() >= MAX_ANCESTRY {
            return Err(unusable(format!(
                "theme `{id}` is more than {MAX_ANCESTRY} variations deep; nothing \
                 that far from a real theme is going to be readable"
            )));
        }
        chain.push(current.clone());

        let Some(file) = written(app, home, &current, id)? else {
            // A built-in ends the chain: it is complete, and it is already
            // the layer underneath everything.
            break;
        };
        next = file.extends.clone();
        ancestry.push(file);
    }
    // Bottom-up, so the stack reads the way the layers apply.
    ancestry.reverse();

    // The theme's own file names it — never an ancestor, and never the
    // overrides layered on top.
    let identity = match ancestry.last() {
        Some(file) => uze_theme::Identity::from_file(id, file),
        None => uze_theme::Identity::from_file(
            id,
            builtin_layer(&chain).unwrap_or(uze_theme::default_file()),
        ),
    };

    let overrides = overrides(home)?;
    let mut layers: Vec<&ThemeFile> = vec![uze_theme::default_file()];
    if let Some(builtin) = builtin_layer(&chain) {
        layers.push(builtin);
    }
    layers.extend(ancestry.iter());
    if let Some(overrides) = overrides.as_ref() {
        layers.push(overrides);
    }

    let mut named: Vec<String> = vec!["the built-in default".to_owned()];
    if builtin_layer(&chain).is_some() {
        named.push(format!("`{}` (built in)", chain[chain.len() - 1]));
    }
    named.extend(
        chain
            .iter()
            .take(ancestry.len())
            .rev()
            .map(|id| format!("`{id}`")),
    );
    if overrides.is_some() {
        named.push(home.theme_overrides_path().display().to_string());
    }

    uze_theme::resolve_stack(&identity, &layers)
        .map(|loaded| (loaded, named))
        .map_err(|error| {
            // Name the file, and the ancestry when there is one: with a stack,
            // "`accent` is not a colour" leaves the operator opening several
            // files to find out which one said it.
            let source = match chain.as_slice() {
                [only] => format!("theme `{only}`"),
                chain => format!("theme `{id}` (or one of {})", chain[1..].join(", ")),
            };
            unusable(format!("{source}: {error}"))
        })
}

/// The file the operator wrote for this id, or `None` when the id names a
/// theme UZE carries instead.
fn written(
    app: &UzeApplication,
    home: &UzeHome,
    id: &str,
    requested: &str,
) -> Result<Option<ThemeFile>> {
    if let Some(path) = app.themes().path_of(id)? {
        return uze_theme::parse_file(&path)
            .map(Some)
            .map_err(|error| unusable(format!("theme `{id}`: {error}")));
    }
    if uze_theme::builtin_names().contains(&id) {
        return Ok(None);
    }
    Err(unusable(format!(
        "no theme `{id}`{} — UZE carries {}, and found none by that name in {}",
        if id == requested {
            String::new()
        } else {
            format!(", which `{requested}` extends")
        },
        uze_theme::builtin_names().join(", "),
        home.themes_dir().display()
    )))
}

/// The bundled layer a chain ended on, when it is one that carries content
/// of its own. `default` is already the bottom of every stack; `ascii` is
/// its glyphs, which is the whole reason to extend it.
fn builtin_layer(chain: &[String]) -> Option<&'static ThemeFile> {
    match chain.last()?.as_str() {
        "ascii" => Some(uze_theme::builtin_file("ascii")),
        _ => None,
    }
}

/// The operator's own overrides, if they wrote any.
fn overrides(home: &UzeHome) -> Result<Option<ThemeFile>> {
    let path = home.theme_overrides_path();
    if !path.exists() {
        return Ok(None);
    }
    uze_theme::parse_file(&path)
        .map(Some)
        .map_err(|error| unusable(format!("{}: {error}", path.display())))
}

fn unusable(message: String) -> UzeError {
    UzeError::UnusableTheme(message)
}

/// Selects the theme the operator chose, and reports only what stopped it
/// from being applied.
///
/// Deliberately not the loader's warnings. Those are worth hearing when you
/// ask about a theme — `uze theme show` prints them, and `uze theme use`
/// prints them as you choose it — but printing eight contrast notes above
/// the output of every `uze status` for the rest of the theme's life is how
/// a useful warning becomes noise the operator learns to scroll past.
/// A theme that will not load at all is different: it silently is not in
/// force, so it has to say so every time until it is fixed.
pub fn install(home: &UzeHome) -> Vec<String> {
    let Ok(app) = UzeApplication::from_env(home.clone()) else {
        // Appearance is not worth failing a command over. If the facade
        // cannot be built, whatever the operator actually asked for is
        // about to report the same problem far more usefully.
        return Vec::new();
    };

    // Overrides with no theme selected still apply: the operator's glyphs
    // are theirs, not a property of having chosen a palette.
    let id = match app.themes().active() {
        Ok(Some(id)) => id,
        Ok(None) if home.theme_overrides_path().exists() => "default".to_owned(),
        _ => return Vec::new(),
    };

    match resolve(&app, home, &id) {
        Ok(loaded) => {
            uze_theme::set_active(loaded.theme);
            Vec::new()
        }
        Err(error) => vec![error.to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(label: &str) -> UzeHome {
        UzeHome::at(uze_testkit::temp::scratch(label))
    }

    fn app(home: &UzeHome) -> UzeApplication {
        UzeApplication::from_env(home.clone()).expect("application")
    }

    fn write_theme(home: &UzeHome, id: &str, contents: &str) {
        fs::create_dir_all(home.themes_dir()).expect("themes dir");
        fs::write(home.themes_dir().join(format!("{id}.json")), contents).expect("theme file");
    }

    fn select(home: &UzeHome, id: &str) {
        app(home).themes().select(id).expect("selected");
    }

    #[test]
    fn no_selection_says_nothing_and_leaves_the_default_in_force() {
        let home = scratch("theme-install-none");
        assert!(install(&home).is_empty());
        assert_eq!(
            uze_theme::active().color(uze_theme::Token::Accent),
            uze_theme::default_theme().color(uze_theme::Token::Accent)
        );
    }

    #[test]
    fn a_broken_theme_names_the_problem_and_keeps_drawing() {
        let home = scratch("theme-install-broken");
        write_theme(
            &home,
            "broken",
            r##"{ "colors": { "accent": "chartreuse" } }"##,
        );
        select(&home, "broken");

        let problems = install(&home);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("broken"), "{problems:?}");
        assert!(problems[0].contains("accent"), "{problems:?}");
        // Still drawing: a theme nobody can load is not a reason to stop.
        assert_eq!(
            uze_theme::active().color(uze_theme::Token::Accent),
            uze_theme::default_theme().color(uze_theme::Token::Accent)
        );
    }

    #[test]
    fn a_selection_naming_nothing_says_where_it_looked() {
        let home = scratch("theme-install-missing");
        select(&home, "nocturne");
        let problems = install(&home);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("nocturne"), "{problems:?}");
        assert!(
            problems[0].contains(&home.themes_dir().display().to_string()),
            "{problems:?}"
        );
    }

    #[test]
    fn a_variation_resolves_over_the_theme_it_varies() {
        let home = scratch("theme-variation");
        write_theme(
            &home,
            "dracula",
            r##"{ "name": "Dracula",
                  "colors": { "surface.background": "#282a36", "accent": "#bd93f9" } }"##,
        );
        write_theme(
            &home,
            "dracula-soft",
            r##"{ "extends": "dracula", "name": "Dracula Soft",
                  "colors": { "surface.background": "#31333f" } }"##,
        );

        let loaded = resolve(&app(&home), &home, "dracula-soft").expect("resolves");
        assert_eq!(loaded.theme.name(), "Dracula Soft");
        // Its own.
        assert_eq!(
            loaded.theme.color(uze_theme::Token::SurfaceBackground),
            uze_theme::Rgb(0x31, 0x33, 0x3f)
        );
        // Its parent's, which it never mentioned.
        assert_eq!(
            loaded.theme.color(uze_theme::Token::Accent),
            uze_theme::Rgb(0xbd, 0x93, 0xf9)
        );
        // And a surface derived against *its* background, not its parent's.
        assert_ne!(
            loaded.theme.color(uze_theme::Token::SurfaceRaised),
            uze_theme::default_theme().color(uze_theme::Token::SurfaceRaised)
        );
    }

    #[test]
    fn a_variation_that_does_not_name_itself_is_called_by_its_id() {
        let home = scratch("theme-unnamed-variation");
        write_theme(&home, "parent", r##"{ "name": "Parent" }"##);
        write_theme(&home, "child", r##"{ "extends": "parent" }"##);

        let loaded = resolve(&app(&home), &home, "child").expect("resolves");
        assert_eq!(loaded.theme.name(), "child", "the parent named its child");
    }

    #[test]
    fn a_variation_can_extend_a_theme_uze_carries() {
        let home = scratch("theme-extends-builtin");
        write_theme(
            &home,
            "mine",
            r##"{ "extends": "ascii", "colors": { "accent": "#aabbcc" } }"##,
        );
        let loaded = resolve(&app(&home), &home, "mine").expect("resolves");
        // The ASCII glyphs came with it; the colour is the author's own.
        assert_eq!(loaded.theme.glyph(uze_theme::Symbol::StatusIdle), ".");
        assert_eq!(
            loaded.theme.color(uze_theme::Token::Accent),
            uze_theme::Rgb(0xaa, 0xbb, 0xcc)
        );
    }

    #[test]
    fn a_chain_that_loops_is_refused_with_the_loop_written_out() {
        let home = scratch("theme-loop");
        write_theme(&home, "a", r##"{ "extends": "b" }"##);
        write_theme(&home, "b", r##"{ "extends": "a" }"##);

        let error = resolve(&app(&home), &home, "a").expect_err("a loop cannot resolve");
        let message = error.to_string();
        assert!(message.contains("a → b → a"), "{message}");
    }

    #[test]
    fn extending_something_that_does_not_exist_says_which_theme_asked() {
        let home = scratch("theme-orphan");
        write_theme(&home, "mine", r##"{ "extends": "nowhere" }"##);
        let error = resolve(&app(&home), &home, "mine").expect_err("no parent");
        let message = error.to_string();
        assert!(message.contains("nowhere"), "{message}");
        assert!(message.contains("`mine` extends"), "{message}");
    }

    #[test]
    fn the_operators_overrides_outlast_the_theme_they_are_applied_over() {
        let home = scratch("theme-overrides");
        fs::write(
            home.theme_overrides_path(),
            r##"{ "symbols": { "status.idle": "@" }, "colors": { "text.muted": "#010203" } }"##,
        )
        .expect("overrides");
        write_theme(&home, "one", r##"{ "symbols": { "status.idle": "1" } }"##);
        write_theme(&home, "two", r##"{ "symbols": { "status.idle": "2" } }"##);

        for id in ["one", "two"] {
            let loaded = resolve(&app(&home), &home, id).expect("resolves");
            assert_eq!(
                loaded.theme.glyph(uze_theme::Symbol::StatusIdle),
                "@",
                "`{id}` overrode the operator's own glyph"
            );
            assert_eq!(
                loaded.theme.color(uze_theme::Token::TextMuted),
                uze_theme::Rgb(1, 2, 3)
            );
        }
    }
}
