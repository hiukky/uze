//! Putting the operator's keyboard in force.
//!
//! The same shape as `theme::install`, for the same reasons. A keymap that
//! will not load is not silently ignored — an operator whose file has a
//! typo would otherwise press a key, get nothing, and have no idea why —
//! but the entries that merely name something this build has never heard
//! of are the loader's warnings, and those belong on the Keys screen
//! rather than above the output of every command for the rest of the
//! file's life.

use uze_application::UzeHome;

/// Reads `keys.json` and puts what it says in force, returning only what
/// stopped it from being applied.
///
/// A missing file is not a problem: the built-in keymap is a complete
/// keyboard on its own, and most people will never write one.
pub fn install(home: &UzeHome) -> Vec<String> {
    let path = home.keymap_path();
    let file = match uze_keys::load::read(&path) {
        Ok(Some(file)) => file,
        Ok(None) => return Vec::new(),
        Err(error) => return vec![format!("{}: {error}", path.display())],
    };
    match uze_keys::load::resolve(&file) {
        Ok(loaded) => {
            uze_keys::set_active(loaded.keymap);
            Vec::new()
        }
        // The keymap already in force stays in force — which is the
        // built-in one, so the operator still has a keyboard to fix the
        // file with.
        Err(problems) => problems
            .into_iter()
            .map(|problem| format!("{}: {}", path.display(), problem.message))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uze_keys::{Action, Chord, Scope};

    /// The active keymap is process-wide, so tests that replace it take
    /// turns — otherwise one test's swap is another's flake.
    static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn home() -> (tempdir::Scratch, UzeHome) {
        let scratch = tempdir::Scratch::new();
        let home = UzeHome::at(scratch.path().to_path_buf());
        (scratch, home)
    }

    #[test]
    fn no_file_is_no_problem() {
        let _turn = SERIAL
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (_scratch, home) = home();
        assert!(install(&home).is_empty());
    }

    #[test]
    fn what_the_operator_wrote_is_in_force_before_anything_is_drawn() {
        let _turn = SERIAL
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (_scratch, home) = home();
        std::fs::write(
            home.keymap_path(),
            r#"{"bindings":{"workspace":{"new-shell-tab":["f4"]}}}"#,
        )
        .expect("write");

        assert!(install(&home).is_empty());
        assert_eq!(
            uze_keys::active().chord_for(Action::NewShellTab, &[Scope::Workspace]),
            Chord::parse("f4").ok()
        );
        uze_keys::set_active(uze_keys::default_keymap().clone());
    }

    #[test]
    fn a_file_that_would_take_the_keyboard_away_says_so_and_changes_nothing() {
        let _turn = SERIAL
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (_scratch, home) = home();
        // `ctrl+m` is Enter on a terminal; binding it would take Enter.
        std::fs::write(
            home.keymap_path(),
            r#"{"bindings":{"workspace":{"close-tab":["ctrl+m"]}}}"#,
        )
        .expect("write");

        let problems = install(&home);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("Enter"), "{problems:?}");
        assert_eq!(
            uze_keys::active().chord_for(Action::CloseTab, &[Scope::Workspace]),
            Chord::parse("ctrl+w").ok(),
            "the keyboard in force is untouched, so the file can be fixed"
        );
    }

    /// A scratch directory that cleans up after itself. The workspace's
    /// test kit is a dev-dependency of other crates, not of the binary.
    mod tempdir {
        use std::path::{Path, PathBuf};

        pub struct Scratch(PathBuf);

        impl Scratch {
            pub fn new() -> Scratch {
                let path = std::env::temp_dir().join(format!(
                    "uze-keymap-{}-{}",
                    std::process::id(),
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|since| since.as_nanos())
                        .unwrap_or_default()
                ));
                std::fs::create_dir_all(&path).expect("scratch");
                Scratch(path)
            }

            pub fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for Scratch {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }
}
