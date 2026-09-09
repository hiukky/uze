//! The chord grammar: what an operator may write, and what a terminal can
//! actually deliver.
//!
//! This module names no terminal library. A [`Chord`] is what someone wrote
//! in a keymap file or read off the Keys screen; turning a real key event
//! into one is the consuming surface's job, exactly as turning a
//! [`crate::Action`] into a gesture is.
//!
//! Two rules live here rather than with the resolver, because both are
//! properties of the *key*, not of what it was bound to:
//!
//! - Some chords are not chords. A terminal transmits `Ctrl+I` as Tab and
//!   `Ctrl+M` as Enter — there is no encoding that tells them apart, so
//!   binding one is how an operator locks themselves out of their own UI.
//!   [`Chord::parse`] refuses them, naming the key they actually are.
//! - The rest differ in what it takes to deliver them ([`Tier`]). A chord
//!   nobody's terminal can send is worse than an unbound one: it looks
//!   bound and does nothing.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A key with no modifiers applied — the physical key, not what it types.
///
/// Deliberately smaller than any terminal library's key enum: this is the
/// set uze binds against, and an event carrying anything else simply
/// resolves to no action.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum Key {
    /// A printable character, always stored lowercase — `Shift` is a
    /// modifier, never a different key, so `alt+shift+i` and `alt+I` are
    /// one chord written two ways.
    Char(char),
    /// A function key, `F1`–`F12`.
    F(u8),
    Enter,
    Esc,
    Tab,
    Space,
    Backspace,
    Delete,
    Insert,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
}

impl Key {
    /// Navigation and editing keys: the register a terminal encodes as an
    /// escape sequence with a modifier parameter, rather than as a
    /// character.
    fn is_navigation(self) -> bool {
        matches!(
            self,
            Key::Up
                | Key::Down
                | Key::Left
                | Key::Right
                | Key::Home
                | Key::End
                | Key::PageUp
                | Key::PageDown
                | Key::Insert
                | Key::Delete
        )
    }

    fn name(self) -> String {
        match self {
            Key::Char(' ') => "space".to_owned(),
            Key::Char(character) => character.to_string(),
            Key::F(number) => format!("f{number}"),
            Key::Enter => "enter".to_owned(),
            Key::Esc => "esc".to_owned(),
            Key::Tab => "tab".to_owned(),
            Key::Space => "space".to_owned(),
            Key::Backspace => "backspace".to_owned(),
            Key::Delete => "delete".to_owned(),
            Key::Insert => "insert".to_owned(),
            Key::Up => "up".to_owned(),
            Key::Down => "down".to_owned(),
            Key::Left => "left".to_owned(),
            Key::Right => "right".to_owned(),
            Key::Home => "home".to_owned(),
            Key::End => "end".to_owned(),
            Key::PageUp => "pageup".to_owned(),
            Key::PageDown => "pagedown".to_owned(),
        }
    }

    fn parse(token: &str) -> Option<Key> {
        let key = match token {
            "enter" | "return" | "cr" => Key::Enter,
            "esc" | "escape" => Key::Esc,
            "tab" => Key::Tab,
            "space" => Key::Space,
            "backspace" | "bs" => Key::Backspace,
            "delete" | "del" => Key::Delete,
            "insert" | "ins" => Key::Insert,
            "up" => Key::Up,
            "down" => Key::Down,
            "left" => Key::Left,
            "right" => Key::Right,
            "home" => Key::Home,
            "end" => Key::End,
            "pageup" | "pgup" => Key::PageUp,
            "pagedown" | "pgdn" => Key::PageDown,
            _ => {
                if let Some(number) = token.strip_prefix('f')
                    && let Ok(number) = number.parse::<u8>()
                    && (1..=12).contains(&number)
                {
                    return Some(Key::F(number));
                }
                let mut characters = token.chars();
                let character = characters.next()?;
                if characters.next().is_some() {
                    return None;
                }
                Key::Char(character)
            }
        };
        Some(key)
    }
}

/// The modifiers a chord carries. Three bools rather than a bitset: the
/// set is closed, and every place that reads one reads it by name.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Mods {
    pub const NONE: Mods = Mods {
        ctrl: false,
        alt: false,
        shift: false,
    };
    pub const CTRL: Mods = Mods {
        ctrl: true,
        alt: false,
        shift: false,
    };
    pub const ALT: Mods = Mods {
        ctrl: false,
        alt: true,
        shift: false,
    };
    pub const SHIFT: Mods = Mods {
        ctrl: false,
        alt: false,
        shift: true,
    };
    pub const ALT_SHIFT: Mods = Mods {
        ctrl: false,
        alt: true,
        shift: true,
    };

    fn is_empty(self) -> bool {
        self == Mods::NONE
    }
}

