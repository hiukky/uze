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
python3 lab.py --harness codex         # pending first clean run
python3 lab.py --harness antigravity   # 27/28 PASS + 2 ADAPTED (1 pre-existing MCP FAIL)
python3 lab.py --harness opencode      # 27/28 PASS + 2 ADAPTED (1 pre-existing MCP FAIL)
```

Evidence JSON goes under `AGY_OUTDIR` (default
`/tmp/harness-conformance/<harness>/run<N>`). The exit code is 0 only when
every asserted check passed (ADAPTED counts as passing — it is an honest
vendor-limitation record, never a rewrite).

## How each harness's synthetic world hooks in

| Harness | Hook | Provider |
|---|---|---|
| Antigravity | `GOOGLE_GEMINI_BASE_URL` + API-key mode | plain HTTP 9999 |
| Claude | TLS interception of hardcoded hosts (`/etc/hosts` + `NODE_EXTRA_CA_CERTS`) | TLS 443, Anthropic Messages SSE |
| Codex | TLS interception of `api.openai.com` + `auth.json` seed | TLS 443, WS-accept-then-close + Responses SSE |
| OpenCode | custom `baseURL` in the global `opencode.json` (no TLS needed) | plain HTTP 9999, Chat Completions SSE |

The per-run TLS certs are generated with openssl into the run's outdir —
nothing is committed, nothing is reused across runs.

## Shared final-resource marketplace

`_fixtures/marketplace/` is the Lab's dedicated, complete marketplace. Each
vertical starts from a fresh copy and selects the plugins it needs, so the
same `flow` Skills, MCP resources, and portable-hook plugins (`hook-plugin`,
`hook-order-plugin` — ADR-033) are exercised across harnesses without each
scenario rebuilding them by hand. `lab.py` validates
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
