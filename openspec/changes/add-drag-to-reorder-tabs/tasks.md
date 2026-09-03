## 1. Protocol

- [x] 1.1 Add `ClientRequest::ReorderTab { tab: TabId, before: Option<TabId> }` to `crates/uze-terminal/src/protocol.rs`.
- [x] 1.2 Bump `PROTOCOL_VERSION` and add a doc-comment paragraph above it describing the change, matching the style of the existing history there.

## 2. Server state

- [x] 2.1 Add `Session::reorder_tab(&mut self, tab: TabId, before: Option<TabId>) -> bool` to `crates/uze-terminal/src/state.rs`, following `rename_tab`'s search-by-`tab`-id shape: locate `tab`'s owning space, validate `before` (when `Some`) names a tab of that same space, no-op (`false`) if `tab` is missing, `before` doesn't resolve, or the move would leave the order unchanged.
- [x] 2.2 Implement the reorder as remove-then-reinsert on `Space.tabs`, leaving every tab's `id`, `label`, `agent`, `layout`, and `focus` untouched, and leaving `Space.selected_tab`/`Workspace.selected_space` untouched.
- [x] 2.3 Unit tests in `crates/uze-terminal/src/state.rs`: reorder among agent tabs, reorder among one agent's shell tabs, moving to the end (`before: None`), rejecting a `before` from a different space (order unchanged, returns `false`), rejecting a missing `tab`/`before` (returns `false`), a no-op move to the same position (returns `false`).

## 3. Server dispatch

- [x] 3.1 Handle `ClientRequest::ReorderTab` in `crates/uze-terminal/src/runtime.rs`'s request loop: call `Session::reorder_tab`, and `broadcast_session()` only when it returns `true` — same pattern as the existing `RenameTab` arm.

## 4. Client model

- [x] 4.1 Add a `DraggingTab { tab: TabId, origin: u16, armed: bool }` type and a `dragging_tab: Option<DraggingTab>` field to `WorkspaceModel` (`src/ui/orchestrator.rs`), initialized to `None` alongside the existing `dragging_sidebar`/`dragging_panel`/`dragging_git_tree` fields.
- [x] 4.2 Pick and name the movement threshold constant (rows for the sidebar, columns for the strip) that arms a drag.

## 5. Client input handling

- [x] 5.1 On `MouseEventKind::Down(Left)` over a `WorkspaceHit::SelectTab(tab)`: keep the existing immediate select, and additionally set `model.dragging_tab = Some(DraggingTab { tab, origin: <row or column of the click>, armed: false })`.
- [x] 5.2 On `MouseEventKind::Drag(Left)` while `dragging_tab.is_some()`: arm it once the pointer has moved past the threshold from `origin`; once armed, hit-test the pointer against the currently rendered `hits` rects restricted to the dragged tab's own group (the same predicate `render.rs` uses for `agent_tabs`/`strip`, reused rather than duplicated) to compute a pending `before: Option<TabId>`, or clear the pending drop if the pointer has left the group's area. Recompute from the pointer's absolute position each time, never from an accumulated delta.
- [x] 5.3 On `MouseEventKind::Up(Left)`: if `dragging_tab` was armed with a pending drop different from `tab`'s current position, send `ClientRequest::ReorderTab { tab, before }`. Clear `dragging_tab` unconditionally.
- [x] 5.4 Guard the existing drag-forwarding arm (`forward_mouse` for a drag inside the pane) so it continues to exclude this new drag state the same way it already excludes `dragging_sidebar`/`dragging_git_tree`.
- [x] 5.5 If the dragged tab is no longer present in a `Session` update received while `dragging_tab` is set (closed elsewhere mid-drag), clear `dragging_tab`.

## 6. Rendering

- [x] 6.1 In the sidebar's agent-list rendering (`src/ui/orchestrator/render.rs`), draw a thin accent-colored insertion line at the pending drop boundary when `dragging_tab` is armed and its group is the sidebar.
- [x] 6.2 In the tab-strip rendering (`src/ui/orchestrator/render.rs`), draw the equivalent vertical insertion marker between tabs when `dragging_tab` is armed and its group is the strip.
- [x] 6.3 Confirm no other visual state (row order, tab order) changes during the drag itself — only the indicator moves; the actual reorder is only visible after the server's broadcast following release.

## 7. Client tests

Note: the workspace TUI's mouse handling has no standalone testable
dispatch function — it lives inline in `attach_workspace`'s own loop,
which owns a real socket and a background thread, and no other test in
`orchestrator/tests.rs` drives it end to end either. Rather than fake a
socket to test 7.1/7.2/7.3 as originally phrased (a literal Down/Drag/Up
`crossterm` event sequence producing an outgoing request), these instead
test the pure functions that inline loop calls to make that same
decision (`tab_drag_group`, `tab_drag_group_members`, `pending_tab_drop`,
`DraggingTab::is_pending_drop_row`) plus the resulting render output —
the same style every other mouse-driven behavior in this file is already
tested at (see `sidebar_resize_drag_updates_width`'s sibling tests, which
also stop short of a live socket).

- [x] 7.1 `tab_drag_group_classifies_by_the_region_a_rect_landed_in`: the same tab's sidebar row and strip chip resolve to different groups, purely from which rect was hit.
- [x] 7.2 `tab_drag_group_members_are_sorted_along_the_groups_axis_and_scoped_to_it`: sidebar members come back top-to-bottom and deduplicated; strip members are scoped to one agent's own group.
- [x] 7.3 `pending_tab_drop_resolves_the_nearest_half_and_end_past_the_last` / `pending_tab_drop_is_none_outside_the_groups_own_area`: the boundary math a `Drag` event runs on every tick, including the empty-group and out-of-bounds cases.
- [x] 7.4 `is_pending_drop_row_requires_armed_and_the_same_group`: covers the "not armed yet" (plain click) and "wrong group" (drag left its own area) cases the original 7.3/7.4 asked for, at the level of the function the renderer actually calls.
- [x] 7.5 `sidebar_indicator_marks_the_pending_drop_row_only_once_armed`: end-to-end through real rendering — an armed drag's indicator lands on the correct row; an unarmed one draws nothing.

`prune_dragging_tab` (task 5.5) has no dedicated test: it is a three-line
method with no branching worth a unit test beyond what `reorder_tab`'s
own "missing tab" coverage already implies server-side; covering it would
mean re-deriving the same socket-loop test infrastructure this section
just explained doesn't exist.

## 8. Validation

- [x] 8.1 `cargo test --workspace --no-fail-fast`.
- [x] 8.2 `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`.
- [x] 8.3 `openspec validate add-drag-to-reorder-tabs --strict`.