/// One key plus its modifiers: everything uze binds an action to.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct Chord {
    pub mods: Mods,
    pub key: Key,
}

/// What it takes for a chord to reach uze at all.
///
/// The product commits to classifying what it binds, because a binding a
/// terminal cannot deliver is indistinguishable from a broken feature. See
/// [`Chord::tier`] for what falls where and why.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tier {
    /// Reaches any terminal, over ssh, inside a multiplexer.
    Universal,
    /// Reaches most, but the host decides: `Alt` must be sent as Meta
    /// rather than composing a character, and a modified navigation key
    /// may be claimed by the emulator first.
    HostConditional,
    /// Needs the keyboard enhancement protocol. Without it the terminal
    /// has no encoding for this chord at all.
    EnhancementOnly,
}

impl Tier {
    pub fn label(self) -> &'static str {
        match self {
            Tier::Universal => "universal",
            Tier::HostConditional => "depends on your terminal",
            Tier::EnhancementOnly => "needs an enhanced keyboard protocol",
        }
    }
}

/// Something worth saying about a chord before someone binds it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Caveat {
    pub kind: CaveatKind,
    pub note: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CaveatKind {
    /// A terminal emulator or multiplexer commonly takes this before an
    /// application sees it.
    HostClaims,
    /// uze can take it, but the program running in a pane then never
    /// receives it — the workspace's chord budget is spent out of the
    /// agent's pocket.
    PaneLoses,
}

/// Why a chord cannot be used.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ChordProblem {
    /// The chord is a different key on a terminal. Carries the key it
    /// actually is, so the refusal can say so.
    Alias { wrote: String, is: &'static str },
    /// Nothing in the grammar matches what was written.
    Unreadable(String),
    /// A modifier name that is not a modifier.
    UnknownModifier { wrote: String, modifier: String },
}

impl fmt::Display for ChordProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChordProblem::Alias { wrote, is } => write!(
                formatter,
                "`{wrote}` is how a terminal transmits {is}; binding it would take {is} away"
            ),
            ChordProblem::Unreadable(wrote) => {
                write!(formatter, "`{wrote}` is not a key this build understands")
            }
            ChordProblem::UnknownModifier { wrote, modifier } => write!(
                formatter,
                "`{wrote}` names `{modifier}`, which is not ctrl, alt or shift"
            ),
        }
    }
}

/// The control chords a terminal encodes as some other key. Each entry is
/// the character after `ctrl+`, and what it actually arrives as.
///
/// `Ctrl` plus a letter is transmitted as a single control byte, so five
/// letters and one punctuation mark collide with keys that already have
/// their own meaning. This is not a policy uze chose and could relax: on a
/// terminal there is no bit that distinguishes them.
const CONTROL_ALIASES: &[(char, &str)] = &[
    ('i', "Tab"),
    ('m', "Enter"),
    ('j', "line feed"),
    ('h', "Backspace"),
    ('[', "Esc"),
    (' ', "NUL"),
    ('@', "NUL"),
];

impl Chord {
    pub fn new(mods: Mods, key: Key) -> Chord {
        let key = match key {
            Key::Char(character) if character.is_ascii_uppercase() => {
                Key::Char(character.to_ascii_lowercase())
            }
            // The space bar is one key however a terminal reports it — as
            // its own key or as the character it types. Folding here is
            // what keeps `space` in a keymap file matching the keystroke.
            Key::Char(' ') => Key::Space,
            other => other,
        };
        Chord { mods, key }
    }

    /// Reads a chord as an operator writes one: modifiers in any order and
    /// any case, joined by `+`, key last. A bare uppercase letter is the
    /// shifted form of its own key, so `alt+I` and `alt+shift+i` are the
    /// same chord.
    pub fn parse(text: &str) -> Result<Chord, ChordProblem> {
        let wrote = text.trim();
        if wrote.is_empty() {
            return Err(ChordProblem::Unreadable(wrote.to_owned()));
        }
        // Split on `+` except when `+` is the key itself (`ctrl++`), which
        // a naive split would read as an empty final token.
        let mut parts: Vec<String> = Vec::new();
        let mut current = String::new();
        for character in wrote.chars() {
            if character == '+' && !current.is_empty() {
                parts.push(std::mem::take(&mut current));
            } else {
                current.push(character);
            }
        }
        parts.push(current);

        let mut mods = Mods::NONE;
        let (last, modifiers) = parts.split_last().expect("at least one part");
        for modifier in modifiers {
            match modifier.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "c" => mods.ctrl = true,
                "alt" | "meta" | "opt" | "option" | "m" => mods.alt = true,
                "shift" | "s" => mods.shift = true,
                other => {
                    return Err(ChordProblem::UnknownModifier {
                        wrote: wrote.to_owned(),
                        modifier: other.to_owned(),
                    });
                }
            }
        }

