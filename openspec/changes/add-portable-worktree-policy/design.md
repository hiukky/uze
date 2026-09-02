## Context

UZE launches every agent it runs and chooses the working directory of every
pane. That fact made isolation a launch-time decision rather than an
instruction, and it remains the foundation. The first design built on it
seated the first agent in the primary checkout and isolated the rest, kept
every checkout forever, and left delivery to prose. Dogfooding showed the
three costs: the operator and an agent sharing one tree, a cold build per
isolated agent with directories accumulating, and no path from a finished
branch back to the target inside the UI.

Three things are already in place and shape the design: the terminal
runtime already tells a quiet agent pane from a working one, which is the
signal behind today's `Completed` status; `uze-git` separates `read` from
`write` precisely so a repository write lock has one place to live; and the
terminal runtime persists per-workspace state under `UzeHome`'s `state/`
layout, which task state follows.

## Decisions

**Isolation is performed, not requested.** Unchanged. Delivering isolation as
text would make the guarantee depend on a model reading, believing and
executing prose. The projected text keeps the one job placement cannot do:
addressing writers UZE never sees.

**Every agent is isolated; the primary checkout is the operator's.** The seat
rule is removed. Its benefit — the first agent sees uncommitted work — is
the collision the policy exists to prevent, with the operator as the other
writer. Its cost — a fresh checkout per agent — is removed by slots below,
which leaves the seat with no defence. An agent on the operator's own tree
is a legitimate need ("write the commit for what I changed") and is served
by the operator running a harness in a shell tab, which is not a UZE agent
and has nothing to deliver.

**A checkout is a slot; a task is what comes and goes.** `.worktrees/<id>`
is long-lived and named by a generated identifier that never changes.
Acquiring a slot is `switch -c agent/<task-id>` from the base, `reset
--hard`, `clean -fd` without `-x`: tracked and untracked files of the
previous task go, ignored artifacts stay. This is what makes the second Rust
agent build incrementally, and it bounds the number of directories by peak
concurrency. A slot is created only when none is free. No warmed pool and no
background replenishment: reuse is the optimisation, and a slot on demand is
cheap enough.

**Identity is not the label.** The task identifier keys the slot, the branch
and the persisted state. The label is derived from the prompt and names the
tab. The branch is `agent/<id>` while local; the readable name derived from
the label is produced once, at publication in `pr` mode, which is the only
place a human reads it. In `merge` mode the branch never leaves the machine.
This removes the rename problem instead of solving it.

**Readiness is a Git fact, evaluated when the pane goes quiet.** Commits
ahead of the base and a clean tree is ready; a dirty tree is surfaced and
not offered for delivery. The evaluation is idempotent, so a quiet pane that
resumes simply flips the state back. No model instruction, no output
parsing, and no signal from the harness: a hook-delivered end-of-turn event
could sharpen the moment later, but nothing depends on one. Delivery is
never triggered by readiness itself, because a quiet pane also means the
agent is waiting for an answer.

**Delivery is serialized under the write lock, in the task's checkout.** One
task at a time: rebase the task's branch onto the target's tip inside its own
slot, run the declared gate on the rebased commits, fast-forward the target.
Rebasing in the task's checkout keeps the target untouched until the last
step; running the gate after the rebase tests the commits that will land,
not a base that no longer exists; `ff-only` makes a half-finished merge
impossible. The second task rebases onto a target that already contains the
first. The lock lives behind `uze_git::write`, keyed on the common
directory, because a lock a second module can spawn Git around is worthless.

**The target is declared, and its tip is read from where it lives.** The
lock names the branch finished work targets, defaulting to the branch the
primary checkout is on when a task is created. In `pr` mode the target is
protected and lives at the remote, so its tip is the remote-tracking branch
after a fetch, and the operator's local branch is out of the flow entirely.
In `merge` and `handoff` the target is the local branch. Switching a project
from direct commits to a protected branch is one field, `completion`, and
changes nothing about slots, readiness, rebase or the gate.

**A live task follows the target on its own.** When the target has moved
and a task's pane goes quiet with a clean tree, its branch is rebased onto
the target under the same rules as delivery, conflicts returned to the
owner. No manual mode: every harness already refreshes its own view of the
tree between turns, and the one unsafe moment — a rebase under an agent
mid-edit — is excluded by the clean-tree condition, not by a setting.

