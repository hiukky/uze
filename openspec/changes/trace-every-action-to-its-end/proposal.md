## Why

UZE has no instrumentation. Finding where the management screen spent
seven seconds (`hold-read-paths-to-the-budget`) meant patching `eprintln!`
timers into `main.rs` and rebuilding; the only log the product writes is
each harness installer's output under `state/logs/`. A person who wants to
know what a command did, how long each part took, and which part failed,
has the exit code and the last line of stderr.

The maintainer asked for an established solution rather than an in-house
timer: something from the Rust ecosystem, for internal use, running
locally, with a dashboard, covering performance and errors — and covering
the whole of an action, from the key or the command line to the end of
the execution, worker threads and child processes included.

## What Changes

- **`tracing` is the instrumentation layer.** Already in the tree through
  `rmcp`; it becomes a direct dependency of every crate that does work
  (`uze-core`, `uze-git`, `uze-application`, `uze-integrations`,
  `uze-terminal`, the binary). Without a subscriber a span costs a branch.
- **A root span per action.** The CLI opens `cli` around `run`, carrying
  the leaf command and its arguments; the TUI opens `tui.session` around
  the whole run and `tui.intent` around each dispatched key or click, in
  both clients. Every `thread::spawn` in the TUI captures the current span
  and enters it on the thread, so a worker's spans are the intent's
  children rather than a second trace.
- **Every application entry point is a span.** Each `pub fn` of
  `UzeApplication`'s services carries `#[tracing::instrument]` with the
  identifiers that name what it acts on, and `err` so a failure marks the
  span. An architecture test scans those modules and fails by name for an
  entry point without one.
- **The mechanisms below it are spans.** One per Git invocation in
  `uze-git` (arguments, exit code, duration), one per acquisition clone,
  one per vendor CLI the integrations run (`shared::process`), one per
  provisioning process, one per hook handler, one per terminal-runtime
  request, one per atomic write at debug level; detection and inspection
  cache hits and misses as events.
- **The trace crosses the process boundary UZE owns.** The runtime shim
  injects `TRACEPARENT` into the harness it `exec`s, and every `uze`
  started under that harness — `hook-exec` for a hook it fires, or a
  command an agent types inside it — adopts it as its root's parent, so
  the work continues the trace of the shim that launched the harness. The terminal server is its own root:
  it outlives every client, and a request's span records which client
  asked.
- **Where it goes.** `UZE_LOG=<filter>` writes spans and events as text
  (stderr for a command, `state/logs/uze.log` for the TUI, which owns the
  terminal). Behind the cargo feature `telemetry`, `OTEL_EXPORTER_OTLP_
  ENDPOINT` exports every span over OTLP/HTTP to a local Jaeger started by
  `make observe`, with the process flushed on exit; a command that runs
  for five milliseconds still reports its trace.
- **Documented once**, in `docs/observability.md`: how to read a trace of
  a command, of a TUI session, of a hook; what the feature costs; what is
  and is not propagated.

## Impact

- Dependencies (written down here per `AGENTS.md`): `tracing-subscriber`
  (tokio-rs) unconditionally; behind `telemetry`: `opentelemetry`,
  `opentelemetry_sdk`, `opentelemetry-otlp` (`http-proto`, blocking
  `reqwest` client, no TLS — the endpoint is localhost) and
  `tracing-opentelemetry` (tokio-rs). Measured in a scratch project on
  2026-09-07: 66 crates new to the workspace, all MIT/Apache-2.0/BSD/
  Unicode/Zlib under `deny.toml`, none compiling C, no TLS stack. The
  release matrix builds without the feature.
- Specs: `observability` (new).
- Code: every crate; `src/telemetry.rs`; `tests/architecture/`.