        // An uppercase single character carries its own shift; every other
        // token is matched lowercase.
        let key = if last.chars().count() == 1 {
            let character = last.chars().next().expect("one character");
            if character.is_ascii_uppercase() {
                mods.shift = true;
            }
            Key::Char(character.to_ascii_lowercase())
        } else {
            Key::parse(&last.to_ascii_lowercase())
                .ok_or_else(|| ChordProblem::Unreadable(wrote.to_owned()))?
        };

        let chord = Chord { mods, key };
        chord.refusal(wrote)?;
        Ok(chord)
    }

    /// The refusal this chord earns, if any — the terminal-alias rule.
    fn refusal(self, wrote: &str) -> Result<(), ChordProblem> {
        if !self.mods.ctrl || self.mods.alt {
            return Ok(());
        }
        let character = match self.key {
            Key::Char(character) => character,
            Key::Space => ' ',
            _ => return Ok(()),
        };
        match CONTROL_ALIASES
            .iter()
            .find(|(aliased, _)| *aliased == character)
        {
            Some((_, is)) => Err(ChordProblem::Alias {
                wrote: wrote.to_owned(),
                is,
            }),
            None => Ok(()),
        }
    }

    /// What it takes to deliver this chord.
    ///
    /// The reasoning, once, since every arm below is an instance of it: a
    /// terminal without the enhancement protocol has exactly three ways to
    /// encode a keystroke — a character, a control byte (`Ctrl` plus a
    /// letter), and an escape sequence (function and navigation keys, plus
    /// `Alt` as an ESC prefix). Anything outside those three has no
    /// encoding, which is what [`Tier::EnhancementOnly`] means.
    pub fn tier(self) -> Tier {
        let Mods { ctrl, alt, shift } = self.mods;
        match self.key {
            // Function keys carry their own modifier parameter, and no
            // emulator composes them into characters the way it may with
            // Alt — so even the modified forms are universal.
            Key::F(_) => Tier::Universal,
            // ESC-prefixed: the host has to send Meta rather than compose.
            _ if alt => Tier::HostConditional,
            Key::Char(character) if ctrl => {
                if character.is_ascii_alphabetic() && !shift {
                    // The one register every terminal has had since VT100.
                    Tier::Universal
                } else {
                    // Ctrl+digit, Ctrl+punctuation and Ctrl+Shift+letter
                    // all fold into the same control byte or none at all.
                    Tier::EnhancementOnly
                }
            }
            Key::Char(_) | Key::Space => {
                if ctrl {
                    Tier::EnhancementOnly
                } else {
                    Tier::Universal
                }
            }
            Key::Tab | Key::Enter | Key::Esc | Key::Backspace => {
                if ctrl {
                    // Ctrl+Enter and friends are the enhancement
                    // protocol's whole reason for existing.
                    Tier::EnhancementOnly
                } else {
                    // Shift+Tab is the one shifted form with an ancient
                    // encoding of its own (CSI Z).
                    Tier::Universal
                }
            }
            key if key.is_navigation() => {
                if self.mods.is_empty() {
                    Tier::Universal
                } else {
                    // Modified navigation keys are widely encoded, and
                    // widely intercepted by the emulator first.
                    Tier::HostConditional
                }
            }
            _ => Tier::Universal,
        }
    }

    /// Whether this chord is one someone memorizes as standing for a
    /// meaning — a letter, a digit, a punctuation mark, a function key —
    /// as opposed to a structural key whose meaning is its position.
    ///
    /// The distinction decides where a chord may mean two things. `Enter`
    /// opens, confirms and submits depending on what is in front of you,
    /// and nobody is confused by that: its meaning is "go on", read from
    /// the surface. `r` standing for remove on one screen and refresh on
    /// the next is the alphabet-memorizing this design exists to end. So a
    /// mnemonic names one action per keyboard, and a structural key is
    /// contextual by nature.
    pub fn is_mnemonic(self) -> bool {
        matches!(self.key, Key::Char(_) | Key::Space | Key::F(_))
    }

    /// What is worth saying about this chord before it is bound.
    pub fn caveats(self) -> Vec<Caveat> {
        let mut caveats = Vec::new();
        let Mods { ctrl, alt, shift } = self.mods;
        if ctrl && shift {
            caveats.push(Caveat {
                kind: CaveatKind::HostClaims,
                note: "terminal emulators commonly reserve Ctrl+Shift for themselves",
            });
        }
        if matches!(self.key, Key::PageUp | Key::PageDown | Key::Tab) && (ctrl || alt) {
            caveats.push(Caveat {
                kind: CaveatKind::HostClaims,
                note: "many emulators use this to switch their own tabs",
            });
        }
        if matches!(self.key, Key::Enter) && alt {
            caveats.push(Caveat {
                kind: CaveatKind::HostClaims,
                note: "commonly the emulator's own fullscreen toggle",
            });
        }
        if ctrl
            && !alt
            && !shift
            && let Key::Char(character) = self.key
            && let Some((_, loses)) = PANE_COSTS.iter().find(|(aliased, _)| *aliased == character)
        {
            caveats.push(Caveat {
                kind: CaveatKind::PaneLoses,
                note: loses,
            });
        }
        caveats
    }
}

