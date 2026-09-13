# Observability

How to see what `uze` did, how long each part took, and where it failed.
Canonical guide for tracing; owned by whoever changes `src/telemetry.rs`.
The reasoning behind the design is in
`openspec/changes/trace-every-action-to-its-end/design.md` until that
change is archived, and in its ADR afterwards.

## What is traced

Every action opens a root span, and everything the action causes nests
under it:

| Root | Opened by | Carries |
|---|---|---|
| `cli` | every `uze <command>` | the leaf command (`context inspect`) and the argument line |
| `tui.session` | the terminal UI | one per run; `tui.intent` under it per key or click that starts work, `tui.event` (debug) per raw event in the workspace client |
| `shim` | a harness launched through `~/.uze/shims` | the harness name and the executable it resolved to |
| `terminal.serve` | the terminal runtime server | `terminal.client` per attached client, `terminal.request` (debug) per request |

Under a root:

- every public method of an application service, named
  `<service>.<method>` (`plugins.list`, `marketplace.plugins`,
  `context.reconcile`, `health.report`, …) with the id, name or path it
  acts on; a method that returns an error records it on the span;
- `integration.detect` / `attach` / `inspect` / `republish` / `provision`
  / `install` around each `IntegrationPort` call, with the integration id;
- `git` per Git invocation through `uze-git` (arguments, exit code),
  `acquisition.git` per clone step, `vendor.cli` per vendor CLI the
  integrations run, `process.run` per provisioning process, `hook.handler`
  per hook handler, `store.ingest`, `marketplace.clone`;
- at debug level: `engine.compose`, `persistence.write`, and the detection
  cache's hit and miss events.

A worker thread in the TUI enters the span that started it, so a refresh
is a child of the key that asked for it. `tests/architecture/
instrumentation.rs` fails by name for an application entry point without
a span.

## Reading it as text

```sh
UZE_LOG=info uze status            # spans and events on stderr
UZE_LOG=uze_git=debug uze status   # tracing_subscriber filter syntax
uze                                # the TUI writes to ~/.uze/state/logs/uze.log
```

`UZE_LOG` is both the switch and the filter, for the text layer and for
the exporter below. Unset, nothing subscribes and a span costs a branch.

## Reading it on a dashboard

The `telemetry` cargo feature compiles an OTLP/HTTP exporter. It is not in
the release binaries: sixty-odd pure-Rust crates that only pay off with a
collector listening.

```sh
make observe                       # Jaeger 2 in Docker: UI :16686, OTLP :4318
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318 \
  cargo run --features telemetry --bin uze -- doctor
make observe-stop                  # stop it; what it collected is gone
```

Open <http://localhost:16686>, service `uze`. Each command is one trace;
the TUI is one trace per session. The exporter batches on its own thread
and is flushed when the process exits — or, in the shim, right before it
`exec`s the harness — so a command that ran for five milliseconds still
reports the whole tree.

### Leaving it on

Jaeger restarts with Docker and is meant to be left running — it keeps
its spans in memory, so a restart of the container starts an empty
history. To trace every `uze` you run rather than one command at a time,
install and export the endpoint from your shell profile:

```sh
make install                                               # always builds --features telemetry
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318   # ~/.zshrc
```

`make install` compiles the exporter in because a binary without it
ignores the endpoint in silence: `UZE_LOG` still prints spans, the
variable is still set, and nothing says the traces are going nowhere. A
plain `cargo install --path .` builds the lean binary, and so does the
release matrix.

Put the export outside any block a tool owns — UZE rewrites its own
`# >>> uze shims path >>>` block on `uze setup`, keeping only the PATH
line it wrote.

One variable covers all three entry points, because all three call the
same `telemetry::init`: the CLI, the TUI (which also writes its text to
`state/logs/uze.log`) and the shim, whose trace continues into every `uze`
the harness it launched runs. The endpoint alone switches the exporter
on — `UZE_LOG` stays a separate switch, for the text layer.

Leaving it exported is safe when Jaeger is down: the export fails on the
exporter's own thread and the command is not delayed.

## Across processes

The shim puts its span's context into the harness's environment as W3C
`TRACEPARENT`; the harness passes its environment to everything it runs,
and every `uze` started under it — `uze status` typed by the agent inside
it, `uze agent task name` on its first action — adopts that context as the
parent of its own root, so both are one trace. A delivered hook is not one
of them: it runs the generated wrapper, and no `uze` is on that path
(ADR-040). Nothing is injected without the feature: there is no trace id to
carry.

The terminal runtime server is deliberately its own root. It outlives
every client and serves several, so no one action could own it; a
request's span records which client asked.

## What a span costs

Nothing subscribed: a branch per span. Text layer: formatting on the
calling thread. OTLP: a clone of each span's fields onto the exporter's
thread. The budget tests in `crates/uze-application/src/application/
performance_tests.rs` run with no subscriber, which is the release
configuration.
