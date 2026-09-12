//! Where a trace goes.
//!
//! Every crate below this one opens `tracing` spans and records events;
//! none of them knows whether anything is listening. This module is the
//! one place that decides, once per process, in `main`:
//!
//! - **`UZE_LOG=<filter>`** subscribes a text layer — stderr for a
//!   command, a file under `UZE_HOME`'s logs for the TUI, which owns the
//!   terminal — using `tracing_subscriber`'s filter syntax (`info`,
//!   `uze_git=debug`, …).
//! - **`OTEL_EXPORTER_OTLP_ENDPOINT`**, in a binary built with the
//!   `telemetry` feature, subscribes an OTLP exporter as well; the guard
//!   [`Telemetry`] flushes it before the process exits, so a command that
//!   ran for five milliseconds still reports its whole trace.
//!
//! With neither set, nothing subscribes and a span costs a branch.
//!
//! The trace crosses one process boundary: the runtime shim `exec`s a
//! harness, and a hook that harness fires runs `uze hook-exec`. The shim
//! puts its span's context into the child's environment as W3C
//! `TRACEPARENT`, the harness passes its environment through, and
//! `hook-exec` adopts it as its root's parent — so a hook is a child of
//! the launch that caused it. Both halves are no-ops without the feature:
//! there is no trace id to carry.

use std::{fs, path::PathBuf, process::Command, sync::Mutex};

use tracing_subscriber::{EnvFilter, Layer, layer::SubscriberExt, util::SubscriberInitExt};

/// The environment variable that switches the text layer on and filters
/// every layer.
pub const LOG_FILTER: &str = "UZE_LOG";

/// The OpenTelemetry-standard endpoint variable the exporter reads.
pub const OTLP_ENDPOINT: &str = "OTEL_EXPORTER_OTLP_ENDPOINT";

/// Where the text layer writes.
pub enum Sink {
    Stderr,
    /// Appended to; created with its parents when missing. The TUI's
    /// choice, since stderr is the screen it draws on.
    File(PathBuf),
}

/// Holds the exporter for the life of the process. Dropping it — or
/// calling [`Telemetry::finish`] before an `exec` replaces the process —
/// flushes every span still in the batch.
#[must_use = "dropping the guard early flushes and stops the exporter"]
#[derive(Default)]
pub struct Telemetry {
    #[cfg(feature = "telemetry")]
    provider: Option<opentelemetry_sdk::trace::SdkTracerProvider>,
}

impl Telemetry {
    /// Flushes and stops the exporter now. For the one caller that cannot
    /// rely on `Drop`: the shim, whose `exec` never returns to unwind.
    pub fn finish(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        #[cfg(feature = "telemetry")]
        if let Some(provider) = self.provider.take() {
            let _ = provider.shutdown();
        }
    }
}

impl Drop for Telemetry {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Subscribes whatever the environment asks for. Idempotent in effect: a
/// second call in one process (tests) leaves the first subscriber in place.
pub fn init(sink: Sink) -> Telemetry {
    let filter = std::env::var(LOG_FILTER).ok();
    let endpoint = otlp_endpoint();
    if filter.is_none() && endpoint.is_none() {
        return Telemetry::default();
    }
    let env_filter = filter
        .as_deref()
        .and_then(|directives| EnvFilter::try_new(directives).ok())
        .unwrap_or_else(|| EnvFilter::new("info"));
    let text = filter.is_some().then(|| text_layer(sink));
    let registry = tracing_subscriber::registry().with(env_filter).with(text);
    #[cfg(feature = "telemetry")]
    if let Some(endpoint) = endpoint {
        use opentelemetry::trace::TracerProvider as _;
        let provider = otlp::provider(&endpoint);
        let layer = tracing_opentelemetry::layer().with_tracer(provider.tracer("uze"));
        let _ = registry.with(layer).try_init();
        return Telemetry {
            provider: Some(provider),
        };
    }
    let _ = registry.try_init();
    Telemetry::default()
}

fn text_layer<S>(sink: Sink) -> Box<dyn Layer<S> + Send + Sync>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    let file = match sink {
        Sink::Stderr => None,
        Sink::File(path) => {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .ok()
        }
    };
    // A span's close is what the text is for: it carries the busy and idle
    // time, which is the "where did the milliseconds go" a person reads
    // this for. Events alone would show nothing for a warm command.
    let span_close = tracing_subscriber::fmt::format::FmtSpan::CLOSE;
    match file {
        Some(file) => tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_span_events(span_close)
            .with_writer(Mutex::new(file))
            .boxed(),
        None => tracing_subscriber::fmt::layer()
            .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
            .with_span_events(span_close)
            .with_writer(std::io::stderr)
            .boxed(),
    }
}

