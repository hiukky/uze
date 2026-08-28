# UZE Harness Conformance Lab

The Lab proves what the **real external harnesses** do with what UZE
produced — the vendor-facing half of the release story. It never mocks a
harness, never re-tests what the main suite owns, and never runs a real
model.

## Architecture: vertical by harness (Python)

The primary unit of understanding is one directory per vendor harness. A
maintainer debugging Antigravity opens `harnesses/antigravity/` and finds
everything there (provider, TUI drive, scenarios, fixtures). Shared code is
deliberately small and vendor-neutral.

```
conformance/
├── lab.py                        # entry: python3 lab.py --harness <h>
├── _fixtures/
│   └── marketplace/               # final isolated resources: Skills + MCP
├── shared/
│   └── common.py                 # vendor-neutral: docker topology, per-run
│                                 #   TLS certs, PTY screen/waiter, evidence
├── discovery/                    # observation-only tooling (mitmproxy
│                                 #   addons, host scripts) — NEVER required
│                                 #   for conformance execution
├── harnesses/
│   ├── antigravity/              # Real AGY (latest channel) + synthetic Gemini
│   │   ├── provider.py           #   fake_gemini (plain HTTP 9999)
│   │   ├── scenarios.py          #   onboarding, /skills, /mcp, turn,
│   │   │                         #   model-facing, MCP round-trip, state,
│   │   │                         #   hooks (deny/allow/order)
│   │   └── fixtures/             #   synthetic seeds (settings, state, sse)
│   ├── claude/                   # Real Claude Code (latest channel) + synthetic Anthropic
│   │   ├── provider.py           #   fake_anthropic (TLS 443, hardcoded hosts)
│   │   ├── scenarios.py          #   onboarding, /plugin, /mcp, turn,
│   │   │                         #   model-facing (policy preserved), finding,
│   │   │                         #   hooks (deny/allow/order)
│   │   └── fixtures/             #   claude.json + settings.json (theme)
│   ├── codex/                    # Real Codex (latest channel) + synthetic OpenAI
│   │   ├── provider.py           #   fake_openai (TLS 443, WS→HTTPS fallback)
│   │   ├── scenarios.py          #   trust, /skills, /plugins, /mcp, turn,
│   │   │                         #   model-facing, plugin-list state,
│   │   │                         #   hooks (deny/allow/order)
│   │   └── fixtures/             #   auth.json seed
│   └── opencode/                 # Real OpenCode (latest channel) + synthetic
│       ├── provider.py           #   OpenAI-compatible (plain HTTP 9999)
│       ├── scenarios.py          #   /skills, /mcps, turn, model-facing,
│       │                         #   MCP tool round-trip in the TUI,
│       │                         #   hooks (deny/allow/order)
│       └── fixtures/             #   (none — provider is config-driven)
```

## Golden signal: Real Harness + Synthetic World

Each vertical proves, against the REAL harness binary in a disposable
`--internal` Docker network (zero external Internet, zero tokens, zero
credentials):

```
real uze → real harness → real TUI → synthetic provider → deterministic result
```

Run (3x clean is the gate):

```bash
python3 lab.py --harness antigravity   # 16/16 PASS + 1 ADAPTED (pre-hooks baseline)
python3 lab.py --harness claude        # 8/8 PASS (pre-hooks baseline)
python3 lab.py --harness codex         # 11/11 PASS (pre-hooks baseline)
python3 lab.py --harness opencode      # 14/14 PASS + 1 ADAPTED (pre-hooks baseline)
```

