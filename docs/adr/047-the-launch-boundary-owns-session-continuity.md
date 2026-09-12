# The launch boundary owns session continuity

Status: Accepted

## Context

An agent UZE launches into a task starts a conversation with its harness.
Until now, coming back to that task — after a restart, a crash, or the
operator picking a preserved task back up — started a stranger in the
same checkout: the same directory, the same branch, none of the context.

The obvious place to fix that is the client: have the TUI build
`claude --resume <id>` when it creates the tab. It fails exactly where
the operator is hurt. The terminal server's restore path replays a
persisted argv with no client running, so a pane the server respawns
never passes through the client at all; and an assigned `--session-id`
replayed a second time is an error, not a resume. Making the client
rewrite persisted commands would put session knowledge into
`uze-terminal`, whose whole point is that it carries no domain.

The other candidate was the PATH shim — an experimental mechanism that
projects `AGENTS.md` into a harness without writing into the project.
Putting the resume decision there makes the shim load-bearing for a
user-visible guarantee, which is a boundary that is expensive to move
later.

## Decision

The launch boundary owns session continuity. The shim decides; the client
never composes a resume argv.

Per launch the shim asks: am I in a slot, whose task is it, does that
task have a conversation for me — and prepends the integration's own
start or resume arguments. A pane the server respawns goes through the
same boundary and gets the same answer, which is why the restart case
needs no code in the client. The cost is one lexical path match, one
small JSON read, and on a first launch one small write: no subprocess, no
network, nothing that scales with the Store, so the shim's stated budget
holds.

Two things follow from putting it there:

- **Agents UZE creates are launched through the shim's absolute path**
  (`shims_dir/<name>`), not by bare name. Relying on PATH would make
  continuity depend on the operator having put `~/.uze/shims` ahead of
  the real binary, which `transparent-harness-attachment` deliberately
  does not require. Where the shim is absent, the launch falls back to
  the bare name and continuity is simply unavailable — stated, not
  silently missing.
- **Continuity is a declaration on `IntegrationPort`, not a second
  capability.** One declaration (`Assigned` | `Observed` |
  `Unsupported`) and three verbs, on the trait the conformance suite
  already proves across four harnesses. `Assigned` mints the identifier
  before the process starts; `Observed` reads it back afterwards, off the
  hot path, from what the harness leaves behind. A harness that declares
  neither gets a fresh conversation, which is what it does today.

## Consequences

The shim stops being experimental in practice: a user-visible guarantee
now runs through it. Every failure path in it falls open to the launch
that happens today, so a missing or broken shim costs continuity and
nothing else — but the mechanism can no longer be removed casually, and
`tests/integrations/runtime_boundary.rs` is now guarding more than recursion.

Adding a harness means answering the continuity question in its
integration and nowhere else; the client, the terminal runtime and the
domain stay unchanged. `Observed` is the weaker half of the bargain: for
Codex it means reading a vendor-private rollout file, which is read and
never written, and a shape change there degrades to a fresh conversation
rather than an error.

The operator's own invocation is never rewritten — only a bare launch of
an agent UZE created is touched.

Source change: openspec/changes/add-agent-session-continuity/