/// What a program running in a pane loses when uze claims the chord. Only
/// the ones an agent's input box actually uses — the point is to tell the
/// truth about a cost, not to list every readline binding in existence.
const PANE_COSTS: &[(char, &str)] = &[
    ('a', "a pane's program loses go-to-line-start"),
    ('b', "a pane's program loses back-one-character"),
    (
        'd',
        "a pane's program loses delete-character and end-of-input",
    ),
    ('e', "a pane's program loses go-to-line-end"),
    ('f', "a pane's program loses forward-one-character"),
    ('k', "a pane's program loses kill-to-end-of-line"),
    ('l', "a pane's program loses clear-screen"),
    ('n', "a pane's program loses next-history"),
    ('p', "a pane's program loses previous-history"),
    ('r', "a pane's program loses reverse-history-search"),
    ('u', "a pane's program loses kill-to-line-start"),
    ('w', "a pane's program loses delete-previous-word"),
    ('y', "a pane's program loses paste-from-kill-ring"),
];

impl fmt::Display for Chord {
    /// The canonical spelling: modifiers in a fixed order, key lowercase.
    /// Whatever this prints, [`Chord::parse`] reads back as the same chord.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.mods.ctrl {
            write!(formatter, "ctrl+")?;
        }
        if self.mods.alt {
            write!(formatter, "alt+")?;
        }
        if self.mods.shift {
            write!(formatter, "shift+")?;
        }
        write!(formatter, "{}", self.key.name())
    }
}

impl Serialize for Chord {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Chord {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Chord, D::Error> {
        let text = String::deserialize(deserializer)?;
        Chord::parse(&text).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_a_chord_prints_parses_back_to_itself() {
        let keys = [
            Key::Char('a'),
            Key::Char('9'),
            Key::Char('/'),
            Key::Char('?'),
            Key::Char('+'),
            Key::F(1),
            Key::F(12),
            Key::Enter,
            Key::Esc,
            Key::Tab,
            Key::Space,
            Key::Backspace,
            Key::Delete,
            Key::Insert,
            Key::Up,
            Key::Down,
            Key::Left,
            Key::Right,
            Key::Home,
            Key::End,
            Key::PageUp,
            Key::PageDown,
        ];
        let modifiers = [
            Mods::NONE,
            Mods::CTRL,
            Mods::ALT,
            Mods::SHIFT,
            Mods::ALT_SHIFT,
            Mods {
                ctrl: true,
                alt: true,
                shift: true,
            },
        ];
        for key in keys {
            for mods in modifiers {
                let chord = Chord { mods, key };
                let printed = chord.to_string();
                match Chord::parse(&printed) {
                    Ok(parsed) => assert_eq!(
                        parsed, chord,
                        "`{printed}` did not read back as what printed it"
                    ),
                    // A refused chord is allowed to fail to parse: that is
                    // the refusal doing its job, not a round-trip defect.
                    Err(ChordProblem::Alias { .. }) => {}
                    Err(problem) => panic!("`{printed}` printed but does not parse: {problem}"),
                }
            }
        }
    }

