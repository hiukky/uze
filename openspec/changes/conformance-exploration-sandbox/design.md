## Design

### Context

The Lab is a scripted ping: `lab.py` provisions the topology, runs the
scenario module, tears everything down. Providers already read a
`PROVIDER_MODE` env and an SSE fixture. Evidence per run is uploaded as
**Actions artifacts** (CI) or kept in the run outdir / `conformance/
evidence/` summaries (local) — never pushed to `main` from CI, after the
parallel jobs raced each other on git pushes. See proposal.md — Why.

### Goals / Non-Goals

- Goals: turn the Lab into an exploration surface (human or agent) with
  recorded evidence; make tolerance mapping and compatibility research
  first-class, versioned activities; keep the canonical gate untouched by
  exploration.
- Non-Goals: automating the agent loop itself (the operator drives);
  changing the canonical suite's gate semantics (that is
  `harden-conformance-gate`); producing product-level compatibility claims —
  matrix output is evidence, routed through the same honest classification.

### Decisions

**D1 — Sandbox as a lab mode, not a new binary.** `lab.py --sandbox <h>`:
provider + network stay up, and the operator gets a recorded PTY session —
default the harness TUI, `--shell` a rootless `sh` inside the harness
container (full fixture tree mounted, market pre-registered). Sessions stream
through `CastRecorder`; teardown on exit, `--keep` for debugging. `make
lab-sandbox HARNESS=X`. Alternatives considered: a separate persistent
compose project (rejected: drifts from the Lab's disposable topology
contract).

**D2 — Experiments are versioned scenario modules with a promotion gate.**
`conformance/experiments/<vendor>/<name>.py` implements the same
`run(cfg, prov_ip)` contract as canonical scenarios, plus an optional
`variation` override. `--experiment <vendor>/<name>` executes only it; its
verdict stays separate (`experiments/` in the outdir). Promotion = 3 clean
runs, then move into `harnesses/<vendor>/` (matching
`extend-conformance-coverage`'s coverage growth). Experiments are committed —
they are hypotheses with evidence, not scratch files.

**D3 — Variations live in a shared helper with per-provider seams.**
`shared/variation.py` implements the transport-level effects on the SSE/HTTP
stream: `slow_sse:<interval>` (chunking delay), `chopped:<n>` (truncate then
resume), `malformed:<point>` (corrupt JSON mid-stream), `disconnect_after:<n>`
(hang up mid-turn), `tool_error:<message>` (tool result carrying an error),
`duplicate:<marker>` (repeat a response). Each provider calls it at its two
choke points (response streaming, tool result delivery); providers that
cannot express a variation (e.g. Antigravity's plain HTTP) record that as the
observed tolerance. The variation string lands in the run manifest.

**D4 — Matrix runner overlays variants on the marketplace fixture.**
`conformance/matrix.py` takes a variant manifest (e.g. `hooks.json`
matcher/effect shapes, invoke-policy blocks, AGENTS.md forms) and a harness
list; for each cell it overlays the variant onto a fresh copy of
`_fixtures/marketplace/` before materialization, runs the harness's relevant
phases, and emits `matrix/<run>.json` plus a human-readable table. Cells are
independent runs (no cross-cell state); the report links evidence for every
non-passing cell. Matrix runs are on-demand/nightly — never part of the PR
gate.

**D5 — Discovery attaches on demand.** `--discovery` tells the sandbox/experiment
runner to route traffic through the mitmproxy addons (`sanitizing_addon.py`,
`replay_addon.py`) with raw captures written next to the run evidence,
gated by the same "never committed raw captures" rule as `discovery/`.

### Risks / Trade-offs

- [Interactive sessions are unasserted by nature] → they are evidence
  (casts), not checks; any finding must become an experiment before it can
  influence the gate.
- [Matrix runs are slow (variants × harnesses)] → on-demand/nightly only,
  cells run in parallel up to a small concurrency bound, report links
  evidence so partial failures remain useful.
- [Variation implementations drift per provider] → shared helper with narrow
  seams keeps the mapping table explicit; a variation that a provider cannot
  express is recorded, not faked.
- [Experiments multiply unchecked] → promotion gate + a documented lifecycle
  (propose → experiment → 3x clean → canonical); stale experiments are
  archived like changes.

### Migration Plan

Additive only: new flags, new directories, new module; no change to canonical
scenario behavior or gate exit semantics. Rollback: revert the additive
commit; canonical suite is untouched by construction.

### Open Questions

None — operator ergonomics (which TUI actions to pre-drive in sandbox mode)
are design constants deferrable to implementation.