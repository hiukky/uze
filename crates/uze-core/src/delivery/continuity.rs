//! Deciding, per launch, whether an agent resumes or starts.
//!
//! [`conversation`](crate::conversation) is the record; this is what is done
//! with it. Both halves are vendor-neutral: everything specific to a harness
//! arrives through [`IntegrationPort`]'s continuity verbs, so this file
//! names no harness and a fifth one changes nothing here.
//!
//! # Where this runs
//!
//! [`plan`] runs inside the launched process, on the shim — the only place
//! that is reached by *every* relaunch, including the one the terminal
//! runtime performs when it restores a workspace with no client in the room.
//! It therefore does no more than a lexical path match, one small read and,
//! on a first launch, one small write.
//!
//! [`refresh`] is the other half, and runs where a background answer already
//! runs: it asks the harness which conversation the agent is actually in and
//! writes that back, so a conversation cleared, forked or switched inside
//! the process is the one that resumes rather than the one it began with.

use std::{ffi::OsString, path::Path};

use crate::{
    conversation::{self, ConversationOrigin, SessionId},
    home::UzeHome,
    integration::{IntegrationPort, ObservationContext, SessionContinuity},
};

/// What a launch should carry, and what could not be done.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LaunchPlan {
    /// Prepended before the caller's own argument list.
    pub args: Vec<OsString>,
    /// Said once, on the shim's existing note channel. Present only when
    /// something was expected to be carried over and could not be.
    pub note: Option<String>,
}

impl LaunchPlan {
    fn nothing() -> Self {
        Self::default()
    }

    fn args(args: Vec<OsString>) -> Self {
        Self { args, note: None }
    }
}

/// Resume this task's conversation, or start one and record it.
///
/// `None` of the interesting cases is an error: a directory belonging to no
/// managed task, a harness that declares no continuity, a record that no
/// longer resolves — each answers with the launch that would have happened
/// anyway. Continuity is never a precondition for an agent starting.
pub fn plan(home: &UzeHome, cwd: &Path, integration: &dyn IntegrationPort) -> LaunchPlan {
    let continuity = integration.session_continuity();
    if continuity == SessionContinuity::Unsupported {
        return LaunchPlan::nothing();
    }
    // Not a managed task's checkout: an ordinary invocation stays ordinary.
    let Some(owner) = conversation::owner_of(home, cwd) else {
        return LaunchPlan::nothing();
    };
    let mut record = conversation::load(home, &owner.primary, &owner.task);
    let id = integration.id();

    // A launch whose read-back never happened — the client was not running,
    // or the agent was relaunched before the refresh came round — would
    // otherwise start a second conversation beside a perfectly good first
    // one. This is the one place the launch path pays for an observation,
    // and it pays for it only here: the entry is pending, so the
    // alternative is losing the conversation, and a harness that answers
    // this question expensively is answering it once per relaunch rather
    // than on a clock.
    if let Some(entry) = record.get(id)
        && entry.conversation.is_none()
        && let Some(observed) = integration.observe_session(&ObservationContext {
            cwd,
            since_unix: entry.launched_at_unix,
            preceded_by: entry.preceded_by.as_ref(),
        })
    {
        let launched_at = entry.launched_at_unix;
        if record.observed(id, launched_at, observed) {
            let _ = conversation::save(home, &owner.primary, &record);
        }
    }

    if let Some(recorded) = record.get(id).and_then(|entry| entry.conversation.clone()) {
        if integration.session_exists(&recorded, cwd) {
            return LaunchPlan::args(integration.resume_session_args(&recorded));
        }
        record.forget_harness(id);
        let mut plan = start(home, cwd, integration, &mut record, &owner);
        plan.note = Some(format!(
            "the recorded conversation ({recorded}) is no longer there; starting a new one"
        ));
        return plan;
    }

    start(home, cwd, integration, &mut record, &owner)
}

