//! The theme every surface draws against, right now.
//!
//! A theme is process-wide and cannot change in the middle of a frame, so it
//! lives here rather than as a parameter threaded through several hundred
//! call sites that could never legitimately disagree about it. The lock is
//! what lets a running TUI switch themes without restarting: the write
//! happens once, between frames, and every read on the draw path is
//! uncontended.
//!
//! There is no initialisation step. Before anything is loaded — in a test, in
//! a CLI invocation that never reads a theme file, in code that runs before
//! `$UZE_HOME` is resolved — [`active`] answers with the built-in default,
//! which needs no I/O.

use std::sync::{Arc, OnceLock, RwLock};

use crate::{Theme, load::default_theme};

fn cell() -> &'static RwLock<Arc<Theme>> {
    static ACTIVE: OnceLock<RwLock<Arc<Theme>>> = OnceLock::new();
    ACTIVE.get_or_init(|| RwLock::new(Arc::new(default_theme().clone())))
}

/// The theme in force. Cheap enough to call per span: it clones an `Arc`.
pub fn active() -> Arc<Theme> {
    match cell().read() {
        Ok(theme) => Arc::clone(&theme),
        // A panic while a theme was being swapped must not take the UI's
        // colours with it — drawing in the default beats not drawing.
        Err(poisoned) => Arc::clone(&poisoned.into_inner()),
    }
}

/// Puts a theme in force. Every surface drawn after this uses it.
pub fn set_active(theme: Theme) {
    let theme = Arc::new(theme);
    match cell().write() {
        Ok(mut active) => *active = theme,
        Err(poisoned) => *poisoned.into_inner() = theme,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::{Token, load::builtin};

    /// The active theme is process-wide, so tests that read or replace it
    /// take turns — otherwise one test's swap is another's flake.
    static SERIAL: Mutex<()> = Mutex::new(());

    #[test]
    fn a_theme_is_active_before_anything_is_loaded() {
        let _turn = SERIAL
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Not a test of the default's values — a test that asking costs no
        // setup, which is what lets a CLI path or a unit test draw at all.
        assert_eq!(
            active().color(Token::Accent),
            default_theme().color(Token::Accent)
        );
    }

    #[test]
    fn setting_a_theme_replaces_what_every_later_read_sees() {
        let _turn = SERIAL
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let ascii = builtin("ascii").expect("bundled").clone();
        set_active(ascii);
        assert_eq!(active().glyph(crate::Symbol::StatusIdle), ".");
        // Put the default back: the active theme is process-wide, and a test
        // that leaves it changed is a test that breaks its neighbours.
        set_active(default_theme().clone());
        assert_eq!(active().glyph(crate::Symbol::StatusIdle), "○");
    }
}
