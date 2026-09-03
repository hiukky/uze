## Context

See `proposal.md` for motivation. Two facts about the current code shape
this design:

- The sidebar's per-space agent list and the horizontal tab strip are both
  *filtered, ordered views* over the one `Vec<Tab>` in `Space.tabs`
  (`render.rs`'s `agent_tabs` filters to `agent_identity_for_tab(...)
  .is_some()`; the strip's `strip` filters to `tab.agent == context`).
  Neither view keeps an order of its own.
- `SelectTab`/`CloseTab`/`RenameTab` all name only a `TabId` — the server
  locates that tab's owning `Space` by searching every space
  (`space_containing_tab_mut`), never taking a `space` field from the
  client. `CreateTab` is the one exception, and only because it has no tab
  yet to search by.
- Drag-to-resize (sidebar width, content dividers, the Git tree width) is
  the one existing drag pattern: `MouseEventKind::Down` on a dedicated
  handle rect arms a `dragging_*` field, each following `Drag` event
  recomputes the target value from the pointer's *absolute* position (not
  a delta), and `MouseEventKind::Up` clears the flag. Nothing is sent to
  the server mid-drag for those — they're client-local. Reordering differs
  in that the mutation is shared session state, so nothing is sent to the
  server until release (see Decisions below).
- `ClientRequest`'s shape is covered by `PROTOCOL_VERSION`
  (`crates/uze-terminal/src/protocol.rs`): adding a variant requires
  bumping it, with a comment explaining what changed, matching every prior
  bump recorded above the constant.

## Goals / Non-Goals

**Goals:**
- One server-side primitive and one client-side gesture pattern serve both
  the sidebar drag and the strip drag.
- A plain click (select a tab) and a double-click (rename) keep working
  exactly as today; a drag must require deliberate movement before it
  visibly does anything, so it never fires on an ordinary click.
- The two views (sidebar, strip) never need their own sync logic — they
  stay consistent because they render the same underlying order.

**Non-Goals:**
- Moving a tab between spaces, between an agent's shells and another
  agent's, or between an agent tab and the space's own shells. A drag only
  ever offers drop targets from the tab's own group; anything else is a
  no-op (confirmed with the requester).
- Reordering spaces themselves.
- Keyboard-driven reordering (e.g., an "move tab" shortcut). Nothing here
  precludes adding one later against the same server primitive.

## Decisions

### Server: `ReorderTab { tab, before }`, no `space` field
Adds `ClientRequest::ReorderTab { tab: TabId, before: Option<TabId> }` and
`Session::reorder_tab(&mut self, tab: TabId, before: Option<TabId>) ->
bool`, matching `rename_tab`'s shape and validation contract (searches for
`tab`'s space, returns whether anything actually moved so the caller knows
whether to broadcast). `before: None` moves `tab` to the end of its
space's `tabs`; `Some(id)` moves it to sit immediately before `id`.

Validation, in order: `tab` must exist (else `false`); if `before` is
`Some`, it must name a tab *in the same space* as `tab` (else `false` —
mirrors `add_tab`'s existing "an agent from elsewhere is dropped" guard,
just refusing instead of silently substituting, since a caller with a
stale/foreign `before` sent something no drag of this client's own could
have produced); if `before == Some(tab)` or resolves to `tab`'s own
current position, `false` (already there). Implementation removes `tab`
from `Space.tabs`, then reinserts it before `before`'s (now-shifted) index
or at the end.

**Alternative considered**: taking `space: SpaceId` explicitly, as the
proposal first sketched. Rejected for the same reason `SelectTab` et al.
don't take it — the client already knows which tab it's dragging, and
requiring `space` too invites a request naming a `tab`/`space` pair that
don't agree, which then needs its own validation branch for no benefit.

**Alternative considered**: two requests, `ReorderAgent`/`ReorderShellTab`,
so server-side validation could enforce the group constraint (agent stays
among agents, shell stays among that agent's shells) directly. Rejected:
the group constraint is already guaranteed by construction, not by
validation — a drag's own hit-testing (below) only ever considers drop
targets already in the dragged tab's group, so `before` never names a tab
outside it. A single request stays simpler and matches every other
tab-scoped request being group-agnostic (`CloseTab`, `RenameTab` don't
care whether a tab is an agent's or a shell either).

### Client: arm-on-threshold, indicator-only during drag, commit-on-release
`WorkspaceModel` gains one field, `dragging_tab: Option<DraggingTab>`,
where:
```
struct DraggingTab {
    tab: TabId,
    /// Row (sidebar) or column (strip) the drag started at — the
    /// threshold origin.
    origin: u16,
    /// Set once movement has passed the threshold; before that, no
    /// indicator is drawn and no group/target is computed yet.
    armed: bool,
}
```
Mousedown on a draggable row (`WorkspaceHit::SelectTab` in the sidebar or
the strip — the same hit both already resolve to) keeps its existing
immediate `SelectTab` behavior unchanged, and additionally sets
`dragging_tab = Some(DraggingTab { tab, origin: <row or column>, armed:
false })`. Nothing else changes yet — a plain click looks identical to
today's.

Each `Drag` event while `dragging_tab.is_some()`:
1. If not yet `armed`, arm once the pointer has moved past a small
   threshold from `origin` (a couple of rows/columns — enough to rule out
   an accidental jitter within a single click, small enough that dragging
   still feels immediate). Below the threshold: nothing renders, nothing
   is computed, exactly like a plain unmoving click.
2. Once armed, hit-test the pointer's current row/column against the
   *currently rendered* rects of the tab's own group (reusing the
   existing `hits: Vec<(Rect, WorkspaceHit)>` list already populated at
   render time — no separate index is kept) to find the nearest boundary
   between two of that group's rows/tabs, and record it as the pending
   drop (`before: Option<TabId>`). If the pointer has left the group's
   own area (a different space's rows, a different agent's strip, blank
   space) the pending drop is cleared instead — no indicator, and a
   release in that state is a no-op.
3. Render draws a thin accent-colored insertion line at the pending drop's
   boundary (between two sidebar rows, or between two strip tabs) whenever
   one is armed and pending. The underlying rows/tabs themselves are never
   reordered client-side during the drag — only this indicator moves.

`MouseEventKind::Up`: if `dragging_tab` was `armed` with a pending drop
that differs from `tab`'s current position, send `ReorderTab { tab,
before }`. Either way, clear `dragging_tab`.

**Why an indicator instead of a live-reordered local list**: `Space.tabs`
is shared session state, broadcast to every attached client
(`broadcast_session`). If this client optimistically reordered its own
copy mid-drag, a `Session` broadcast arriving mid-drag (another client
renamed a tab, closed one, anything) would have to be reconciled against
that local reorder or risk visibly snapping back. Never touching the
underlying order until the server confirms it — and recomputing the
indicator's position from the pointer's absolute location and the
*latest* rendered rects on every `Drag` event, not from an accumulated
delta — sidesteps that entirely, the same way the sidebar-width drag
already recomputes width from an absolute column rather than integrating
deltas (see Context). It also means a `Drag` event's coalescing on
terminals that report it coarsely (see Risks) never leaves the indicator
in a stale position: whatever the next `Drag` event's position is, that's
what's shown.

**Alternative considered**: reordering a local scratch copy of the
relevant tab slice live, snapping it back if the eventual server response
disagrees. Rejected — adds a reconciliation path for a rare race, for a
visual benefit (seeing rows actually swap places while dragging, instead
of an indicator line) that most terminal drag UIs of this shape don't
provide either.

### Group scoping reuses the existing filters, not a new concept
"The dragged tab's group" is computed with the exact same predicates
`render.rs` already uses to build `agent_tabs` and `strip` — an agent row
belongs among the other tabs in its space with
`agent_identity_for_tab(...).is_some()`; a strip tab belongs among the
other tabs in the selected space with `.agent == context`. No new
grouping concept is introduced; the drag's hit-testing filters `hits` by
the same rule the render pass used to produce those rects in the first
place.

## Risks / Trade-offs

- **Coalesced or absent `Drag` reporting** (`PaneSnapshot::reports_drag` /
  `MouseMode`, already a known per-terminal constraint — see
  `protocol.rs`) → mitigated by the indicator always being computed from
  the pointer's current absolute position rather than a delta (a missed
  intermediate `Drag` event just means the indicator jumps straight to
  where the pointer next reports, never drifts wrong); a terminal that
  never reports `Drag` at all simply never arms a reorder, and plain
  click-to-select is unaffected either way.
- **A tab or its drop target disappears mid-drag** (closed by another
  client, or by a concurrent `CloseTab`/`RenameTab` broadcast arriving
  while this client is dragging) → mitigated the same way `rename_tab`
  already handles a stale target: `reorder_tab` re-resolves both `tab` and
  `before` against the *current* session at commit time and returns
  `false` (no broadcast) if either is gone; client-side, if the dragged
  tab itself vanishes from a `Session` update received mid-drag,
  `dragging_tab` is cleared so the eventual release is a no-op rather than
  reordering something the pointer was never actually over.
- **Threshold tuned wrong** (too small: accidental drags on a slightly
  shaky click; too large: dragging feels unresponsive) → not a
  correctness risk, just a feel one; picked empirically during
  implementation and easy to adjust in one place (`DraggingTab`'s
  threshold constant) since nothing else depends on its exact value.
