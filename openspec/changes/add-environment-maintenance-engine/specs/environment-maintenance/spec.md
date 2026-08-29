## Purpose

Keeps the machine-level UZE environment converged without turning ordinary
recoverable drift into manual work or silently replacing user-owned state.

## ADDED Requirements

### Requirement: Maintenance converges only safe local desired state
The system SHALL compare the Store and receipt ledger desired state with live
integration inspection, then repair a finding only when the repair is local,
deterministic, UZE-owned, and requires neither network access nor a new trust
decision. It SHALL publish derived views and recreate missing receipt-owned
artifacts when those conditions hold.

#### Scenario: Missing UZE-owned artifact is restored
- **WHEN** a receipt proves that a UZE-owned derived artifact is missing
- **THEN** maintenance recreates it from the Store and receipt without user action

#### Scenario: Maintenance has nothing to repair
- **WHEN** all detected integrations match the desired state
- **THEN** maintenance completes without invoking a vendor attach operation

### Requirement: Maintenance preserves unsafe external divergence
The system SHALL NOT overwrite, delete, or replace an artifact that is
drifted, conflicted, blocked, user-owned, or cannot be proven safe to repair.
It SHALL report the precise unresolved reason and an explicit next action.

#### Scenario: Marketplace root differs from its receipt
- **WHEN** an integration reports that a native marketplace root differs from
  the receipt's root
- **THEN** maintenance preserves the current root and reports that human
  confirmation is required

### Requirement: Existing external native delivery is non-failing
When a harness reports that a package-native plugin with the requested name
is already installed but UZE has no receipt for it, the system SHALL preserve
that external installation and treat the package-native delivery as already
available. It SHALL NOT uninstall, overwrite, claim ownership, attach
duplicate capability fallbacks, or surface the condition as a setup warning
or doctor problem.

#### Scenario: Native plugin name is already imported
- **WHEN** a harness already imports a plugin named `git` and UZE installs a
  package whose native plugin name is `git`
- **THEN** UZE performs no destructive mutation and setup completes without a
  warning for that harness

### Requirement: Maintenance never updates plugin bytes
Background maintenance SHALL NOT acquire marketplace sources, contact the
network, install a new plugin revision, or authorize executable capability
changes. Update discovery and update installation remain separate operations.

#### Scenario: Marketplace update is available
- **WHEN** cached or explicitly refreshed marketplace metadata reports a newer plugin revision
- **THEN** maintenance leaves the installed bytes unchanged and reports an available update

### Requirement: Doctor and TUI invoke bounded maintenance
The existing CLI `doctor` command and the TUI startup and refresh paths SHALL
invoke bounded maintenance before presenting their completed health report.
The operation SHALL use cached detection and inspection where valid and SHALL
not require a resident daemon. The system SHALL NOT add a user-facing sync
command or cause unrelated read commands to mutate the environment.

#### Scenario: TUI opens after a recoverable interruption
- **WHEN** the TUI starts with a missing UZE-owned attachment
- **THEN** the attachment is restored before the refreshed health report is shown

#### Scenario: Doctor is run on a healthy environment
- **WHEN** the CLI doctor command runs with no maintenance work due
- **THEN** it completes within its existing budgeted-command classification

### Requirement: TUI maintenance does not block rendering
The TUI SHALL execute maintenance outside its rendering and event loop. It
SHALL render available data immediately, keep at most one maintenance run in
flight, and coalesce refresh requests received while that run is outstanding.
While maintenance is in flight, the header SHALL use the existing working
status presentation (`Refreshing environment…` at the time of this change).

#### Scenario: TUI opens with maintenance work due
- **WHEN** the TUI starts and bounded maintenance has work to perform
- **THEN** the TUI renders without waiting for the maintenance result and the
  header shows its working status until the consolidated refresh completes

#### Scenario: Repeated refreshes do not duplicate maintenance
- **WHEN** a user requests refresh while a TUI maintenance run is already in flight
- **THEN** the TUI coalesces the request and does not start a second concurrent
  inspection or repair pass
