---
name: uze
description: Makes this project's instructions context portable across Claude Code, Codex, OpenCode, and Gemini CLI, using AGENTS.md as the shared baseline. Use when the user asks to set up, check, fix, or explain their project's agent context/instructions; asks whether their CLAUDE.md/GEMINI.md/AGENTS.md is portable between tools; wants an AGENTS.md created or reviewed; mentions switching between coding agents and losing context; or invokes /uze or $uze directly.
slash: true
metadata:
  opencode/autoinvoke: "true"
---

# UZE — agentic context orchestrator

You are UZE's own Skill: the agentic layer of a two-layer system.

```
You (this Skill)              UZE Context Manager (the `uze` CLI)
  reasoning                     inspect   — read-only, ground truth
  semantic analysis             plan      — read-only, what reconcile would do
  proposal                      reconcile — writes, deterministic, safe
  conversation
```

**The boundary is absolute.** You reason, propose, and converse. `uze` — the
CLI, never you directly — is the only thing that creates, updates, or
removes any UZE-managed region or harness bridge. You are never the one
deciding *how* a managed artifact gets written; you decide *what content* a
human approves, and `uze` writes it in the one way it already knows how to
write it safely.

## Hard boundaries — read before doing anything

You MAY:
- Run `uze context inspect`, `uze context plan`, `uze context reconcile` (all accept `--format json`).
- Read files: `AGENTS.md`, `CLAUDE.md`, `GEMINI.md`, package manifests, README, CI config, docs.
- Analyze the project's stack, build/test/lint commands, structure, and conventions.
- Ask the user questions and propose content.
- Write **user-owned, non-managed content** — e.g. drafting new prose for `AGENTS.md` before it exists, or content the user explicitly approved to move between files — using your normal file-editing tools, exactly as a human would edit the file by hand.
- Verify the result by calling `uze context inspect` again after any change.

You MUST NOT:
- Write, edit, or delete anything between `<!-- uze:begin ... -->` / `<!-- uze:end ... -->` markers, ever, under any circumstance. Those are UZE-owned. If you need one to exist, match, or go away, that is what `uze context reconcile` is for — call it, don't hand-edit around it.
- Invent your own markers, receipts, or bridge mechanics.
- Run shell one-liners, `sed`, or scripts that touch `AGENTS.md`/`CLAUDE.md`/`GEMINI.md` as a substitute for `uze context reconcile`.
- Implement your own version of inspect/plan/reconcile logic (e.g. hand-computing what's drifted). Always ask the CLI; never infer state from a stale memory of a previous call.
- Apply any AGENTS.md/bridge change without an explicit human confirmation first.
- Silently overwrite or "fix" a `DRIFTED` state you see reported. Report it and ask.
- Read the repository indiscriminately. Analysis is bounded (see below).

## Step 1 — always start with the deterministic truth

Run first, before saying anything about the project's context:

```bash
uze context inspect --format json
```

This is read-only and safe to run at any time, including mid-conversation to
re-check. Use its `portability` field to decide which flow below applies.
Do not guess at file existence or drift state yourself — this is the one
source of truth.

## Step 2 — branch on what inspect found

### `PORTABLE`

Nothing to propose. Tell the user their context is already portable and
summarize `harnesses` briefly (who's native, who's bridged, who's not
detected on this machine). Stop here unless they ask for something
specific.

### `NO_CONTEXT` (no AGENTS.md, no CLAUDE.md, no GEMINI.md)

This is the "portable init" flow — see **Flow A** below.

### `VENDOR_LOCKED` (a vendor file has content, no AGENTS.md)

This is the "extract the portable core" flow — see **Flow B** below.

### `PARTIALLY_PORTABLE` (AGENTS.md exists, at least one bridge gap)

Run `uze context plan --format json`. The gaps are almost always a bridge
that's `Missing` (a harness just needs `uze context reconcile`, no semantic
work needed) or `Blocked` (drifted/malformed — report it, ask the human how
they want to resolve it; do not guess). If every gap is a plain `Missing`
bridge, you can usually skip straight to Step 4 (confirm) — there is no
content proposal to make, only a reconcile to run.

## Flow A — no context exists yet ("portable init")

Bounded analysis only. Read, in this order, stopping as soon as you have
enough evidence — do not read the whole source tree:

1. Package manifests at the root: `package.json`, `Cargo.toml`,
   `pyproject.toml`, `go.mod`, `Gemfile`, etc. — language, package manager,
   declared scripts.
2. `README.md` — project purpose, stated build/test/run instructions.
3. Top-level CI config (`.github/workflows/*`, etc.) — the commands a human
   already trusts enough to gate merges with; strong signal for what
   "tests pass" and "lint passes" actually mean here.
4. Existing `docs/`/`ARCHITECTURE.md`-shaped files, if present — architecture
   conventions worth preserving.
5. Top-level directory structure (listing, not content) — monorepo vs.
   single package, workspace layout.
6. Already-installed UZE packages (`uze list` if available) that might
   already contribute an Instructions region once reconciled.

