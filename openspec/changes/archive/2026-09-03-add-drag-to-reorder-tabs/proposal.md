## Why

Today the order of an agent's row in the sidebar's per-space list, and of a
shell tab in the horizontal strip for the agent it was opened alongside, is
fixed at creation time — the only way to change it is closing tabs and
reopening them in the order wanted. Neither order carries any meaning the
user chose; it should, since both are exactly how the user scans and clicks
between agents and their shells while working.

## What Changes

- New drag interaction in the sidebar: dragging an agent's row up or down
  within its space reorders it relative to the other agent rows of that
  same space.
- New drag interaction in the horizontal tab strip: dragging a shell tab
  left or right reorders it relative to the other shell tabs shown in that
  same strip (the ones opened alongside the same agent, or the space's own
  shells when no agent is in front).
- New protocol request, `ClientRequest::ReorderTab { tab, before }` —
  matching `SelectTab`/`CloseTab`/`RenameTab`'s existing shape, it names no
  space; the server locates `tab`'s own space by searching, the same way
  those do. It moves `tab` to sit immediately before `before` within that
  space's `tabs` (`before: None` moves it to the end) — handled
  server-side by mutating `Space.tabs`'s order directly. The sidebar list
  and the horizontal strip
  are both already filtered views over that one ordered vector (see
  `render.rs`'s `agent_tabs` and `strip` derivations), so this one
  primitive drives both interactions without either view needing its own
  reorder logic.
- A drag only ever offers drop targets from the same group the dragged tab
  is already showing among (other agent rows for an agent row; the other
  tabs of the same strip for a shell tab) — the request itself does not
  change what an existing move already couldn't: `tab`'s own `agent` field,
  which tab is selected, or the space it belongs to.
- Dropping outside the strip/list, or with no net movement, is a no-op:
  nothing is sent and the tab stays where it was.
- Out of scope, per explicit confirmation: moving a tab from one space (or
  one agent) to another, and reordering spaces themselves.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `terminal-runtime`: adds a tab-reordering requirement — dragging a tab in
  the sidebar or the horizontal strip changes `Space.tabs`'s order, and the
  server accepts and applies that reordering.

## Impact

- `crates/uze-terminal/src/protocol.rs`: new `ClientRequest::ReorderTab`
  variant.
- `crates/uze-terminal/src/state.rs`: new `Session`/`Space` mutation
  applying a reorder, validating that `tab` and `before` (when given) name
  tabs of the same space — an invalid or stale pair is rejected rather than
  silently reordering something else.
- `src/ui/orchestrator.rs` (mouse handling, `WorkspaceHit`,
  `WorkspaceModel`), `src/ui/orchestrator/render.rs` (sidebar and tab-strip
  rendering, hit-rect registration, in-progress-drag insertion indicator):
  the new drag-to-reorder gesture, built on the same mousedown-arms /
  `Drag`-updates / release-commits shape the existing sidebar/panel
  drag-resize already uses, plus a press-move distance threshold so a plain
  click-to-select is never reinterpreted as a drag.
- New `WorkspaceModel` fields tracking an in-progress tab drag (which tab,
  current insertion point), mirroring `dragging_sidebar`/`dragging_panel`.
- Tests: `crates/uze-terminal` state tests for the new mutation and its
  validation; `src/ui/orchestrator/tests.rs` for the mouse-event sequence
  driving a reorder and the resulting outgoing request.
