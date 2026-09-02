---
name: conformance-debug
description: Fast, targeted debugging of the Harness Conformance Lab (conformance/) — use whenever a conformance check fails (locally or in CI), a check is recorded ADAPTED, a harness version drifted, a harness TUI check needs poking at directly, or you're about to guess at a fix instead of reproducing the real failure. Covers reading the failure's shape, checking the vendor's current docs/changelog/issues before touching code, the --sandbox and --experiment fast loops, proving the provider speaks the real wire shape with --discovery and mitmproxy, telling a vendor limitation from a Lab defect, and reading evidence/verdict.json.
---

# Debugging the Harness Conformance Lab

The Lab runs the real vendor CLI (Claude Code, Codex, OpenCode, Antigravity)
against a synthetic provider in a disposable Docker container. What every
harness must prove lives in `conformance/contract/` (outcome terms, no
vendor named); how each harness is driven lives in
`conformance/harnesses/<vendor>/bindings.py`; what is genuinely unique to a
vendor stays in its `scenarios.py`. A full gate run
(`python3 conformance/lab.py --harness <h>`) takes 6-11 minutes and only
tells you pass/fail per check. **Never iterate against that loop.** There
are seconds-long loops built for exactly this — reach for them first.

The Lab's whole value is that a green is real. Two things make a green
false, and both have shipped for months before: a check that passes
because nothing happened (a hook that never ran, a tool call the harness
rejected before executing), and a provider that speaks a wire shape the
real API does not, so the harness silently does something else than it
would in production. Every step below exists to catch one of those.

## Step 0 — is the world still the one the Lab assumes?

Do this **before** touching code, on any failure, any ADAPTED, and any
`VERSION DRIFT` line in the run summary. The Lab pins nothing: the image
installs each vendor's channel-latest, so a run that fails "for no reason"
is usually the vendor moving.

- **Read the vendor's current docs and changelog — the ones that shipped
  with the binary you are testing, not the ones in your memory.** Several
  ship inside the CLI: Antigravity keeps its customization docs under
  `~/.gemini/antigravity-cli/builtin/skills/agy-customizations/docs/`
  (`hooks.md`, `plugins.md`, `skills.md`, `json_configs.md`, …) and prints
  release notes with `agy changelog`; Claude Code's are at
  `code.claude.com/docs` (skills, hooks, plugins); Codex and OpenCode
  publish theirs on their sites and repos. A one-line `--sandbox -- "agy
  changelog | head -80"` or a `WebFetch` of the docs page is cheaper than
  any hypothesis.
- **Search the vendor's GitHub issues** for the exact symptom (the error
  text the TUI rendered, the log line, the flag name). A known regression
  with an issue number is evidence to pin a version against; a fixed one
  tells you the next channel bump resolves it.
- **Treat every ADAPTED entry in `conformance/evidence/expected.json` as a
  question, not a fact.** Each says "this harness could not do X when we
  last looked". On every version drift, and whenever you touch the area,
  re-ask it against the current docs: the control may now exist (Claude
  gained `user-invocable: false`; the Lab kept declaring it unverifiable
  because the binding read the wrong surface). An ADAPTED that has become
  false is a false green in the making — the gate escalates a registered
  ADAPTED that starts passing precisely so you cannot miss it.
- **Assume nothing about the wire.** Harness behaviour that depends on the
  provider's response shape (streaming events, tool-call argument names,
  which request of a turn is the user's) changes across vendor versions.
  Claude Code accumulates a tool's input only from `input_json_delta`;
  Antigravity 1.1.24 validates a scripted call against the tool's declared
  `parametersJsonSchema` before any hook runs, and makes a side call to a
  lighter model with no tools before the user's turn. A provider that
  counts requests or invents argument names produces turns that "settle"
  with nothing having happened. See Step 3.

## Step 1 — read the shape of the failure

- **The same first check fails identically across all four harnesses**
  (e.g. `tui-reached-prompt` / `tui-started`, with a near-empty first
  `.raw` capture) → the shared **setup script** died before any harness TUI
  ever launched. Do not touch TUI-driving/wait logic — the bug is in the
  setup steps every vertical shares (`uze market add`, `uze plugin install
  ...`), or in `uze` itself.
- **Only one harness fails** → the bug is specific to that harness's
  `crates/uze-integrations/src/<vendor>/` module, its `bindings.py`, its
  `scenarios.py`, or its provider — or the vendor moved (Step 0).
- **A late-stage check fails after earlier ones pass** → the TUI/harness
  came up fine; the bug is in that specific interaction (skills, MCP,
  hooks), not in provisioning.
