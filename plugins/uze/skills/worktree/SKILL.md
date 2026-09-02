---
name: worktree
description: Works inside the isolated checkout UZE placed you in — knowing where you are, committing on your own branch, giving parallel subagents checkouts of their own, and handing your work to UZE's delivery instead of integrating it yourself. Use when coordinating more than one writing agent, when resuming work in an existing checkout, when UZE reports a paused rebase or failed checks on your branch, or when a conflict or an unknown checkout owner needs resolving.
slash: true
metadata:
  opencode/autoinvoke: "true"
---

# UZE — working in an isolated checkout

You do not decide whether to work in isolation, and you do not create your
own top-level worktree. UZE places every agent it launches in a checkout of
its own under `.worktrees/<id>`, on branch `agent/<id>`, before you start.
The primary checkout belongs to the operator. Read the "Concurrent work
isolation" section of `AGENTS.md` — it states the layout and what happens
to finished work.

This skill is what no harness does for you: the part of that arrangement
you have to carry yourself.

## Know where you are

```bash
git rev-parse --show-toplevel
git branch --show-current
git status --short
```

If your working directory is inside `.worktrees/`, you are already isolated:
work here, commit here, and do not create another worktree or switch
branches. If you find yourself in the primary checkout, you were started by
hand rather than by UZE; the operator's uncommitted work there is theirs —
never stash, reset, clean, or move it.

## Commit on your branch, and stop there

Commit your work on your own branch as you go, in focused commits that each
pass the project's checks. Never commit to, merge into, rebase, or reset the
target branch: delivery is UZE's. When you are done, leave a clean tree —
uncommitted work is reported to the operator as exactly that, and is not
delivered — and end your turn. What happens next is the project's declared
completion behavior, and it is not yours to perform.

## When UZE hands work back to you

UZE rebases your branch onto the target before delivering it, and runs the
project's checks on the result. Two things come back to you, as a message
in your own session:

- **A paused rebase.** The target moved and your branch no longer applies
  cleanly. The rebase is paused in your checkout with the conflict markers
  in place. Resolve them preserving the intent of your change, run
  `git rebase --continue`, run the project's checks, and end your turn.
  Never abort the rebase to make the message go away.
- **Failed checks.** Fix them on your branch, commit, and end your turn.

Do not try to deliver again yourself; UZE re-reads your checkout when your
turn ends.

## Give parallel subagents their own checkout

UZE cannot see subagents you spawn inside your own session, so isolating
them is yours to do. Before two of them write files, give each one its own
checkout, resolved against the *primary* so one worktree never nests inside
another:

```bash
git worktree add -b agent/<topic> \
  "$(git rev-parse --path-format=absolute --git-common-dir)/../.worktrees/<topic>" HEAD
```

One checkout has exactly one writer. Split the work by file or component
boundary and state each owner's paths before they start. If their changes
cannot be made disjoint, sequence them rather than hoping Git can merge them
later. Repository-level Git metadata is shared across worktrees, so do not
rebase, force-push, or delete branches while other writers are active.

When a subagent is done, bring its commits onto *your* branch — a
fast-forward or a merge on your side — and hand the whole to UZE as one
branch. Do not leave work only on a subagent's branch: UZE delivers your
branch, not theirs.

## Retire checkouts safely

Checkouts are UZE's to reuse and remove; leave them. If you created one for a
subagent, remove it only when it is clean *and* its commits are on your
branch: a clean working tree is not proof that there is nothing to lose.
Never force removal to discard uncommitted work.

Finish with a compact handoff: your branch, its tip commit, the checks you
ran, and any file another agent is likely to have touched too.
