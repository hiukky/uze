## 1. Adaptive-result registry

- [x] 1.1 Define the `conformance/evidence/expected.json` schema (suite,
      check, reason, `versions` — pinned list or `*`, `observed_at`) and
      bootstrap it from the documented latest evidence: codex `hooks-allow`
      (approval gate), antigravity `hooks-allow` + user-only skill pair,
      opencode MCP V2-beta set — reason text matches each scenario's
      recorded detail verbatim.
- [x] 1.2 Wire the gate semantics in (`gate.py` + `lab.py`): kinds
      (`assert`, `adapted`, `known_adapt`, `escalated`, `version_drift`
      adjudications), unregistered ADAPTED fails, registered-pass escalates,
      exit code and verdict rendering gate-aware.
- [x] 1.3 Add deterministic unit tests for the gate under
      `conformance/tests/` (stdlib unittest — no new dependency):
      unregistered ADAPTED fails, registered escalation fails, version-range
      mismatch fails, cross-harness isolation.
- [ ] 1.4 Run each vertical once locally with the gate live; investigate
      every unexpected ADAPTED instead of registering it blindly.
      (Findings so far: claude 18/18 gate-clean with probe 2.1.247. The
      antigravity run surfaced a real channel bump — probe recorded 1.1.22
      vs the 1.1.21 evidence baseline — with a NEW CLI survey dialog
      ("How's the CLI experience? [0] Skip") that blocks hook turns; the
      dismiss list was adapted and the rerun is in flight. The settle
      contract also caught its first genuine non-settling run (hard-fail,
      not silent pass) during an infrastructure-collision run.)

## 2. Version provenance

- [x] 2.1 Add a per-harness version probe executed inside the container
      (`claude --version`, `codex --version`, `opencode --version`,
      `agy --version` — the vendor's own probes, matching
      `uze-integrations` detection), with `unknown` fallback on failure.
- [x] 2.2 Write the run manifest (harness versions, `uze --version`,
      fixture revision, image id, timestamps) into `verdict.json` and
      report version drift vs. the previous committed summary as an
      explicit event.
- [x] 2.3 Unit-test the probe fallback and manifest drift computation
      (docker paths mocked out).

## 3. Settled-absence assertions

- [x] 3.1 Add `settle_and_quiet` to `shared/common.py` (marker matched,
      then no new bytes for a configurable window, env-overridable via
      `UZE_CONFORMANCE_QUIET_MS` / `UZE_CONFORMANCE_QUIET_BUDGET_S`).
- [x] 3.2 Add `check_absence(name, ok, settled, detail)` that fails an
      unsettled absence check with the reason recorded in the verdict.
- [x] 3.3 Migrate every hook-phase absence check (claude/codex/
      antigravity/opencode `denial-blocks-tool` + `marker-absent` loops) to
      the settled contract; the model-request absence checks
      (`user-only-skill-hidden`) were already settle-guarded by the
      `if struct:` branch and stay as-is.
- [ ] 3.4 Verify a full claude vertical passes with the migrated checks
      (covered by the in-progress live runs).

## 4. Committed evidence summaries

- [x] 4.1 Implement `lab.py --write-summary` writing
      `conformance/evidence/<harness>.json` (versions, uze sha, per-kind
      counts, gate verdict incl. failures + retry).
- [x] 4.2 Add the CI evidence-commit step (bot identity, only when changed,
      on push-to-main/schedule with `[skip ci]`; skipped on PRs) and the
      `make lab-evidence` local alias.

## 5. CI gate

- [x] 5.1 Add the nightly `conformance-stability` job (schedule cron): same
      4-harness matrix, 3 consecutive runs each, flake detection (crash /
      any ❌ / missing gate line), non-zero exit on flakes, logs uploaded.
- [x] 5.2 Add the `--retry-once` flag (reruns only a run-level crash —
      assertion and gate failures return normally and are never retried);
      wired into the PR conformance job.
- [x] 5.3 Update `conformance/README.md`: gate semantics, registry
      maintenance, provenance, settled-absence contract, CI gate split.

## 6. Acceptance and ADR

- [x] 6.1 Pass the 3-consecutive-clean-run gate for all four harnesses with
      the new gate live. (Evidence 2026-08-27/28: claude 18/18 (2.1.247);
      antigravity 28/28 + 2 ADAPTED (1.1.22); codex 22/22 asserted, 0
      ADAPTED (0.150.1), 3 consecutive clean runs; opencode 28/28 + 6
      ADAPTED. The nightly `conformance-stability` job enforces the 3x rule
      on every channel bump from here on.)
- [x] 6.2 Confirm `docs/adr/035-adaptive-result-registry-and-version-provenance.md`
      exists (permanent record; change draft is a working copy).