//! Which finished agent turns ring the terminal's bell, as it stands in
//! this process.
//!
//! Held process-wide, the way the active theme is: the workspace rings and
//! the management modal over it chooses, and neither owns the other's
//! model. Read at the moment of ringing, so a choice made in the modal is
//! in force for the very next finished turn.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use uze_application::{Chime, UzeHome};

use super::tui_application;

static CHOSEN: AtomicU8 = AtomicU8::new(0);
static PREVIEW: AtomicBool = AtomicBool::new(false);

pub(crate) fn current() -> Chime {
    Chime::ALL
        .get(usize::from(CHOSEN.load(Ordering::Relaxed)))
        .copied()
        .unwrap_or_default()
}

/// Asks for one ring now, so a choice is heard as it is made. Only asked
/// for: the bell goes out with the frames, from the loop that owns the
/// terminal, never from wherever the choice happened to be handled.
pub(crate) fn preview() {
    PREVIEW.store(true, Ordering::Relaxed);
}

pub(crate) fn take_preview() -> bool {
    PREVIEW.swap(false, Ordering::Relaxed)
}

pub(crate) fn set(chime: Chime) {
    let index = Chime::ALL
        .iter()
        .position(|candidate| *candidate == chime)
        .unwrap_or(0);
    CHOSEN.store(index as u8, Ordering::Relaxed);
}

/// What the operator chose, or silence when it cannot be read: a sound is
/// never worth refusing to start over.
pub(crate) fn load(home: &UzeHome) {
    set(tui_application(home.clone())
        .and_then(|app| app.notifications().agent_finished())
        .unwrap_or_default());
}

/// What a choice is called where it is made.
pub(crate) fn label(chime: Chime) -> &'static str {
    match chime {
        Chime::Silent => "silent",
        Chime::OutOfSight => "out of sight",
        Chime::Always => "always",
    }
}
