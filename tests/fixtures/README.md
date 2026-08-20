# Test fixtures

These are immutable source inputs for deterministic and opt-in conformance
tests. They are not `$UZE_HOME`: each test creates an isolated `UzeHome` and
installs package fixtures into its temporary Store.

- `projects/`: project-owned resources used by composition and CLI tests.
- `packages/`: external standard package inputs installed through `UzeStore`.
- `native-harness/`: intentional harness-layout fixtures used only to measure
  native discovery without UZE.

The UZE integration conformance path starts from `packages/`, installs once,
and then composes the stored package with a clean caller project.