**Rebase, not merge commits.** Agents already produce granular, verified
commits; a rebase keeps them one by one and keeps `git bisect` useful. This
is a deliberate choice, recorded so it is not revisited.

**Conflicts go to the owner, with the rebase paused.** The agent that wrote
the branch is the only party holding the intent. The rebase stays paused in
its slot, with markers in place, and a message with the conflicting files
and the target's range is written into its pane. The next evaluation reads
Git: rebase finished and tip descending from the target's tip means the
gate runs and delivery continues; anything else keeps the task in conflict,
visibly. UZE does not touch a slot while its rebase is paused.

**The target lives in the operator's tree, and `merge` says so.** A
fast-forward on the checked-out target updates the primary working tree;
that is what Git does. If the operator has uncommitted changes to files the
task touched, Git refuses and UZE reports it without writing anything. The
rule is narrowed from "any dirty primary" to real overlap.

**Nothing that can hold work is removed automatically.** A dirty orphan is
parked, never deleted. A branch with commits absent from the target is never
deleted. Two removals are safe and automatic: a branch fully reachable from
the target, and the directory of a clean slot idle beyond a declared age,
whose branch stays. Discard is an operator action on a named task. This is
what keeps three slots from becoming thirty after a busy week.

**Materialisation is minimal and declared.** `link` symlinks ignored files
from the primary — `.env` and friends — and `setup` prepares the checkout.
`link` is validated at read time: relative, inside the repository, ignored.
A `setup` failure warns and launches; a checkout without dependencies is
still better than no agent. `copy`, ports and compose namespacing are not
added; the lock gains a field when a project needs one. `setup` and `gate`
run through the bounded subprocess helper and inherit the hook surface's
timeout and output limits.

**Adoption at startup, prune last.** The isolation directory, the `agent/`
branches and the persisted state are reconciled when a repository's space
starts. Legacy checkouts become tasks labelled from their branch, and no
branch is renamed since it may have been pushed. `git worktree prune` runs
after adoption so a registry entry is never dropped before its directory has
been looked at.

**Task state lives under `UzeHome`, outside every checkout.** `state/`
already holds the terminal's per-workspace snapshot, keyed on the resolved
root; task state uses the same identity and layout, is written atomically
and carries a schema version. Removing a worktree can never remove history.

**The task model carries `base` and `base_commit` from the first commit**,
without behaviour. Stacking is what the operator already does by hand and
retrofitting a graph onto flat tasks is a migration; carrying two fields is
not.

**The projection must not trigger foreign isolation.** Unchanged, and the
text gains three statements: the reader commits on its own branch, never
writes the target, and delivery is UZE's.

**The region's identity carries the rendered content's digest.** Unchanged.

**The replaced lock key fails loudly.** Unchanged, and extended: unknown
fields inside the policy block are rejected by name, while the top level
stays tolerant so a lock from a newer UZE still loads.

## Risks

- **Subagents are out of reach.** A writer spawned inside a harness session
  is never placed by UZE. The projected text asks; the harness complies or
  does not.
- **The target can be written by convention only.** Git refuses to check out
  the target in a second worktree, but refs are shared and an agent could
  write `main` without checking it out. No harness does; the projected text
  forbids it; the fast-forward step detects an unexpected move.
- **The write lock covers UZE, not the agent's own Git.** Each agent writes
  only its own branch, and every shared write — target, worktree add and
  prune, branch deletion — is UZE's. A paused rebase is the one state where
  an agent and UZE could touch the same slot, and UZE stays out of it.
- **`merge` writes into the operator's tree.** The overlap rule keeps this
  from destroying anything, but it is a surprise the first time.
- **`setup` runs project-declared code on first launch.** Remote executable
  capabilities cross an explicit consent boundary in this project. Whether
  the lock's `setup` inherits that boundary is decided during
  implementation and recorded in the invariants.
- **Two spaces on one repository share one slot pool.** Correct by design;
  the state file needs its own lock for it to be safe.

## Candidate ADRs

- **Isolate every agent in a reusable checkout, and deliver from the
  system** — every agent gets a slot, the primary belongs to the operator,
  readiness is read from Git, delivery is rebase-gate-fast-forward under
  the write lock, and nothing holding work is removed automatically. Hard to
  reverse: it fixes the on-disk layout and branch naming, adds lock fields,
  makes delivery a product mechanism, and puts the write lock in the Git
  transport.
