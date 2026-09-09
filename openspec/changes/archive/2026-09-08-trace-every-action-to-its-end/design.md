## Context

`tracing` spans are the unit: named, timed, nested, with typed fields, and
free to enter when nothing subscribes. Which subscriber runs — a text
writer, an OTLP exporter — is the binary's decision, made once in
`main`, and nothing below it knows.

## Decisions

### 1. `tracing`, not `log`, not a metrics registry

A CLI run is a tree of operations, not a stream of lines; a span tree is
the shape a person reads to find where the time went. `log` has no
spans. A metrics crate answers a different question (rates over a
long-lived process) that a 5 ms process does not ask.

### 2. Every service `pub fn` is instrumented, and a test says so

The application facade is the surface a command or a key lands on; if
every entry point is a span, every action has at least one span with a
name a person recognises from the command they typed. `#[instrument]` on
each is mechanical, and the test that scans for it is what keeps the next
entry point from shipping silent — the same shape as
`command_performance`, for the same reason.

`skip_all` by default: arguments are paths, authorities and whole
manifests. Fields are chosen: the plugin id, the marketplace name, the
project root. `err` so a `Result::Err` marks the span, at error level,
with the message.

### 3. Call sites instrument the integrations

`IntegrationPort` is implemented four times; wrapping each method in each
vendor is twenty-odd edits that drift. The application calls the port
from a handful of places (`detect_cached`, `attach_package_to`,
`reconcile_cached_report`, republish, provision), and a span there,
carrying `integration = id`, covers every vendor at once and stays
vendor-neutral. What the vendors do spawn — their CLIs — goes through
`shared::process`, which gets one span for all of them.

### 4. The TUI's threads inherit the intent

`thread::spawn` starts a thread with no current span. Each site captures
`tracing::Span::current()` before spawning and enters it inside, so a
refresh's `snapshot.machine` hangs under the `tui.intent` that asked for
it. The architecture test that requires the literal `thread::spawn` in
the workspace client is untouched: the capture is two lines around it.

### 5. `TRACEPARENT` across the one boundary UZE owns

A hook runs because the harness fired it, and the harness runs because
the shim `exec`ed it. The shim has no application and no subscriber
beyond the one `main` installs, but it does have a root span; under the
feature, its OTLP context is injected into the child's environment as
W3C `TRACEPARENT`, and every `uze` started under the harness — not only
`hook-exec`: an agent typing `uze status` inside it too — extracts it as
its root's parent, before the root is entered, since a started span keeps
the parent it has. The harness in between passes environment through
untouched, which is all that is asked of it. Without the feature there is
no trace id to carry, and nothing is injected.

The terminal server is deliberately not joined: it outlives every client
and serves several, so one trace could not own it. Its requests are
spans of its own root, each recording the client that sent it.

### 6. Text always, OTLP behind a feature

`tracing-subscriber`'s text layer costs ten small crates and answers
"what did this command do" with no infrastructure, so it is always
compiled and switched on by `UZE_LOG`. The OTLP path costs sixty-six
crates and only pays off with a collector listening, so it is a cargo
feature the release matrix does not enable; a developer builds
`--features telemetry` and points `OTEL_EXPORTER_OTLP_ENDPOINT` at the
Jaeger `make observe` starts. The blocking `reqwest` client, not the
async one: the exporter runs on its own thread, and the binary's tokio
runtime exists only for the MCP fixture.

### 7. Flush on exit, cheaply

A batch exporter that is never shut down loses the last batch, which for
a 5 ms command is the whole trace. `telemetry::init` returns a guard
whose drop shuts the provider down; `main` holds it across `run`. With no
endpoint configured no provider exists and the drop is a no-op.

### 8. The TUI writes to a file

The TUI owns the terminal; a text layer on stderr would draw over it.
`ui::run` is entered with the sink already chosen by `main`: a file under
`state/logs/` when the command is the TUI, stderr otherwise.
