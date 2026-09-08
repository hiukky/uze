//! The keymap in force, right now.
//!
//! A keymap is process-wide and cannot change in the middle of a keystroke,
//! so it lives here rather than as a parameter threaded through every
//! dispatcher and every surface that prints a key — none of which could
//! legitimately disagree about it. The lock is what lets the Keys screen
//! rebind without restarting: the write happens between events, and every
//! read on the input path is uncontended.
//!
//! There is no initialisation step. Before anything is loaded — in a test,
//! in a CLI invocation that never reads a keymap file, in code that runs
//! before `$UZE_HOME` is resolved — [`active`] answers with the built-in
//! default, which needs no I/O.

use std::sync::{Arc, OnceLock, RwLock};

use crate::{Keymap, load::default_keymap};

fn cell() -> &'static RwLock<Arc<Keymap>> {
    static ACTIVE: OnceLock<RwLock<Arc<Keymap>>> = OnceLock::new();
    ACTIVE.get_or_init(|| RwLock::new(Arc::new(default_keymap().clone())))
}

/// The keymap in force. Cheap enough to call per keystroke: it clones an
/// `Arc`.
pub fn active() -> Arc<Keymap> {
    match cell().read() {
        Ok(keymap) => Arc::clone(&keymap),
        // A panic while a keymap was being swapped must not take the
        // operator's keyboard with it — the default beats nothing.
        Err(poisoned) => Arc::clone(&poisoned.into_inner()),
    }
}

/// Puts a keymap in force. Every keystroke after this resolves against it.
pub fn set_active(keymap: Keymap) {
    let keymap = Arc::new(keymap);
    match cell().write() {
        Ok(mut active) => *active = keymap,
        Err(poisoned) => *poisoned.into_inner() = keymap,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::{Action, Chord, Scope, keymap::Resolution};

    /// The active keymap is process-wide, so tests that read or replace it
    /// take turns — otherwise one test's swap is another's flake.
    static SERIAL: Mutex<()> = Mutex::new(());

    #[test]
    fn a_keymap_is_active_before_anything_is_loaded() {
        let _turn = SERIAL
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Not a test of what it binds — a test that asking costs no setup,
        // which is what lets a unit test or a CLI path resolve a key at all.
        assert_eq!(
            active().resolve(
                Chord::parse("ctrl+o").unwrap(),
                &[Scope::Workspace, Scope::Pane]
            ),
            Resolution::Act(Action::SwitchMode)
        );
    }

    #[test]
    fn what_is_put_in_force_is_what_answers() {
        let _turn = SERIAL
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let moved = default_keymap()
            .rebind(
                Action::NewShellTab,
                Scope::Workspace,
                Chord::parse("f4").ok(),
            )
            .expect("no conflict");
        set_active(moved);
        assert_eq!(
            active().resolve(
                Chord::parse("f4").unwrap(),
                &[Scope::Workspace, Scope::Pane]
            ),
            Resolution::Act(Action::NewShellTab)
        );
        set_active(default_keymap().clone());
    }
}