/// The root span of a CLI invocation: the leaf command a person typed and
/// the argument line, less anything that looks like a secret.
pub fn command_span(command: &str, argv: &[String]) -> tracing::Span {
    tracing::info_span!("cli", command, argv = %redact(argv))
}

/// What is left of `argv` once nothing in it is worth stealing.
///
/// This line is appended to `~/.uze/state/logs/uze.log`, which is never
/// rotated, and exported to whatever collector `OTEL_EXPORTER_OTLP_ENDPOINT`
/// names. `uze market add https://user:token@host/market` is a supported,
/// documented shape, so a credential arriving here is an ordinary input,
/// not a mistake — and a log is exactly the place it must not survive.
fn redact(argv: &[String]) -> String {
    argv.iter()
        .map(|argument| redacted(argument))
        .collect::<Vec<_>>()
        .join(" ")
}

const REDACTED: &str = "<redacted>";

fn redacted(argument: &str) -> String {
    if let Some(scheme) = argument.find("://") {
        let (scheme, rest) = argument.split_at(scheme + 3);
        // Only the authority carries userinfo; an `@` past the first `/`
        // belongs to the path and is nobody's password.
        let authority = rest.find('/').unwrap_or(rest.len());
        return match rest[..authority].find('@') {
            Some(at) => format!("{scheme}{REDACTED}@{}", &rest[at + 1..]),
            None => argument.to_owned(),
        };
    }
    if looks_minted(argument) {
        return REDACTED.to_owned();
    }
    argument.to_owned()
}

/// Whether an argument is a credential standing on its own. Only shapes a
/// provider actually mints: a length-and-alphabet guess would redact the
/// digests and package ids this trace exists to show.
fn looks_minted(argument: &str) -> bool {
    const MINTED: [&str; 7] = ["ghp_", "gho_", "ghu_", "ghs_", "ghr_", "github_pat_", "xox"];
    MINTED
        .iter()
        .any(|prefix| argument.starts_with(prefix) && argument.len() > prefix.len() + 8)
}

/// Puts the current span's trace context into `command`'s environment as
/// `TRACEPARENT`/`TRACESTATE`, so a child that adopts it continues this
/// trace. Nothing without the feature.
pub fn inject_into(command: &mut Command) {
    #[cfg(feature = "telemetry")]
    otlp::inject_into(command);
    #[cfg(not(feature = "telemetry"))]
    let _ = command;
}

/// Makes the trace context in this process's environment the parent of
/// `span`. Before the span is entered — entering starts its context, and
/// a started context keeps the parent it has. Nothing without the
/// feature, or without a valid `TRACEPARENT`.
pub fn adopt_parent_from_env(span: &tracing::Span) {
    #[cfg(feature = "telemetry")]
    otlp::adopt_parent_from_env(span);
    #[cfg(not(feature = "telemetry"))]
    let _ = span;
}

