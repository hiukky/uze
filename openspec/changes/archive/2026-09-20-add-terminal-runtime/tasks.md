## 1. Runtime foundation

- [x] 1.1 Remove the experimental terminal orchestration implementation so no
      alternate runtime path remains active.
- [x] 1.2 Add `crates/uze-terminal` to the Cargo workspace with a
      dependency boundary that excludes Core, Application, and integrations.
- [x] 1.3 Add the selected PTY and terminal-emulation dependencies, locking
      versions compatible with the workspace MSRV.
- [x] 1.4 Implement serializable session, workspace, tab, pane, focus, and
      layout state with deterministic identifiers and unit tests.

## 2. Persistent server and local transport

- [x] 2.1 Define versioned platform-local transport requests and events for attach,
      detach, snapshots, input, resize, terminal damage, tab/pane lifecycle,
      and stop.
- [x] 2.2 Implement Unix socket and Unix PTY backends for Linux/macOS behind
      portable transport and PTY ports.
- [x] 2.3 Implement per-user, per-workspace runtime discovery, server
      startup, liveness checks, and safe stale-endpoint recovery.
- [x] 2.4 Implement PTY ownership, child-process lifecycle, reader/writer
      loops, terminal replies, and resize propagation for each pane.
- [x] 2.5 Implement explicit session stop with inspect-before-destructive
      cleanup and tests covering orphaned clients and process termination.
      `runtime.rs` `stop()` and `ClientRequest::Stop`; inspect-before-
      destructive in `a_claim_this_build_cannot_name_is_reported_rather_than_called_stopped`,
      termination in `a_stopped_pane_takes_its_process_group_with_it`, and
      orphaned clients in `a_client_that_stops_reading_is_bounded_and_resynchronized`.

## 3. Terminal rendering and workspace client

- [x] 3.1 Adapt terminal-emulator cells, attributes, cursor, scrollback, and
      alternate-screen state into a Ratatui workspace renderer
      (`src/ui/orchestrator/render.rs`, fed by `uze-terminal`'s snapshots).
- [x] 3.2 Implement sidebar, tab header, tab creation/selection, focused
      pane input, and workspace resize behavior.
- [x] 3.3 Add transcript-driven tests for styled output, cursor movement,
      resize, terminal replies, and alternate-screen transitions:
      `transcript_preserves_style_cursor_and_alternate_screen`,
      `resize_changes_snapshot_dimensions`,
      `snapshot_renders_the_scrollback_viewport`, and
      `a_resize_to_the_largest_number_on_the_wire_leaves_the_server_answering`.
- [x] 3.4 Add process-lifecycle tests proving tab switches preserve pane PID
      and output while a client is detached
      (`pane_process_keeps_output_until_explicit_stop`, plus
      `process_probe.rs` for identifying the pane's process).

## 4. UZE composition and lifecycle

- [x] 4.1 Add experimental `uze terminal attach` and `uze terminal stop`
      command paths and classify them in `command_performance.rs`.
- [x] 4.2 Make the no-argument entry point attach the workspace client only
      after preserving the established management-TUI compatibility path.
- [x] 4.3 Implement the global workspace-to-management context switch as
      client detach/attach, with no server-side pane mutation.
- [x] 4.4 Add integration tests for attach, detach, reattach, management
      switching, and explicit stop using synthetic agent processes:
      `an_attach_replaces_only_a_server_it_can_name`,
      `a_second_client_attaches_to_a_live_server_of_another_build`,
      `stop_is_heard_as_a_first_frame_by_a_server_nobody_attached_to`, and
      `tests/acceptance/engine.rs`'s
      `two_clients_keep_their_own_focus_and_a_nested_launch_opens_a_space`.

## 5. Architecture and verification

- [x] 5.1 Confirm `docs/adr/038-adopt-local-terminal-runtime-server.md`
      exists and link the terminal-runtime entry point to it in code.
- [x] 5.2 Update the Mermaid diagrams under `docs/architecture/` with the
      terminal server and workspace client containers, then run the
      repository's architecture validation (`cargo test -p uze-extensions`).
      Both appear in `containers.mmd` and `crate-layering.mmd`. (This task
      originally named `docs/architecture/likec4/`, retired since.)
- [x] 5.3 Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
      `cargo test --no-fail-fast`, and the applicable terminal-runtime tests.
      Green on 2026-09-20: fmt and clippy clean, 2144 passed / 0 failed /
      8 ignored across 36 test binaries.
- [x] 5.4 Document Windows named-pipe and ConPTY support as a future backend
      without making it a release blocker for the initial Linux/macOS runtime.

## 6. One server per user, spaces with roots

- [x] 6.1 Key the endpoint and the persisted document on `UZE_HOME` rather than on a workspace root; `serve --root` only roots the first space when nothing is persisted.
- [x] 6.2 Give `Space` a root, drop the workspace's; derive a space's label from its root; `CreateSpace` names the root, `Attach` names the root the client wants a space for, and the server ensures one exists and selects it for that client. Protocol bumped.
- [x] 6.3 Keep selection per attached client, overlaid on the session each client receives; structure and damage still broadcast to all.
- [x] 6.4 Stamp every pane with `UZE_PANE`; a `uze` started with it asks the server for a space at its workspace root and exits reporting it.
- [x] 6.5 TUI: `+ new` prompts for the root in place, prefilled with the selected space's; the space header shows its root; prompt history keys on the space's root.
- [x] 6.6 Prove it end to end in `tests/acceptance/engine.rs`: two clients on one server keep their own focus, a second directory becomes a second space, a nested launch opens a space without stealing focus; and in `uze-terminal`, that a client's view overlays its selection and heals a stale one.

## 7. Choosing a root, and one agent at a time

- [x] 7.1 TUI: `+ new` chooses the root from the directories that exist — the input names a directory to list and a segment to match inside it, the listing narrows on every keystroke, `Tab` descends, `Enter` creates, and the picker has the sidebar to itself while open.
- [x] 7.2 A tab belongs with the agent it was born from: `Tab` carries it, `CreateTab` names it, the snapshot persists it by position, and closing an agent hands its shells to the space instead of leaving them dangling. Protocol bumped.
- [x] 7.3 TUI: the tab strip shows one context — the agent leads it, the shells opened alongside it follow, no other agent's appear; a shell opened by hand starts in the agent's own directory; a space's own row lands on a shell belonging to no agent.
