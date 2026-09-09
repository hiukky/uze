## 1. The layer and the sinks

- [x] 1.1 `tracing` as a direct dependency of `uze-core`, `uze-git`,
      `uze-integrations`, `uze-terminal`; `tracing-subscriber`
      (`env-filter`, `fmt`) in the binary.
- [x] 1.2 `src/telemetry.rs`: `init(sink) -> Guard`; text layer from
      `UZE_LOG`; under `telemetry`, the OTLP layer from
      `OTEL_EXPORTER_OTLP_ENDPOINT`, resource `service.name = uze` with the
      version; `Guard` flushes on drop.
- [x] 1.3 Cargo feature `telemetry` with the four OTel crates;
      `make observe` starts Jaeger and prints the endpoint.

## 2. A root per action

- [x] 2.1 `main.rs`: `cli` span around `run` with the leaf command path and
      arguments; the error printed by `main` is recorded on it first.
- [x] 2.2 `ui.rs`: `tui.session`; `management::dispatch` and the workspace
      client's input handling open `tui.intent` with the intent's name.
- [x] 2.3 Every `thread::spawn` under `src/ui` captures and enters the
      current span.

## 3. Every entry point, every mechanism

- [x] 3.1 `#[tracing::instrument(skip_all, err, fields(...))]` on every
      service `pub fn` in `uze-application`; `tests/architecture` scans for
      it and fails by name.
- [x] 3.2 Call-site spans for the integration port: detect, attach,
      inspect, republish, provision, install.
- [x] 3.3 `uze-git`: a span per invocation with arguments, root, exit code.
- [x] 3.4 `uze-core`: acquisition clone, store ingest, engine compose,
      hook handler run, provisioning process, `write_atomic` (debug),
      cache hit/miss events.
- [x] 3.5 `uze-integrations::shared::process`: a span per vendor CLI run.
- [x] 3.6 `uze-terminal`: a span per client request, and around `serve`,
      `attach`, `open_space`, `stop`.
- [x] 3.7 The TUI: `tui.session`, `tui.intent` per dispatched intent,
      `tui.event` (debug) per raw event, and a named span per worker
      (`tui.git_read`, `tui.task_evaluation`, …).

## 4. Across the process boundary

- [x] 4.1 `telemetry::inject_into(&mut Command)` and `telemetry::adopt_parent()`
      (feature-gated; no-ops otherwise); the shim injects, `hook-exec`
      adopts.
- [x] 4.2 A test that a child started with the injected environment
      reports the parent's trace id (feature-gated).

## 5. Holding it and telling it

- [x] 5.1 Capturing-subscriber tests: a command's spans nest under `cli`;
      a `machine_snapshot` yields its children; a failed service call
      carries the error.
- [x] 5.2 `docs/observability.md` (purpose: how to trace uze; owner:
      whoever changes `telemetry.rs`); `AGENTS.md` names `make observe`;
      `docs/architecture/invariants.md` records the coverage rule.
