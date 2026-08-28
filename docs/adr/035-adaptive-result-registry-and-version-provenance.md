# Adaptive-result registry and version provenance as the conformance evidence-integrity contract

Status: Accepted

## Context

The Conformance Lab proves real-harness behavior, but its gate scores an
ADAPTED record (an honest vendor-limitation observation) as green. That makes
a harness losing a capability pass silently, and a scenario that was recorded
ADAPTED because the vendor channel lacked a surface can stay disabled forever
without anyone noticing the surface appeared. Harness versions are not
probed — the Lab image builds channel-latest and never records what the
channel delivered — so a vendor release that changes semantics produces an
undifferentiated run result. Absence assertions can also pass when a turn
never settles, because nothing requires the turn to have ended before
"never appeared" is concluded.

## Decision

UZE's conformance gate adopts a checked-in adaptive-result registry as its
single anti-false-positive contract: every ADAPTED result must be registered
with a reason and an observed harness-version range; an unregistered ADAPTED
fails the run; a registered ADAPTED that starts passing fails the run with an
escalation instruction until the scenario is promoted to an asserted check.
Harness versions stay channel-latest by policy, but every run records real
version provenance (harness versions, uze hash, fixture revision, image id,
timestamps) and reports version drift as an explicit event. Absence
assertions evaluate only after the turn settles and the TUI goes quiet.

## Consequences

Easier: CI catches the exact failure modes that previously passed silently —
vendor capability loss surfaces as an unregistered ADAPTED, capability
recovery surfaces as an escalation, version drift surfaces as a report event,
and unsettled turns surface as hard failures. The evidence trail is
auditable through per-run summaries uploaded as GitHub Actions artifacts
(retention-days 90; local runs keep summaries in `conformance/evidence/`
for the version-drift baseline) — revised away from in-repo CI commits
after the parallel E2E jobs raced each other on git pushes and direct
main-pushes proved fragile. Promotion of any adaptive record remains a
measured 3-consecutive-clean-run event.

Harder: maintaining the registry is a recurring cost (every new ADAPTED must
be investigated and registered with its version range); the quiescence
window adds wall-time to every turn; channel-latest runs can fail on vendor
releases whose behavior the registry does not yet cover — a deliberate
trade-off accepted so that a silent change can never pass. Pinning exact
harness versions was considered and rejected: it would protect against
drift by hiding the moving vendor surface instead of measuring it.

Source change: openspec/changes/harden-conformance-gate/