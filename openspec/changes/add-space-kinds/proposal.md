## Why

Every space is a worktree space: an agent created in it is placed in a slot
of its own, on a branch of its own, and its work comes back through
delivery. That is the right default and the wrong rule. Two harnesses on
one tree — one writing, one reviewing — a quick fix committed straight to
the branch the operator is on, a harness pointed at a directory that is not
a repository: today each of these is either impossible or a degraded
fallback that opens the tab with a warning. The design of
`add-portable-worktree-policy` named "a mode that seats an agent in the
primary checkout" as a non-goal; dogfooding has shown it is a second way of
working, not a lesser one, and it deserves a mechanism of its own rather
than an exception in the existing one.

**The first round of this change put that mechanism on the space, and
dogfooding it found the choice in the wrong place.** The operator is asked
which way of working they want before the agent exists — before the
question the agent is for has been asked. Wanting to ask one thing costs a
checkout; wanting both ways over one project costs two spaces over one
directory, which this change had to allow on purpose. The axis is right
and the object is wrong: isolation belongs to the agent, which is where
the domain had already put it. This round moves it there and removes the
kind.

## What Changes

**The first round shipped two kinds of space and dogfooding found the
choice in the wrong place** — before the agent exists, before anyone knows
what it is for. Asking a question costs a checkout; a project that wants
both ways of working costs two spaces over one directory, which the first
round had to allow explicitly. The axis was right and the object was
wrong: it belongs to the agent, which is where the domain already put it
(`PlacementKind`, `Placement`, the identifier a task and a tenant share).

- **A space has no kind.** It is a directory somebody opened, and any
  directory can be one. One root, one space.
- **One record per agent, with isolation as a field.** An agent has an
  identity, a harness, a lifetime, and *optionally* a checkout and a
  branch. The separate tenant record is removed, and with it the question
  "which kind of record is this identifier". **BREAKING** for the task
  document's schema.
- **Isolation is an action on one agent.** `Isolate` acquires a slot as
  the project's policy says, records the isolation against the same
  identity, and relaunches the agent there continuing the conversation it
  was in. Offered only inside a Git working tree with a commit to branch
  from.
- **The work follows the agent, and the operator's tree is never
  touched.** A clean root isolates with no question. A dirty one asks
  whether to carry a copy of the changes into the checkout; either answer
  leaves the root's working tree exactly as it was, and nothing is
  discarded.
- **A project may declare that its agents start isolated**, in the
  `worktrees:` block it already has. That is where a policy belongs —
  declared, versioned, the same for everyone — rather than in a container
  each person creates by hand.
- **Placement that cannot do what was asked refuses.** Isolating without
  a slot leaves the agent where it is, with the reason stated; the "no
  checkout" fallback that opened a tab in the operator's tree is removed.
- **The column groups agents by where they work.** The agents in the
  space's root first, the isolated ones after, a blank row between the
  two groups only when both exist, and a colour per group. Isolating
  moves the row from one group to the other, which is how the operator
  sees it happened.
- **A space can be created from the keyboard.** The `new-space` action
  gains a default chord and a handler; today it exists in the vocabulary
  and only the mouse reaches it.
- **The projected declaration describes both cases**: a reader inside
  `.worktrees/` is isolated and commits on its own branch; a reader
  anywhere else in the project is on the operator's branch and commits
  there, never switching or resetting it.

## Capabilities

### New Capabilities
- `agent-isolation`: what a space is, what an agent is, how one agent is
  isolated and what that costs, how the project declares a default, and
  how the column tells the two apart.

### Modified Capabilities
- `worktree-policy`: `Every agent is isolated and the primary checkout
  belongs to the operator` is scoped to agents launched into a worktree
  space; `The declaration is projected without triggering foreign
  isolation` describes the reader outside an isolated checkout. Its spec
  lives in the still-open `add-portable-worktree-policy` change and is
  edited there in place, with the task it reopens pointing here; no delta
  is carried in this change. That change's design non-goal is reversed in
  place as well.
- `terminal-runtime`: a space is a root and nothing else — the kind leaves
  the protocol and the persisted workspace, and a root names one space.
- `agent-session-continuity`: `A managed agent's conversation is recorded
  against its task` records a conversation against any agent UZE launches,
  and the record survives that agent being isolated.

## Impact

- **`uze-terminal`** — `Space.kind` and `CreateSpace.kind` removed from the
  protocol and from the persisted workspace; the open-existing-space rule
  keyed on root alone. Protocol bump, and a persisted document of an older
  version, read by the rule `upgrade-resilience` states. 57 mentions in
  `state.rs`, 9 in `runtime.rs`, 3 in `protocol.rs` — all of them deleted,
  none left as a field nobody reads.
- **Core** — one agent record with `isolation: Option<…>`; `Tenant` and
  `AgentRecord` deleted (the latter has no caller outside its own module);
  the harness recorded against every agent rather than inferred from the
  running process; task-document schema bump.
- **Application** — `isolate(agent)` built from the two halves that exist
  (`place_in_slot`, `resume_task`); the placement at launch decided by the
  project's declared default; the sweep and slot occupancy reading one
  collection.
- **CLI/TUI** — the two groups in a space's column, the colour per group,
  the `Isolate` action in the agent's own menu, the row moving between
  groups; the picker loses the kind row, the `⇄` kind toggle,
  `choose_kind`, `slots_available`, `RootProfile` and the
  repository-reach filter, since any directory can be a space.
- **Manifest** — `worktrees.default` in `agents.yaml`, read where the
  placement at launch is decided.
- **Docs, spec and Lab** — the worktree-policy design and spec edited in
  place; `docs/architecture/invariants.md`; the `worktree` Skill;
  `web/content/docs/workspace.mdx`; the journeys that prove isolation and
  the upgrade; `conformance/contract/isolation.py`'s copy of the
  projected text.
- **Depends on** `identify-agents-at-launch`: an agent that is not
  isolated has no directory of its own, so its identity must come from the
  launch.
