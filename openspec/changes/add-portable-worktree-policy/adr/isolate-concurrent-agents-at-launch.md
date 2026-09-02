# Isolate every agent in a reusable checkout, and deliver from the system

Status: Proposed

## Context

Two coding agents editing one checkout collide: one overwrites the other's
edits, or stops mid-task on a conflict it cannot resolve. The operator's
requirement is a single guarantee — never start work in a tree that someone
else is writing to.

The first answer was placement at launch: UZE chooses the working directory
of every pane it starts, so isolation can be a property of the environment
an agent is born into rather than an instruction a model has to read,
believe and execute. That premise held, and it stays. The specific rule
built on it — seat the first agent in the primary checkout, isolate the rest,
keep every checkout forever, leave delivery to prose — did not survive
dogfooding.

The seat mixes the operator and an agent in one tree; its own justification,
that the first agent "keeps access to the operator's uncommitted work", is
the collision the policy exists to prevent, with the operator as the other
writer. A fresh worktree carries none of the repository's ignored state, so
in a Rust project every isolated agent pays a cold build, and since nothing
is reused or removed, directories accumulate with no path back inside the
UI. And a finished branch has no way home: `completion` was a sentence in
`AGENTS.md`, and bringing work back was the operator's manual Git.

Three facts already in the codebase shape the replacement. The terminal
runtime already tells a quiet agent pane from a working one. `uze-git`
separates `read` from `write` so a repository write lock has one place to
live. And the terminal runtime persists per-workspace state
under `UzeHome`, keyed on the resolved root.

## Decision

Every agent UZE launches in a Git repository with a commit starts in an
isolated checkout of its own, created before its harness starts. The primary
checkout belongs to the operator and is never assigned to an agent. Nobody
is asked, nothing is configured, and no harness has to cooperate. Where
isolation is impossible — no repository, no commit, no Git — the agent
starts in place and its tab says it is not isolated. There is no mode that
seats an agent in the primary checkout: an agent on the operator's own tree
is the operator running a harness in a shell tab, which is not a UZE agent.

A checkout is a slot, and a task is what comes and goes. `.worktrees/<id>`
is long-lived and named by a generated identifier that never changes. A new
agent takes a free slot — new branch from the base, tracked and untracked
files reset, ignored artifacts preserved — and a directory is created only
when no slot is free. The number of checkouts is bounded by peak
concurrency, and a project may cap it.

A task has an immutable identifier and a derived label. The identifier keys
the slot, the branch and the persisted state; the label comes from the
prompt and names the tab. The branch is `agent/<id>` while local. A readable
name derived from the label is produced once, when the branch is first
published, because that is the only place a human reads it.

Readiness is a Git fact evaluated when the agent's pane goes quiet or on
demand: commits ahead of the base and a clean tree. A dirty tree is
surfaced, not delivered. Nothing is inferred from what the agent says, and
no signal from the harness is required.

Delivery is a mechanism, performed by UZE on an explicit operator action,
one task at a time under the repository write lock, according to the
project's declared `completion`. `handoff` leaves the branch. `merge`
rebases the task's branch onto the target inside the task's own checkout,
runs the declared gate on the rebased commits, and advances the target by
fast-forward only. `pr` publishes the branch and opens a pull request. A
conflict or a failed gate returns the task to the agent that owns it, with
the rebase paused in its slot and the target untouched. The target is
written by UZE, in the fast-forward step, and by nothing else. Rebase is
chosen over merge commits to keep the granular, verified commits agents
already produce, and to keep `git bisect` useful.

Nothing that can hold work is removed automatically. A dirty orphan is
parked and listed. A branch with commits absent from the target is never
deleted. Two removals are safe and automatic: a branch fully reachable from
the target, and the directory of a clean slot idle beyond a declared age,
whose branch stays. Discard is an operator action on a named task.

The lock's `worktrees` block declares what a checkout needs and what happens
to finished work — `completion`, `link`, `setup`, `gate`, `slots` — all
optional, with `link` restricted to ignored paths inside the repository and
validated when the lock is read. The write lock lives behind
`uze_git::write`, keyed on the common directory. Task state lives under
`UzeHome`'s `state/` layout, outside every checkout, written atomically.

The shared instruction file keeps its short projected statement for the one
audience placement cannot reach — a subagent spawned inside a harness
session — and gains three sentences: the reader is already isolated, commits
on its own branch and never writes the target, and delivery is UZE's. It
still never asks for a top-level worktree.

Rejected: keeping the seat as a default or as a mode, for the reasons above.
Rejected: a rename command, since the readable name exists only at
publication. Rejected: a warmed pool, port allocation and compose
namespacing, and stacked tasks with restack — each earns its place when a
project needs it; the task model carries `base` and `base_commit` so
stacking is not a migration. Rejected: delivering automatically when a task reads as ready, because a
quiet pane also means the agent is waiting for an answer. Rejected: making
readiness depend on a hook, since Git already holds the answer and a hook
would tie the core to a capability that is not its own. Rejected: resolving conflicts
without the agent, which is the only party holding the intent.

## Consequences

Easier: the operator's guarantee holds for every agent, and for the operator
too. The second agent in a Rust project builds incrementally. Directories
are bounded by concurrency, not by history. A finished task reaches the
target with one action, linearly, gated on the commits that land, and a
conflict comes back to the one who can resolve it. Preserved work is a list
in the UI rather than a directory to remember. A new harness vertical still
needs no worktree code: placement happens before it starts, readiness is read
from Git, and the text rides the context bridge.

Harder: the write lock covers UZE's own writes, not an agent's own Git; the
design relies on agents writing only their branch, which the projected text
states and the fast-forward step verifies. `merge` updates the operator's
working tree, because the target lives there; overlap with uncommitted
changes refuses delivery. `setup` runs project-declared code at first
launch, and whether it inherits the consent boundary of remote executable
capabilities is settled during implementation. Subagents spawned inside a
harness session and the operator's own editor remain placed by nobody. The
`agents.lock` schema change is additive; the branch and directory naming
change is not, and legacy checkouts are adopted rather than renamed.

Source change: openspec/changes/add-portable-worktree-policy/
