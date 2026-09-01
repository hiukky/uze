---
name: worktree
description: Runs concurrent agent work in isolated Git worktrees — assigning one writer per branch, collecting handoffs, and integrating finished branches back into the primary checkout. Use when coordinating more than one writing agent, when resuming work in an existing worktree, when integrating or merging an agent branch, or when a conflict or an unknown worktree owner needs resolving.
slash: true
metadata:
  opencode/autoinvoke: "true"
---

# UZE — concurrent worktree coordinator

You do not decide whether to work in isolation, and you do not create your
own top-level worktree. UZE places every agent it launches: the first agent
in a repository works in the primary checkout, and every additional live
agent is started inside an isolated checkout of its own. Read the
"Concurrent work isolation" section of `AGENTS.md` — it states the layout and
what to do with finished work.

This skill is what no harness does for you: coordinating several writers and
integrating their branches back.

## Know where you are

```bash
git rev-parse --show-toplevel
git branch --show-current
git status --short
git worktree list --porcelain
```

If your working directory is inside `.worktrees/`, you are already isolated —
work here, and do not create another worktree or switch branches. If you are
in the primary checkout, you are the only agent there; treat the operator's
uncommitted work as theirs, and never stash, reset, clean, or move it.

## Give parallel subagents their own checkout

UZE cannot see subagents you spawn inside your own session, so isolating them
is yours to do. Before two of them write files, give each one its own
checkout, resolved against the *primary* so one worktree never nests inside
another:

```bash
git worktree add -b agent/<topic> \
  "$(git rev-parse --path-format=absolute --git-common-dir)/../.worktrees/<topic>" HEAD
```

One checkout has exactly one writer. Split the work by file or component
boundary and state each owner's paths before they start. If their changes
cannot be made disjoint, sequence them rather than hoping Git can merge them
later.

Repository-level Git metadata is shared across worktrees, so do not rebase,
force-push, or delete branches while other writers are active.

## Coordinate the implementation

Split parallel work by file or component boundaries. Before two workers
begin, state their owned paths and the integration order. If their changes
cannot be made disjoint, sequence them rather than hoping Git can merge them
later.

Each worker must leave a handoff with:

- the worktree path, branch, and resulting commit SHA;
- a short change summary and the exact checks run;
- known failures or files likely to conflict;
- a clean working tree, with intentional uncommitted work called out instead
  of hidden.

Workers may commit their own focused changes. What happens to finished work
is the project's declared completion behavior — honor what `AGENTS.md` states
rather than deciding case by case.

## Integrate deliberately

Only begin integration after every selected worker has handed off a commit.
Inspect the primary branch again: current branch, worktree ownership, status,
and the commits to integrate. A dirty primary checkout, an unknown worktree
owner, or an uncommitted worker is a stop condition; preserve the state and
ask for direction rather than trying to repair it.

Present the integration order, commit SHAs, and checks to run. Get explicit
approval before changing the primary branch unless the user has already
clearly authorized integration in this request.

Integrate one worker branch at a time into a clean, up-to-date primary
branch. Prefer a fast-forward when it preserves the intended history; use a
merge commit when it preserves a meaningful independent branch. Do not
squash, rebase, or force-push somebody else's work without explicit
direction.

If a conflict occurs:

1. stop the merge and identify the overlapping ownership or semantic choice;
2. resolve only when the intended behavior is clear from the task and tests;
3. otherwise abort the merge to return to the known clean state and ask the
   user to decide.

Run the relevant formatter, tests, and checks after the final integration.
Report the resulting primary-branch commit and any checks not run.

## Retire worktrees safely

Isolated checkouts are kept by default — an agent's tab closing does not
remove one, and the operator may come back to it tomorrow. Remove one only
when the user asks, and only when it is clean *and* carries no commits the
primary branch does not already have: a clean working tree is not proof that
there is nothing to lose. Never force removal to discard uncommitted work.

Finish with a compact ledger: primary commit, integrated branches, retained
worktrees, removed worktrees, and any remaining risks. This makes the next
agent's starting point unambiguous.