- **A presence check fails while its absence checks pass** (e.g.
  `hooks-*-denial-relayed` red, `hooks-*-marker-absent-*` green) → the turn
  ran with nothing intercepted. The absence checks are gated on the
  presence check for this reason; read what the harness rendered
  (`Invalid tool parameters`, `invalid arguments`, a permission prompt)
  and go to Step 3.

The setup scripts run with `set -e` and `>/dev/null 2>&1` on purpose (a
clean gate run must be quiet) — which means a setup failure is **silent**
in the normal evidence: a 0-byte or garbage-looking first `.raw` file is
the signature of "the container died during setup," not a TUI bug. Don't
debug the wait loop for a dead process.

## Step 2 — reproduce in seconds, not minutes

`lab.py` has a sandbox mode built for this (`--help` documents it in full):

```bash
# Runs one command inside the real, fully-provisioned harness container
# (fixture marketplace + synthetic provider already up), then tears down.
python3 conformance/lab.py --harness <claude|codex|opencode|antigravity> \
  --sandbox -- "<shell command>"
```

Use it to poke at the exact step you suspect, e.g.:

```bash
python3 conformance/lab.py --harness opencode --sandbox -- \
  "uze market add /work/market >&2; uze plugin install mcp-plugin@uze-lab; uze doctor"
```

This isolates "did `uze` do the right thing here" from "did the TUI render
the right thing," in seconds, with real (uncensored) stdout/stderr.

- The sandbox shell is bare: set `PATH=/usr/local/bin:/usr/bin:/bin:/usr/local/.local/bin`
  and copy the vendor fixtures the way `scenarios.py`'s setup fragment does
  (Antigravity: `settings.json`, `jetski_state.pbtxt`, `installation_id`
  into `~/.gemini/antigravity-cli/`) before running the vendor CLI.
- `--shell` instead of `-- cmd` for an interactive rootless shell in the
  container (needs a real TTY — from a non-interactive agent session, use
  `-- cmd` instead). `--keep` leaves the provider + network alive between
  rounds.
- To test a **vendor CLI's own behavior** in isolation (does it parse this
  path/argument the way we assume? what does its validator say about this
  file?) — the sandbox drops you next to the real vendor binary. Never
  guess from docs or your own priors when a one-line call gets you the
  real exit code, stderr, or log. `agy plugin validate <dir>` counted 1
  hook where UZE meant 3, which is how the `hooks.json` wrapper bug was
  found; `agy -p "/hooks" --output-format json` lists what the harness
  actually registered; `--log-file` gives you the vendor's own log.
- **A hypothesis that needs a real turn is an experiment**, not a sandbox
  command: `conformance/experiments/<vendor>/<name>.py` gets the
  provisioned topology, starts its own provider in the mode it needs
  (`common.start_provider(cfg, "toolcall", {...})`), drives a headless or
  TUI turn, and records its own checks under the run outdir — outside the
  canonical suite, versioned in the repo as evidence. Run with
  `lab.py --harness <h> --experiment <vendor>/<name>`. Switch behaviour
  with environment variables rather than editing the file between runs.
  `experiments/antigravity/hook-tui.py` is the template: it names its
  container so the vendor's log can be read mid-session with `docker exec`.

## Step 3 — prove the provider speaks the real wire shape

The synthetic provider is the Lab's model of the vendor's API. If it drifts
from what the real API sends, the harness is not being tested — a
different program is. Whenever a turn "settles" without the expected
effect, or a harness rejects a scripted call, read the wire:

```bash
python3 conformance/lab.py --harness <h> --discovery      # any mode: sandbox, experiment, vertical
```

`--discovery` appends every raw request the harness sent to the provider
to `<outdir>/raw-requests.log`, across every per-phase provider restart.
Read the request that carries the tool result: Antigravity's
`functionResponse.output` said *"missing properties 'Cwd',
'WaitMsBeforeAsync', 'CommandLine', 'toolSummary', 'toolAction' —
additional properties 'command' not allowed"*, which named the fix. Read
the request's `tools`/`functionDeclarations` (or Anthropic `tools`) for
the schema the harness declared — script calls in **that** shape, never
one you remember. Read which request is the user's turn (roles, declared
tools, `<USER_REQUEST>`) and make the provider answer by content, the way
a model would, never by request count.

For the shape of the **response** side — what the real API streams, which
the Lab cannot capture offline — use the observation tooling in
`conformance/discovery/` (mitmproxy addons that sanitize at capture time,
`run_agy_obs.sh` for a real authenticated session through the proxy). It
is observation-only and its raw captures never enter the repository; what
it teaches becomes the provider's contract, written down in the provider's
module docstring with the vendor version it was observed on (see
`harnesses/claude/provider.py`: streaming event sequence, `input_json_delta`,
no `data: [DONE]` for Gemini).