Every vertical additionally runs the **portable-hooks phase** (ADR-033):
three TUI-first scenarios — `deny` (a real tool call blocked by a portable
hook), `allow` (the same hook lets the real tool execute, proven by the
tool's own output reaching the conversation), and `order` (first-deny-wins:
the second handler's marker must never appear). Runs are grouped
`describe`/`test`-style (tui, cli.state, hooks > deny/allow/order) so a
growing suite stays interpretable, and every wait aborts immediately when
the harness process dies instead of burning its try budget.

Latest evidence per harness (run-by-run, recorded honestly — including the
ADAPTED vendor-limitation records and any pre-existing base-phase failure):

```bash
python3 lab.py --harness claude        # 18/18 PASS (hooks deny/allow/order proven)
python3 lab.py --harness codex         # 20/20 PASS + 1 ADAPTED (allow recorded ADAPTED: approval gate)
python3 lab.py --harness antigravity   # 28/28 PASS + 2 ADAPTED (MCP round-trip proven: the proof returns)
python3 lab.py --harness opencode      # 28/28 PASS + 6 ADAPTED (MCP tool not exposed on the V2 beta channel — recorded, never fabricated)
```

Evidence JSON goes under `AGY_OUTDIR` (default
`/tmp/harness-conformance/<harness>/run<N>`). The exit code is 0 only when
every asserted check passed (ADAPTED counts as passing — it is an honest
vendor-limitation record, never a rewrite).

## Gate semantics and evidence integrity (ADR-035)

Every run is adjudicated against `conformance/evidence/expected.json`, the
**adaptive-result registry** — the anti-false-positive contract:

- an **ADAPTED result without a registry entry** fails the run (a harness
  losing a capability can never pass silently);
- a **registered ADAPTED check that starts passing** fails with an
  *escalate* verdict until the scenario is promoted to an asserted check
  and the entry removed;
- entries record a reason and observed harness versions (`*` covers any;
  pin versions once probed runs establish them) — a vendor bump that
  changes the meaning of a registered adaptation fails visibly.

Every run also records **version provenance**: the real harness version is
probed with the vendor's own `--version` flag (`claude --version`,
`codex --version`, `opencode --version`, `agy --version` — the same probes
the product integrations use), and the run manifest in `verdict.json`
carries harness/uze versions, fixture revision, image id, and timestamps. A
harness version change vs. the previous committed summary is reported as an
explicit `VERSION DRIFT` event.

**Absence assertions** (a marker that must never appear) evaluate only after
the turn settled and the TUI went quiet (`settle_and_quiet` + `check_absence`
in `shared/common.py`); an unsettled turn fails the check instead of passing
by accident.

Per-harness **evidence summaries** are written beside the run evidence and
uploaded as **Actions artifacts** (retention-days 90; local runs write into
`conformance/evidence/` for the version-drift baseline) — the audit trail
without CI-to-main push races or commit churn (ADR-035 revised).

**CI gate**: PR runs each vertical once (with `--retry-once`, which reruns
only a run-level crash, never an assertion failure); the nightly
`conformance-stability` job runs each vertical **3 consecutive times** and
fails on any flake — the promotion gate for changing the registry or the
suite. Verify the gate locally with `python3 conformance/tests/test_gate.py`
(no docker needed).

## Exploration modes (sandbox, experiments, variations, matrix)

The Lab doubles as an exploration surface for the agent or a maintainer —
the same real-harness + synthetic-world topology, made interactive:

- **`lab.py --harness <h> --sandbox [--shell] [-- cmd...]`** keeps the
  disposable network + provider alive and hands over a recorded session:
  the harness's own TUI (default), a rootless shell inside the harness
  container with the fixture market pre-registered (`--shell`), or one
  scripted command (`-- cmd...` — the non-interactive path agents use).
  Sessions are recorded with the usual cast/timing evidence; teardown is
  disposable unless `--keep`. `make lab-sandbox HARNESS=<h>`.
- **Experiments** (`conformance/experiments/<vendor>/<name>.py`) are
  versioned hypotheses outside the canonical suite — same scenario
  contract, no gate registry. A new finding is an experiment first;
  promotion into the canonical suite requires **3 consecutive clean runs**
  (the same rule as the registry). `lab.py --harness <h> --experiment
  <vendor>/<name>` — verdict recorded under
  `<outdir>/experiments/<vendor>-<name>/verdict.json`.
- **Adversarial variations** (`--variation SPEC`) script degraded provider
  paths: `slow_sse:<s>`, `disconnect_after:<n>`, `duplicate:<event>`,
  `malformed:<event>`, `chopped:<n>`. Providers emit through
  `shared/variation.py`; a kind a provider cannot express is *recorded* as
  its observed tolerance (`/app/variation.json`), never faked. Unset spec =
  exactly the canonical single write.
- **Compatibility matrix** (`lab.py --matrix conformance/variants.json
  [--harnesses a,b,c]`) runs every (variant × harness) cell — overlay
  variants on the fixture marketplace (`hooks.json` shapes, `invoke:`
  policies, AGENTS.md forms) — through the harness's canonical vertical,
  and renders one PASS/ADAPTED/FAIL grid with evidence links under
  `conformance/matrix/<run>/`. Trade-offs are measured, never assumed;
  cells are independent runs, on-demand/nightly only (never the PR gate).
  `make lab-matrix VARIANTS=... HARNESSES=...`.

