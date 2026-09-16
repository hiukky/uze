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

## What Changes

- **A space has a kind, chosen when it is created.** A *worktree* space
  places every agent in an isolated slot, exactly as today. A *workspace*
  space places every agent in the space's own directory, on whatever
  branch it is on. The kind is part of the space's identity: one root may
  carry one of each. The picker offers the worktree kind only when the
  root is a Git repository with a commit.
- **An agent of a workspace space is a tenant, not a task.** It has an
  identity, a harness, a conversation and a lifetime; it has no branch, no
  checkout, no readiness, no delivery, no name and no preserved work. A
  tenant ends when no live pane carries its identity, and its root may be
  a directory that is not a repository. It is recorded in the store keyed
  by its space's root, under the same lock and the same sweep as that
  root's tasks.
- **Placement that cannot do what was asked refuses.** An agent asked for
  in a worktree space either gets a slot or is not started, with the
  reason stated; the "no checkout" fallback that opened a tab in the
  operator's tree is removed.
- **The sidebar draws each kind its own way.** A worktree space keeps the
  tree. A workspace space lists its agents flat, one row each, carrying
  the tab's label and the harness running at the row's edge, with the
  selected row marked by a vertical accent bar in place of the selection
  glyph; the branch and its upstream sync appear once, on the space's
  header.
- **A space can be created from the keyboard.** The `new-space` action
  gains a default chord and a handler; today it exists in the vocabulary
  and only the mouse reaches it.
- **The projected declaration describes both cases**: a reader inside
  `.worktrees/` is isolated and commits on its own branch; a reader
  anywhere else in the project is on the operator's branch and commits
  there, never switching or resetting it.

## Capabilities

### New Capabilities
- `space-kinds`: the two kinds of space, how the kind is chosen and kept,
  what an agent of each kind is, how placement refuses, and how each kind
  is presented.

### Modified Capabilities
- `worktree-policy`: `Every agent is isolated and the primary checkout
  belongs to the operator` is scoped to agents launched into a worktree
  space; `The declaration is projected without triggering foreign
  isolation` describes the reader outside an isolated checkout. Its spec
  lives in the still-open `add-portable-worktree-policy` change and is
  edited there in place, with the task it reopens pointing here; no delta
  is carried in this change. That change's design non-goal is reversed in
  place as well.
- `terminal-runtime`: a space carries a kind the server persists and never
  interprets, and a root may carry more than one space when their kinds
  differ.
- `agent-session-continuity`: `A managed agent's conversation is recorded
  against its task` records a conversation against any agent UZE launches,
  tenants included.

## Impact

- **`uze-terminal`** — `Space.kind`, `CreateSpace.kind`, persistence and
  the open-existing-space rule keyed on root and kind. Protocol bump. An
  architecture rule forbids the server naming the kind's vocabulary.
- **Core** — `project/tenant`, the agent identifier shared by task and
  tenant, `TaskStore.tenants`, schema version; the projected text.
- **Application** — `Placement { Slot, Tenant }` and a placement that
  returns an error instead of a fallback; `AgentRow::{Task, Tenant}` as the
  sidebar's read model; tenant occupancy in the per-repository sweep.
- **CLI/TUI** — the sidebar's per-space body extracted into rows built once
  and two layouts; the flat layout; the picker's kind choice; the
  keyboard `new-space` action.
- **Docs, spec and Lab** — the worktree-policy design and spec edited in
  place; `docs/architecture/invariants.md`; the `worktree` Skill;
  `web/content/docs/workspace.mdx` before the journeys that prove it; two
  new journeys; the hand-kept copy of the projected text in
  `conformance/contract/isolation.py`.
- **Depends on** `identify-agents-at-launch`: a tenant has no directory of
  its own, so its identity must come from the launch.
