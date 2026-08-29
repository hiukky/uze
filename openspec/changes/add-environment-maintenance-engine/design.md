## Context

See proposal.md. TUI startup currently reads `doctor()` before and after
default seeding; CLI doctor is read-only. `install_materialized` already
contains the safe ordering `prepare → ingest → republish → attach`, but there
is no equivalent convergence pass for existing Store state. Receipts are the
ownership boundary and inspection already distinguishes Matched, Missing,
Drifted, Conflict, and Blocked.

## Goals / Non-Goals

**Goals:**

- Converge safe machine-level artifacts through `doctor` and TUI entry points.
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
and the existing CLI `doctor` command. It has one bounded pass over stored
plugins and detected integrations, coalescing work by integration. This
adopts the desired-versus-observed reconciliation pattern used by Kubernetes
sync loops, but deliberately runs on process entry because UZE is a CLI, not
a resident controller. It preserves `doctor` as an observation/report surface
rather than embedding repair decisions in the TUI. Other read commands remain
read-only; there is no new `sync` command.

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

### Externally present native packages are available, not failed

A native harness may report a package under the desired name before UZE has a
receipt for it. This is not a UZE-managed divergence and must not trigger
uninstall or overwrite. The integration returns a successful no-op, and the
Application treats the native plan as providing its declared resources so it
does not add duplicate capability fallbacks. This is intentionally silent in
setup and doctor; it makes no ownership claim and applies to every package,
not only UZE's own plugin.

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

### Responsive TUI execution and status

The TUI schedules maintenance on its existing worker path; it never performs
maintenance on the rendering/event loop. It renders available model data
immediately, keeps one maintenance job in flight, and coalesces startup or
manual-refresh requests that arrive while that job is running. While the job
is outstanding, the header uses the existing `Status::Working` presentation
(currently `Refreshing environment…`); it returns to the normal success or
idle presentation only after the refreshed report is ready. The worker emits
one consolidated report, so a burst of refreshes cannot spawn duplicate
inspection or repair work.

The CLI `doctor` runs the same bounded use case synchronously because its
output is the completed report. Its healthy warm path remains cache-backed;
maintenance work is admitted only for actionable anomalies or stale,
UZE-owned derived views. No background worker survives after the TUI process
exits, and no maintenance step may perform network I/O.

### Architecture impact

The Application gains an orchestration component but dependency direction is
unchanged: it consumes Core Store/receipts and IntegrationPort inspection/
repair contracts. No container or external dependency is added, so the LikeC4
model remains accurate without modification. This is architecturally
significant; create the ADR artifact for the reconciliation policy.

## Risks / Trade-offs

- [A vendor CLI call makes launch slow] → render before worker completion,
  cached detection, one coalesced maintenance job, an anomaly-only admission
  policy, a per-entry budget, and tests that reject unclassified CLI cost.
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
