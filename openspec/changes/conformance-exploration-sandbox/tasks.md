## 1. Interactive sandbox mode

- [x] 1.1 Implement `lab.py --sandbox <harness>`: provision topology, keep
      provider + network alive, open a recorded session — default: the
      harness's own TUI built by the same per-harness container builders
      the canonical phases use; `--shell`: rootless `sh` inside the harness
      container with the fixture market pre-registered; `-- cmd...`: one
      scripted non-interactive command (recorded to
      `sandbox-command.log`).
- [x] 1.2 Record sessions with `CastRecorder` into the run outdir; add
      `make lab-sandbox HARNESS=X` and a shell variant.
- [x] 1.3 Teardown in a `finally` even on a killed/dying session, with
      `--keep` for debugging; network/provider names are nonced per process
      so concurrent labs (vertical loop + sandbox + matrix cells) never
      share or cross-talk.

## 2. Experiment scenarios

- [x] 2.1 Establish `conformance/experiments/<vendor>/<name>.py` (canonical
      `run(cfg, prov_ip)` contract, optional module-level `VARIATION`);
      `lab.py --experiment <vendor>/<name>` with a verdict recorded
      separately from the canonical gate (`<outdir>/experiments/...`);
      first experiment: `claude/slow-sse` tolerance mapping.
- [x] 2.2 Promotion checklist documented (3 consecutive clean runs → move
      into `harnesses/<vendor>/`); stale experiments are archived like
      changes.
- [x] 2.3 Experiment lifecycle documented in `conformance/README.md`.

## 3. Adversarial provider variations

- [x] 3.1 Implement `shared/variation.py` (`slow_sse`, `disconnect_after`,
      `duplicate`, `malformed`, `chopped`) and route every provider's
      emission through it (claude/codex/opencode/antigravity write sites +
      mount); unset spec = exactly one verbatim write (zero behavior
      change).
- [x] 3.2 Record the applied spec and any kind a provider cannot express as
      its observed tolerance (`/app/variation.json`); the run manifest and
      experiment verdicts carry the variation.
- [x] 3.3 First tolerance-mapping experiment: `claude/slow-sse` (canonical
      TUI drive under `slow_sse:0.4`) — live-run in the Lab.

## 4. Compatibility matrix

- [x] 4.1 Implement `conformance/matrix.py`: variant manifest overlay on a
      fresh marketplace copy (replace / delete, canonical tree untouched),
      per-cell canonical vertical runs with the market mounted into the
      harness container, `matrix/<run>/matrix.json` report + readable
      PASS/ADAPTED/FAIL grid with evidence links.
- [x] 4.2 Cells are independent runs (nonced networks), on-demand/nightly
      only (never the PR gate); `make lab-matrix VARIANTS=... HARNESSES=...`
      and ship `conformance/variants.json` (hooks shapes, native matchers,
      user-only invocation).
- [x] 4.3 Unit tests for the overlay builder + report rendering (no
      docker); live matrix runs are on-demand (each cell is a full
      vertical).

## 5. Discovery integration and docs

- [x] 5.1 Implement `--discovery` for sandbox/experiment/vertical runs:
      provider-side raw request capture (`shared/capture.py`, mounted into
      every provider; `DISCOVERY=1` env from the lab) pulled beside the run
      evidence (`raw-requests.log`) — raw captures never enter the repo,
      same rule as `discovery/`. The mitmproxy observation addons remain
      documented host-side tooling for deployment-level observation.
- [x] 5.2 Update `conformance/README.md`: sandbox, experiments, variations,
      and matrix usage; gate untouched by exploration.