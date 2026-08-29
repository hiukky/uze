---
name: conformance-debug
description: Fast, targeted debugging of the Harness Conformance Lab (conformance/) — use whenever a conformance check fails (locally or in CI), a harness TUI check needs poking at directly, or you're about to guess at a fix instead of reproducing the real failure. Covers the --sandbox fast loop, reading evidence/verdict.json, and telling a shared-setup failure apart from a per-harness bug.
---

# Debugging the Harness Conformance Lab

The Lab runs the real vendor CLI (Claude Code, Codex, OpenCode, Antigravity)
against a synthetic provider in a disposable Docker container — one
`conformance/harnesses/<vendor>/scenarios.py` per harness. A full gate run
(`python3 conformance/lab.py --harness <h>`) takes 6-11 minutes and only
tells you pass/fail per check. **Never iterate against that loop.** There is
a ~5-second sandbox loop built for exactly this — reach for it first.

## Step 1 — read the shape of the failure before touching anything

- **The same first check fails identically across all four harnesses**
  (e.g. `tui-reached-prompt` / `tui-started`, with a near-empty first
  `.raw` capture) → the shared **setup script** died before any harness TUI
  ever launched. Do not touch `scenarios.py`'s TUI-driving/wait logic — the
  bug is in the setup steps every vertical shares (`uze market add`,
  `uze plugin install ...`), or in `uze` itself.
- **Only one harness fails** → the bug is specific to that harness's
  `crates/uze-integrations/src/<vendor>/` module or its `scenarios.py`.
- **A late-stage check fails after earlier ones pass** → the TUI/harness
  came up fine; the bug is in that specific interaction (skills, MCP,
  hooks), not in provisioning.

The setup scripts run with `set -e` and `>/dev/null 2>&1` on purpose (a
clean gate run must be quiet) — which means a setup failure is **silent**
in the normal evidence: a 0-byte or garbage-looking first `.raw` file is
the signature of "the container died during setup," not a TUI bug. Don't
debug the wait loop for a dead process.

## Step 2 — reproduce in ~5 seconds, not ~10 minutes

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

This is the single highest-leverage move: it isolates "did `uze` do the
right thing here" from "did the TUI render the right thing," in seconds,
with real (uncensored) stdout/stderr — instead of guessing from a failed
gate run's opaque assertion list.

- `--shell` instead of `-- cmd` for an interactive rootless shell in the
  container (needs a real TTY — from a non-interactive agent session, use
  `-- cmd` instead).
- `--keep` leaves the provider container + network alive after exit, if
  you need more than one round without re-provisioning.
- To test a **vendor CLI's own behavior** in isolation (does it parse this
  path/argument the way we assume?) — the sandbox drops you next to the
  real vendor binary. Never guess from vendor docs or your own priors about
  CLI argument parsing; run it and read the real exit code/stderr. This is
  exactly what found a case where `agy plugin install <path>` parsed a
  `name@marketplace`-shaped path segment as a marketplace selector instead
  of a literal path — undocumented, only visible by running it.

## Step 3 — after any Rust change, rebuild the Lab image before trusting a run

The image bakes a release build of `uze` plus the real vendor CLIs. A stale
image silently tests your *old* code:

```bash
docker build -f conformance/Dockerfile -t conformance-harness:latest .
```

Rebuilds are fast when only `crates/`/`src/` changed (cached apt/provision
layers); expect ~30-40s from a clean base, ~5-10s incremental.

## Reading evidence

Each run's `outdir` (default `/tmp/harness-conformance/<harness>/run<N>/`,
or `$AGY_OUTDIR`) holds:

- `NN_<checkpoint>.raw` — plain-text screen snapshot at that checkpoint.
  Empty/near-empty on an early one is the setup-died signature (Step 1).
- `<phase>.typescript` + `.timing` — full terminal recording (`script(1)`
  format); replay with `make lab-replay` for the actual pixels-and-timing
  session, not just a text snapshot.
- `verdict.json` — structured result per check, including `gate.adjudication`
  (`asserted` vs `unregistered_adapt` vs escalated) and the run manifest
  (harness/uze/image/fixture versions — check `version_drift` first if nothing
  else changed).
- `--discovery` (flag on `lab.py`) additionally captures the raw
  provider-side (synthetic-LLM) requests — use it when the question is
  "what did the model/tool-call payload actually contain," not "did the
  TUI render it."

## Don't

- Don't iterate against the full gate run (`lab.py --harness <h>` with no
  `--sandbox`) while narrowing down a cause — that's the ~10-minute loop,
  reserved for confirming the fix at the end.
- Don't assume vendor CLI behavior from memory or docs when a one-line
  `--sandbox -- "<vendor-cli> ..."` call gets you the real answer.
- Don't treat "all 4 harnesses broke on the same push" as 4 separate bugs —
  check the shared setup path first (Step 1).
