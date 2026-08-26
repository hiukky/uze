## ADDED Requirements

### Requirement: Attachment inspection verdicts are cached on read paths
The system SHALL serve `Matched` per-receipt attachment verdicts from a
two-tier cache (in-process + on-disk under the UZE cache directory) on
read/report paths, so `doctor` and plugin managed-state reads are
milliseconds in steady state. Anomaly verdicts
(Missing/Drifted/Conflict/Blocked) SHALL NOT be cached and SHALL be
re-inspected live on every read.

#### Scenario: Warm doctor is milliseconds
- **WHEN** `doctor()` runs after a prior successful run with no mutations in between
- **THEN** no vendor CLI is spawned for `Matched` receipts

#### Scenario: Anomaly is always re-inspected
- **WHEN** a receipt inspects as `Drifted`
- **THEN** every subsequent read re-runs the live inspection for that receipt

#### Scenario: Mutation invalidates cached verdicts
- **WHEN** a plugin is installed, updated, or removed
- **THEN** previously cached verdicts are discarded and the next read re-inspects

### Requirement: Stat-able artifacts invalidate by fingerprint
The system SHALL re-check a stat-able artifact's fingerprint
(`SymlinkReference` link state + target) on every cached read, so a
hand-removed or re-pointed artifact is detected without running a vendor
CLI.

#### Scenario: Hand-removed symlink is detected
- **WHEN** a managed skill symlink is removed outside UZE
- **THEN** the next inspection reports the attachment as missing (not a stale `Matched`)

### Requirement: Read paths are the only cached ones
The system SHALL keep removal planning and detach on the live
reconciliation path; a cached `Matched` verdict SHALL NOT authorize
destroying a vendor artifact.

#### Scenario: Removal plans against live evidence
- **WHEN** `remove_plugin` plans a detach and the vendor artifact has drifted since the last cached read
- **THEN** the plan is computed from a live inspection and the removal is blocked

### Requirement: Steady-state bootstrap does not re-run vendor attach
`ensure_default_plugins` SHALL skip the vendor-CLI attach pass for an
integration that already holds the package's receipts with stat-able
artifacts still in place. A vanished stat-able artifact SHALL be
re-attached, and explicit setup/install/update SHALL keep attaching
unconditionally.

#### Scenario: Repeated invocations are quiet
- **WHEN** the TUI/CLI starts and every default plugin is already attached and in place
- **THEN** no vendor CLI (`codex plugin add`, `claude plugin …`) is spawned by the bootstrap

#### Scenario: Vanished skill link is healed
- **WHEN** a managed skill symlink is removed and the bootstrap runs
- **THEN** the symlink is re-created cheaply (no vendor CLI)
