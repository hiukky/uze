## Why

Two coding agents editing one checkout collide. The first answer to that
was the seat rule: the first agent works in the primary checkout, every
additional agent gets an isolated one. It closed the agent-versus-agent
collision and opened three others that dogfooding made plain.

The seat mixes the operator and an agent in one working tree. Its own
justification — the first agent "keeps access to the operator's uncommitted
work" — is the collision the policy exists to prevent, with the operator as
the second writer.

An isolated checkout is expensive and disposable. A fresh worktree carries
none of the repository's ignored state, so in a Rust project every isolated
agent starts with a cold `target/` and pays a full build. Nothing is ever
reused and nothing is ever removed, so checkouts pile up in `.worktrees/`
with no path back inside the UI.

Finished work has no way home. `completion` is projected as prose; bringing
a branch back is the operator's manual Git work, outside the TUI, with no
view of state or conflict.

The premise that fixed the first collision still holds and fixes the other
three: UZE launches every agent itself and chooses its working directory.
Isolation delivered as a working directory depends on nothing. What changes
is what the directory is — a reusable slot rather than a disposable
worktree — and that delivery is a mechanism rather than a sentence.

## What Changes

- **Every agent is isolated; the primary checkout belongs to the operator.**
  The seat rule is removed. An agent starts in an isolated checkout in every
  Git repository with a commit, before its harness starts, with no question
  asked. Where isolation is impossible the agent starts in place and the tab
  says so.
- **Checkouts are slots.** `.worktrees/<id>` is a long-lived checkout named
  by a generated identifier. A new agent takes a free slot — new branch from
  the base, tracked and untracked files reset, ignored artifacts preserved —
  and a new directory is created only when no slot is free. The number of
  checkouts is bounded by peak concurrency, not by the number of tasks.
- **Tasks have an immutable identity and a derived label.** The identifier
  keys the slot, the branch (`agent/<id>`) and persisted state; the label
  comes from the prompt and names the tab. A readable branch name is
  produced once, when the branch is first published.
- **Readiness is observed** from the checkout's Git state when the agent's
  pane goes quiet or on demand, never from the agent announcing it and
  without requiring any signal from the harness.
- **Delivery is a mechanism.** One operator action delivers a ready task
  according to `completion`: `handoff` leaves the branch; `merge` rebases in
  the task's checkout, runs the declared gate on the rebased commits and
  fast-forwards the target; `pr` publishes and opens a pull request. Conflicts
  and gate failures return the task to the agent that owns it, with the target
  untouched. Only UZE ever writes the target.
- **Nothing that can hold work is removed automatically.** A dirty orphan is
  parked and listed; an unintegrated branch outlives its directory. What is
  safe goes: branches fully contained in the target, and the directory of a
  clean slot idle beyond a declared age.
- **`agents.lock`: the `worktrees` block grows** from `completion` alone to
  `target`, `completion` (`handoff` | `merge` | `pr`), `link`, `setup`,
  `gate` and `slots`, all optional. `link` is restricted to ignored paths inside the
  repository and rejected at read time otherwise.
- **The write lock lands in `uze-git`**, where its two entry points were
  reserved for it, keyed on the repository's common directory.
- **Projection and Skill follow.** The projected text says the reader is
  already isolated, commits on its own branch, never writes the target, and
  that delivery is UZE's. The `worktree` Skill loses its integration
  guidance.

Space roots, the single terminal server, per-client focus and the nested
launch are decided alongside this change but belong to the
`terminal-runtime` capability, and are carried in `add-terminal-runtime`.

## Non-goals

- No mode that seats an agent in the primary checkout. An agent on the
  operator's own tree is the operator running a harness in a shell tab.
- No rename command, agentic or otherwise. The readable name exists only
  where it matters, at publication.
- No warmed pool, no background replenishment. Reuse is what makes a slot
  cheap; a slot is created on demand.
- No port allocation, compose namespacing or other resource injection. The
  lock gains fields when a project needs them.
- No stacked tasks and no restack. The task model carries `base` and
  `base_commit` so stacking can be added without a migration, and nothing
  reads them yet.
- No automatic delivery when a task reads as ready. A quiet pane also
  means "I need clarification".
- In-session subagents and the operator's own editor are still outside
  UZE's reach and are addressed by the projected text only.

## Capabilities

### New Capabilities
- `worktree-policy`: launch-time isolation of every agent in reusable
  checkouts, task identity, observed readiness, system-performed delivery,
  safe lifecycle, and the projection into the shared baseline.

### Modified Capabilities
- `project-agent-environment`: the `worktrees` block gains `target`, `pr`,
  `link`, `setup`, `gate` and `slots`, with read-time validation of `link`.
- `context`: `inspect`/`plan`/`reconcile` project the updated region.

## Impact

- **`uze-git`** — the repository write lock behind `write`.
- **Core** — `project/checkout` (slots, reuse, parking, adoption),
  `project/task` (identity, label, persisted state), `project/landing`
  (readiness, rebase, gate, deliver); `project_lock` schema; the projected
  text. The existing `worktree` module is absorbed.
- **Application** — a service and read model per surface: launch placement,
  task list with state, delivery, preserved work at startup. `src/` never
  names the domain.
- **Integrations** — none. No vertical learns about slots, and no hook is
  required.
- **CLI/TUI** — agent creation no longer chooses a directory; the sidebar
  shows label and state; one delivery action; the preserved-work list; the
  isolation marker is removed.
- **Extensions** — the Git diff overlay keeps scoping to the tab's checkout.
- **Package** — the `uze` package's `worktree` Skill is rewritten.
- **Docs** — `invariants.md` rewrites the "Concurrent work isolation"
  section; the ADR in this change is evolved in place.
