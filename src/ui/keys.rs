//! TUI — the one place uze names a physical key.
//!
//! `uze-keys` is a leaf crate that knows no terminal, the way `uze-theme`
//! knows no rendering library. This module is the adapter between the two
//! worlds: crossterm's key events become [`Chord`]s here, and nothing
//! outside this file may name `KeyCode` at all — a rule
//! `tests/architecture/layering.rs` holds, with the PTY encoder sanctioned
//! by name because translating a key into bytes for a pane is a different
//! job from binding one.
//!
//! It also owns the keyboard handshake. Without the enhancement protocol a
//! terminal has three ways to encode a keystroke — a character, a control
//! byte, an escape sequence — and several chords have no encoding at all
//! (see `uze_keys::Tier`). When the host supports the protocol uze asks for
//! **disambiguation only**: reporting key *releases* as well would double
//! every keystroke forwarded into a pane, and reporting alternate keys
//! would change what a keystroke means on a non-US layout. What arrives is
//! then filtered to presses, so nothing downstream has to know whether the
//! handshake happened.

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};

use uze_keys::{Chord, Key, Mods, Tier};

/// What this terminal can deliver, asked once at startup.
///
/// Whether a chord reaches uze at all is a property of the host, not of the
/// keymap, and the Keys screen shows the difference rather than letting a
/// binding look alive and do nothing.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct KeyboardSupport {
    pub(crate) enhanced: bool,
}

impl KeyboardSupport {
    /// Whether a chord of this tier can reach uze on this machine.
    ///
    /// `HostConditional` answers `true`: uze cannot know from here whether
    /// Option sends Meta or composes a character, and claiming otherwise
    /// would be a guess dressed as a fact. The Keys screen says what the
    /// tier means and offers the probe, which is the only honest answer.
    pub(crate) fn can_deliver(self, tier: Tier) -> bool {
        match tier {
            Tier::Universal | Tier::HostConditional => true,
            Tier::EnhancementOnly => self.enhanced,
        }
    }
}

/// Asks the terminal for disambiguated keys, and says whether it agreed.
pub(crate) fn begin_enhanced_input() -> KeyboardSupport {
    let enhanced = crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false);
    if enhanced {
        let _ = crossterm::execute!(
            std::io::stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        );
    }
    KeyboardSupport { enhanced }
}

/// Gives the terminal its own keyboard back, with the rest of the cleanup.
pub(crate) fn end_enhanced_input(support: KeyboardSupport) {
    if support.enhanced {
        let _ = crossterm::execute!(std::io::stdout(), PopKeyboardEnhancementFlags);
    }
}

/// The chord a key event stands for, or `None` for an event that is not a
/// keystroke uze binds against — a release or repeat under the enhancement
/// protocol, or a key outside the vocabulary.
///
/// Repeats are dropped rather than treated as presses: a held key that
/// closed a tab once must not close nine more.
pub(crate) fn chord_of(event: KeyEvent) -> Option<Chord> {
    if event.kind != KeyEventKind::Press {
        return None;
    }
    let key = match event.code {
        KeyCode::Char(character) => Key::Char(character),
        KeyCode::F(number) => Key::F(number),
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Esc,
        KeyCode::Tab => Key::Tab,
        // Crossterm reports Shift+Tab as its own code; the vocabulary
        // treats Shift as a modifier of Tab, so it is re-joined here.
        KeyCode::BackTab => Key::Tab,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::Delete => Key::Delete,
        KeyCode::Insert => Key::Insert,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Left => Key::Left,
        KeyCode::Right => Key::Right,
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        _ => return None,
    };
    let mut mods = Mods {
        ctrl: event.modifiers.contains(KeyModifiers::CONTROL),
        alt: event.modifiers.contains(KeyModifiers::ALT),
        shift: event.modifiers.contains(KeyModifiers::SHIFT),
    };
    if event.code == KeyCode::BackTab {
        mods.shift = true;
    }
    // An uppercase character carries its own shift on every terminal, and
    // only some of them also set the modifier bit. `Chord::new` folds the
    // case; the flag has to be set here or the same keystroke would be two
    // different chords depending on the host.
    if let KeyCode::Char(character) = event.code
        && character.is_uppercase()
    {
        mods.shift = true;
    }
    Some(Chord::new(mods, key))
}

