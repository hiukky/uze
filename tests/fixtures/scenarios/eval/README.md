# Context eval scenarios

These are real project directories for judging the `/uze` Skill's
**reasoning**, not its mechanics. The deterministic layer (does `inspect`
ever write, does `reconcile` respect drift, does a bridge appear/disappear
correctly) is already covered by `cargo test` in `tests/context_*.rs` and
`tests/uze_skill.rs`. What's left — does the Skill draft a *good*
`AGENTS.md`, does it correctly separate portable from vendor-specific
content, does it ask the right questions — requires a model's judgment and
is not a `cargo test` assertion. There is no automated eval runner wired up
yet; this is the fixture set and rubric such a runner (or a human doing a
manual pass across the four real harnesses) should use.

For each scenario: install the official `uze` Skill package
(`uze add ./plugins/uze && uze setup`) somewhere it's discoverable, `cd`
into the scenario directory, invoke the Skill (`/uze` in Claude Code, `$uze`
or a matching prompt in Codex, a matching natural-language prompt in
OpenCode/Antigravity CLI — see `docs/capabilities/uze-skill.md` for the exact
per-harness invocation), and judge the transcript against the rubric below.

## `empty-rust-project/` — Fase 4, "portable init"

No `AGENTS.md`/`CLAUDE.md`/`GEMINI.md` at all. A `Cargo.toml`, a `README.md`
with real build/test/lint/format commands, a `.github/workflows/ci.yml`
confirming those same commands, and a trivial `src/main.rs`.

**Good:**
- Detects Rust, `cargo build`/`test`/`clippy`/`fmt` — from the CI file and
  README, not invented.
- Does not open `src/main.rs`'s content looking for conventions (there's
  nothing there to find — the signal is exhausted after manifest/README/CI).
- Presents a dry-run (semantic proposal + deterministic plan, separately)
  before writing anything.
- Only after confirmation, runs `uze context reconcile`.

**Bad:** invents commands not evidenced anywhere; writes without
confirmation; reads deeply into source files that add no signal beyond what
the manifest/CI already gave.

## `claude-only-node-project/` — Fase 5, "extract the portable core"

A `package.json` and a `CLAUDE.md` mixing portable, vendor-specific, and
project-convention content in one file.

**Good — classification:**
- Portable: "Use pnpm", "Run `pnpm test` before finishing", the `zod`
  validation convention.
- Vendor-specific, stays in `CLAUDE.md`: "Use Claude subagents for code
  review."
- Proposes `AGENTS.md` with the portable subset; leaves `CLAUDE.md`'s
  vendor-specific line in place (below a `@AGENTS.md` bridge once
  reconciled — never deleted, never duplicated verbatim into `AGENTS.md`).

**Bad:** `cp CLAUDE.md AGENTS.md`; moving "use Claude subagents" into
`AGENTS.md`; deleting it from `CLAUDE.md` instead of leaving it alongside
the bridge; not asking about anything genuinely ambiguous in a real project
(this fixture is cleanly classifiable on purpose, but the transcript should
still show the reasoning, not just an answer).

## `claude-and-gemini-divergent/` — Fase 5/6, two vendor files, no `AGENTS.md`

`CLAUDE.md` and `GEMINI.md` share one overlapping instruction (`npm run
test:unit`) and each carry their own vendor-specific and ambiguous content.

**Good:**
- Recognizes the overlapping test-command instruction and proposes it once
  in `AGENTS.md`, not duplicated.
- Keeps Claude's plan-mode note and a vendor-specific `/memory show` note
  vendor-specific.
- The dependency-budget note in `GEMINI.md` is genuinely ambiguous (arguably
  portable, arguably vendor-workflow-specific) — a good transcript asks
  about it rather than silently guessing either way.
- States explicitly that it's reading two independent files and is not
  assuming they mean the same thing — mirrors the deterministic layer's own
  `derive_warnings` "divergent sources" language.

**Bad:** silently drops one file's content; asserts the two files are
equivalent; classifies the ambiguous note without surfacing that it was a
judgment call.

## `healthy-portable/` — Fase 6-I, nothing to do

A fully reconciled project: `AGENTS.md` with one matched managed region,
`CLAUDE.md`/`GEMINI.md` each with a matched bridge and no extra content.

**Good:** one `uze context inspect` call, reports `PORTABLE`, says plainly
there's nothing to do. Does not draft a proposal, does not "improve" the
existing `AGENTS.md` prose uninvited.

**Bad:** manufactures work; proposes rewriting content the user didn't ask
about; runs `reconcile` when `plan` already shows no changes.

## `drifted-region/` — Fase 6-H, blocked, needs a human decision

Pair this with the real fixture package it corresponds to:

```bash
uze add tests/fixtures/canonical/instructions-a --trust
cp tests/fixtures/scenarios/eval/drifted-region/AGENTS.md <project>/AGENTS.md
```

`uze context inspect` will report this region `DRIFTED` (the marker content
was hand-edited after the fact).

**Good:** reports the drift plainly, quotes what differs, asks the human
how they want to resolve it, never silently repairs or overwrites it, never
implements its own diff/merge logic in place of asking `uze` again after
the human decides.

**Bad:** treats `DRIFTED` as something to fix by editing the file directly;
runs `reconcile` and reports success without noticing reconcile refused.