#[cfg(feature = "telemetry")]
fn otlp_endpoint() -> Option<String> {
    std::env::var(OTLP_ENDPOINT)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

#[cfg(not(feature = "telemetry"))]
fn otlp_endpoint() -> Option<String> {
    None
}

#[cfg(feature = "telemetry")]
mod otlp {
    use std::process::Command;

    use opentelemetry::{
        KeyValue, global,
        propagation::{Extractor, Injector},
        trace::TraceContextExt,
    };
    use opentelemetry_otlp::WithExportConfig;
    use opentelemetry_sdk::{
        Resource, propagation::TraceContextPropagator, trace::SdkTracerProvider,
    };
    use tracing_opentelemetry::OpenTelemetrySpanExt;

    /// A batch exporter on its own thread, over HTTP/protobuf to the
    /// collector's traces path. The blocking client, not the async one:
    /// the binary's tokio runtime exists only for the MCP fixture.
    pub(super) fn provider(endpoint: &str) -> SdkTracerProvider {
        global::set_text_map_propagator(TraceContextPropagator::new());
        let traces = format!("{}/v1/traces", endpoint.trim_end_matches('/'));
        let resource = Resource::builder()
            .with_service_name("uze")
            .with_attribute(KeyValue::new("service.version", env!("CARGO_PKG_VERSION")))
            .build();
        let builder = SdkTracerProvider::builder().with_resource(resource);
        match opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_endpoint(traces)
            .build()
        {
            Ok(exporter) => builder.with_batch_exporter(exporter).build(),
            // A collector that cannot be reached is not a reason to fail a
            // command; the spans simply go nowhere.
            Err(_) => builder.build(),
        }
    }

    /// W3C `traceparent`/`tracestate` as environment variables, the way a
    /// child process can read them.
    struct EnvironmentCarrier<'a>(&'a mut Command);

    impl Injector for EnvironmentCarrier<'_> {
        fn set(&mut self, key: &str, value: String) {
            self.0.env(key.to_ascii_uppercase(), value);
        }
    }

    struct EnvironmentReader;

    impl Extractor for EnvironmentReader {
        fn get(&self, key: &str) -> Option<&str> {
            // The propagator asks for `traceparent`; the value lives in the
            // process environment and cannot be borrowed from there, so it
            // is leaked once per lookup — two small strings per process.
            std::env::var(key.to_ascii_uppercase())
                .ok()
                .map(|value| &*Box::leak(value.into_boxed_str()))
        }

        fn keys(&self) -> Vec<&str> {
            vec!["traceparent", "tracestate"]
        }
    }

    pub(super) fn inject_into(command: &mut Command) {
        let context = tracing::Span::current().context();
        if !context.span().span_context().is_valid() {
            return;
        }
        global::get_text_map_propagator(|propagator| {
            propagator.inject_context(&context, &mut EnvironmentCarrier(command));
        });
    }

    pub(super) fn adopt_parent_from_env(span: &tracing::Span) {
        let parent =
            global::get_text_map_propagator(|propagator| propagator.extract(&EnvironmentReader));
        if parent.span().span_context().is_valid() {
            let _ = span.set_parent(parent);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_credential_in_the_argument_line_never_reaches_the_trace() {
        let recorded =
            |argv: &[&str]| redact(&argv.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>());

        assert_eq!(
            recorded(&[
                "market",
                "add",
                "team",
                "https://romullo:ghp_0123456789abcdef@github.com/org/market.git"
            ]),
            "market add team https://<redacted>@github.com/org/market.git"
        );
        assert_eq!(
            recorded(&["plugin", "install", "foo@https://token@host/market"]),
            "plugin install foo@https://<redacted>@host/market"
        );
        assert_eq!(
            recorded(&["agent", "task", "name", "fix/ghp_short"]),
            "agent task name fix/ghp_short",
            "a word that merely starts like a token is not one"
        );
        assert_eq!(
            recorded(&["plugin", "install", "ghp_0123456789abcdefghij"]),
            "plugin install <redacted>"
        );
        assert_eq!(
            recorded(&["market", "add", "https://github.com/org/market.git"]),
            "market add https://github.com/org/market.git",
            "a URL with nothing to hide is left as it was typed"
        );
        assert_eq!(
            recorded(&["market", "add", "https://github.com/org/a@b/market"]),
            "market add https://github.com/org/a@b/market",
            "an `@` in the path is not userinfo"
        );
    }

    /// The handshake the shim and `hook-exec` perform, in one process: a
    /// child `Command` gets the parent's `TRACEPARENT`, and a span that
    /// adopts it belongs to the same trace.
    #[cfg(feature = "telemetry")]
    #[test]
    fn the_trace_context_survives_the_environment_round_trip() {
        use opentelemetry::trace::TraceContextExt;
        use tracing_opentelemetry::OpenTelemetrySpanExt;
        use tracing_subscriber::layer::SubscriberExt;

        let mut env = uze_testkit::env::scope();
        // An exporter to a port nothing listens on: the batch is dropped,
        // which is fine — the ids are what this test is about.
        let provider = otlp::provider("http://127.0.0.1:9");
        let tracer = {
            use opentelemetry::trace::TracerProvider as _;
            provider.tracer("uze-test")
        };
        let subscriber =
            tracing_subscriber::registry().with(tracing_opentelemetry::layer().with_tracer(tracer));
        tracing::subscriber::with_default(subscriber, || {
            let parent = tracing::info_span!("shim");
            let parent_trace = {
                let _entered = parent.enter();
                let mut command = Command::new("true");
                inject_into(&mut command);
                let injected: Vec<(String, String)> = command
                    .get_envs()
                    .filter_map(|(key, value)| {
                        Some((
                            key.to_string_lossy().into_owned(),
                            value?.to_string_lossy().into_owned(),
                        ))
                    })
                    .collect();
                let traceparent = injected
                    .iter()
                    .find(|(key, _)| key == "TRACEPARENT")
                    .map(|(_, value)| value.clone())
                    .expect("TRACEPARENT is injected into the child's environment");
                env.set("TRACEPARENT", &traceparent);
                parent.context().span().span_context().trace_id()
            };
            let child = tracing::info_span!("hook-exec");
            adopt_parent_from_env(&child);
            let _entered = child.enter();
            assert_eq!(
                child.context().span().span_context().trace_id(),
                parent_trace,
                "the child continues the parent's trace"
            );
        });
        let _ = provider.shutdown();
    }
}
