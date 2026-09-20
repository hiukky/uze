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
- [x] 1.4 Run each vertical once locally with the gate live; investigate
      every unexpected ADAPTED instead of registering it blindly.
      (Evidence 2026-09-20, this worktree, gate live: claude 38/38 asserted 0 ADAPTED (2.1.278); codex 50/50 asserted 0 ADAPTED (0.155.1); opencode 44/44 asserted 6 registered ADAPTED (v2.0.11); antigravity 48/48 asserted 0 ADAPTED (1.2.7). Every summary in `conformance/evidence/` carries `failures: []`.)
      The investigation this demanded was real: the settled-absence
      contract failed two claude `hooks > order` checks on a turn that had
      settled correctly, and the cause was this file's own measurement, not
      the vendor — `settle_and_quiet` timed the window on `time.time()`, and
      a WSL guest re-syncing with its Windows host stepped the wall clock
      mid-window so the budget expired on its first comparison. Durations
      are now `time.monotonic()`, covered by `conformance/tests/test_settle.py`.

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
- [x] 3.4 Verify a full claude vertical passes with the migrated checks.
      38/38 asserted, 0 ADAPTED on 2.1.278 (2026-09-20) — after fixing the
      wall-clock measurement in 1.4, which is what the migrated checks were
      failing on.

## 4. Committed evidence summaries

- [x] 4.1 Implement `lab.py --write-summary` writing
      `conformance/evidence/<harness>.json` (versions, uze sha, per-kind
      counts, gate verdict incl. failures + retry).
- [~] 4.2 Add the CI evidence-commit step (bot identity, only when changed,
      on push-to-main/schedule with `[skip ci]`; skipped on PRs) and the
      `make lab-evidence` local alias. Built, run, and withdrawn inside the
      same day: the four matrix legs run in parallel and raced each other
      pushing to `main`. Evidence moved to Actions artifacts
      (`retention-days: 90`), CI never pushes, and `conformance/evidence/`
      is a local baseline a maintainer records with `make lab-evidence` —
      which shipped and is the part of this task that stands. ADR-035's
      Consequences records the revision. Known consequence, left open
      deliberately: nothing advances that baseline on its own, so it can go
      stale or stay red without the gate objecting (`opencode.json` sat at
      3/24 for twenty-two days). Closing that is its own change; see the
      note in `conformance/README.md`.

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