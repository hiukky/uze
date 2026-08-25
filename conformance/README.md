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
├── shared/
│   └── common.py                 # vendor-neutral: docker topology, per-run
│                                 #   TLS certs, PTY screen/waiter, evidence
├── discovery/                    # observation-only tooling (mitmproxy
│                                 #   addons, host scripts) — NEVER required
│                                 #   for conformance execution
├── harnesses/
│   ├── antigravity/              # Real AGY 1.1.20 + synthetic Gemini
│   │   ├── provider.py           #   fake_gemini (plain HTTP 9999)
│   │   ├── scenarios.py          #   onboarding, /skills, /mcp, turn,
│   │   │                         #   model-facing, MCP round-trip, state
│   │   └── fixtures/             #   synthetic seeds (settings, state, sse)
│   ├── claude/                   # Real claude 2.1.245 + synthetic Anthropic
│   │   ├── provider.py           #   fake_anthropic (TLS 443, hardcoded hosts)
│   │   ├── scenarios.py          #   onboarding, /plugin, /mcp, turn,
│   │   │                         #   model-facing (policy preserved), finding
│   │   └── fixtures/             #   claude.json + settings.json (theme)
│   └── codex/                    # Real codex 0.149.1 + synthetic OpenAI
│       ├── provider.py           #   fake_openai (TLS 443, WS→HTTPS fallback)
│       ├── scenarios.py          #   trust, /skills, /plugins, /mcp, turn,
│       │                         #   model-facing, plugin-list state
│       └── fixtures/             #   auth.json seed
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
python3 lab.py --harness antigravity   # 16/16 PASS + 1 ADAPTED
python3 lab.py --harness claude        # 8/8 PASS
python3 lab.py --harness codex         # 11/11 PASS
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

The per-run TLS certs are generated with openssl into the run's outdir —
nothing is committed, nothing is reused across runs.

## Honest findings (documented, never a pass)

- **Claude**: MCP tools are deferred behind ToolSearch (deferred-tool
  protocol); a direct `mcp__` tool_use fails with "No such tool available".
  Registration + connection are proven via `/mcp`; deep execution is not
  asserted.
- **Codex**: with the current UZE delivery the plugin skills are not listed
  in codex's model catalog (only built-ins), and the UZE MCP config does not
  reach the `/mcp` inventory.

## Discovery

`discovery/` holds observation-only tooling (mitmproxy addons, host
observation scripts). It is never required for conformance execution, and
raw vendor captures never enter the repository.

## Adding the next harness

1. Use `discovery/` locally to learn the harness's synthetic-auth and
   provider-redirect hooks (observed, never assumed).
2. Add `harnesses/<vendor>/` (provider.py, scenarios.py, fixtures/).
3. Run 3x offline; the vertical is green when it passes 3 consecutive runs.