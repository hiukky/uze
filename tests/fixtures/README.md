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

Opt-in real-harness probes use inexpensive defaults and accept explicit
overrides: `UZE_E2E_CLAUDE_MODEL=haiku`,
`UZE_E2E_CODEX_MODEL=gpt-5.6-luna`, and
`UZE_E2E_OPENCODE_MODEL=opencode/deepseek-v4-flash-free`.
