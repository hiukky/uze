## ADDED Requirements

### Requirement: Every action is one trace
Every action a person takes — a CLI command, a key or click in the TUI —
SHALL open a root span that every span of the work it causes nests under,
including work on worker threads. A span SHALL carry the identifiers that
name what it acts on (command, plugin id, marketplace, project root,
integration) and its duration; a span whose operation fails SHALL record
the error.

#### Scenario: A command is a tree
- **WHEN** a CLI command runs with tracing subscribed
- **THEN** one root span named for the command wraps every span the
  command produced, and no span produced by the command lies outside it

#### Scenario: A TUI refresh belongs to the key that asked for it
- **WHEN** a key or click in the TUI starts work on a worker thread
- **THEN** the worker's spans nest under the intent's span, not under a
  root of their own

#### Scenario: A failure is on the span
- **WHEN** an application entry point returns an error
- **THEN** its span records the error message at error level

### Requirement: Every application entry point is instrumented
Every public function of the application facade's services SHALL be
instrumented as a span, and the test suite SHALL fail, naming the
function, when one is not.

#### Scenario: A new entry point without a span fails the suite
- **WHEN** a `pub fn` is added to an application service module without
  instrumentation
- **THEN** the architecture suite fails and names it

### Requirement: Every process UZE runs is a span
Each Git invocation, acquisition clone, vendor CLI run, provisioning
process, hook handler and terminal-runtime request SHALL be a span
carrying what was run and how it ended.

#### Scenario: A Git invocation is visible
- **WHEN** `uze-git` runs Git
- **THEN** a span records the arguments, the working tree and the exit
  code

### Requirement: A trace continues into what the harness runs
When a harness launched through UZE's runtime shim runs `uze` — a hook UZE
dispatches, or a command an agent types — that invocation's root span
SHALL be a child of the shim's root span, carried across the harness
process through `TRACEPARENT`.

#### Scenario: A hook joins the launch that caused it
- **WHEN** telemetry is enabled and a harness launched by the shim runs
  `uze hook-exec`
- **THEN** the hook dispatch's root span has the shim's span as its parent

#### Scenario: Without telemetry nothing is carried
- **WHEN** the binary was built without the `telemetry` feature
- **THEN** the shim injects no `TRACEPARENT` and a `uze` started under
  the harness opens a root of its own

### Requirement: Traces can be read as text or on a local dashboard
Setting `UZE_LOG` SHALL write spans and events as text — to stderr for a
command, to a file under `UZE_HOME`'s logs for the TUI. A binary built
with the `telemetry` feature SHALL export every span over OTLP to the
endpoint `OTEL_EXPORTER_OTLP_ENDPOINT` names, flushing before the process
exits; without the feature or the endpoint, tracing SHALL cost nothing
observable and add no dependency to the release.

#### Scenario: A short command's trace is not lost
- **WHEN** a command that completes in milliseconds runs with an OTLP
  endpoint configured
- **THEN** its complete trace reaches the collector before the process
  exits

#### Scenario: The release carries no telemetry stack
- **WHEN** the binary is built without the `telemetry` feature
- **THEN** no OpenTelemetry crate is linked and spans are inert
