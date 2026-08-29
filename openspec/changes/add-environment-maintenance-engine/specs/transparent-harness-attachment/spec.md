## MODIFIED Requirements

### Requirement: Attachment is a persistent, UZE-managed, user-scope reference
A transparent attachment SHALL be a reference (such as a filesystem symlink)
UZE creates under the harness's own user-scope discovery location, pointing
at the package's content inside the UZE store, rather than a copy of that
content. UZE SHALL own the reference's lifecycle: it MAY refresh it when the
store package changes, SHALL recreate it during safe maintenance when its
receipt proves it is missing, and SHALL remove it on `uze remove`/uninstall.
UZE SHALL NOT duplicate the store's package content as a second permanent
installation or overwrite a divergent artifact without an explicit operation.

#### Scenario: Store update is reflected without a rewrite
- **WHEN** a UZE-managed attachment references a package already installed in
  the UZE store
- **AND WHEN** that store package's content changes through UZE
- **THEN** the harness resolves the updated content without UZE recreating
  the attachment

#### Scenario: Missing managed reference is restored
- **WHEN** a receipt-owned user-scope reference is missing during maintenance
- **THEN** UZE recreates the reference from the Store and receipt

#### Scenario: Divergent managed reference is preserved
- **WHEN** a receipt-owned user-scope reference points somewhere other than
  its recorded target
- **THEN** UZE does not overwrite it during background maintenance
- **AND THEN** it reports a human-actionable divergence

#### Scenario: Removing a package removes its attachment
- **WHEN** a package with an active transparent attachment is removed from
  the UZE store
- **THEN** UZE removes the managed reference for every harness that had it
- **AND THEN** no dangling reference is left in any harness's discovery
*** Add File: /home/hiukky/uze/openspec/changes/add-environment-maintenance-engine/design.md
## Context

See proposal.md. TUI startup currently reads `doctor()` before and after
default seeding; CLI doctor is read-only. `install_materialized` already
contains the safe ordering `prepare → ingest → republish → attach`, but there
is no equivalent convergence pass for existing Store state. Receipts are the
ownership boundary and inspection already distinguishes Matched, Missing,
Drifted, Conflict, and Blocked.

## Goals / Non-Goals

**Goals:**

- Converge safe machine-level artifacts at normal CLI/TUI entry points.
- Keep health reporting honest while reducing repairable noise.
- Preserve the existing no-daemon CLI architecture and command performance.

**Non-Goals:**

- No persistent daemon, filesystem watcher, polling loop, or background
  network activity after the process exits.
- No automatic plugin-byte update, trust grant, destructive overwrite, or
  vendor executable provisioning.
- No replacement for explicit `plugin update`, setup, or remove workflows.

## Decisions

### A bounded reconciliation pass, not a daemon

Add an Application-owned maintenance use case invoked by TUI startup/refresh
and by CLI report paths that opt in. It has one bounded pass over stored
plugins and detected integrations, coalescing work by integration. This
adopts the desired-versus-observed reconciliation pattern used by Kubernetes
sync loops, but deliberately runs on process entry because UZE is a CLI, not
a resident controller. It preserves `doctor` as an observation/report surface
rather than embedding repair decisions in the TUI.

### Explicit repair classification

Inspection findings produce one of: `AlreadyConverged`, `Repaired`,
`UpdateAvailable`, `NeedsHumanAction`, or `Unavailable`. Only `Missing` and
known UZE-owned derived views enter repair planning; `Drifted`, `Conflict`,
and `Blocked` are terminal for automatic maintenance unless an integration
offers a stronger, receipt-backed proof that restoration cannot overwrite
external state. The plan is computed before writes, then executed under the
existing mutation lock, and re-inspected for the report.

This favors conservative ownership over a generic “re-run update” action.
The alternative — blindly reattach every non-Matched receipt — would erase a
user's marketplace/root choice and violates receipt-driven lifecycle safety.

### Three independent maintenance lanes

1. **Local convergence**: Store/receipts/derived artifacts only; automatic.
2. **Update discovery**: cached marketplace metadata, refreshed only by an
   explicit check policy; produces a notification, never byte mutation.
3. **Update installation**: existing explicit lifecycle and trust boundary.

This follows mature extension/dependency managers: background checks can
discover availability, while installation remains policy-controlled. No new
network dependency or scheduler is added in this change.

### One operation, multiple presenters

`UzeApplication` owns maintenance and returns a typed report. CLI doctor
renders its outcomes and unresolved findings; TUI worker invokes it before
refreshing model data, turns `Repaired`/`UpdateAvailable` into transient
notifications, and renders only `NeedsHumanAction` as a problem. This keeps
the same behavior available outside the TUI without a redundant `sync`
command.

### Architecture impact

The Application gains an orchestration component but dependency direction is
unchanged: it consumes Core Store/receipts and IntegrationPort inspection/
repair contracts. No container or external dependency is added, so the LikeC4
model remains accurate without modification. This is architecturally
significant; create the ADR artifact for the reconciliation policy.

## Risks / Trade-offs

- [A vendor CLI call makes launch slow] → cached detection, coalesced
  integration work, a per-entry budget, and tests that reject unclassified
  CLI cost.
- [A repair masks external change] → allowlist only receipt-provable missing
  or derived UZE-owned artifacts; re-inspect every repaired target.
- [A failed repair leaves partial state] → each repair is idempotent, reports
  its outcome, and never hides subsequent inspection failure.
- [Update notices become noisy] → cache timestamped discovery and display
  notifications only on a state transition.

## Migration Plan

Introduce maintenance as no-op-safe. Existing receipts retain their current
inspection semantics; an artifact is repaired only when the new planner can
prove safe ownership. Rollback is removing the entry-point invocation; no
state migration is required.
