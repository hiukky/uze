//! What a call to the application looks like as a trace: one tree under
//! the caller's span, named for the service and the method, with the
//! error on the span when there is one.

use std::sync::{Arc, Mutex};

use tracing::{Subscriber, span};
use tracing_subscriber::{Layer, layer::Context, layer::SubscriberExt, registry::LookupSpan};

use super::*;

/// A span as this records it: its name, and its parent's name when it had
/// one.
type Span = (String, Option<String>);

/// Every span opened while the subscriber was current: its name, and its
/// parent's name when it had one.
#[derive(Clone, Default)]
struct Recorded {
    spans: Arc<Mutex<Vec<Span>>>,
    errors: Arc<Mutex<Vec<String>>>,
}

impl<S> Layer<S> for Recorded
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, context: Context<'_, S>) {
        let parent = context
            .span(id)
            .and_then(|span| span.parent().map(|parent| parent.name().to_owned()));
        self.spans
            .lock()
            .unwrap()
            .push((attrs.metadata().name().to_owned(), parent));
    }

    fn on_event(&self, event: &tracing::Event<'_>, _context: Context<'_, S>) {
        if *event.metadata().level() == tracing::Level::ERROR {
            let mut message = String::new();
            event.record(
                &mut |field: &tracing::field::Field, value: &dyn std::fmt::Debug| {
                    if field.name() == "error" {
                        message = format!("{value:?}");
                    }
                },
            );
            self.errors.lock().unwrap().push(message);
        }
    }
}

fn recorded<T>(body: impl FnOnce() -> T) -> (T, Recorded) {
    let recorded = Recorded::default();
    let subscriber = tracing_subscriber::registry().with(recorded.clone());
    let result = tracing::subscriber::with_default(subscriber, body);
    (result, recorded)
}

#[test]
fn a_service_call_is_one_span_tree() {
    let root = uze_testkit::temp::scratch("tracing-tree");
    let app = UzeApplication::new(UzeHome::at(&root), Vec::new());
    let ((), recorded) = recorded(|| {
        let action = tracing::info_span!("action");
        let _entered = action.enter();
        let _ = app.machine_snapshot(&root, 5);
    });
    let spans = recorded.spans.lock().unwrap().clone();
    let named = |name: &str| spans.iter().find(|(span, _)| span == name).cloned();
    assert_eq!(
        named("snapshot.machine"),
        Some(("snapshot.machine".to_owned(), Some("action".to_owned()))),
        "the snapshot hangs under the caller's span: {spans:?}"
    );
    for child in [
        "plugins.list",
        "health.report",
        "marketplace.list",
        "marketplace.plugins",
        "profiles.list",
        "workspace.summary",
    ] {
        assert_eq!(
            named(child),
            Some((child.to_owned(), Some("snapshot.machine".to_owned()))),
            "{child} is a child of the snapshot: {spans:?}"
        );
    }
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_failed_service_call_records_its_error_on_the_span() {
    let root = uze_testkit::temp::scratch("tracing-error");
    let app = UzeApplication::new(UzeHome::at(&root), Vec::new());
    let (result, recorded) = recorded(|| app.plugins().inspect("no-such-plugin"));
    assert!(result.is_err());
    let spans = recorded.spans.lock().unwrap().clone();
    assert!(
        spans.iter().any(|(name, _)| name == "plugins.inspect"),
        "the call opened its span: {spans:?}"
    );
    let errors = recorded.errors.lock().unwrap().clone();
    assert!(
        errors
            .iter()
            .any(|message| message.contains("no-such-plugin")),
        "the error was recorded on the span: {errors:?}"
    );
    let _ = fs::remove_dir_all(&root);
}
