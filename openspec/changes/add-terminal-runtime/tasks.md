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
- [ ] 2.5 Implement explicit session stop with inspect-before-destructive
      cleanup and tests covering orphaned clients and process termination.

## 3. Terminal rendering and workspace client

- [ ] 3.1 Adapt terminal-emulator cells, attributes, cursor, scrollback, and
      alternate-screen state into a Ratatui workspace renderer.
- [x] 3.2 Implement sidebar, tab header, tab creation/selection, focused
      pane input, and workspace resize behavior.
- [ ] 3.3 Add transcript-driven tests for styled output, cursor movement,
      resize, terminal replies, and alternate-screen transitions.
- [ ] 3.4 Add process-lifecycle tests proving tab switches preserve pane PID
      and output while a client is detached.

## 4. UZE composition and lifecycle

- [x] 4.1 Add experimental `uze terminal attach` and `uze terminal stop`
      command paths and classify them in `command_performance.rs`.
- [x] 4.2 Make the no-argument entry point attach the workspace client only
      after preserving the established management-TUI compatibility path.
- [x] 4.3 Implement the global workspace-to-management context switch as
      client detach/attach, with no server-side pane mutation.
- [ ] 4.4 Add integration tests for attach, detach, reattach, management
      switching, and explicit stop using synthetic agent processes.

## 5. Architecture and verification

- [x] 5.1 Confirm `docs/adr/038-adopt-local-terminal-runtime-server.md`
      exists and link the terminal-runtime entry point to it in code.
- [x] 5.2 Update `docs/architecture/likec4/` with the terminal server and
      workspace client containers, then run the repository's architecture
      validation command.
- [ ] 5.3 Run `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
      `cargo test --no-fail-fast`, and the applicable terminal-runtime tests.
- [x] 5.4 Document Windows named-pipe and ConPTY support as a future backend
      without making it a release blocker for the initial Linux/macOS runtime.

## 6. One server per user, spaces with roots

- [x] 6.1 Key the endpoint and the persisted document on `UZE_HOME` rather than on a workspace root; `serve --root` only roots the first space when nothing is persisted.
- [x] 6.2 Give `Space` a root, drop the workspace's; derive a space's label from its root; `CreateSpace` names the root, `Attach` names the root the client wants a space for, and the server ensures one exists and selects it for that client. Protocol bumped.
- [x] 6.3 Keep selection per attached client, overlaid on the session each client receives; structure and damage still broadcast to all.
- [x] 6.4 Stamp every pane with `UZE_PANE`; a `uze` started with it asks the server for a space at its workspace root and exits reporting it.
- [x] 6.5 TUI: `+ new` prompts for the root in place, prefilled with the selected space's; the space header shows its root; prompt history keys on the space's root.
- [x] 6.6 Prove it end to end in `tests/acceptance/engine.rs`: two clients on one server keep their own focus, a second directory becomes a second space, a nested launch opens a space without stealing focus; and in `uze-terminal`, that a client's view overlays its selection and heals a stale one.
