## MODIFIED Requirements

### Requirement: Attachment inspection verdicts are cached on read paths
The system SHALL serve `Matched` per-receipt attachment verdicts from a
two-tier cache (in-process + on-disk under the UZE cache directory) on
read/report paths, so `doctor` and plugin managed-state reads are
milliseconds in steady state. Anomaly verdicts
(Missing/Drifted/Conflict/Blocked) SHALL NOT be cached and SHALL be
re-inspected live on every read. A read entry point that invokes maintenance
SHALL report its repair outcomes separately from unresolved anomalies.

#### Scenario: Warm doctor is milliseconds
- **WHEN** `doctor()` runs after a prior successful run with no mutations in between
- **THEN** no vendor CLI is spawned for `Matched` receipts

#### Scenario: Anomaly is always re-inspected
- **WHEN** a receipt inspects as `Drifted`
- **THEN** every subsequent read re-runs the live inspection for that receipt

#### Scenario: Recoverable anomaly is repaired before report
- **WHEN** maintenance recreates a missing UZE-owned artifact before doctor renders
- **THEN** doctor reports the repair outcome and does not classify that artifact as unresolved drift

#### Scenario: Mutation invalidates cached verdicts
- **WHEN** a plugin is installed, updated, or removed
- **THEN** previously cached verdicts are discarded and the next read re-inspects
