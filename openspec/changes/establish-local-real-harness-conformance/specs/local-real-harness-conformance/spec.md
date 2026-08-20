## ADDED Requirements

### Requirement: Preserve tier separation

The system SHALL keep L0/L1 Rust tests runnable without Docker, model files,
or provider credentials. Isolated real-harness behavioral tests SHALL be opt-in
L2 tooling, and vendor-provider behavioral tests SHALL remain opt-in L3.

#### Scenario: Default Rust test invocation

- **WHEN** a developer runs `cargo test`
- **THEN** no Docker daemon, model file, or external provider account is
  required.

### Requirement: Run actual harnesses in isolated L2 environments

The L2 lab SHALL execute the real UZE binary and real Claude Code, Codex, or
OpenCode CLI in a container with an empty HOME, UZE_HOME, and project directory.
It SHALL not mount the host HOME or Docker socket. When a routed provider is
used, provider credentials SHALL be available only to the gateway service, not
the harness service.

#### Scenario: Fresh local conformance run

- **WHEN** the developer starts an L2 run for a harness
- **THEN** UZE installs the fixture once into that run's Store and the selected
  real harness runs headlessly from the isolated environment.

### Requirement: Report layered evidence

The L2 runner SHALL record package identity, resource identities, stored paths,
exposure strategies, harness version, inference route, model identity, and
model version or hash when available. It SHALL distinguish attachment,
discovery, routed/local behavior, environment block, timeout, harness failure,
and model failure.

#### Scenario: Model does not use an available capability

- **WHEN** a routed or local model fails to exercise a discovered capability
- **THEN** the report marks behavioral/model evidence as failed without
  changing the integration compatibility classification.
