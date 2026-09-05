## Why

The Conformance Lab produces strong real-harness evidence, but its gate has
four false-positive sources: an ADAPTED result counts as green (a harness
losing a capability would pass silently), harness versions are never probed
(the Dockerfile claims a per-run version probe that does not exist), absence
assertions can pass by accident on an unsettled turn, and evidence lives only
in ephemeral CI artifacts with no auditable history in git.

## What Changes

- Add a checked-in **adaptive-result registry** (`expected.json`): an
  ADAPTED result not listed fails the run; a listed ADAPTED check that starts
  passing fails with an "escalate" verdict until the scenario is promoted to
  an asserted check; every entry records reason + observed harness version
  range.
- **Probe real harness versions** at run start (the missing R6 probe) and
  record a run manifest — harness versions, `uze --version` + binary sha,
  fixture revision, image id, timestamps — in `verdict.json`; harness version
  drift becomes an explicit report event, never a silent behavior change.
- **Settle-coupled absence assertions**: absence checks evaluate only after
  the turn settle marker and a quiescence window; an unsettled turn fails the
  check with the captured screen as evidence.
- **Committed per-harness evidence summaries** (versions, per-kind check
  counts, hashes, gate verdict) written into the repository for auditable
  history without keeping large artifacts in-tree.
- **Split CI gate**: PR runs each vertical once; a nightly stability job runs
  every vertical 3 consecutive times and reports flakes; promotion of a
  formerly adaptive check requires 3 consecutive clean runs.
- Keep the deliberate channel-latest harness policy — provenance replaces
  pinning (see ADR: adaptive-result registry and version provenance).

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `local-real-harness-conformance`: new requirements — adaptive-result gate,
  version provenance, settled-absence assertions, committed evidence
  summaries, and the 3 consecutive clean-run promotion gate.

## Impact

- `conformance/shared/common.py` — `check()` kinds, gate semantics, waiter
  quiescence, `check_absence`.
- `conformance/lab.py` — version probing, run manifest, gate exit semantics,
  summary writer.
- `conformance/harnesses/*/scenarios.py` — migration of existing absence
  checks to the settled-absence contract.
- `conformance/evidence/` — new adaptive-result registry and committed
  summaries.
- New deterministic unit tests for the gate logic (registry parsing, escalate,
  unsettled-absence).
- `.github/workflows/conformance.yml` — nightly stability job; evidence-commit step.
- `conformance/README.md`, `docs/adr/035-*.md`.