---
name: worktree
description: Works inside the isolated checkout UZE placed you in — knowing where you are, committing on your own branch, giving parallel subagents checkouts of their own, and handing your work to UZE's delivery instead of integrating it yourself. Use when coordinating more than one writing agent, when resuming work in an existing checkout, when UZE reports a paused rebase or failed checks on your branch, or when a conflict or an unknown checkout owner needs resolving.
slash: true
metadata:
  opencode/autoinvoke: "true"
---

# UZE — working where UZE placed you

You do not decide where to work: UZE places every agent it launches before
you start. Either you were **isolated** — a checkout of your own under
`.worktrees/<id>`, on branch `agent/<id>`, with the primary checkout left
to the operator — or you are in the operator's own checkout, on the branch
they are on, beside them. The project's `worktrees.default` decides which
you got, and the operator can isolate you afterwards, in which case you
are relaunched in the new checkout — with whatever their tree had
uncommitted, if they said to carry it, and possibly with a conversation
that starts over, because a harness that files a conversation under the
directory it ran in cannot resume it elsewhere.
Read the "Concurrent work isolation" section of `AGENTS.md` — it states
the layout and what happens to finished work.

This skill is what no harness does for you: the part of that arrangement
you have to carry yourself.

## Know where you are

```bash
git rev-parse --show-toplevel
git branch --show-current
git status --short
```

If your working directory is inside `.worktrees/`, you are isolated: work
here, commit here, and do not switch branches. A worktree you make for
yourself is yours — UZE neither sees nor delivers it, so bring its work back
onto your own branch.

If your working directory is not inside `.worktrees/`, you are in the
operator's own checkout, on their branch. Commit there, as you go, and
never switch, reset, stash, clean or move it: the operator's uncommitted
work is theirs, and so is the branch's name. Nothing below about naming,
delivery and rebases applies to you — there is nothing to deliver, because
your commits already land where the operator is. Ask again after the
operator isolates you: the answer changes, and your own working directory
is where it is written.

## Name the work before you do it

Your first action in the checkout, before you read a file or plan anything:

```bash
uze agent task name <type>/<subject>
```

The branch UZE placed you on is a generated identifier, and a reviewer
meeting it learns nothing. The subject is one or two words naming the
intention — `fix/branch-naming`, not a description of the task — and the
types the project accepts are spelled out in the "Concurrent work
isolation" section of `AGENTS.md`. A project that declares none refuses the
command, which is that project saying it does not name work; carry on.

Do it now rather than later: the request you were given is where the
intention comes from, so nothing you read afterwards makes the name easier
to choose, and work that reaches a commit unnamed is named by UZE from that
commit's subject instead. Naming renames your branch, so ask Git for its
name rather than remembering it. A name you or the operator already chose
is never replaced — including by a second call of your own.

## Commit on your branch, and stop there

Commit your work on your own branch as you go, in focused commits that each
pass the project's checks. Never commit to, merge into, rebase, or reset the
target branch: delivery is UZE's. When you are done, leave a clean tree —
uncommitted work is reported to the operator as exactly that, and is not
delivered — and end your turn. What happens next is the project's declared
completion behavior, and it is not yours to perform.

## When UZE hands work back to you

UZE rebases your branch onto the target before delivering it, and runs the
project's checks on the result. Three things come back to you, as a message
in your own session:

- **A paused rebase.** The target moved and your branch no longer applies
  cleanly. The rebase is paused in your checkout with the conflict markers
  in place. Resolve them preserving the intent of your change, run
  `git rebase --continue`, run the project's checks, and end your turn.
  Never abort the rebase to make the message go away.
- **Failed checks.** Fix them on your branch, commit, and end your turn.
- **A published branch with no request open for it**, under the `pr`
  completion. UZE has pushed the branch; opening the pull request — the
  merge request, on a forge that calls it that — is yours, because the
  title and the description are the change's argument and you are the one
  holding it. Name and describe it by this project's convention, do not
  merge it, and end your turn. UZE finds the request on the remote by
  itself afterwards, and from then on delivering the task only pushes new
  commits onto it.

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