fn start(
    home: &UzeHome,
    cwd: &Path,
    integration: &dyn IntegrationPort,
    record: &mut conversation::ConversationRecord,
    owner: &conversation::Owner,
) -> LaunchPlan {
    let id = integration.id();
    let plan = match integration.session_continuity() {
        // Named here, so nothing has to be read back and a crash between
        // this line and the harness's first write costs nothing.
        SessionContinuity::Assigned => {
            let session = SessionId::generate();
            record.launched(
                id,
                ConversationOrigin::Assigned,
                Some(session.clone()),
                None,
            );
            LaunchPlan::args(integration.start_session_args(&session))
        }
        // The harness names its own; all this launch can do is say when it
        // started and what was already there, so the read-back can tell one
        // from the other.
        SessionContinuity::Observed => {
            record.launched(
                id,
                ConversationOrigin::Observed,
                None,
                integration.session_recorded_for(cwd),
            );
            LaunchPlan::nothing()
        }
        SessionContinuity::Unsupported => return LaunchPlan::nothing(),
    };
    // A record that cannot be written is a conversation that will not be
    // carried over next time — never a launch that does not happen.
    let _ = conversation::save(home, &owner.primary, record);
    plan
}

/// Writes back the conversation the agent is actually in.
///
/// Answers whether the record changed, so a caller can avoid a write it does
/// not need. Deliberately silent about everything else: a harness with
/// nothing new to report, a task that has moved on, an unreadable record —
/// all of them mean "nothing to do", and none of them is worth interrupting
/// anybody over.
pub fn refresh(home: &UzeHome, cwd: &Path, integration: &dyn IntegrationPort) -> bool {
    if integration.session_continuity() == SessionContinuity::Unsupported {
        return false;
    }
    let Some(owner) = conversation::owner_of(home, cwd) else {
        return false;
    };
    let mut record = conversation::load(home, &owner.primary, &owner.task);
    let id = integration.id();
    let Some(entry) = record.get(id) else {
        return false;
    };
    let launched_at = entry.launched_at_unix;
    let known = entry.conversation.clone();
    let observed = integration.observe_session(&ObservationContext {
        cwd,
        since_unix: launched_at,
        preceded_by: entry.preceded_by.as_ref(),
    });
    let Some(observed) = observed else {
        return false;
    };
    if known.as_ref() == Some(&observed) {
        return false;
    }
    if !record.observed(id, launched_at, observed) {
        return false;
    }
    conversation::save(home, &owner.primary, &record).is_ok()
}

