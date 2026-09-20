# Isolation belongs to the agent, not to the space

Status: Accepted

## Context

Every space was a worktree space: an agent created in it got a slot of its
own, a branch of its own, and its work came back through delivery. That is
the right default and the wrong rule. Two harnesses on one tree — one
writing, one reviewing — a quick fix committed straight to the branch the
operator is already on, a harness pointed at a directory that is not a
repository: each was either impossible or a degraded fallback that opened
the tab with a warning. `add-portable-worktree-policy` had named "a mode
that seats an agent in the primary checkout" a non-goal; dogfooding showed
it is a second way of working, not a lesser one.

**The first round of this change gave that mechanism to the space, and
dogfooding found the object wrong.** A space with a kind asks the operator
which way of working they want *before the agent exists* — before the
question the agent is for has been asked. Wanting to ask one thing cost a
checkout; wanting both ways over one project cost two spaces over one
directory, which the first round had to permit on purpose.

## Decision

The axis was right and the object was wrong. **Isolation is a property of
the agent**, which is where the domain had already put it (`PlacementKind`,
`Placement`, the identifier a task and a tenant share).

- **A space has no kind.** It is a directory somebody opened, any directory
  can be one, and one root means one space.
- **One record per agent, with isolation as a field.** An agent has an
  identity, a harness, a lifetime, and *optionally* a checkout and a
  branch. The separate tenant record is gone, and with it the question
  "which kind of record is this identifier".

An agent that is not isolated is a tenant of the operator's own checkout,
not a task: it commits where the operator is, never switches, resets,
stashes or cleans, and the projected context tells it so.

## Consequences

The choice moves to the moment it can actually be made — when someone knows
what the agent is for — and a project needs one space per directory again
rather than one per way of working. Dropping the kind also removes the
degraded fallback: placement either isolates or seats, and a placement that
cannot isolate refuses with the reason instead of quietly starting the
agent somewhere else.

It is breaking for the task store, and that is the cost: the record shape
changed and the old one climbs the ladder of
[052](052-what-uze-persists-is-tiered-by-what-deleting-it-costs.md) rather
than being read by a compatibility branch. A space persisted by a build
that still wrote kinds loses nothing, because a kind is exactly what this
decision says a space never had.

Two rounds also left a lesson worth keeping: the first shipped, was used,
and was wrong in a way no review had caught — the object an axis belongs to
is not decidable from the design, only from asking the question the feature
exists to ask.

Source change: openspec/changes/archive/2026-09-20-add-space-kinds/
