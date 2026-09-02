## Context

See [proposal.md](proposal.md). The current Ratatui application owns one
alternate-screen client and starts short-lived workers for management
operations. It has no durable process owner for interactive panes. The new
runtime must preserve the repository's one-way product dependency direction
and must not turn package/environment maintenance into a daemon.

## Goals / Non-Goals

**Goals:**

- Provide a local client/server terminal runtime whose server owns PTYs and
  pane lifecycles.
- Preserve a running pane while a user changes tabs, detaches a workspace
  client, or enters the existing management TUI.
- Render terminal cells, attributes, cursor, resize, and alternate-screen
  state through a maintained terminal-emulation engine.
- Keep workspace/tab/pane layout and focus as serializable application state,
  distinct from PTY handles and reader/writer tasks.

**Non-Goals:**

- Remote attachment, multi-user collaboration, session migration across
  machines, or live server-binary handoff.
- Agent-specific behavior, inference, or a public automation API in the
  initial release.
- Replacing the existing management TUI, Store, Engine, Router, Application,
  or harness integrations.

## Decisions

### Introduce a dedicated terminal-runtime crate

`crates/uze-terminal` will own terminal domain state, server runtime, local
transport, and terminal rendering adapters. It depends on neither
`uze-application` nor harness-specific integrations. The root binary remains
the composition root that starts either a management client or a workspace
client.

This prevents PTY/runtime concerns from leaking into `uze-core`, whose domain
remains package and harness neutral. It also lets the runtime be tested with
synthetic commands without invoking product maintenance workers.

### Use a local server as the durable process owner

`uze terminal attach` starts or attaches to one per-user, per-workspace local
server identified through UZE-owned runtime state. The server owns PTY masters,
child-process groups, readers, writers, terminal emulators, and session state.
An attached client owns only presentation and input forwarding. Linux and
macOS use a Unix-domain socket and Unix PTY backend in the initial release.

The server retains PTYs when the workspace client detaches. The explicit stop
operation is the only lifecycle path that intentionally terminates panes.
This is a terminal-runtime exception to ADR-037's maintenance-daemon
prohibition; it performs no maintenance, network polling, package mutation,
or harness reconciliation. See ADR produced by this change.

### Model state separately from runtime handles

The serializable model contains `Session`, `Workspace`, `Tab`, `Pane`, focus,
layout, labels, and terminal dimensions. Runtime objects map pane identifiers
to PTY handles, child handles, terminal emulator state, and reader/writer
tasks. Runtime objects are rebuilt from model state only for newly created
panes; an existing live pane is always represented by its current PTY.

### Use a maintained terminal-emulation engine

The runtime will use `alacritty_terminal` for VT parsing, terminal grid,
scrollback, cursor/mode state, and terminal replies. The workspace client
adapts the engine's cells to Ratatui rendering rather than flattening output
into plain text. PTY creation and child-process control use a maintained,
cross-platform PTY abstraction.

This deliberately rejects direct stdout inheritance and a plain-text parser:
both bypass the layout or lose terminal semantics that interactive agents rely
on.

### Use a versioned, private local protocol with platform backends

Clients connect through a UZE-owned platform-local endpoint with a versioned
request/event protocol. The initial Unix implementation uses a
permission-restricted Unix-domain socket. A future Windows implementation
uses a named pipe with an owner-only ACL and a ConPTY backend. The protocol
covers attach/detach, session snapshots, focus, tab and pane lifecycle, input
bytes, resize, terminal damage, and stop; it is not a public agent automation
API.

### Keep the existing TUI as a separate client context

The management TUI continues to run its existing event loop and workers.
Switching to management detaches the workspace client; returning attaches a
fresh workspace client to the already-running server. The global switch is a
client concern and cannot kill or directly mutate a pane.

### Architecture documentation

The LikeC4 model will add the terminal runtime server and workspace client as
containers, show the root CLI/TUI as their composition point, and preserve the
existing Core/Application/Integration relationships.

**One server per user, not per repository.** The first iteration keyed the
server on the resolved workspace root, so a repository had a server of its
own and every space in it. That put two layers on one job: the server's
root and a space's root meant the same thing, and a person with three
projects had three servers to attach to. The server is now one per
`UZE_HOME`; the space is the unit that carries a root. What the launch
directory decides is which space the client lands in, never which server.
Persisted state moves from one file per root to one file for the user;
per-root files from before are simply not read — this is pre-1.0 and a
migration would outlive the mistake it patches.

**A space is born from a root.** Its label derives from the root, its first
shell opens there, an agent created in it starts from it, and whether the
root is a Git repository with a commit is what decides whether that agent
gets a slot. Two spaces may share a root: they are organisation, and the
repository's slot pool is the repository's. There is no "global" space; a
space rooted at the home directory is a space whose root has no Git.

**Selection is per client, structure is shared.** Two terminals attached to
one server used to move each other's cursor, because `selected_space` and
each space's `selected_tab` lived in the shared session. They still do, as
the server's defaults; each client's own selection is kept beside its event
channel and overlaid on the session it receives, healing lazily when what it
points at is gone. Damage and structure broadcast to everyone; selection
never does.

**A `uze` inside a pane opens a space and leaves.** The server stamps every
pane with `UZE_PANE`. A `uze` that finds it does not attach a client — a
client inside a client is never what that means — but asks the running
server for a space rooted at its directory, created when none is, and
reports it. The way `tmux` inside `tmux` is refused, except that here the
request has a useful answer.

## Risks / Trade-offs

- [Terminal compatibility is broader than text rendering] → Use the selected
  emulator engine and add transcript-driven tests for shell, agent, resize,
  cursor, and alternate-screen behavior before expanding interactions.
- [A background server can leave stale runtime state] → Use a per-session PID
  record and socket liveness check; a stale socket is removed only after its
  owning server is proven absent.
- [Client/server protocol changes can strand an old server] → Version the
  protocol from the first release and report incompatibility without stopping
  existing panes.
- [A live server broadens the attack surface] → Bind only a user-private local
  endpoint; never expose TCP or accept commands from a different OS user.
- [Cross-platform PTY behavior differs] → Support Linux/macOS first and keep
  transport and PTY implementations behind platform-neutral runtime ports so a
  future Windows backend can use named pipes and ConPTY without redesign.

## Migration Plan

1. Remove the experimental in-process terminal orchestration path before the
   new runtime is exposed.
2. Add the terminal-runtime crate, private session storage, and attach/stop
   CLI surface behind an experimental feature boundary.
3. Add the workspace client and management-context switch after server/client
   attach is verified with synthetic processes.
4. Retain existing no-argument management behavior behind an explicit
   compatibility decision during implementation; no package or harness state
   migrates.
