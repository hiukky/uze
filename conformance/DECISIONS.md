# Decisions taken while restructuring the Lab

Recorded because the operator was not available to arbitrate. Each is the
most conservative, most reversible option available at the time; each names
what was discarded so it can be revisited cheaply.

See `openspec/changes/assert-one-capability-contract/` for the change these
belong to.

---

## A vertical without bindings keeps running unchanged

**Context.** Adopting the contract needs every harness to grow a
`bindings.py`. Doing all four in one step makes an unreviewable diff and
risks four simultaneous regressions.

**Chosen.** `lab.load_bindings` returns `None` for a harness that has no
bindings module, and the run then behaves exactly as before. Adoption is
per harness.

**Discarded.** Requiring bindings for every harness at once (a single large
change, all-or-nothing); a feature flag (a second way to say the same thing
as "the module is absent").

---

## `Unsupported` is asked and answered, never omitted

**Context.** Codex cannot enforce a canonical `user: false` — it documents
no way to disable explicit `$skill` invocation, and the product already
routes that as `Degraded`. The contract asks every harness the same
questions, so it will ask one Codex cannot satisfy.

**Chosen.** The harness declares it through `bindings.unsupported(prop)`,
returning a reason. The run records an `adapt` result carrying that reason,
beside the passes.

**Discarded.** Omitting the check for that harness — which is precisely how
the old suite hid divergence: a check nobody wrote is a check nobody can
disagree with. Also discarded: failing the harness for a limitation the
product already reports honestly.

---

## Absence assertions are gated on a presence assertion

**Context.** `user-only-skill-hidden` passed for months while nothing at all
was delivered: "the policy worked" and "the surface was empty" are the same
observation.

**Chosen.** Every absence check in the contract runs only after a presence
check proved the surface was populated. `check_absence` already refuses an
unsettled turn (ADR-035); this adds the other half.

**Discarded.** Trusting the settled-turn contract alone — it proves the turn
finished, not that the surface has content.

---

## The driver returns the text a wait consumed

**Context.** `wait_for` consumes reads; a `collect()` afterwards sees an
empty screen. Every vertical had learned this separately and expressed it
differently, and the contract's first run failed on exactly this.

**Chosen.** `Tui.until()` returns the plain text that satisfied the wait.

**Discarded.** Re-reading after the wait (racy); having each binding
remember (which is what produced four different spellings of it).

---

## Ready markers must name the prompt, not the process

**Context.** The first Codex bindings used `("Ask Codex", "▌", "codex")`.
`"codex"` matched the onboarding splash, so `skill-tui-ready` passed against
a screen that accepts no input — the same class of false pass the audit had
just found elsewhere.

**Chosen.** `ready_markers` names the real prompt only, and a harness with a
pre-prompt flow overrides `prepare()` to drive it.

**Discarded.** Keeping a loose marker with a longer warmup (hides the
problem behind a sleep).
