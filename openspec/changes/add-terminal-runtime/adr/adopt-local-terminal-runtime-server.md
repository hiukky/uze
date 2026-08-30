# Adopt a local terminal runtime server

Status: Accepted

## Context

UZE needs to preserve interactive agent processes while users switch between a
workspace-orchestration surface and the existing management TUI. An
in-process TUI cannot survive closing or changing its client without owning
terminal process state independently. Direct stdout inheritance bypasses the
layout, while a plain-text terminal parser loses the terminal semantics used
by interactive programs.

This decision introduces a long-lived local process boundary. ADR-037 rejects
permanent daemons for environment maintenance because that work can mutate
machine configuration and poll external state; terminal process ownership is
a distinct, opt-in runtime concern.

## Decision

UZE will provide an opt-in local terminal runtime server that owns PTY masters,
child-process groups, terminal-emulation state, and workspace/tab/pane state.
Workspace and management UIs will be attachable clients; detaching a client
will not terminate panes, and an explicit terminal-session stop operation is
the only normal destructive lifecycle path.

The runtime will live in a dedicated `uze-terminal` crate, outside
`uze-core`, `uze-application`, and `uze-integrations`. It will use a
user-private, versioned platform-local transport and `alacritty_terminal` for
terminal emulation. Linux and macOS are the initial targets, using Unix-domain
sockets and Unix PTYs; a future Windows backend will use named pipes and
ConPTY behind the same runtime ports. A pre-existing external multiplexer was
rejected because it would make UZE's session model and cross-context lifecycle
dependent on an external server. A direct in-process runtime was rejected
because it cannot support reattachment after a client exits.

## Consequences

Interactive panes can remain live across tab changes, UI-context changes, and
client detach/reattach, while package installation and harness maintenance
remain bounded operations. The product gains a clear runtime boundary and can
test terminal state independently from the current TUI.

UZE must now maintain a local protocol, stale-socket recovery, PTY lifecycle
safety, and compatibility for the supported terminal-emulation surface. The
initial runtime intentionally excludes remote attachment, public automation,
and live server handoff.

## Implementation Plan

- Add `crates/uze-terminal/Cargo.toml` and `crates/uze-terminal/src/` with
  serializable session state and runtime-owned PTY handles isolated behind its
  public API; add the member in the root `Cargo.toml`.
- Add `portable-pty` 0.9 and `alacritty_terminal` 0.26 only to the terminal
  crate, preserving the workspace MSRV and keeping them out of Core,
  Application, and integrations.
- Add platform-local endpoint discovery, server startup, attach, detach, and
  explicit stop operations under the UZE-owned runtime directory in `UzeHome`;
  implement Unix sockets and PTYs first, behind portable transport and PTY
  ports reserved for future named-pipe and ConPTY backends.
- Render the selected pane through `alacritty_terminal` cell state in a new
  workspace-client module under `src/ui/` without modifying existing
  management-route rendering.
- Add root CLI composition in `src/main.rs` and a global context switch in
  `src/ui.rs` that only attaches or detaches clients.
- Add synthetic-process integration tests under `tests/terminal_runtime/` and
  extend LikeC4 under `docs/architecture/likec4/` with terminal server and
  workspace client containers.

## Verification

- [ ] A synthetic long-running pane stays alive after its client detaches and
      a second client attaches.
- [ ] Switching between workspace and management leaves the pane PID and
      terminal state intact.
- [ ] Styled output, cursor positioning, resize, and alternate-screen
      transitions are verified through terminal transcripts.
- [ ] Unix-socket permissions reject access from a different operating-system user.
- [ ] `cargo test`, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
      and LikeC4 validation pass.
