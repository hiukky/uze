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
| `cli` | every `uze <command>` | the leaf command (`agent context inspect`) and the argument line |
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
- `acquisition.git` per clone step, `vendor.cli` per vendor CLI the
  integrations run, `process.run` per provisioning process, `hook.handler`
  per hook handler, `store.ingest`, `marketplace.clone`;
- at debug level: `git` per Git invocation through `uze-git` (arguments,
  exit code), the TUI's timer-driven refreshes (`tui.git_read`,
  `tui.task_evaluation`, `tui.occupancy_reconcile`, `tui.support_refresh`,
  `tui.conversation_refresh`, `tui.code_*`, `tui.architect_artifacts`,
  `tui.preserved_sweep`), `engine.package_resources`, `persistence.write`,
  and the detection cache's hit and miss events.

**What decides the level is who asked, not what it cost.** A span a
*person* opened — a gesture, a command, a placement, a delivery — is
`info`; a span a *timer* opened is `debug`. The TUI refreshes its badge
every 750 ms and evaluates its tasks every 20 s, so at `info` those two
alone wrote four fifths of a 64 MB day and buried the gesture that a
report is actually about. The operation's own span still carries what it
cost, which is the number an operator reads; `UZE_LOG=uze_git=debug`
brings every invocation back.

A worker thread in the TUI enters the span that started it, so a refresh
is a child of the key that asked for it. `tests/architecture/
instrumentation.rs` fails by name for an application entry point without
a span.

## The journal

The two processes that outlive the gesture that started them keep one
without being asked:

```
~/.uze/cache/logs/uze.<date>.log         # the TUI
~/.uze/cache/logs/terminal.<date>.log    # the terminal server
```

Rolled daily and pruned to seven days, at `info` — every action, every
integration call, and every failure. Written from a thread of its own
(`tracing-appender`'s non-blocking writer, never lossy), so a render loop
never waits on a disk, and flushed by the `Telemetry` guard when the
process ends.

Seven days is the rule; 64 MB is the floor under it. Days alone bound the
history by a number nobody checks — how much a day weighs is set by how
the code happens to be instrumented — so the oldest days are also dropped
until the history fits in the ceiling. The newest is never one of them: a
single day over it is reported, never truncated, because cutting the run
the journal is about loses exactly the lines worth having.

It is on by default because the run worth reading is the one nobody
expected to have to read: a switch turned on after the fact is a switch
that was off when it mattered. It sits under `cache/` because that is the
tier it belongs to — deleting it costs nothing.

Neither process could use stderr anyway: it is the screen the TUI draws
on, and `/dev/null` for the server, which is started detached.

## Reading it as text

```sh
UZE_LOG=info uze status            # spans and events on stderr
UZE_LOG=uze_git=debug uze status   # tracing_subscriber filter syntax
tail -f ~/.uze/cache/logs/uze.*.log
```

`UZE_LOG` is both the switch and the filter: it is what puts a *command's*
text on stderr, and it raises or narrows the journal's own level along
with the exporter's. For a one-shot command with it unset, the only thing
subscribed is the step layer below, filtered to its own target, so every
other span costs a branch.

## Steps: what a person watching a command sees

An operation says what it is doing as `info` events on the `uze::step`
target — a `step` field and its subject (`step="reach"`, `repository`,
`via`; `step="download"`, `files`; `step="deliver"`, `harness`; …), never a
sentence. `src/steps.rs` carries them, on a layer of their own that is
always installed, to the one listener the running surface registers: the
CLI puts each on its spinner line and leaves every finished one above it,
checked, with how long it took; `--verbose` adds every failed attempt with
why.
The words live beside the spinner (`src/progress.rs`); a new step is an
event in the domain and a line in `describe`.

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

One variable covers all four entry points, because all four call the same
`telemetry::init`: the CLI, the TUI and the terminal server (which journal
as well), and the shim, whose trace continues into every `uze` the harness
it launched runs. The endpoint alone switches the exporter
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

Nothing subscribed — every one-shot command, unless asked: a branch per
span. Journal: formatting on the calling thread, and a move onto the
writer's. OTLP: a clone of each span's fields onto the exporter's
thread. The budget tests in `crates/uze-application/src/application/
tests/performance.rs` run with no subscriber, which is the release
configuration.
