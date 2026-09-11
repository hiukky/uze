## Context

See proposal.md - Why/What Changes. The workspace TUI client
(`src/ui/orchestrator.rs`) already has one full-frame, dismiss-with-Esc
popup mechanism used twice — `AgentPicker` and the tab/space
close-confirmation `ContextMenu` — both `Option<T>` fields on
`WorkspaceModel`, rendered last (on top of everything else) when `Some`,
discarded on Esc or a click outside their own content. This change adds a
third popup of the same shape, sized to the full frame instead of a small
anchored box.

`crates/uze-terminal` (the server) is untouched: the client already tracks
each pane's live `cwd` (`Pane.cwd`, kept current by the existing
foreground-process ticker), which is all this needs to know where to run
`git`.

## Goals / Non-Goals

**Goals:**
- A read-only, contextual (active tab's `cwd`) git changes + diff view,
  opened from a tab-strip button or `Ctrl+G`, dismissed with `Esc` back to
  the exact prior state.
- Syntax-highlighted diff content, matching the visual bar VS Code's own
  diff view sets.

**Non-Goals:**
- Stage/unstage/commit/discard, or any other write to the working tree,
  index, or repository (proposal.md's Impact section already excludes
  this; restated here because it bounds several implementation choices
  below — no watch-for-external-edits concerns, no confirmation flows).
- A real multi-pane split layout (rejected earlier in favor of full-frame,
  per the user's own UX exploration this design is downstream of).
- Any change to `crates/uze-terminal` or the client/server protocol.

## Decisions

**Shell out to the `git` CLI, parse its text output, rather than a git
library (e.g. `git2`/libgit2 bindings).** The workspace TUI already
shells out to external programs for comparable "run a thing, read its
output" needs (the PATH shim resolves and execs harness binaries; the
foreground-status probe reads `/proc` directly rather than linking a
process-introspection library). `git status --porcelain=v1` and unified
`git diff` output are stable, scriptable, well-documented formats — no
need to take on a native-library binding (build-time linking
considerations, a much larger API surface than this needs) for two
subcommands. Not an ADR candidate: confined entirely to one new module's
internals, swappable later without touching anything outside it — a
routine implementation choice, not a durable architectural commitment.

**Syntax highlighting via `syntect`.** Same reasoning covered in the prior
conversation with the user: it's what `bat`/`delta` use for this exact
niche, ships bundled grammars/themes (`load_defaults_newlines`/
`load_defaults`, no asset files to manage), and only needs
foreground-color spans (not full editor-grade highlighting) to color diff
content. `tree-sitter` was considered and rejected for this pass — it
needs a separate grammar crate per language, more dependency surface for
no benefit here. This is the one decision here worth an ADR — a new
external dependency, per the `adr` artifact's own qualifying criteria.

**No LikeC4 model change.** This adds no new container, no new component
distinguishable from the existing `workspaceClient` container in
`docs/architecture/likec4/model.c4`, and no new relationship between
modeled elements — `git` here is an implementation detail of one client
-side view (comparable to how the client already reads `/proc` directly
without that appearing in the model), not a system uze integrates with
the way Claude Code/Codex/OpenCode are modeled. `syntect` is a library
dependency, not a modeled element, same as `ratatui`/`portable-pty`
already aren't.

**Own module (`src/ui/git_diff.rs`), own hit-testing type
(`GitViewHit`).** Declared as a sibling of `orchestrator`/`management` in
`src/ui.rs` (mirroring how `view` already sits alongside them, used by
`management.rs`). `orchestrator.rs` is already large; this keeps the new
subsystem's git/parsing/highlighting/rendering code out of it, and keeps
`WorkspaceHit` from growing overlay-internal variants (`AgentPicker` took
the same approach: its own `PickAgent(usize)` variant lives on
`WorkspaceHit` today, but a whole second overlay's internal navigation
doesn't need to).

**Synchronous fetch on open, not a background thread.** `git status`/
`git diff` are fast for the sizes of change a "quick peek" implies; this
matches the existing popups' own synchronous-open behavior (`AgentPicker`
opening does no I/O, but nothing in this client currently defers work to
a background thread the way `management`'s `worker.rs` does for heavier
Application-facade calls). If this proves janky on a very large diff in
practice, moving the fetch to a background thread with the exact
`worker.rs` pattern is the natural upgrade — not needed for this pass.

## Risks / Trade-offs

- **A very large diff renders slowly or floods memory** → out of scope
  for this pass (matches the "not needed for this pass" call above); if
  it becomes a real problem, cap rendered lines with a "diff truncated"
  footer rather than parsing unboundedly.
- **`git` not on `PATH`, or the active tab's `cwd` isn't inside a repository**
  → both are ordinary, expected conditions (not exceptional failures):
  the overlay opens either way and shows that condition in place of a
  changed-files list (see specs/terminal-runtime's added scenarios),
  never a panic or a refusal to open.
- **Active tab's `cwd` moves (further `cd`) while the overlay is already
  open** → the view keeps the directory it snapshotted at open time (per
  proposal.md, contextual to the tab *at the moment it opens*); reopening
  (`Esc` then `Ctrl+G` again) picks up the new `cwd`. No live-tracking
  needed for a "quick peek" popup.

## Open Questions

None — the one decision here worth an ADR (`syntect` as the new
dependency) is flagged above for the `adr` artifact rather than left open.
