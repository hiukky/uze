//! UZE's input vocabulary.
//!
//! This crate is to the keyboard what `uze-theme` is to the palette: the
//! product names what things *mean* ([`Action`]), says where each meaning
//! is live ([`Scope`]), and keeps which key reaches it as a separate,
//! changeable question ([`Chord`]). A [`Keymap`] answers both directions —
//! what a keystroke means, and what key reaches a meaning — and it is the
//! only thing allowed to answer either.
//!
//! # Why the crate names no terminal library
//!
//! A [`Chord`] is its own type rather than a re-export of some event
//! struct, exactly as `uze_theme::Rgb` is its own type rather than a
//! colour from a drawing library. One adapter in the consuming crate turns
//! real key events into chords, and everything here — parsing, refusing,
//! resolving, the reverse lookup, the portability rules — is testable with
//! no terminal in sight.
//!
//! # The two directions
//!
//! ```text
//!   a keystroke  ──▶ Chord ──▶ Keymap::resolve(chord, scopes) ──▶ Action
//!   a surface    ──▶ Action ─▶ Keymap::chord_for(action, scopes) ─▶ Chord
//! ```
//!
//! The second is not a convenience. It is what makes an advertised key true
//! by construction: a help line, a footer hint or a menu entry asks what
//! chord reaches an action rather than saying what it believes, so a
//! rebound key is right everywhere at once and an invented one cannot be
//! printed at all.
//!
//! # What a terminal can deliver
//!
//! Some chords are not chords ([`ChordProblem::Alias`]) and the rest differ
//! in what it takes to send them ([`Tier`]). Both live with the grammar,
//! because both are facts about the key rather than about what it was bound
//! to — see [`chord`] for the reasoning and [`load`] for what the built-in
//! keymap does with it.

pub mod action;
pub mod active;
pub mod chord;
pub mod file;
pub mod keymap;
pub mod load;
pub mod scope;

pub use action::{ALL_ACTIONS, Action, Mode};
pub use active::{active, set_active};
pub use chord::{Caveat, CaveatKind, Chord, ChordProblem, Key, Mods, Tier};
pub use file::{CURRENT_VERSION, KeymapFile};
pub use keymap::{Binding, Conflict, Keymap, Resolution};
pub use load::{Loaded, Problem, Severity, default_keymap, difference_from_default};
pub use scope::{ALL_SCOPES, Scope};