/// The text a keystroke types, for a surface that is taking text (see
/// `uze_keys::Scope::consumes_text`). `None` when the keystroke is not a
/// character at all.
pub(crate) fn text_of(event: KeyEvent) -> Option<char> {
    match event.code {
        KeyCode::Char(character)
            if !event.modifiers.contains(KeyModifiers::CONTROL)
                && !event.modifiers.contains(KeyModifiers::ALT) =>
        {
            Some(character)
        }
        _ => None,
    }
}

/// A press, as this build reads one. Test helper and the shape every
/// dispatcher assumes: `KeyEventKind::Press`, no enhancement state.
#[cfg(test)]
pub(crate) fn press(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
    KeyEvent::new_with_kind_and_state(
        code,
        modifiers,
        KeyEventKind::Press,
        crossterm::event::KeyEventState::NONE,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(text: &str) -> Chord {
        Chord::parse(text).expect(text)
    }

    #[test]
    fn a_keystroke_becomes_the_chord_someone_would_write() {
        assert_eq!(
            chord_of(press(KeyCode::Char('o'), KeyModifiers::CONTROL)),
            Some(chord("ctrl+o"))
        );
        assert_eq!(
            chord_of(press(KeyCode::F(1), KeyModifiers::NONE)),
            Some(chord("f1"))
        );
        assert_eq!(
            chord_of(press(KeyCode::BackTab, KeyModifiers::SHIFT)),
            Some(chord("shift+tab"))
        );
        assert_eq!(
            chord_of(press(KeyCode::PageUp, KeyModifiers::CONTROL)),
            Some(chord("ctrl+pageup"))
        );
    }

    #[test]
    fn a_shifted_character_is_one_chord_however_the_host_reports_it() {
        // Some terminals set the modifier bit for a capital and some only
        // send the capital. Both are `alt+shift+i`, or the same physical
        // gesture would resolve differently per emulator.
        let with_flag = press(KeyCode::Char('I'), KeyModifiers::ALT | KeyModifiers::SHIFT);
        let without_flag = press(KeyCode::Char('I'), KeyModifiers::ALT);
        assert_eq!(chord_of(with_flag), Some(chord("alt+shift+i")));
        assert_eq!(chord_of(without_flag), Some(chord("alt+shift+i")));
    }

    #[test]
    fn only_presses_are_keystrokes() {
        // The enhancement protocol reports releases and repeats too, and a
        // held key that closed a tab once must not close nine more.
        for kind in [KeyEventKind::Release, KeyEventKind::Repeat] {
            let event = KeyEvent::new_with_kind_and_state(
                KeyCode::Char('w'),
                KeyModifiers::CONTROL,
                kind,
                crossterm::event::KeyEventState::NONE,
            );
            assert_eq!(chord_of(event), None, "{kind:?}");
        }
    }

    #[test]
    fn a_key_outside_the_vocabulary_is_not_a_chord() {
        assert_eq!(chord_of(press(KeyCode::CapsLock, KeyModifiers::NONE)), None);
    }

    #[test]
    fn text_is_what_a_surface_taking_text_receives() {
        assert_eq!(
            text_of(press(KeyCode::Char('a'), KeyModifiers::NONE)),
            Some('a')
        );
        assert_eq!(
            text_of(press(KeyCode::Char('A'), KeyModifiers::SHIFT)),
            Some('A')
        );
        assert_eq!(
            text_of(press(KeyCode::Char('a'), KeyModifiers::CONTROL)),
            None
        );
        assert_eq!(text_of(press(KeyCode::Enter, KeyModifiers::NONE)), None);
    }

    #[test]
    fn what_the_terminal_cannot_send_is_reported_as_such() {
        let plain = KeyboardSupport { enhanced: false };
        assert!(plain.can_deliver(Tier::Universal));
        assert!(plain.can_deliver(Tier::HostConditional));
        assert!(
            !plain.can_deliver(Tier::EnhancementOnly),
            "a chord with no encoding must not look bindable"
        );
        assert!(KeyboardSupport { enhanced: true }.can_deliver(Tier::EnhancementOnly));
    }
}
