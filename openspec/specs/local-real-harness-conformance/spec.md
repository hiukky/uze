# local-real-harness-conformance Specification

## Purpose
TBD - created by archiving change conformance-exploration-sandbox. Update Purpose after archive.
## Requirements
### Requirement: Provide an interactive sandbox mode

The Lab SHALL provide a sandbox mode that keeps the disposable network, the
synthetic provider, and the real harness container alive and interactive
after provisioning, so an agent or maintainer can explore harness behavior
and test hypotheses against the real binary without writing a scenario
first. Sandbox sessions SHALL be recorded with the same cast/timing evidence
as canonical phases, and the topology SHALL be torn down on exit.

#### Scenario: A sandbox session is recorded

- **WHEN** a sandbox session ends
- **THEN** a cast/timing recording of the session exists in the run evidence
  directory

#### Scenario: Sandbox topology is disposable

- **WHEN** the sandbox exits normally or is interrupted
- **THEN** the provider container and network are removed

### Requirement: Isolate experiment scenarios from the canonical suite

The Lab SHALL support experiment scenarios stored outside the canonical
per-harness suite, runnable by name with a verdict separate from the
canonical gate. An experiment SHALL be promoted into the canonical suite
only after three consecutive clean runs of the affected harness.

#### Scenario: An experiment runs without touching the canonical suite

- **WHEN** an experiment scenario is selected by name
- **THEN** only that scenario executes and its verdict is recorded separately
  from the canonical gate verdict

#### Scenario: Promotion requires three consecutive clean runs

- **WHEN** an experiment passes three consecutive clean runs
- **THEN** it may be promoted into the canonical suite; otherwise it remains
  an experiment

### Requirement: Script adversarial provider behavior

The synthetic provider SHALL support scripted variation modes that exercise
degraded paths — slow or chopped streaming, malformed payloads, mid-turn
disconnect, tool errors, and duplicated responses. Each run SHALL record the
variation applied and the observed harness tolerance in its verdict.

#### Scenario: An adversarial variation is scripted

- **WHEN** a scenario or sandbox run selects a variation mode
- **THEN** the provider serves the degraded behavior and the verdict records
  the variation and the harness's observed tolerance

### Requirement: Produce a cross-harness compatibility matrix

The Lab SHALL support matrix runs over a set of package/configuration
variants and harnesses, producing a report of `PASS`/`ADAPTED`/`FAIL` per
(variant, capability, harness) cell with an evidence link, so compatibility
trade-offs across harnesses are measured rather than assumed.

#### Scenario: A matrix run reports every cell

- **WHEN** a matrix run completes
- **THEN** the report lists every variant × harness cell with its verdict and
  an evidence link for non-passing cells

### Requirement: The Antigravity vertical runs the harness signed in
The Antigravity vertical SHALL run the real harness in its signed-in mode against a synthetic identity and synthetic CloudCode endpoints served by the Lab's provider, with zero Internet, so that the harness executes `hooks.json` hooks as it does for a real signed-in account. The identity, token and every served payload SHALL be synthetic and SHALL carry no real account data.

#### Scenario: Vendor control hook executes
- **WHEN** the vertical serves a vendor-format deny hook at the vendor's shared `hooks.json` and scripts a `run_command` call in the signed-in session
- **THEN** the harness denies the command with the hook's reason before any permission prompt
- **AND** `hooks > vendor` passes, so the mode in which the vendor executes hooks is proven rather than assumed

#### Scenario: Delivery is a second live precondition
- **WHEN** the vertical starts a session with UZE's hook package installed
- **THEN** it records from the harness's own log how many `hooks.json` files the harness read
- **AND** `hooks > delivery` passes only if the harness loaded the hooks UZE delivered; while it does not, the UZE hook checks are declared against that measurement, never against a stale reason and never silently dropped

#### Scenario: UZE's own hooks are asserted
- **WHEN** the vertical scripts the intercepted tool against UZE's delivered hook groups in the signed-in session
- **THEN** the denial reason reaches the conversation, the intercepted tool never executes, the handler's portable vocabulary (`tool=shell`) is relayed from the harness's own payload, a second handler after a denial never runs, and an allowed call executes

#### Scenario: Model calls go through the signed-in protocol
- **WHEN** the harness sends `v1internal:streamGenerateContent`
- **THEN** the provider answers with the same deterministic content it serves in API-key mode, wrapped in the signed-in response envelope
- **AND** every existing Antigravity check (skills, MCP, TUI) keeps its verdict

#### Scenario: API-key mode stays visible
- **WHEN** the vertical completes
- **THEN** one declared check records that hooks do not execute under an API key, citing the vendor issue, without gating the vertical
