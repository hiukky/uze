//! Where a trace goes.
//!
//! Every crate below this one opens `tracing` spans and records events;
//! none of them knows whether anything is listening. This module is the
//! one place that decides, once per process, in `main`:
//!
//! - **A journal**, for the two processes that outlive the gesture that
//!   started them: the TUI and the terminal server. Always on, whatever
//!   the environment says, because the thing worth reading afterwards is
//!   the run nobody expected to have to read — see [`Sink::Journal`].
//! - **`UZE_LOG=<filter>`** subscribes a text layer on stderr for a
//!   command, and raises the journal's own level, using
//!   `tracing_subscriber`'s filter syntax (`info`, `uze_git=debug`, …).
//! - **`OTEL_EXPORTER_OTLP_ENDPOINT`**, in a binary built with the
//!   `telemetry` feature, subscribes an OTLP exporter as well; the guard
//!   [`Telemetry`] flushes it before the process exits, so a command that
//!   ran for five milliseconds still reports its whole trace.
//!
//! For a one-shot command with none of them set, nothing subscribes and a
//! span costs a branch.
//!
//! The trace crosses one process boundary: the runtime shim `exec`s a
//! harness, and the harness runs `uze` again — an agent inside it asking
//! UZE something. The shim puts its span's context into the child's
//! environment as W3C `TRACEPARENT`, the harness passes its environment
//! through, and that `uze` adopts it as its root's parent, so the two are
//! one trace. Both halves are no-ops without the feature: there is no
//! trace id to carry.

use std::{path::PathBuf, process::Command};

use tracing_subscriber::{EnvFilter, Layer, layer::SubscriberExt, util::SubscriberInitExt};

/// The environment variable that switches the text layer on and filters
/// every layer.
pub const LOG_FILTER: &str = "UZE_LOG";

/// The OpenTelemetry-standard endpoint variable the exporter reads.
pub const OTLP_ENDPOINT: &str = "OTEL_EXPORTER_OTLP_ENDPOINT";

/// How many days of journal are kept. A week is the distance between a
/// thing going wrong and somebody sitting down to look at it; past that
/// the file answers about a build nobody is running any more.
const JOURNAL_DAYS: usize = 7;

/// The most a journal's whole history may take. A day's file is written
/// by a process nobody is watching, at a volume set by how the code
/// happens to be instrumented — which is a number no reviewer checks, and
/// the one that quietly reached 64 MB in an afternoon. A ceiling in bytes
/// is the only limit that stays true when somebody adds a span to a loop.
///
/// Days, not bytes, is still the *rule*; this is the floor under it. The
/// current day's file is never one of the ones dropped, so a single day
/// that exceeds this on its own is reported rather than truncated — a
/// journal that cuts the run it is about would lose exactly the lines
/// worth having.
const JOURNAL_BYTES: u64 = 64 * 1024 * 1024;

/// Where the text layer writes.
pub enum Sink {
    /// A command's diagnostics, where the person who typed it is looking.
    /// Only with `UZE_LOG`: an unasked-for line on stderr is output a
    /// caller has to parse around.
    Stderr,
    /// The journal a long-lived process keeps whether anybody asked or
    /// not: `<dir>/<name>.<date>.log`, rolled daily and pruned to
    /// [`JOURNAL_DAYS`].
    ///
    /// Kept without being asked because the alternative is what this
    /// exists to end: an operator describes something the product did,
    /// and the only record of it is the screen they were looking at. The
    /// two processes that get one — the TUI and the terminal server —
    /// also cannot use stderr, which is a screen in one and `/dev/null`
    /// in the other.
    ///
    /// Written from a thread of its own, so a render loop never waits on
    /// a disk, and never lossy: a journal that drops the lines around a
    /// burst drops exactly the ones worth having.
    Journal { dir: PathBuf, name: String },
}

impl Sink {
    /// The journal `name` keeps under `dir` — `UzeHome::logs_dir`, which
    /// sits with the caches because losing it costs nothing.
    pub fn journal(dir: PathBuf, name: &str) -> Self {
        Self::Journal {
            dir,
            name: name.to_owned(),
        }
    }
}

