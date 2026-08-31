---
name: worktree
description: Coordinates isolated Git worktrees for concurrent agent work, deciding when to create one and safely validating and integrating completed changes. Use when work involves parallel agents, an existing worktree, a risky or long-lived change, merge conflicts, or bringing isolated implementation back to the primary branch.
slash: true
metadata:
  opencode/autoinvoke: "true"
---

# UZE — concurrent worktree coordinator

Use Git worktrees to isolate concurrent *writes*, not as ceremony for every
request. The outcome is a stable primary checkout, one owner per isolated
branch, and an integration that is reviewed, tested, and reversible.

## Decide before editing

Start with read-only discovery from the repository root:

```bash
git rev-parse --show-toplevel
git branch --show-current
git status --short
git worktree list --porcelain
```

Always inspect the `agents.lock` at the **primary worktree** before choosing a
worktree location. `git worktree list --porcelain` identifies that checkout;
read its lock, rather than assuming that the linked worktree's relative path
has the same meaning. The lock's optional `worktrees_dir` is the project's
portable directory policy. When it is configured, resolve it relative to the
primary worktree and use that directory for every newly created agent
worktree. Do not substitute a convenient sibling path, and never edit
`agents.lock` as a side effect of this workflow.

If `agents.lock` is absent or has no `worktrees_dir`, say so before using a
user-supplied or conventional sibling directory. If the lock is malformed,
the configured path is unsafe, or an existing worktree sits outside the
configured directory, stop and ask for direction; do not silently bypass the
project policy.

Create or reuse an isolated worktree before making changes when any of these
are true:

- another agent or developer can write in the current checkout;
- the task is a feature, refactor, investigation likely to produce edits, or
  otherwise more than a tiny, self-contained change;
- the primary checkout has uncommitted work that must be preserved;
- the requested work needs an independent test/build cycle or a clean review
  boundary.

A read-only task, a user-directed edit in an already-owned worktree, or one
small urgent fix may stay in the current checkout. Say which choice you made
and why. If the primary checkout is dirty, never stash, reset, clean, or move
its changes to make room for an agent. Create a separate worktree instead.

## Prepare an isolated workspace

One worktree has exactly one writer. Do not assign two agents to the same
path or branch; do not have a worker edit the primary checkout.

1. Choose a concise, unique topic branch, such as `agent/<topic>`.
2. Confirm the intended base branch and its current commit. Do not assume
   `main` is the primary branch.
3. Inspect existing worktrees and branches first. Reuse a worktree only when
   it is clean, its branch is the intended one, and its prior owner has
   explicitly handed it off. Otherwise create a topic directory beneath the
   `agents.lock` `worktrees_dir` (when configured), with a path that makes the
   topic identifiable:

   ```bash
   git worktree add -b agent/<topic> <configured-worktrees-dir>/<topic> <base-branch>
   ```

   With no configured directory, agree on the location first. If the branch
   already exists, omit `-b` only after confirming that it is the intended
   branch and no other worktree owns it.
4. Work, build, and test exclusively from that new path. Report the path,
   branch, and base commit in the handoff.

Do not create a worktree inside another worktree, share generated artifacts
between worktrees, or use destructive Git commands to resolve ownership.
Repository-level Git metadata is shared, so avoid rebasing, force-pushing, or
deleting branches while workers are active.

## Coordinate the implementation

Split parallel work by file or component boundaries. Before two workers begin,
state their owned paths and the integration order. If their changes cannot be
made disjoint, sequence them rather than hoping Git can merge them later.

Each worker must leave a handoff with:

- the worktree path, branch, and resulting commit SHA;
- a short change summary and the exact checks run;
- known failures or files likely to conflict;
- a clean working tree, with intentional uncommitted work called out instead
  of hidden.

Workers may commit their own focused changes. They must not merge into,
rebase, reset, or otherwise advance the primary branch unless the user has
explicitly asked them to perform integration.

## Integrate deliberately

Only begin integration after every selected worker has handed off a commit.
Inspect the primary branch again: current branch, worktree ownership, status,
and the commits to integrate. A dirty primary checkout, an unknown worktree
owner, or an uncommitted worker is a stop condition; preserve the state and
ask for direction rather than trying to repair it.

Present the integration order, commit SHAs, and checks to run. Get explicit
approval before changing the primary branch unless the user has already
clearly authorized integration in this request.

Integrate one worker branch at a time into a clean, up-to-date primary branch.
Prefer a fast-forward when it preserves the intended history; use a merge
commit when it preserves a meaningful independent branch. Do not squash,
rebase, or force-push somebody else's work without explicit direction.

If a conflict occurs:

1. stop the merge and identify the overlapping ownership or semantic choice;
2. resolve only when the intended behavior is clear from the task and tests;
3. otherwise abort the merge to return to the known clean state and ask the
   user to decide.

Run the relevant formatter, tests, and checks after the final integration.
Report the resulting primary-branch commit and any checks not run.

## Retire worktrees safely

After successful integration and confirmation that no follow-up work remains,
inspect each worker worktree. Remove only a clean, merged, non-current
worktree. Preserve its branch by default; delete it only when the user asks
or the repository's documented policy permits it. Never use force removal to
discard uncommitted work.

Finish with a compact ledger: primary commit, integrated branches, retained
worktrees, removed worktrees, and any remaining risks. This makes the next
agent's starting point unambiguous.
