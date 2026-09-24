//! When this machine rings for a finished agent. Only the choice: ringing
//! is the client's, which owns the terminal.

use uze_core::{Result, notifications, notifications::Chime};

use super::services::Notifications;

impl Notifications<'_> {
    /// Which finished turns ring; [`Chime::Silent`] until the operator
    /// chooses otherwise.
    #[tracing::instrument(name = "notifications.agent_finished", skip_all, err)]
    pub fn agent_finished(&self) -> Result<Chime> {
        notifications::agent_finished(&self.0.home)
    }

    #[tracing::instrument(name = "notifications.set_agent_finished", skip_all, fields(chime = ?chime), err)]
    pub fn set_agent_finished(&self, chime: Chime) -> Result<()> {
        notifications::set_agent_finished(&self.0.home, chime)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UzeApplication;
    use std::time::Instant;

    /// `config notification` is one `config.toml` key read or written — a
    /// choice a person makes standing at a prompt, not a report they wait
    /// on. Held to the budget so it can never grow into a startup-cost
    /// read of the machine.
    #[test]
    fn notification_choice_meets_the_budget() {
        let home = uze_core::UzeHome::at(uze_testkit::temp::scratch("notification-budget"));
        let app = UzeApplication::new(home, Vec::new());
        let started = Instant::now();
        app.notifications()
            .set_agent_finished(Chime::OutOfSight)
            .unwrap();
        let chosen = app.notifications().agent_finished().unwrap();
        let elapsed = started.elapsed();
        assert_eq!(chosen, Chime::OutOfSight);
        assert!(
            elapsed < std::time::Duration::from_millis(50),
            "the notification choice took {elapsed:?}"
        );
    }
}