- **`--discovery`** captures every raw request the harness sends to the
  provider (provider-side, no proxy topology) beside the run evidence as
  `raw-requests.log` — raw captures never enter the repository. Works on
  sandbox, experiment, and canonical runs.

## How each harness's synthetic world hooks in

| Harness | Hook | Provider |
|---|---|---|
| Antigravity | `GOOGLE_GEMINI_BASE_URL` + API-key mode | plain HTTP 9999 |
| Claude | TLS interception of hardcoded hosts (`/etc/hosts` + `NODE_EXTRA_CA_CERTS`) | TLS 443, Anthropic Messages SSE |
| Codex | TLS interception of `api.openai.com` + `auth.json` seed | TLS 443, WebSocket (RFC 6455, JSON events per frame) + Responses SSE fallback |
| OpenCode | custom `baseURL` in the global `opencode.json` (no TLS needed) | plain HTTP 9999, Chat Completions SSE |

Harness containers run with `--security-opt seccomp=unconfined`: codex's
own tool sandbox (bubblewrap) needs user namespaces, which the default
seccomp profile blocks — the topology is already disposable and rootless
(`--internal` net, tmpfs, no Docker socket), so relaxing userns for the
harness's own sandboxing is a documented prerequisite, not an escape
hatch.

The per-run TLS certs are generated with openssl into the run's outdir —
nothing is committed, nothing is reused across runs.

## Shared final-resource marketplace

`_fixtures/marketplace/` is the Lab's dedicated, complete marketplace. Each
vertical starts from a fresh copy and selects the plugins it needs, so the
same `flow` Skills, MCP resources, and portable-hook plugins (`hook-plugin`,
`hook-order-plugin`, `hook-fail-plugin` — the fail-closed contract fixture —
ADR-033) are exercised across harnesses without each scenario rebuilding
them by hand. `lab.py` validates
the inventory and the MCP runtime placeholders before it starts Docker.

This is intentionally separate from `tests/_fixtures/`: those fixtures are
small, stable inputs for deterministic Rust tests, while this marketplace is
the evolving final resource set for real-harness evidence. The two trees may
have equivalent examples, but neither is an implicit source for the other.

## Watching a run (the TUI, rendered correctly)

Every TUI phase is recorded (`scriptreplay`-compatible typescript + timing)
into the run's outdir. `make lab-replay` replays the most recent session
with correct rendering — ANSI, colors and all — as if it were live:

```bash
make lab-run HARNESS=opencode   # run the vertical (records the session)
make lab-replay                 # replay it, rendered correctly
```

In CI, the same suite runs per harness (`ci.yml` → `conformance` job, matrix
`antigravity | claude | codex | opencode`); each run's evidence lands as a
build artifact.

## Honest findings (documented, never a pass)

- **Claude**: MCP tools are deferred behind ToolSearch (deferred-tool
  protocol); a direct `mcp__` tool_use fails with "No such tool available".
  Registration + connection are proven via `/mcp`; deep execution is not
  asserted.
- **Codex**: with the current UZE delivery the plugin skills are not listed
  in codex's model catalog (only built-ins), and the UZE MCP config does not
  reach the `/mcp` inventory. Codex hooks require the `[features].hooks`
  feature flag in `~/.codex/config.toml` (the deprecated `codex_hooks` key
  stops being honored — verified: codex-cli prints the deprecation warning
  and hooks stay disabled without the new key).
- **Hooks (ADR-033)**: the deny/allow/order phases assert semantic markers
  — a deny is proven by the intercepted tool *never executing* (its output
  never reaches the conversation), an allow by the tool's output arriving.
  OpenCode V2 exposes no input-based block (the action-level deny lives in
  the permission hook, which carries no tool input), so its deny/order
  scenarios are recorded ADAPTED with the observed behavior; AGY 1.1.21's
  `allow` decision did not produce an observable execution in the lab turn,
  also recorded ADAPTED.

## Discovery

`discovery/` holds observation-only tooling (mitmproxy addons, host
observation scripts). It is never required for conformance execution, and
raw vendor captures never enter the repository.

## Adding the next harness

1. Use `discovery/` locally to learn the harness's synthetic-auth and
   provider-redirect hooks (observed, never assumed).
2. Add `harnesses/<vendor>/` (provider.py, scenarios.py, fixtures/).
3. Run 3x offline; the vertical is green when it passes 3 consecutive runs.
