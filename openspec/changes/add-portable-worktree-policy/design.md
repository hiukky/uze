## Context

Three delivery routes were available for a project's worktree policy, and the
choice between them determines whether the policy reaches a harness at all.

1. **A Skill that decides.** What existed. It fails structurally: the skill
   must be selected before it can argue for its own selection, and selection
   is driven by matching a description against the user's request, where the
   conditions for isolation never appear.
2. **A per-harness native configuration.** Attractive, but no harness
   surveyed exposes a configurable worktree root. One exposes a base-ref
   setting; one pins its worktree location entirely. Writing vendor settings
   would deliver a fraction of the policy to a fraction of the harnesses.
3. **The shared instruction baseline.** `AGENTS.md` is already projected into
   every harness through bridges UZE owns. It is in context for every agent
   and every subagent, in all four harnesses, at the moment work begins.

Route 3 is not a fallback. The harness that ships a native worktree primitive
documents its activation condition as the user or the project's instructions
— so the projected policy is the trigger for the native mechanism, and the
substitute for it where none exists. One route serves both.

## Decisions

**Isolation is performed, not requested.** UZE chooses the working directory
of every agent it launches, so isolation is a property of the environment the
agent is born into. Delivering it as instruction text would make the
guarantee depend on a model reading, believing, and correctly executing
prose. The projected text is demoted to the one job placement cannot do:
addressing writers UZE never sees.

**The seat rule.** The primary checkout holds one agent. The first agent
starts there and keeps access to the operator's uncommitted work — the
common case pays nothing. Every additional live agent is isolated. This uses
the one fact UZE has and no agent does: how many agents are live in this
repository. No question, no trigger to evaluate, no model judgment.

**Occupancy is a checkout, not a path.** Pane directories are probed live, so
comparing exact paths would free the seat the moment an agent changed
directory. Membership is decided against the fixed layout, which also keeps
an isolated agent from being mistaken for the seat's occupant.

**Degrade to the seat, never to a refusal.** No repository, no commit to
branch from, no Git — the agent still starts. Failing to isolate is a risk
only when something else is already writing there; failing to launch is
certain.

**The layout is fixed.** Every tool in this space either fixes the location
or demands it per invocation; none offers a project-level default to
configure. A configuration axis is earned by a concrete case, not offered
preventively.

**The only declared axis is what happens to finished work.** That is a team
decision; a path is not. `handoff` is the default because nothing should
reach the primary branch without someone deciding it should.

**The projection must not trigger foreign isolation.** A harness with its own
worktree primitive activates on an instruction to isolate — exactly what the
first draft of this text said. It would have isolated a second time on top of
the checkout UZE had already placed the agent in. The text now states where
the reader already is, and asks for a worktree only for a subagent, resolved
against the primary so directories never nest.

**Nothing is deleted.** Checkouts are kept; a closing tab removes nothing. A
clean working tree is not proof there is nothing to lose — commits absent
from the primary branch look identical to an empty checkout — so removal
waits for a guard that checks both, and for a real request.

**The region's identity carries the rendered content's digest.** With a fixed
identity, editing the lock renders different bytes into a region that already
exists, which `text_region` correctly refuses as drift — the declaration
would be projectable once and never updatable. Keying on content turns an
edit into "one region is stale, another is missing". A hand edit still
drifts, because it changes content *inside* an unchanged identity.

**Discovery avoids a subprocess.** Status commands are budgeted; linked
worktrees are read from `.git/worktrees/*/gitdir`, Git's own registry, which
stays flat however the directories nest.

**The replaced lock key fails loudly.** `ProjectLock` does not deny unknown
fields, so leaving `worktrees_dir` in place would silently drop a declared
policy. This is a rejection, not a compatibility path.

## Risks

- **Subagents are out of reach.** A writer spawned inside a harness session
  is never placed by UZE. The projected text asks; the harness complies or
  does not. This is the original pain and it is only partly closed.
- **A harness started outside UZE** — a bare CLI in a terminal — is in the
  same category, as is the operator's own editor holding the seat invisibly.
- **A fresh checkout lacks ignored state.** Build caches, `.env` files, and
  dependency directories do not travel. This is a property of worktrees; no
  ecosystem-specific handling is added, deliberately.
- **Checkouts accumulate**, because nothing is deleted, and a checkout whose
  tab was closed has no path back inside the UI yet.

## Candidate ADRs

- **Isolate concurrent agents at launch** — UZE places every agent it starts,
  seating one per primary checkout and isolating the rest, instead of asking
  a model to isolate itself. Hard to reverse: it makes agent placement a
  product guarantee, fixes the on-disk layout, adds a lock axis, and changes
  what the terminal runtime is keyed on.
