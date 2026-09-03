# Conformance asserts one outcome contract per capability; vendors bind, never assert

Status: Accepted

## Context

UZE's thesis is one sentence: a capability declared once is delivered
natively to every harness. The Harness Conformance Lab is the only place
that thesis meets a real binary, and it did not assert it. Measured across
the four verticals: 42 distinct checks, exactly three present in all four,
and all three liveness — the TUI reached a prompt, the provider saw a
request, a deterministic response rendered. 28 of 42 existed in exactly
one harness, with counts uneven for no product reason. Each vertical had
grown its own private notion of correct, and nothing contradicted it: a
hook check passed for months on an empty screen because "nothing was
listed" and "the policy worked" were the same observation.

Checks also asserted mechanism — a vendor file exists, a word appears on a
screen, a directory is symlinked. Mechanism has to diverge across
harnesses; that is the delivery precedence of ADR-013 working. Outcome
must not.

## Decision

The Lab asserts what a *user* would observe, never how a vendor arranges
it, and it does so through one contract shared by every harness.

- `conformance/contract/` states what every harness must prove, per
  capability, in outcome terms — a Skill discovered by the model produces
  X, invoked explicitly produces X, a model-only Skill is discovered but
  not user-invocable, a user-only Skill is user-invocable but not
  discovered. It names no vendor.
- `conformance/harnesses/<vendor>/bindings.py` says how that harness is
  driven — how its TUI is launched and known to be ready, how a user
  invokes a Skill there, how the catalog is read — and carries no
  assertion. `scenarios.py` keeps only what is genuinely unique to the
  vendor.
- **Unsupported is an answer.** A harness that cannot deliver part of the
  contract declares it through `bindings.unsupported` with a reason, and
  the run records it. An omission is invisible; a declaration is
  reviewable and shows up in the evidence beside the passes. This is the
  same discipline the exposure model applies to capabilities.
- **A presence check is a precondition for its matching absence check.**
  An absence only means something when a presence has proved the surface
  was populated; together with the settled-turn rule of ADR-035 this is
  what keeps a check from passing vacuously.

The contract is not a generic test runner. `check`/`check_absence` carry
semantics pytest does not — the settled-turn rule, the evidence manifest,
the adaptation adjudication the gate reads — and four scripts asserting 42
different things do not converge by being parametrised. Once
`(capability × harness)` is a real matrix, parametrisation is the natural
way to express it, and that decision can be made against something that
exists.

## Consequences

Every check gets a second opinion: the same assertion runs on four
binaries, so a vacuous pass in one is contradicted by the others. A
capability that produces different results on two harnesses is now the
product failing, which is precisely what the Lab exists to catch.

Adding a harness means writing bindings, not assertions. Adding a
capability means one contract entry, and every vertical either proves it
or declares why it cannot.

A vertical that cannot express part of the contract reads as
`Unsupported` in the report rather than as green, so the gate's numbers go
down before they go up.

Source change: openspec/changes/assert-one-capability-contract/
