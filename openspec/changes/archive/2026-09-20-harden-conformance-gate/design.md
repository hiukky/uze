## Design

### Context

The Lab's gate semantics live in `check()` (`shared/common.py`) and the exit
rule in `lab.py` (`exit(0)` when every verdict is pass). ADAPTED shares the
green path. The Dockerfile claims a per-run harness version probe that does
not exist. Absence checks run unconditionally after a `wait_for` returns, even
when the turn never settled. Evidence is written to `AGY_OUTDIR` (ephemeral CI
artifacts; `/tmp` locally). See proposal.md — Why for the four false-positive
sources this change removes.

### Goals / Non-Goals

- Goals: make the Lab's gate free of silent green (ADAPTED, unsettled
  absence, unproven versions); give every run provenance and an auditable
  in-repo trail; make promotion a measured, 3-run event.
- Non-Goals: pinning harness versions (deliberately rejected — see ADR-035);
  expanding scenario coverage (that is `extend-conformance-coverage`);
  restructuring the provider topology or the TUI drive.

### Decisions

**D1 — The adaptive-result registry is the single anti-false-positive
contract.** A checked-in `conformance/evidence/expected.json` lists every
check that may legitimately record ADAPTED, each with `suite`, `check`,
`reason`, `harness_version_range`, `observed_at`. `check()` gains explicit
kinds: `assert`, `adapted` (unexpected → FAIL), `known_adapted`
(registered → pass), `escalated` (registered but now passing → FAIL with a
promotion instruction). Alternatives considered: warning-only logging
(rejected: cannot gate), failing on every ADAPTED forever (rejected: kills
honest vendor-limitation records). Policy formalized in ADR-035.

**D2 — Version provenance over pinning.** Keep the channel-latest policy;
add a real version probe per harness executed inside the container at run
start (`claude --version`, `codex --version`, `opencode --version`,
`ag --version`), a fallback to `unknown` (probe failure is recorded, never a
crash), and a run manifest in `verdict.json`: harness versions, `uze
--version` + binary sha256, fixture revision (repo HEAD), image id,
timestamps. Drift vs. the previous summary is reported as an explicit event.
Alternatives considered: pinning exact versions as Dockerfile build args
(rejected: loses channel coverage, high maintenance; the Lab's value is
tracking the moving vendor surface). Policy formalized in ADR-035.

**D3 — Settle-coupling via a quiescence probe.** `make_waiter` gains a
`settle_and_quiet` mode: after a marker matches, it waits until no new bytes
arrive for a window (default ~2.5s, env-overridable for debugging) and
returns the settled flag. New `check_absence(name, ok, settled, detail)`:
unsettled → FAIL with the last screen embedded in the verdict. All existing
absence checks (hooks `deny_absent`, `user-only-skill-hidden`, opencode deny
markers) migrate to it.

**D4 — Committed summaries with CI-only churn control.** `lab.py
--write-summary` writes `conformance/evidence/<harness>.json` (versions, uze
sha, per-kind counts, gate verdict). A CI step commits it (git identity
`uze-conformance`) only when the content changed, only on `main` and
nightly. Rollback: reverting the commit restores the previous summary.

**D5 — CI split: PR 1x, nightly 3x.** The PR matrix keeps one run per
vertical (fast signal). A new nightly "stability" job runs the same matrix
three consecutive times; any FAIL or new ADAPTED in the set is a flake and
fails the job with the run-by-run report. Promotion of any registry entry
requires the nightly gate to pass 3/3 first. PR runs get a `--retry-once`
flag for infrastructure flakes (recorded as a retry in the summary, never a
silent second chance on assertion failures).

### Risks / Trade-offs

- [Registry rot — entries accumulate and stop being reviewed] → the escalate
  rule (registered ADAPTED that passes fails the run) plus version ranges
  force every stale entry to surface loudly; `observed_at` must be bumped on
  any touch.
- [CI commit churn on `conformance/evidence/`] → commit only when changed,
  only on main/nightly, single-purpose bot identity.
- [Quiescence window adds ~2–3s per turn] → acceptable wall-time; window is
  env-tunable so debugging can shorten it.
- [Unsettled turns now hard-fail previously "passing" runs] → intended: the
  old behavior was a false positive; the first nightly after rollout may show
  flakes that the retry/promotion workflow absorbs.

### Migration Plan

1. Bootstrap `expected.json` from the documented latest evidence: codex
   `hooks-allow` (approval gate), antigravity `hooks-allow` + MCP capture
   ADAPTED pair, opencode MCP V2-beta ADAPTED set — each with the observed
   harness version.
2. Land gate kinds + settle-coupling behind the registry; run each vertical
   once locally; unfold any unexpected ADAPTED (investigate, don't
   register).
3. Enable CI: PR 1x gate live; nightly 3x job; evidence-commit step.
4. Rollback: revert the gate commit; registry and gate live only in
   `expected.json` + `check()` kinds, isolated from scenario logic.

### Open Questions

None — the deferrable details (exact probe flags per vendor, quiescence
length) are design constants, not spec-level decisions.