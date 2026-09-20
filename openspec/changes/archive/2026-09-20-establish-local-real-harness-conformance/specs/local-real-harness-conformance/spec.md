## ADDED Requirements

### Requirement: Preserve tier separation

The system SHALL keep the deterministic Rust suites (L0 unit, L1 component
contract, L3 acceptance) runnable without a Docker daemon, a model, or a
provider account. Real-harness conformance SHALL be a separate L2 tier that
is never linked into the deterministic suite, and model-invocation behavior
SHALL be an L4 tier that never gates CI.

#### Scenario: Default Rust test invocation

- **WHEN** a developer runs `cargo test`
- **THEN** no Docker daemon, model, or external provider account is
  required.

### Requirement: Run actual harnesses in an isolated, credential-free world

The L2 lab SHALL execute the real UZE binary and the real vendor harness
binary in a container with an empty HOME, UZE_HOME, and project directory.
It SHALL NOT mount the host HOME or the Docker socket, and SHALL NOT reach
the external Internet. The harness SHALL speak to a synthetic provider
serving that vendor's real wire protocol, so that a run requires no provider
credential and no token spend.

#### Scenario: Fresh conformance run

- **WHEN** the developer starts an L2 run for a harness
- **THEN** UZE installs the fixture once into that run's Store and the real
  harness runs from the isolated environment against the synthetic provider.

#### Scenario: No credential is required to reproduce a run

- **WHEN** a run is started on a machine holding no vendor account or
  provider key
- **THEN** the run completes and produces the same evidence.

### Requirement: Report layered evidence

The L2 runner SHALL record package identity, resource identities, stored
paths, exposure strategies, harness version, and fixture and image
provenance. It SHALL distinguish attachment, discovery, behavior,
environment block, timeout, harness failure, and declared vendor
limitation as independent results.

#### Scenario: A harness cannot deliver part of the contract

- **WHEN** a vendor offers no surface for a capability the contract covers
- **THEN** the run records a declared limitation with its reason, rather
  than omitting the check or reporting a product incompatibility.
