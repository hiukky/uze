//! The `[notifications]` section: when the operator wants to be told that
//! an agent finished its turn.
//!
//! Only the choice lives here. How the telling is done — the terminal's own
//! bell, rung by the client that draws the workspace — is presentation, and
//! the domain has no terminal to ring.
//!
//! Machine-scoped, like the theme: whether a machine makes a sound is the
//! operator's business, not the project's.

use crate::{Result, config, home::UzeHome};

/// Which finished turns ring the bell.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Chime {
    /// Never. The default, because a sound nobody asked for is the one
    /// kind of notification that cannot be ignored.
    #[default]
    Silent,
    /// Only a turn that ended in a tab the operator was not looking at —
    /// the one they would otherwise find out about late.
    OutOfSight,
    /// Every finished turn, the one on screen included, for an operator
    /// who looks away from the terminal rather than away from the tab.
    Always,
}

const SECTION: &str = "notifications";
const AGENT_FINISHED: &str = "agent_finished";

impl Chime {
    pub const ALL: [Chime; 3] = [Chime::Silent, Chime::OutOfSight, Chime::Always];

    /// How the choice is spelled in `config.toml`.
    pub fn id(self) -> &'static str {
        match self {
            Chime::Silent => "silent",
            Chime::OutOfSight => "out_of_sight",
            Chime::Always => "always",
        }
    }

    fn from_id(id: &str) -> Option<Chime> {
        Chime::ALL.into_iter().find(|chime| chime.id() == id)
    }
}

/// The choice in force. A value this build does not know reads as
/// [`Chime::Silent`]: an unrecognised word is not a reason to make noise.
pub fn agent_finished(home: &UzeHome) -> Result<Chime> {
    Ok(config::get(home, SECTION, AGENT_FINISHED)?
        .as_deref()
        .and_then(Chime::from_id)
        .unwrap_or_default())
}

pub fn set_agent_finished(home: &UzeHome, chime: Chime) -> Result<()> {
    config::set(home, SECTION, AGENT_FINISHED, chime.id())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home(label: &str) -> UzeHome {
        UzeHome::at(uze_testkit::temp::scratch(label))
    }

    #[test]
    fn a_machine_that_never_chose_stays_silent() {
        let home = home("chime-none");
        assert_eq!(agent_finished(&home).expect("readable"), Chime::Silent);
    }

    #[test]
    fn an_unknown_word_in_the_file_stays_silent() {
        let home = home("chime-unknown");
        config::set(&home, SECTION, AGENT_FINISHED, "loudly").expect("written");
        assert_eq!(agent_finished(&home).expect("readable"), Chime::Silent);
    }

    #[test]
    fn a_choice_survives_being_written_and_read_back() {
        let home = home("chime-selection");
        for chime in Chime::ALL {
            set_agent_finished(&home, chime).expect("written");
            assert_eq!(agent_finished(&home).expect("readable"), chime);
        }
    }
}
