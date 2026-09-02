## Outcome, not mechanism

The rule that makes the contract possible: assert what a *user* would
observe, never how a vendor arranges it.

| asserted today (mechanism) | asserted by the contract (outcome) |
|---|---|
| `openai.yaml` exists | the user-only Skill is not offered to the model |
| `"MCP"` appears on a screen | the server is connected and its tool returns X |
| the skill dir is symlinked | invoking the Skill produces X |

Mechanism has to diverge — that is `Native > Generated Native > Safe
Adaptation` working. Outcome must not: a Skill that produces different
results on two harnesses is the product failing, and that is precisely
what the Lab exists to catch.

This is also the only way a check gets a second opinion. Today a check
lives in one vertical and nothing contradicts it; the `hooks-*` vacuity
survived because no other harness ran the same assertion.

## The seam: contract vs bindings

```
contract/skill.py     invoke by model discovery → X
                      invoke explicitly (slash/$) → X
                      model-only: discovered, not user-invocable
                      user-only: user-invocable, not discovered

harnesses/<v>/bindings.py
                      how this TUI is launched and known to be ready
                      how a user invokes a Skill here
                      how the catalog is read here
                      what this harness cannot express, and why
```

The contract never contains a vendor name; the bindings never contain an
assertion. That split is what keeps a vertical from quietly growing its
own private notion of correct.

## Unsupported is an answer

A harness that cannot express part of the contract declares it, with a
reason, and the run records `Unsupported` rather than silently omitting a
check. An omission is invisible; a declaration is reviewable, and it shows
up in the evidence beside the passes.

This is the same discipline the exposure model already applies to
capabilities — `Unsupported` with a rationale is a route, not a gap.

## Why the assertions survive an empty screen

Every presence check is a precondition for its matching absence check.
`user-only-skill-hidden` may not pass unless the *default* Skill was seen
first — otherwise "nothing was listed" and "the policy worked" are the same
observation, which is the bug that shipped for months.

`check_absence` already refuses to pass on an unsettled turn (ADR-035).
The contract adds the other half: an absence is only meaningful when
something present proves the surface was actually populated.

## Why not pytest

The gap is a contract, not a runner. Four scripts asserting 42 different
things do not converge by being parametrised. And `check`/`check_absence`
carry semantics a generic runner does not have — the settled-turn rule, the
evidence manifest, the adaptation adjudication the gate reads.

Once `(capability × harness)` is a real matrix, parametrisation is the
natural way to express it, and the decision can be made against something
that exists.

## A vertical for UZE itself

The Lab drives four harness TUIs and never drives UZE's. That is backwards
for a product whose own TUI is where an operator spends the day.

The view-model split makes most of it cheap without a container: an
extension answers with data, the host renders, so a `View` snapshot is a
deterministic unit test. The Lab vertical covers only what needs the real
thing — launching an agent, isolating its checkout, surviving a client
reattach, delivering project context to a harness that then reads it.
