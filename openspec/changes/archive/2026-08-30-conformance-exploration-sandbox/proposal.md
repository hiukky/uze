## Why

The Lab is scripted-only: to test a hypothesis against a real harness (e.g.
"does Codex honor a user-only skill with config shape X?"), a maintainer or
agent must write a full scenario and attach it to a canonical suite before
any evidence exists. That blocks exploratory use of the environment — the
very environment best suited to map harness tolerance empirically and find
the maximum-compatibility delivery shape across the four harnesses.

## What Changes

- Add an **interactive sandbox mode**: the disposable network, provider, and
  real harness container stay alive and reachable; the operator (human or
  agent) drives the real TUI or a shell inside it; every session is recorded
  with the same cast/timing evidence as a canonical phase, and teardown
  remains disposable.
- Add **experiment scenarios** stored outside the canonical suite
  (`conformance/experiments/`): same scenario contract, separate verdict;
  promotion into the canonical suite requires three consecutive clean runs.
- Add **adversarial provider variations** — slow/chopped streaming,
  malformed payloads, mid-turn disconnect, tool errors, duplicated responses
  — so harness tolerance to degraded paths is mapped and recorded, not
  assumed.
- Add a **cross-harness compatibility matrix runner**: a set of package /
  configuration variants × harnesses, producing a `PASS/ADAPTED/FAIL` report
  per (variant, capability, harness) cell, making compatibility trade-offs
  measured rather than assumed.
- Make the mitmproxy observation addons (`discovery/`) attachable to sandbox
  and experiment sessions for raw contract observation.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `local-real-harness-conformance`: new requirements — interactive sandbox
  mode, isolated experiment scenarios, scripted adversarial provider
  behavior, and the cross-harness compatibility matrix.

## Impact

- `conformance/lab.py` — `--sandbox`, `--experiment`, matrix entry points.
- `conformance/shared/common.py` — sandbox session plumbing; `shared/
  variation.py` (new) for adversarial provider behavior.
- `conformance/harnesses/*/provider.py` — variation seams per harness.
- `conformance/experiments/` (new) — versioned experiment scenarios.
- `conformance/matrix.py` (new) — variant overlay + matrix runner + report.
- `discovery/` — attachable observation addons.
- `Makefile`, `conformance/README.md`.