/// Records a conversation an authoritative source named for `cwd` — a
/// harness stating its own identifier, which needs no observation and no
/// guessing at all.
///
/// Nothing calls this today: the one authoritative source UZE had was the
/// hook dispatch it ran itself, and hooks now run a generated wrapper with
/// no UZE on the path (ADR-040, amended). Kept because the channel is the
/// harness's to offer, not UZE's to invent — [`refresh`] above is the
/// observing route, and it is the one in use.
pub fn record_observed(
    home: &UzeHome,
    cwd: &Path,
    integration_id: &str,
    session: SessionId,
) -> bool {
    let Some(owner) = conversation::owner_of(home, cwd) else {
        return false;
    };
    let mut record = conversation::load(home, &owner.primary, &owner.task);
    let Some(entry) = record.get(integration_id) else {
        return false;
    };
    if entry.conversation.as_ref() == Some(&session) {
        return false;
    }
    let launched_at = entry.launched_at_unix;
    if !record.observed(integration_id, launched_at, session) {
        return false;
    }
    conversation::save(home, &owner.primary, &record).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        checkout::CheckoutId,
        exposure::ExposurePlan,
        integration::HarnessDetection,
        router::HarnessCapabilities,
        task::{self, Base, Task, TaskStore},
    };
    use std::{
        cell::RefCell,
        path::PathBuf,
        sync::atomic::{AtomicUsize, Ordering},
    };

    /// A harness whose every continuity answer the test decides.
    struct Harness {
        continuity: SessionContinuity,
        exists: bool,
        observed: RefCell<Option<SessionId>>,
        recorded_for: Option<SessionId>,
    }

    impl Harness {
        fn new(continuity: SessionContinuity) -> Self {
            Self {
                continuity,
                exists: true,
                observed: RefCell::new(None),
                recorded_for: None,
            }
        }
    }

    impl IntegrationPort for Harness {
        fn id(&self) -> &'static str {
            "harness"
        }
        fn capabilities(&self) -> HarnessCapabilities {
            HarnessCapabilities::default()
        }
        fn exposure_plan(&self, _resource: &crate::Resource) -> ExposurePlan {
            panic!("not used")
        }
        fn detect(&self) -> HarnessDetection {
            HarnessDetection::default()
        }
        fn session_continuity(&self) -> SessionContinuity {
            self.continuity
        }
        fn start_session_args(&self, session: &SessionId) -> Vec<OsString> {
            vec![OsString::from("--start"), OsString::from(session.as_str())]
        }
        fn resume_session_args(&self, session: &SessionId) -> Vec<OsString> {
            vec![OsString::from("--resume"), OsString::from(session.as_str())]
        }
        fn session_recorded_for(&self, _cwd: &Path) -> Option<SessionId> {
            self.recorded_for.clone()
        }
        fn observe_session(&self, _ctx: &ObservationContext) -> Option<SessionId> {
            self.observed.borrow().clone()
        }
        fn session_exists(&self, _session: &SessionId, _cwd: &Path) -> bool {
            self.exists
        }
    }

    /// A repository with one task in one slot, and the slot's path.
    fn managed(label: &str) -> (UzeHome, PathBuf, PathBuf) {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root = uze_testkit::temp::scratch(&format!("{label}-{unique}"));
        let home = UzeHome::at(root.join("uze"));
        let primary = root.join("repo");
        let slot = primary.join(".worktrees").join("slot-1");
        std::fs::create_dir_all(&slot).unwrap();

        let mut task = Task::new(None, Base::Ref("main".into()), String::new(), "main".into());
        task.checkout = Some(CheckoutId::adopted("slot-1"));
        let mut store = TaskStore::default();
        store.upsert(task);
        task::save(&home, &primary, &store).unwrap();
        (home, primary, slot)
    }

    fn recorded(home: &UzeHome, primary: &Path) -> Option<SessionId> {
        let store = task::load(home, primary).unwrap();
        let task = store.tasks.first()?;
        conversation::load(home, primary, &task.id)
            .get("harness")?
            .conversation
            .clone()
    }

    #[test]
    fn a_first_launch_of_an_assigning_harness_names_and_records_the_conversation() {
        let (home, primary, slot) = managed("continuity-assign");
        let plan = plan(&home, &slot, &Harness::new(SessionContinuity::Assigned));

        let session = recorded(&home, &primary).expect("the conversation was recorded");
        assert_eq!(
            plan.args,
            vec![OsString::from("--start"), OsString::from(session.as_str())]
        );
        assert_eq!(plan.note, None);
    }

    #[test]
    fn a_second_launch_resumes_what_the_first_recorded() {
        let (home, primary, slot) = managed("continuity-resume");
        let harness = Harness::new(SessionContinuity::Assigned);
        plan(&home, &slot, &harness);
        let session = recorded(&home, &primary).unwrap();

        let plan = plan(&home, &slot, &harness);
        assert_eq!(
            plan.args,
            vec![OsString::from("--resume"), OsString::from(session.as_str())]
        );
    }

    #[test]
    fn a_first_launch_of_an_observing_harness_carries_no_argument_but_is_recorded() {
        let (home, primary, slot) = managed("continuity-observe");
        let mut harness = Harness::new(SessionContinuity::Observed);
        harness.recorded_for = Some(SessionId::new("previous-tenant"));

        assert_eq!(plan(&home, &slot, &harness), LaunchPlan::nothing());

        let store = task::load(&home, &primary).unwrap();
        let entry = conversation::load(&home, &primary, &store.tasks[0].id)
            .get("harness")
            .cloned()
            .expect("the launch was recorded");
        assert_eq!(entry.conversation, None);
        assert_eq!(entry.origin, ConversationOrigin::Observed);
        assert_eq!(entry.preceded_by, Some(SessionId::new("previous-tenant")));
    }

    /// Without this, a harness that names its own conversation would start
    /// a second one every time it was relaunched with no client watching —
    /// which is exactly the relaunch that matters.
    #[test]
    fn a_launch_resolves_a_read_back_nobody_else_did() {
        let (home, primary, slot) = managed("continuity-pending");
        let harness = Harness::new(SessionContinuity::Observed);
        plan(&home, &slot, &harness);
        assert_eq!(recorded(&home, &primary), None, "still pending");

        *harness.observed.borrow_mut() = Some(SessionId::new("named-by-the-harness"));
        let plan = plan(&home, &slot, &harness);

        assert_eq!(
            plan.args,
            vec![
                OsString::from("--resume"),
                OsString::from("named-by-the-harness")
            ]
        );
        assert_eq!(
            recorded(&home, &primary),
            Some(SessionId::new("named-by-the-harness"))
        );
    }

    #[test]
    fn a_directory_no_task_owns_is_left_exactly_as_it_was() {
        let (home, _primary, _slot) = managed("continuity-unmanaged");
        let elsewhere = uze_testkit::temp::scratch("continuity-elsewhere");
        assert_eq!(
            plan(
                &home,
                &elsewhere,
                &Harness::new(SessionContinuity::Assigned)
            ),
            LaunchPlan::nothing()
        );
    }

    #[test]
    fn a_harness_that_declares_no_continuity_contributes_nothing_and_records_nothing() {
        let (home, primary, slot) = managed("continuity-unsupported");
        assert_eq!(
            plan(&home, &slot, &Harness::new(SessionContinuity::Unsupported)),
            LaunchPlan::nothing()
        );
        assert_eq!(recorded(&home, &primary), None);
    }

    #[test]
    fn a_conversation_the_harness_no_longer_holds_starts_a_new_one_and_says_so() {
        let (home, primary, slot) = managed("continuity-vanished");
        let mut harness = Harness::new(SessionContinuity::Assigned);
        plan(&home, &slot, &harness);
        let gone = recorded(&home, &primary).unwrap();

        harness.exists = false;
        let plan = plan(&home, &slot, &harness);

        let replacement = recorded(&home, &primary).unwrap();
        assert_ne!(replacement, gone);
        assert_eq!(
            plan.args,
            vec![
                OsString::from("--start"),
                OsString::from(replacement.as_str())
            ]
        );
        assert!(plan.note.is_some_and(|note| note.contains(gone.as_str())));
    }

    #[test]
    fn unreadable_state_still_launches_the_agent() {
        let (home, primary, slot) = managed("continuity-unreadable");
        let store = task::load(&home, &primary).unwrap();
        let path = conversation::store_path(&home, &primary, &store.tasks[0].id);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"not json at all").unwrap();

        let plan = plan(&home, &slot, &Harness::new(SessionContinuity::Assigned));
        assert!(!plan.args.is_empty(), "the agent still starts");
    }

    #[test]
    fn a_read_back_records_where_the_agent_actually_is() {
        let (home, primary, slot) = managed("continuity-refresh");
        let harness = Harness::new(SessionContinuity::Observed);
        plan(&home, &slot, &harness);

        *harness.observed.borrow_mut() = Some(SessionId::new("started-by-the-harness"));
        assert!(refresh(&home, &slot, &harness));
        assert_eq!(
            recorded(&home, &primary),
            Some(SessionId::new("started-by-the-harness"))
        );

        // Nothing new to say is not a write.
        assert!(!refresh(&home, &slot, &harness));
    }

    /// The clear/fork case: the agent left the conversation it started in,
    /// and what resumes has to be where the work actually went.
    #[test]
    fn a_conversation_the_agent_moved_to_replaces_the_one_it_started_in() {
        let (home, primary, slot) = managed("continuity-moved");
        let harness = Harness::new(SessionContinuity::Assigned);
        plan(&home, &slot, &harness);
        let started_in = recorded(&home, &primary).unwrap();

        *harness.observed.borrow_mut() = Some(SessionId::new("moved-to"));
        assert!(refresh(&home, &slot, &harness));

        let plan = plan(&home, &slot, &harness);
        assert_eq!(
            plan.args,
            vec![OsString::from("--resume"), OsString::from("moved-to")]
        );
        assert_ne!(recorded(&home, &primary), Some(started_in));
    }

    #[test]
    fn an_identifier_an_authoritative_source_named_is_recorded_without_observing() {
        let (home, primary, slot) = managed("continuity-authoritative");
        plan(&home, &slot, &Harness::new(SessionContinuity::Observed));

        assert!(record_observed(
            &home,
            &slot,
            "harness",
            SessionId::new("from-a-hook")
        ));
        assert_eq!(
            recorded(&home, &primary),
            Some(SessionId::new("from-a-hook"))
        );
    }

    #[test]
    fn an_identifier_for_a_launch_that_never_happened_is_not_recorded() {
        let (home, _primary, slot) = managed("continuity-no-launch");
        assert!(!record_observed(
            &home,
            &slot,
            "harness",
            SessionId::new("out-of-nowhere")
        ));
    }
}