/// Holds the exporter for the life of the process. Dropping it — or
/// calling [`Telemetry::finish`] before an `exec` replaces the process —
/// flushes every span still in the batch.
#[must_use = "dropping the guard early flushes and stops the exporter"]
#[derive(Default)]
pub struct Telemetry {
    #[cfg(feature = "telemetry")]
    provider: Option<opentelemetry_sdk::trace::SdkTracerProvider>,
    /// The journal's writer thread. Dropping it flushes what it still
    /// holds — which is the last thing written before a crash, and the
    /// reason this is a field rather than a `let _`.
    journal: Option<tracing_appender::non_blocking::WorkerGuard>,
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
        drop(self.journal.take());
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
    let journals = matches!(sink, Sink::Journal { .. });
    if filter.is_none() && endpoint.is_none() && !journals {
        return Telemetry::default();
    }
    // `info` is what a journal nobody configured carries: the actions, the
    // Git calls, the integration calls and every failure, which is the
    // level this trace was instrumented at. `UZE_LOG` raises or narrows
    // it for both the journal and stderr — one switch, so a person cannot
    // be reading one level and telling somebody about another.
    let env_filter = filter
        .as_deref()
        .and_then(|directives| EnvFilter::try_new(directives).ok())
        .unwrap_or_else(|| EnvFilter::new("info"));
    let (text, journal) = match (journals, filter.is_some()) {
        (false, false) => (None, None),
        _ => {
            let (layer, guard) = text_layer(sink);
            (Some(layer), guard)
        }
    };
    let registry = tracing_subscriber::registry().with(env_filter).with(text);
    #[cfg(feature = "telemetry")]
    if let Some(endpoint) = endpoint {
        use opentelemetry::trace::TracerProvider as _;
        let provider = otlp::provider(&endpoint);
        let layer = tracing_opentelemetry::layer().with_tracer(provider.tracer("uze"));
        let _ = registry.with(layer).try_init();
        return Telemetry {
            provider: Some(provider),
            journal,
        };
    }
    let _ = registry.try_init();
    #[cfg(feature = "telemetry")]
    return Telemetry {
        provider: None,
        journal,
    };
    #[cfg(not(feature = "telemetry"))]
    Telemetry { journal }
}

/// The text layer, and the writer thread it is to be kept alive by.
fn text_layer<S>(
    sink: Sink,
) -> (
    Box<dyn Layer<S> + Send + Sync>,
    Option<tracing_appender::non_blocking::WorkerGuard>,
)
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    // A span's close is what the text is for: it carries the busy and idle
    // time, which is the "where did the milliseconds go" a person reads
    // this for. Events alone would show nothing for a warm command.
    let span_close = tracing_subscriber::fmt::format::FmtSpan::CLOSE;
    let Sink::Journal { dir, name } = sink else {
        let layer = tracing_subscriber::fmt::layer()
            .with_ansi(std::io::IsTerminal::is_terminal(&std::io::stderr()))
            .with_span_events(span_close)
            .with_writer(std::io::stderr)
            .boxed();
        return (layer, None);
    };
    // A journal that cannot be opened is not a reason to fail the run the
    // journal is about: the process goes on with nothing written, exactly
    // as it did before there was one.
    prune_to_size(&dir, &name, JOURNAL_BYTES);
    let appender = match tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix(&name)
        .filename_suffix("log")
        .max_log_files(JOURNAL_DAYS)
        .build(&dir)
    {
        Ok(appender) => appender,
        Err(_) => return (Box::new(NoLayer), None),
    };
    let (writer, guard) = tracing_appender::non_blocking::NonBlockingBuilder::default()
        .lossy(false)
        .finish(appender);
    let layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_span_events(span_close)
        .with_writer(writer)
        .boxed();
    (layer, Some(guard))
}

/// Drops the oldest days of `name`'s journal until what is left fits in
/// `budget`, newest kept first. The newest is kept whatever it weighs: it
/// is the run being started, or the one just before it.
///
/// Runs before the appender opens, which is the one moment nothing is
/// writing. A file that cannot be read or removed is skipped — losing a
/// journal costs nothing, and failing a run over one would be the tail
/// wagging the dog.
fn prune_to_size(dir: &std::path::Path, name: &str, budget: u64) {
    let prefix = format!("{name}.");
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut journals: Vec<(std::ffi::OsString, u64)> = entries
        .flatten()
        .filter_map(|entry| {
            let file = entry.file_name();
            let named = file.to_str()?;
            if !named.starts_with(&prefix) || !named.ends_with(".log") {
                return None;
            }
            Some((file.clone(), entry.metadata().ok()?.len()))
        })
        .collect();
    // The rolled name ends in the date, so the name sorts the way the
    // clock does — no timestamp is read, and none is trusted.
    journals.sort_by(|left, right| right.0.cmp(&left.0));
    let mut kept = 0u64;
    for (index, (file, bytes)) in journals.iter().enumerate() {
        kept = kept.saturating_add(*bytes);
        if index > 0 && kept > budget {
            let _ = std::fs::remove_file(dir.join(file));
        }
    }
}