    #[test]
    fn every_control_alias_is_refused_by_the_key_it_actually_is() {
        for (character, is) in CONTROL_ALIASES {
            // The space is spelled `ctrl+space`, asserted on its own below;
            // `ctrl+ ` is trimmed away before the grammar ever sees it.
            if *character == ' ' {
                continue;
            }
            let wrote = format!("ctrl+{character}");
            let problem = Chord::parse(&wrote).expect_err("an alias is refused");
            assert!(
                matches!(&problem, ChordProblem::Alias { is: named, .. } if named == is),
                "ctrl+{character} should be refused as {is}, got {problem}"
            );
            assert!(problem.to_string().contains(is));
        }
        // `ctrl+space` is written two ways and refused both.
        assert!(matches!(
            Chord::parse("ctrl+space"),
            Err(ChordProblem::Alias { .. })
        ));
    }

    #[test]
    fn an_alias_is_only_an_alias_without_other_modifiers() {
        // Alt+Ctrl+I has an escape prefix, so it is distinguishable again.
        assert!(Chord::parse("ctrl+alt+i").is_ok());
        // And the bare key is untouched.
        assert!(Chord::parse("i").is_ok());
    }

    #[test]
    fn shift_is_a_modifier_never_a_different_key() {
        assert_eq!(Chord::parse("alt+I"), Chord::parse("alt+shift+i"));
        assert_eq!(Chord::parse("A"), Chord::parse("shift+a"));
        assert_eq!(Chord::parse("alt+I").unwrap().to_string(), "alt+shift+i");
    }

    #[test]
    fn modifiers_are_read_in_any_order_and_any_case() {
        let canonical = Chord::parse("ctrl+alt+shift+x").unwrap();
        for spelling in ["Shift+ALT+Control+X", "alt+ctrl+shift+x", "C+M+S+x"] {
            assert_eq!(Chord::parse(spelling).unwrap(), canonical, "{spelling}");
        }
    }

    #[test]
    fn the_key_may_be_the_separator_itself() {
        let chord = Chord::parse("ctrl++").unwrap();
        assert_eq!(chord.key, Key::Char('+'));
        assert!(chord.mods.ctrl);
    }

    #[test]
    fn an_unknown_modifier_says_which_one() {
        let problem = Chord::parse("hyper+x").expect_err("not a modifier");
        assert!(problem.to_string().contains("hyper"), "{problem}");
    }

    #[test]
    fn tiers_follow_what_a_terminal_can_encode() {
        let tier = |text: &str| Chord::parse(text).expect(text).tier();
        assert_eq!(tier("ctrl+o"), Tier::Universal);
        assert_eq!(tier("f1"), Tier::Universal);
        assert_eq!(tier("shift+f5"), Tier::Universal);
        assert_eq!(tier("shift+tab"), Tier::Universal);
        assert_eq!(tier("esc"), Tier::Universal);
        assert_eq!(tier("/"), Tier::Universal);
        assert_eq!(tier("alt+i"), Tier::HostConditional);
        assert_eq!(tier("alt+shift+i"), Tier::HostConditional);
        assert_eq!(tier("alt+1"), Tier::HostConditional);
        assert_eq!(tier("ctrl+up"), Tier::HostConditional);
        assert_eq!(tier("ctrl+pageup"), Tier::HostConditional);
        // The chord this design retired: unbindable on a plain terminal.
        assert_eq!(tier("ctrl+1"), Tier::EnhancementOnly);
        assert_eq!(tier("ctrl+shift+w"), Tier::EnhancementOnly);
        assert_eq!(tier("ctrl+enter"), Tier::EnhancementOnly);
    }

    #[test]
    fn a_chord_states_what_it_costs_a_pane() {
        let ctrl_w = Chord::parse("ctrl+w").unwrap();
        let caveats = ctrl_w.caveats();
        assert!(
            caveats
                .iter()
                .any(|caveat| caveat.kind == CaveatKind::PaneLoses
                    && caveat.note.contains("delete-previous-word")),
            "ctrl+w takes delete-word from every agent input: {caveats:?}"
        );
        // A function key costs a pane nothing, which is why the workspace
        // spends them first.
        assert!(Chord::parse("f2").unwrap().caveats().is_empty());
    }

    #[test]
    fn a_chord_states_what_its_host_may_take_first() {
        assert!(
            Chord::parse("ctrl+pageup")
                .unwrap()
                .caveats()
                .iter()
                .any(|caveat| caveat.kind == CaveatKind::HostClaims)
        );
    }
}
