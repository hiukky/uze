## Context

### The log was never UZE's

Before this change, every mutating vendor command ran with inherited stdio
(`Command::status()`), so the vendor CLIs wrote their own progress straight
into the terminal: `codex plugin add`'s "Added plugin `x` from marketplace
`y`", Gemini's consent narrative, "WARNING: ... Refusing to create helper
binaries", and so on. In the CLI this interleaved with UZE's own report; in
the TUI it wrote across the alternate screen and corrupted the layout.

The user-facing consequence is ownership ambiguity: the user cannot tell
UZE's log from the harness CLIs', and the vendor lines are written in the
vendor's vocabulary. The user's framing is the requirement: **the log
should be UZE's, designed by UZE; harness logs are internal signals that
feed UZE's own status, not surface material.**

### Three report sections collapsed into data

The install report printed, per harness:
1. `Package delivery to <h>: Native (N components)` — route + count
2. an indented, full-sentence `evidence` paragraph
3. `Attached to <h>: <location>` — the attachment receipt location

Sections 1 and 3 overlap (same harness, same delivery), and section 2 is a
narrative better owned by `doctor`/`plugin inspect` and opt-in detail.

## Decisions

### D1 — Vendor CLI output is captured; UZE owns the log

All mutating vendor commands run through `shared::process::capture`:
- stdin is null, so an accidentally interactive vendor prompt fails fast
  instead of hanging a captured pipeline;
- stdout/stderr are captured and **discarded on success**;
- on failure, the error carries `exit status` plus the vendor's own last
  non-empty lines (capped, so a megabyte-long vendor error stays one
  diagnostic line). The vendor's words are evidence *about the failure*,
  never a primary log stream.

Rejected alternative: `--verbose` passthrough of vendor output. The vendor
CLI's vocabulary (marketplace names like `uze-store`, installed
roots) is exactly what confused the user; UZE's read models already state
the same facts in UZE's terms, and a `doctor`/`inspect` view is the right
place for raw system evidence.

### D2 — One compact line per harness

`render_add_report` prints, per plan: `{harness}: {route}` plus
`({location})` when an attachment was recorded for that harness — e.g.
`claude-code: native (claude-plugin-generated:flow@uze-store)`.
Attachments that are not package delivery (Agent Skill symlinks in the
shared skills root, like opencode's) still get their own line:
`{harness}: attached at {location}`. Nothing is dropped from the default
view; only prose is moved out.

### D3 — Evidence is opt-in via `--verbose`

A global `--verbose` flag (also accepted by the project shorthand's own
arg parser) prints each harness's `evidence` sentence indented under its
line. The evidence strings themselves are unchanged — they are UZE's
wording describing UZE's generated vs. explicit delivery.

### D4 — JSON and other output shapes unchanged

`--format json` keeps the full `AddPluginReport` (plans, evidence,
attachments) — machine consumers get everything; humans get the compact
view.

## Impact

| File | Change |
|---|---|
| `crates/uze-integrations/src/shared/process.rs` | new `capture` / `run_quiet` / `failed_message` helpers (+ unit tests) |
| `codex.rs`, `codex/plugin.rs`, `codex/mcp.rs` | plugin/marketplace/mcp calls capture output |
| `claude.rs`, `claude/plugin.rs`, `claude/mcp.rs` | same |
| `gemini.rs`, `gemini/extension.rs`, `gemini/mcp.rs` | same |
| `src/main.rs` | global `--verbose`, `ShorthandArgs.verbose`, `render_add_report` in both install paths |
| `tests/cli.rs` | assertions on the new compact lines |
| vendor inspection calls | unchanged (already `.output()`) |

## Non-goals

- No change to vendor CLI *behavior* or UZE's attachment semantics — same
  commands, same receipts, same state.
- No change to JSON output.
- No change to `doctor`/`plugin inspect` evidence text or TUI internals
  (the TUI inherits the fix for free: vendor noise can no longer reach the
  alternate screen).
- No ADR: presentation choice, reversible, not architecturally significant.

## LikeC4

No model update: no component/dependency/relationship change.
