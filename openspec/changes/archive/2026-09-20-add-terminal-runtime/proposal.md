## Why

UZE's management TUI cannot currently host multiple interactive agent sessions
without either terminating them during navigation or reducing their terminal
behavior to an incomplete text rendering. Users need a native terminal
workspace where agent processes outlive UI-context changes and can later be
reattached safely.

## What Changes

- Add an opt-in, local terminal runtime with a persistent server that owns
  workspace tabs, panes, PTYs, and their child processes.
- Add an attachable workspace client alongside the existing management TUI.
- Make the workspace and management surfaces independently switchable without
  terminating a running pane.
- Define a minimal workspace model: one project workspace containing tabs and
  panes, with a sidebar that reflects the active structure.
- Add explicit lifecycle commands for attaching to and stopping a terminal
  session.
- Keep terminal orchestration separate from package installation, harness
  projection, and bounded environment maintenance.

## Capabilities

### New Capabilities

- `terminal-runtime`: Own and render persistent local terminal workspaces,
  including attach/detach lifecycle and management-context switching.

### Modified Capabilities

- None.

## Impact

- New terminal-runtime crate and platform-local transport abstraction.
- Root CLI/TUI gains workspace and terminal-session entry points.
- New PTY and terminal-emulation dependencies, isolated outside Core,
  Application, and harness integrations.
- Architecture model and a numbered ADR are updated to document the new
  client/server runtime boundary.