/// A layer that subscribes to nothing — what is installed when the journal
/// could not be opened, so `init`'s one return shape does not have to
/// carry "and also there is no layer".
struct NoLayer;

impl<S: tracing::Subscriber> Layer<S> for NoLayer {}

/// The root span of a CLI invocation: the leaf command a person typed and
/// the argument line, less anything that looks like a secret.
pub fn command_span(command: &str, argv: &[String]) -> tracing::Span {
    tracing::info_span!("cli", command, argv = %redact(argv))
}

/// What is left of `argv` once nothing in it is worth stealing.
///
/// This line is appended to the journal under `~/.uze/cache/logs`, which
/// an operator attaches to a report, and exported to whatever collector `OTEL_EXPORTER_OTLP_ENDPOINT`
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

    /// The ceiling is in bytes because that is the quantity that ran away:
    /// the oldest days go until the history fits, and the newest stays
    /// whatever it weighs — it is the run being started.
    #[test]
    fn the_journal_drops_its_oldest_days_until_the_history_fits() {
        let scratch = uze_testkit::temp::scratch("telemetry-budget");
        let write = |name: &str, bytes: usize| {
            std::fs::write(scratch.join(name), vec![b'x'; bytes]).expect("the journal is written");
        };
        write("probe.2026-09-17.log", 40);
        write("probe.2026-09-18.log", 40);
        write("probe.2026-09-19.log", 40);
        write("other.2026-09-17.log", 40);

        prune_to_size(&scratch, "probe", 90);

        let left = |name: &str| scratch.join(name).exists();
        assert!(left("probe.2026-09-19.log"), "the newest day stays");
        assert!(left("probe.2026-09-18.log"), "and the one that still fits");
        assert!(!left("probe.2026-09-17.log"), "the oldest goes");
        assert!(left("other.2026-09-17.log"), "another journal is not ours");
    }

    /// A single day over the ceiling is reported, never truncated: cutting
    /// the run the journal is about loses the lines worth having.
    #[test]
    fn a_single_day_over_the_ceiling_is_kept() {
        let scratch = uze_testkit::temp::scratch("telemetry-budget-one");
        std::fs::write(scratch.join("probe.2026-09-19.log"), vec![b'x'; 200])
            .expect("the journal is written");

        prune_to_size(&scratch, "probe", 50);

        assert!(scratch.join("probe.2026-09-19.log").exists());
    }

    /// The point of the journal: it is written because the process is one
    /// that keeps one, not because anybody set `UZE_LOG` beforehand. The
    /// incident it exists for is always described after the fact.
    #[test]
    fn a_journal_is_written_without_anybody_asking_for_one() {
        let scratch = uze_testkit::temp::scratch("telemetry-journal");
        let (layer, guard) = text_layer(Sink::journal(scratch.clone(), "probe"));
        let subscriber = tracing_subscriber::registry().with(layer);
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(root = "/somewhere", "a space was created");
        });
        // The writer's thread holds the line until the guard goes.
        drop(guard);

        let written: String = std::fs::read_dir(&scratch)
            .expect("the journal directory")
            .filter_map(|entry| std::fs::read_to_string(entry.ok()?.path()).ok())
            .collect();
        assert!(
            written.contains("a space was created") && written.contains("/somewhere"),
            "the event and its fields are in the journal: {written:?}"
        );
        let _ = std::fs::remove_dir_all(scratch);
    }

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

    /// The handshake the shim and a `uze` under it perform, in one
    /// process: a child `Command` gets the parent's `TRACEPARENT`, and a
    /// span that adopts it belongs to the same trace.
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
            let child = tracing::info_span!("uze-under-the-shim");
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
