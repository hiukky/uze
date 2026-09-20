## ADDED Requirements

### Requirement: Gate on the adaptive-result registry

The conformance runner SHALL keep a checked-in registry of checks whose
adaptive result is expected. A run SHALL fail when a check records an
adaptive result absent from the registry, when a registered adaptive check
records an asserted pass, and when a registered entry does not cover the
harness version recorded by the run; in each case the verdict SHALL state the
reason with the observed behavior recorded in the evidence.

#### Scenario: Unexpected adaptive result fails the run

- **WHEN** a check records an adaptive result without a matching registry entry
- **THEN** the run exits non-zero, names the check and its suite, and records
  the observed behavior in the evidence

#### Scenario: Recovered capability escalates to an asserted check

- **WHEN** a registered adaptive check records an asserted pass
- **THEN** the run fails and instructs the maintainer to promote the scenario
  to an asserted check before the registry entry can be removed

#### Scenario: Version drift invalidates a registry entry

- **WHEN** a run's probed harness version falls outside the entry's recorded
  version range
- **THEN** the verdict reports the drift with the observed version and the
  registered range

### Requirement: Record version provenance per run

Each run SHALL record the probed harness version, the uze binary version and
hash, the fixture tree revision, the container image identifier, and run
timestamps in its run manifest. The manifest SHALL be part of the run
evidence, and a harness version change since the previous recorded run SHALL
be reported explicitly rather than silently.

#### Scenario: A completed run carries a manifest

- **WHEN** a run completes
- **THEN** its evidence records the harness version, uze binary version and
  hash, fixture revision, image identifier, and timestamps

#### Scenario: Harness version change is an explicit event

- **WHEN** the probed harness version differs from the previous recorded
  summary for that harness
- **THEN** the report marks the change as an explicit event with both
  versions

### Requirement: Assert absence only on a settled turn

A check that asserts the ABSENCE of an artifact SHALL evaluate only after the
turn's settle marker was observed and a quiescence window with no further TUI
output elapsed. When the turn did not settle, the absence check SHALL fail
and record the captured screen as evidence.

#### Scenario: Unsettled turn fails absence checks

- **WHEN** the settle marker never appears before the wait budget is spent
- **THEN** every pending absence check fails with the captured screen instead
  of passing by accident

#### Scenario: Absence asserted after quiescence

- **WHEN** the settle marker appears and the TUI stays quiet for the
  quiescence window
- **THEN** the absence check evaluates normally and passes only when the
  artifact genuinely never appeared

### Requirement: Upload per-harness evidence summaries

The runner SHALL produce a per-harness evidence summary — harness version,
uze hash, per-kind check counts, and the gate verdict — uploaded as a CI
Actions artifact beside the run evidence (local runs write into
`conformance/evidence/` for the version-drift baseline). Summaries SHALL
never be pushed to the main branch by CI (parallel E2E jobs racing git
pushes broke the original commit design; artifacts carry the same audit
trail without churn).

#### Scenario: A CI run leaves an evidence artifact

- **WHEN** a CI run completes
- **THEN** its evidence artifact contains the per-harness summary with the
  gate verdict and the recorded versions

### Requirement: Require consecutive clean runs for promotion

Promoting a formerly adaptive check to an asserted check, or otherwise
changing the adaptive-result registry, SHALL require three consecutive clean
gate runs of the affected harness. The nightly stability job SHALL run each
vertical three times and report any failure or new adaptive result as a flake.

#### Scenario: Flaky run blocks promotion

- **WHEN** any of the three consecutive runs fails or records a new adaptive
  result
- **THEN** the promoted state is refused and the flake is recorded in the
  stability report