The checklist for any provider change:

1. The request the harness sends after a tool call carries the tool's
   *real* result (proof marker, stdout marker, denial reason) — not an
   error string. Grep `raw-requests.log` for it.
2. The provider answers the user's turn, not the harness's side calls
   (title generation, summaries, a lighter model with no tools).
3. Streaming events match what the vendor's SDK accumulates.
4. A structural summary (`struct.json`) records *presence* of markers per
   request; aggregate with `common.observed_markers` (a union), never
   last-write-wins.

## Step 4 — vendor limitation, or Lab defect?

Only declare ADAPTED when the vendor itself cannot do it **in a way you
have measured**, and register the declaration with its reason and the
versions it was observed on. The tools for the measurement:

- **A control probe in the vendor's own format at the vendor's own path,
  with no UZE in the loop.** If a vendor-format deny hook at
  `~/.gemini/config/hooks.json` is loaded, listed by `/hooks`, and never
  executes — not even a `touch` per event — then no UZE delivery can be
  observed there, and the vertical must measure that gate at run time
  (`hooks > vendor` in the Antigravity vertical) rather than assume it in
  a spec. A live precondition is the only declaration that can expire on
  its own: the gate escalates the day the vendor opens it.
- **Pin the previous image.** `UZE_LAB_IMAGE=<image id> python3
  conformance/lab.py ...` runs the same scenario against the harness
  version that last passed. Same result on both → it was never the
  vendor's regression (and probably never worked: check the old green for
  vacuity). Different → a regression, pin the version in the registry and
  file/reference the vendor issue.
- **Read the vendor's log and binary before concluding "impossible".**
  `--log-file` shows what loaded and what surfaced; `strings` on the binary
  (copy it out with `docker cp`) shows the flags and proto fields that
  gate a feature (`CustomizationConfig.enable_json_hooks` was found this
  way). A gate fed by a server-side feature provider the offline Lab never
  receives is a real limitation; a gate that is a documented setting is a
  fixture to set.

## Step 5 — after any Rust change, rebuild the Lab image before trusting a run

The image bakes a release build of `uze` plus the real vendor CLIs. A stale
image silently tests your *old* code:

```bash
docker build -f conformance/Dockerfile -t conformance-harness:latest .
```

Rebuilds are fast when only `crates/`/`src/` changed (cached apt/provision
layers); expect ~30-40s from a clean base, ~5-10s incremental. Python under
`conformance/` (providers, scenarios, bindings, contract) is mounted at run
time — no rebuild needed.

## Reading evidence

Each run's `outdir` (default `/tmp/harness-conformance/<harness>/run<N>/`,
or `$AGY_OUTDIR`) holds:

- `NN_<checkpoint>.raw` / `<phase>.raw` — plain-text screen snapshot at
  that checkpoint. Empty/near-empty on an early one is the setup-died
  signature (Step 1). The screen helper returns only the bytes that
  arrived since the last read; a snapshot is what one wait consumed.
- `<phase>.typescript` + `.timing` — full terminal recording (`script(1)`
  format); replay with `make lab-replay`, or strip ANSI and read the tail
  to see what the harness rendered right before a turn stalled (a
  permission prompt, `Invalid tool parameters`, a survey).
- `<phase>_struct.json` — the provider's structural summary per request
  (path, roles, declared tools, marker presence). Which model/path each
  request hit tells you which request was the user's turn.
- `raw-requests.log` — with `--discovery`, the verbatim requests.
- `verdict.json` — structured result per check, including `gate.adjudication`
  (`asserted` vs `unregistered_adapt` vs escalated) and the run manifest
  (harness/uze/image/fixture versions — check `version_drift` first if
  nothing else changed).

## Don't

- Don't iterate against the full gate run (`lab.py --harness <h>` with no
  `--sandbox`/`--experiment`) while narrowing down a cause — that's the
  ~10-minute loop, reserved for confirming the fix at the end.
- Don't assume vendor CLI or API behaviour from memory, or from docs older
  than the binary under test, when a one-line sandbox call, the vendor's
  shipped docs, its changelog, or a `--discovery` capture gets you the
  real answer.
- Don't treat "all 4 harnesses broke on the same push" as 4 separate bugs —
  check the shared setup path first (Step 1).
- Don't hard-code an ADAPTED in a scenario. A declaration is either a
  binding's `unsupported` reason (a harness limitation, reviewable) or a
  live precondition that measures the vendor each run; a registry entry
  must exist for it, pinned to the observed versions.
- Don't let a check pass on a turn where nothing happened. Gate every
  absence on a presence; demand the tool's own stdout, not merely "a tool
  result"; read the wire when in doubt.
