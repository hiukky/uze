//! What an operation is doing now, for whoever is showing a person.
//!
//! The domain says what it is doing as structured `tracing` events on one
//! target — a step and its subject, never a sentence — because the words
//! belong to whoever draws them: a spinner line in the CLI, the header in
//! the TUI. This is the one layer that hands those events on, to at most
//! one listener, which the surface running the command installs.

use std::{collections::BTreeMap, sync::Mutex};

use tracing::{Event, Subscriber, field::Field};
use tracing_subscriber::{Layer, filter::Targets, layer::Context};

/// The target every step is emitted on.
pub const TARGET: &str = "uze::step";

/// One step, as the domain described it: `step` names what is happening,
/// and the other fields what it is happening to.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Step {
    fields: BTreeMap<&'static str, String>,
}

impl Step {
    pub fn get(&self, field: &str) -> Option<&str> {
        self.fields.get(field).map(String::as_str)
    }

    /// A step built by hand, for a surface's own tests.
    pub fn of(fields: &[(&'static str, &str)]) -> Self {
        Self {
            fields: fields
                .iter()
                .map(|(key, value)| (*key, (*value).to_owned()))
                .collect(),
        }
    }
}

type Listener = Box<dyn Fn(&Step) + Send + Sync>;

static LISTENER: Mutex<Option<Listener>> = Mutex::new(None);

/// Hands every step from now on to `listener`, replacing any before it.
pub fn listen(listener: impl Fn(&Step) + Send + Sync + 'static) {
    if let Ok(mut current) = LISTENER.lock() {
        *current = Some(Box::new(listener));
    }
}

/// The layer that carries steps to the listener, filtered to their target
/// alone so it costs nothing on any other callsite.
pub fn layer<S>() -> impl Layer<S>
where
    S: Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    Steps.with_filter(Targets::new().with_target(TARGET, tracing::Level::INFO))
}

struct Steps;

impl<S: Subscriber> Layer<S> for Steps {
    fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
        let mut step = Step::default();
        event.record(&mut Recorder(&mut step));
        if let Ok(listener) = LISTENER.lock()
            && let Some(listener) = listener.as_ref()
        {
            listener(&step);
        }
    }
}

struct Recorder<'a>(&'a mut Step);

impl tracing::field::Visit for Recorder<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.fields.insert(field.name(), value.to_owned());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.fields.insert(field.name(), format!("{value:?}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::layer::SubscriberExt;

    #[test]
    fn a_step_reaches_the_listener_with_its_fields_and_nothing_else_does() {
        let seen = std::sync::Arc::new(Mutex::new(Vec::new()));
        {
            let seen = seen.clone();
            listen(move |step| seen.lock().unwrap().push(step.clone()));
        }
        let subscriber = tracing_subscriber::registry().with(layer());
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(target: TARGET, step = "deliver", harness = "some-harness");
            tracing::info!(target: "uze::other", step = "ignored");
        });

        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!(seen[0].get("step"), Some("deliver"));
        assert_eq!(seen[0].get("harness"), Some("some-harness"));
    }
}
