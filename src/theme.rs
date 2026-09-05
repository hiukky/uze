//! Putting the operator's chosen theme in force.
//!
//! One entry point for both surfaces UZE draws: the CLI resolves it before
//! it prints anything, the TUI before it takes the terminal. Everything
//! downstream of that just asks `uze_theme::active()`, which is why nothing
//! else in the binary has to know a theme file exists.
//!
//! Nothing here can stop UZE from running. A theme is decoration; a theme
//! file with a typo in it must not be the reason a machine cannot install a
//! plugin. So every failure resolves to "keep the built-in default and say
//! why" — and *says why*, by name, because a theme that silently does
//! nothing leaves the author staring at an unchanged screen with no idea
//! which of the two possible mistakes they made.

use uze_application::{UzeApplication, UzeHome};

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
    let application = match UzeApplication::from_env(home.clone()) {
        Ok(application) => application,
        // Appearance is not worth failing a command over. If the facade
        // cannot be built, whatever the operator actually asked for is
        // about to report the same problem far more usefully.
        Err(_) => return Vec::new(),
    };
    let themes = application.themes();
    let Ok(Some(id)) = themes.active() else {
        return Vec::new();
    };

    // Their own file first: a theme they wrote named `ascii` is theirs, not
    // ours. Matches what `Themes::list` shows them.
    let written = themes.path_of(&id).ok().flatten();
    let loaded = match written {
        Some(path) => match uze_theme::load_file(&path) {
            Ok(loaded) => loaded,
            Err(error) => return vec![format!("theme `{id}`: {error}")],
        },
        None => match uze_theme::builtin(&id) {
            Some(theme) => {
                uze_theme::set_active(theme.clone());
                return Vec::new();
            }
            None => {
                return vec![format!(
                    "theme `{id}` is neither one UZE carries nor a file in {} — \
                     drawing in the default",
                    home.themes_dir().display()
                )];
            }
        },
    };

    uze_theme::set_active(loaded.theme);
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(label: &str) -> UzeHome {
        UzeHome::at(uze_testkit::temp::scratch(label))
    }

    fn write_theme(home: &UzeHome, id: &str, contents: &str) {
        fs::create_dir_all(home.themes_dir()).expect("themes dir");
        fs::write(home.themes_dir().join(format!("{id}.json")), contents).expect("theme file");
    }

    fn select(home: &UzeHome, id: &str) {
        UzeApplication::from_env(home.clone())
            .expect("application")
            .themes()
            .select(id)
            .expect("selected");
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
}
