# Isolate concurrent agents at launch

Status: Proposed

## Context

Two coding agents editing one checkout collide: one overwrites the other's
edits, or stops mid-task on a conflict it cannot resolve. The operator's
requirement is a single guarantee — never start work in the primary branch
and abandon it partway because another agent was writing there too.

`agents.lock` had carried a `worktrees_dir` field that was parsed, validated,
and read by nothing but a status overlay. The policy's only transport was
prose in the `uze` package's `worktree` Skill, and that transport cannot
work: a Skill is selected by matching its description against the user's
request, and the conditions for isolation are properties of how work will be
organized, not words a user types. The Skill also had to decide whether it
should be used, which is circular.

The decisive fact was elsewhere. UZE launches agents itself, choosing the
working directory of every pane it starts. Isolation delivered as instruction
depends on a model reading, believing, and correctly executing prose.
Isolation delivered as a working directory depends on nothing.

The harnesses are not uniform: one ships its own worktree primitive, pinned
to a location it does not let anyone configure, and documented to activate
when the project's instructions ask for isolation. The others ship nothing.

## Decision

UZE places every agent it launches. The primary checkout seats one agent: the
first agent in a repository starts there, keeping access to the operator's
uncommitted work, and every additional live agent starts in an isolated
checkout of its own. Nobody is asked, nothing is configured, and no harness
has to cooperate. This uses the one fact UZE has and no agent does — how many
agents are live in this repository.

The layout is fixed: `.worktrees/<name>` under the primary checkout, on
branch `agent/<name>`, one name shared by the tab, the directory, and the
branch. Every surveyed tool either fixes the location or demands it per
invocation; none offers a project-level default to configure, and a
configuration axis is earned by a concrete case rather than offered
preventively. Creating a checkout prunes stale registry entries, suffixes a
name already taken, and ensures the isolation directory is ignored — without
which the seated agent's `git add -A` stages another agent's whole working
tree as an embedded repository.

Occupancy is judged by which checkout a pane is in, not by an exact path:
pane directories are probed live, so an agent that changes directory inside
the repository must keep its seat. Where isolation is impossible — no
repository, no commit to branch from, no Git — the agent starts at the seat
rather than failing to start.

The one thing a project declares is what happens to finished work:
`completion: handoff | merge`, defaulting to `handoff` so nothing reaches the
primary branch without a person deciding it should. Everything else
previously declarable — directory, branch prefix, base ref, isolation
triggers, integration authority — is deleted.

The shared instruction file carries a short projected statement for the one
audience placement cannot reach: a subagent spawned inside a harness session,
which UZE never sees created. It states the layout, that a reader inside an
isolated checkout is already isolated, how to isolate a subagent against the
primary checkout, and the completion rule. It never asks for a top-level
worktree — that instruction is precisely what activates a harness's own
worktree primitive, which would isolate a second time on top of the checkout
UZE already provided.

The terminal runtime is keyed on the resolved workspace root rather than the
launch directory, so a repository has one server, one set of panes, and one
seat.

UZE removes no checkout. A clean working tree is not proof there is nothing
to lose, since commits absent from the primary branch look identical to an
empty checkout.

Rejected: keeping the Skill as the decision point, for the circularity above.
Rejected: asking the operator at agent creation — a guarantee that depends on
answering correctly is not a guarantee. Rejected: per-harness worktree
configuration, since isolation now happens before a harness starts and no
surveyed harness exposes a configurable location anyway. Rejected: a
compatibility path for `worktrees_dir`, because the lock does not deny
unknown fields and silently dropping a declared policy is worse than a parse
error.

## Consequences

Easier: the operator's guarantee holds structurally for every agent UZE
launches, with no ceremony in the single-agent case and no question ever
asked. A new harness vertical needs no worktree code at all — placement
happens before the harness starts, and the text rides the context bridge that
already exists. The whole configurable surface is one enum with a safe
default.

Harder: subagents spawned inside a harness session, harnesses started outside
UZE, and the operator's own editor are placed by nobody, and are addressed by
text that asks rather than guarantees. Isolated checkouts accumulate, because
nothing is deleted, and one whose tab was closed has no path back inside the
UI yet. A fresh checkout lacks every ignored file — build caches, environment
files, dependency directories — and UZE deliberately adds no
ecosystem-specific handling for that. The `agents.lock` schema change is
breaking, deliberately, and pre-1.0 locks are edited by hand.

Source change: openspec/changes/add-portable-worktree-policy/
