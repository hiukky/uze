## ADDED Requirements

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