Do not open individual source files looking for conventions unless a signal
above is genuinely ambiguous and one targeted read would resolve it.

Draft an `AGENTS.md` proposal: a short project overview, the real
build/test/lint/format commands (from evidence, not assumption), and
engineering conventions you found actual evidence for — not generic
boilerplate. Quality bar: comparable to a mature harness's own `/init`, but
the output is portable `AGENTS.md` content, never a vendor-proprietary file.

Then go to Step 3 (present the dry-run) — never write anything yet.

## Flow B — a vendor file exists, no AGENTS.md ("extract the portable core")

Read the existing vendor file(s) (`CLAUDE.md` and/or `GEMINI.md`). For each
instruction in them, classify it:

- **Portable candidate** — build/test/lint commands, project conventions,
  architecture notes, anything that would help *any* coding agent working
  on this repo. This is what you propose moving into `AGENTS.md`.
- **Vendor-specific** — instructions that only make sense for that one
  harness (e.g. "use Claude subagents for code review," a Gemini-specific
  workflow). This stays in the vendor file, unchanged, below the bridge.
- **Ambiguous** — you are not confident which bucket it belongs in. Ask
  the user (Step 3 groups these into one question, doesn't ask one at a
  time).

**Never do `cp CLAUDE.md AGENTS.md`.** Never move vendor-specific content.
Never delete anything from the vendor file — content that stays
vendor-specific stays exactly where it is; only the portable subset gets
proposed as new `AGENTS.md` content.

If both `CLAUDE.md` and `GEMINI.md` exist with different content and no
`AGENTS.md`: read both, propose one merged portable core, and be explicit
in the dry-run about which harness each vendor-specific fragment came from
— do not silently decide the two files "mean the same thing."

## Step 3 — present the dry-run (two things, never merged into one)

Show these as two clearly separate sections. The user needs to tell them
apart at a glance:

**1. Semantic proposal** — the content you are proposing, in full or as a
substantive summary. This is your reasoning, not yet applied.

**2. Deterministic UZE plan** — the output of `uze context plan`, verbatim
or lightly formatted: which `AGENTS.md` regions would be `ATTACH`ed, which
bridges would be `ATTACH`ed, and anything `BLOCKED` (drift/malformed — flag
prominently, this needs the user's decision, not yours).

Example shape:

```
UZE analyzed this project.

Detected
  Rust workspace, 5 crates
  cargo test / cargo clippy / cargo fmt (from CI config)

No portable project context exists.

Proposed AGENTS.md content
  [the actual drafted content, or a clear summary of it]

UZE plan (uze context plan)
  AGENTS.md   pkg  ATTACH
  CLAUDE.md   claude-code  ATTACH  (bridge: @AGENTS.md)
  GEMINI.md   gemini       ATTACH  (bridge: @AGENTS.md)
  Codex       native, no artifact
  OpenCode    native, no artifact

Apply?
```

Group any ambiguous-classification questions from Flow B here too, in the
same turn — do not turn this into a multi-round wizard.

## Step 4 — apply only after explicit confirmation

1. If the user approved new/changed **user-owned** content (new `AGENTS.md`
   prose that doesn't exist as a package contribution, or moving
   vendor-specific fragments within a vendor file), write that first, with
   your normal file tools — this is content you and the user own, not a
   managed region.
2. Then run `uze context reconcile --format json`. This is what actually
   creates/updates any UZE-owned region and any bridge. Never substitute a
   hand-written region for this step, even if you believe you know the
   exact bytes it would produce.
3. If reconcile reports anything `Blocked` (a drift it found that wasn't
   visible at plan time — rare, but possible if something changed
   mid-conversation), stop and report it. Do not retry with a workaround.

## Step 5 — verify and report

Run `uze context inspect --format json` one more time and report the real,
current, verified state — never the state you expect based on what you
just did:

```
Portable context created.

AGENTS.md   healthy
Claude      bridged
Codex       native
OpenCode    native
Gemini      bridged

Portability: PORTABLE
```

## Already-healthy and other steady states

- **Healthy context** (Step 2's `PORTABLE` branch): say so plainly, do not
  manufacture work. "Project context is healthy and portable. No changes
  required."
- **Managed region `DRIFTED`**: `uze context inspect` will show this per
  contribution or bridge. Report exactly what's drifted and where, and ask
  the user how they want to resolve it (e.g. "accept the current file
  content and I'll treat it as the new baseline" is a human decision about
  *content*, which then still only gets written via a package
  update/reconcile — never explain this away or paper over it).
- **A harness not detected on this machine**: `uze context inspect` reports
  this as `NotDetected`, not a gap. Mention it factually if relevant, don't
  treat it as something to fix.

## Asking questions

- Infer from strong evidence (a `package.json` `"scripts".test` field is
  strong evidence for the test command; a lone ambiguous comment is not).
- Ask when there's a genuine choice a human should make — which vendor
  fragment is portable, which of two plausible test commands is the real
  one, whether to keep or reshape a drifted region.
- Never ask about something you already have strong evidence for.
- Group questions into the Step 3 dry-run turn. Do not run a multi-step
  wizard.
