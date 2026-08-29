## Why

UZE currently diagnoses attachment drift when the CLI or TUI reads health, but
does not converge recoverable UZE-owned artifacts back to their desired state.
This makes normal harness updates and missing derived artifacts look like
manual support work. Users should only see failures that require their trust,
choice, or intervention.

## What Changes

- Add a machine-level maintenance/reconciliation capability that compares the
  Store + receipt ledger desired state with each detected harness's observed
  state.
- Automatically repair only deterministic, UZE-owned, local artifacts whose
  repair cannot replace user-owned state, acquire bytes, or expand trust.
- Make maintenance available through the existing `doctor` command and TUI
  startup/refresh, without adding a user-facing `sync` command or making
  unrelated read commands mutate the environment.
- Keep `doctor` a transparent report: it includes repair outcomes and reports
  only unresolved or unsafe findings as problems.
- Separate local maintenance from marketplace update checks and from explicit
  plugin-byte updates; background maintenance never performs network access or
  installs new plugin bytes.
- Run TUI maintenance in one coalesced worker so first render remains
  responsive, show the existing header working state while it runs, and add
  concise notifications for repairs and available updates while retaining
  actionable human-decision findings.

## Capabilities

### New Capabilities

- `environment-maintenance`: Safe desired-state reconciliation, repair
  outcomes, and read-entry-point behavior for machine-level UZE environments.

### Modified Capabilities

- `doctor`: Doctor reports maintenance outcomes and only escalates findings
  that cannot safely be repaired.
- `transparent-harness-attachment`: Persistent UZE-owned attachments are
  repaired automatically when their receipt proves the repair is safe.

## Impact

Changes affect `uze-application` lifecycle/reconciliation orchestration,
receipt inspection contracts in `uze-core`, integration repair semantics, CLI
read paths, TUI worker/startup behavior, cached performance classification,
and deterministic/conformance tests. No daemon, network dependency, or new
external runtime dependency is introduced.
