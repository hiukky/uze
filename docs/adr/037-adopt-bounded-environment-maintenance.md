# Adopt bounded environment maintenance

Status: Accepted

## Context

Receipts let UZE detect whether its machine-level desired state diverges from
what harnesses currently expose, but a read-only doctor leaves recoverable
UZE-owned artifacts broken until a user discovers and manually chooses an
update. A permanent daemon would add a new operational system to a CLI and
continuous polling/network access would conflict with UZE's performance and
trust boundaries.

## Decision

UZE will run a bounded, application-owned maintenance reconciliation through
the existing CLI `doctor` command and TUI startup/refresh. It compares desired
Store + receipt state to live observed integration state and repairs only
deterministic local artifacts that are proven UZE-owned. It will preserve
Drifted, Conflict, and Blocked state unless an integration supplies stronger
receipt-backed proof that repair is non-destructive.

Maintenance, update discovery, and plugin-byte installation are separate
lanes. There is no persistent daemon or user-facing sync command. TUI
maintenance runs in its worker rather than the rendering/event loop, with one
coalesced run in flight and the existing working header status displayed until
the report is ready. A daemon was rejected as disproportionate operational
complexity; blind reattachment was rejected because it can overwrite a
human's external configuration.

## Consequences

Recoverable disruptions heal when users enter `doctor` or the TUI, and both
presenters use the same policy rather than embedding repair rules in
presentation. Doctor can focus on unresolved decisions while TUI remains
responsive during maintenance. Startup work is bounded and must remain within
explicit command performance budgets. Users will not receive instant repairs
while no UZE process is running, and an external divergence will still
deliberately require intervention.

Source change: openspec/changes/add-environment-maintenance-engine/
