## Context

See proposal.md — Why. This change builds on `identify-agents-at-launch`
(an agent's identity travels with its launch) and on the still-open
`add-portable-worktree-policy` (slots, tasks, delivery), whose design
lists "a mode that seats an agent in the primary checkout" as a non-goal.
That non-goal is reversed here, in place, as that change's rules require.

Facts that shape the approach:

- `Task` carries everything delivery needs (`base`, `target`, `branch`,
  `checkout`, publication) and a nine-state `TaskState`, seven of whose
  states presuppose a branch. Fourteen functions in `landing.rs` and
  `checkout.rs` take `&Task`; `end_without_checkout` reads
  `checkout: None` as "the checkout is gone".
- `uze-terminal`'s `Space { root, label, tabs }` carries no kind;
  `Session::open_space` reuses the space a root already has, while
  `create_space` deliberately allows several spaces over one root.
- The sidebar is one 560-line function; `tree_rows` pre-measures three
  rows per agent so the scroll bound and the foot sections (timeline,
  first steps) can be laid out before the first row is drawn.
- `place_new_agent` has five fallback paths that open the tab in the
  operator's tree with a "no checkout — reason" notice.
- `Action::NewSpace` exists in the vocabulary with no chord and no handler.
- `WorkspaceKind` already exists in core and is live: it classifies a
  directory by its anchor files for the Overview. It is unrelated and is
  not renamed here.

## Goals / Non-Goals

**Goals:**
- Two mechanisms, one seam: the kind decides who is asked at placement
  and at drawing; nothing below placement learns about kinds.
- A tenant cannot be in a state that has no meaning for it.
- Placement never does something other than what the operator asked.
- The terminal keeps the kind the way it keeps a label.

**Non-Goals:**
- Renaming `WorkspaceKind` (the anchor classification) — a separate PR,
  as `ProjectAnchor`, outside this change.
- Any per-project default for the kind in `agents.yaml`. The kind is a
  property of a space, chosen by the operator; a project-level default is
  a later field if a project needs one.
- Scoping the changes surface per tenant. Tenants share the operator's
  tree and therefore its diff; this is a property of the kind and is
  documented as such.
- Stacked tasks, delivery from a tenant, or naming a tenant's work.

## Decisions

**One record per agent, with the work nested inside its isolation.**

The first round rejected `work: Work::Isolated | InPlace` inside `Task`
for a good reason: it makes `Task { work: InPlace, state: Ready }`
representable and forces a match into every function that takes a
`&Task`. It answered with a second record, `Tenant`, beside `Task`.

Isolation being an action rather than a property of the space breaks that
answer: an agent has to keep its identity, its harness and its
conversation while *acquiring* work, and moving a record between two
collections mid-life is the one operation two types make hardest — the
sweep can see the agent twice, and every reader has to ask which
collection it came from.

The shape that answers both objections is neither of the two drafts: one
record, and the work nested *inside* the isolation rather than flattened
beside a flag.

```
Agent     { id, harness, created_at, ended_at, isolation: Option<Isolation> }
Isolation { checkout, branch, base, base_commit, target, state, published… }
```

`Ready` is then not representable without a checkout to be ready in, which
is what the first round was protecting. Isolating is filling a field under
the document's lock, not moving a row between lists. And the harness
becomes a fact the record carries rather than one the sidebar infers from
whatever process a pane happens to be running.

What this deletes, rather than deprecates: `Tenant`, and `AgentRecord` —
the enum that existed to answer "which kind of record is this identifier",
which has no caller outside its own module.

**`Option<CheckoutId>` stays.** With tenants out of `Task`, "never had a
checkout" no longer needs representing; "lost its checkout" is what
`None` plus the state already mean, and "lost while a pane still sits in
it" is an observation (`slot_path(...).is_dir()`), not a record.

**The kind is deleted, not kept as a field nobody reads.** The first round
put it on the terminal's `Space`, opaque to the server, with an
architecture rule proving the runtime never named its vocabulary. With one
kind there is nothing to carry: `Space.kind` and `CreateSpace.kind` leave
the protocol and the persisted workspace, a seat is a root, and
`space_for` looks a space up by root alone. Keeping the field "for
compatibility" would leave exactly the kind of dead weight this round is
paid to remove — and the document it lives in is versioned, so an older
workspace is read by the rule `upgrade-resilience` states rather than by a
reader that tolerates both shapes.

**Two vocabularies for the kind, translated once.** `uze-application`
does not depend on `uze-terminal`, and `src/` may not name `uze_core`, so
the wire enum (`uze_terminal::SpaceKind`) and the placement enum
(`uze_application::Placement`'s request side) are distinct and mapped in
`spawn_agent_placement`, the one place a request becomes a domain call.
Code names the mechanism (`Worktree`, `Workspace` on the wire;
`Slot`, `Tenant` in placement); "worktree" and "workspace" as screen
labels are presentation.

**`place_new_agent(root, kind) -> Result<Placement>`, and every failure
refuses.** `Placement { Slot { task, checkout, branch, reused }, Tenant
{ id } }` with no warning variant. The picker offers the worktree kind
only for a Git working tree with a commit, which turns "not inside a Git
working tree" from a fallback into a declared choice; the five remaining
failures (unreadable manifest, no branch, no commit, cap reached, record
unwritable, Git refusing) return `Err` with the reason, and
`absorb_placement` already treats `Err` as "open nothing, say why". An
agent asked for isolated and started in the operator's tree is the one
case where UZE writes where it was not asked to, and a notice after the
fact does not undo it.

**The record is written before the tab opens, for both kinds.** The task
is already recorded before `CreateTab`; the tenant follows the same order,
so the launch can carry the identity (`identify-agents-at-launch`).
`slot_claims` loses its reason to exist: the session echoes the identity
on the first status after the spawn.

**The store is keyed by the space's root, and the sweep runs per root.**
A task's project root is the primary checkout because `space_root` maps
every slot to it; a tenant's is the space's root as created, canonical,
which is the key every project state already uses and which accepts a
directory that is not a repository. `spawn_occupancy_reconcile` already
collects one root per space and the directories live panes hold; it
carries the identities live tabs echo as well, and the application runs
two passes per root: `release_abandoned_tasks`, which needs a repository
and keeps skipping a root without one, and `end_abandoned_tenants(root,
echoed)`, which needs none and ends every live tenant no echoed identity
names. A repository holding a worktree space and a workspace space over
its top level shares one store; a workspace space rooted below the top
level, or over a plain directory, has a store of its own. The client never
writes domain state; the server never persists it.

**The store answers for any record through one accessor.** With tasks and
tenants in one store, "which directory does this identity own" must have
one answer or every reader grows a match. `TaskStore::agent(id) ->
Option<AgentRecord<'_>>` with `AgentRecord::{Task, Tenant}` and one
`own_directory()` — the slot for a task, the root for a tenant — is what
`owner_of` (from `identify-agents-at-launch`) consults. A third record
kind adds a variant and a directory, never a reader, and a third space
kind is absorbed the same way.

**A tenant records the harness, not a label.** The label a person reads is
the tab's, which the terminal already owns, persists and lets the operator
rename; the fact the record holds is which harness the tenant runs. So
`Tenant { id, harness, root, created_at_unix, ended_at_unix }`, and the
sidebar row carries the tab's label with the harness id at its edge, which
is what tells two tenants of one harness apart.

**The sidebar's seam is rows built once, layouts that measure and draw
together.** Extracting "one function per kind" would duplicate the twenty
interleaved concerns of `render_sidebar`. Instead a per-agent `AgentRow`
(tab, cwd, status, selection, current-ness, mark, rename state, lost
checkout) is built by one function from `AgentRow::{Task(TaskView),
Tenant(TenantView)}`, the application's read model, reached through one
`tab_agent(tab)`; and one two-row item drawn for both kinds, measured by
the one function the drawing loop's rows come from, so `tree_rows` and the
drawing cannot disagree. The extraction lands first, behaviour-preserving.

**One look, two shapes.** A first cut gave a workspace space one row per
agent, the harness at its edge and the branch on the header. In use it
read as a different product: the tenant lost its status and its caption,
the two things an operator scans the column for. Both kinds now draw the
same item — status and label over the branch or directory, the header
lighter than the block beneath it. What differs is only what the kind
means: a worktree space hangs its items on a tree, one branch each; a
workspace space's tenants all run in the root, so they are drawn flat,
and with no tree to anchor them the selected one carries a vertical
accent bar (`Symbol::BarMedium` in `Token::Accent`) down both rows. No
token or symbol is added.

**A tenant's row reads label then harness.** In a slot the label names
the work; a tenant has no work to name, and the tab's label stays what it
is today — generated, counted per space, renameable — because the strip,
the notices and `Rename` all read it. The harness id at the row's edge is
what distinguishes two tenants; the root toggle flips the header as it
does elsewhere. The root's branch and its `⇣⇡` appear once, on the
header, because every tenant shares them.

**The picker learns the kind as a chip row under the directory, and never
waits on a repository.** Two `+ new` controls do not fit the 40-column
count-and-action line, and the kind cannot change after agents are
placed, so it is chosen at creation. Whether the worktree kind is
available is a Git question, and the picker runs on the frame, so the
answer comes as a read model (`RootProfile { repository, has_commit }`)
through a `spawn_root_profile`/`absorb_root_profile` pair asked when the
landed candidate changes and cached per path; until it answers both chips
show with worktree selected, and creating a worktree space over a root the
profile refuses is refused with the reason rather than created. The
keyboard reaches the picker first: `Action::NewSpace` gets a default
chord and a `perform` arm that opens `RootPicker::opened_in` exactly as
`WorkspaceHit::NewSpace` does, as its own PR before the picker changes.

**Isolating a live agent is place, open, close — a flow that already
runs.** A process cannot be moved between directories, so the agent is
relaunched in the new checkout resuming its conversation. Every piece
exists: `place_in_slot` acquires the slot, the launch carries the agent's
identity, `--resume` continues the conversation, and
`PlacementResolution.replacing` closes the tab the new one takes over
from — which is how a task whose checkout was removed is resumed today.
Isolation is that flow with the record updated instead of created.

**The operator's tree is never moved, only copied from.** UZE cannot
attribute an uncommitted change to an agent: the root's working tree is
shared by the operator and every agent in the space. So "bring the
agent's changes" is not implementable honestly — only "bring the tree's
changes" is. Moving them would take the operator's own work out of their
tree, which is the one thing `add-portable-worktree-policy`'s invariant
forbids; discarding them is never an answer. What is left is copying,
and only when there is something to copy: a clean root isolates with no
question at all, which is the overwhelming case and the one this round
exists for.

**The isolated agent continues from where it stood.** Its branch is cut
from the root's current `HEAD`, not from the target. Carrying a copy of
the changes only means anything against the base they were made on, and
"isolate" promises the same work somewhere of its own — not a fresh
start. Starting from the target is the other answer the prompt offers
when the tree is dirty.

**The column groups by where an agent works, and the move is the
feedback.** The space's own agents first, the isolated ones after, in two
groups with a blank row between them *only when both exist* — a
separator between collections, not between siblings, which is the gap
this round's predecessor removed. Each group has its own colour, the two
the kinds used to wear, so a row's group is read without reading the row.
Isolating moves the row from the first group to the second: the agent
really did move, the tab really was replaced, and a row that jumps says
so better than a glyph that changes. Dragging to reorder stays inside a
group, since crossing the boundary is the action, not a drag.

**The default belongs to the project, not to the container.** Someone who
always isolates should not pay a click per agent, and the first round
answered that with a space they created by hand. `agents.yaml` already
carries the `worktrees:` policy — target, slots, completion — so the
default goes there: declared once, versioned with the project, the same
for everyone who opens it. A container each person creates by hand is
where that answer goes wrong.

**The spec and the invariant narrow; the tests keep their words.** "Every
agent is isolated" becomes "every agent launched into a worktree space".
The five tests behind it never create a workspace space and stay verbatim;
a new invariant with its own test states that a tenant never acquires a
slot and never creates a branch. The open change that owns that spec is
edited in place, as this project's rule for open changes says, rather
than shadowed by a delta that two archives could resolve in either order:
its `worktree-policy` spec already reads as this change needs it, its
task 9.5 points here, and its design non-goal is rewritten as the decision
it became, with the reason it was once a non-goal.

**Docs move before the journeys that prove them.** `journeys/suites/
04-workspace/*` name `web/content/docs/workspace.mdx` in `proves:`, and
that page says every agent gets an isolated checkout. The page changes
first, or `journey validate` passes proving false prose.

**No diagram update.** No container or component is added or removed. The
kind is data on an existing container; the tenant is a record in an
existing store.

## Candidate ADRs

- **A space chooses whether its agents are isolated, and an agent that is
  not isolated is a tenant, not a task** — reverses a written non-goal of
  ADR `isolate-concurrent-agents-at-launch` (still open, evolved in
  place) and fixes the shape of the task store and of the terminal's
  persisted space. Hard to reverse once either is on disk.

## Risks / Trade-offs

- [Tenants and the operator write one tree] → This is the kind's
  contract, chosen per space; the projected text tells a tenant it is on
  the operator's branch and must never switch, reset or stash it. The
  worktree kind stays the default the picker lands on for a repository.
- [The changes surface shows every tenant the same diff] → Documented as
  a property of the kind. `04-an-agents-changes-are-its-own` keeps
  proving the worktree kind only.
- [A `Session` that reuses a space by root alone silently ignores the
  kind] → `space_for(seat)`, a root and a kind together; the scenario is
  in the spec.
- [The sidebar extraction moves the foot-section tests] → Three tests
  budget the foot from `tree_rows`; they are re-derived from `measure()`
  in the extraction PR, before any behaviour changes.
- [Old task-state files and old terminal state] → Pre-1.0: versions bump,
  stale state is cleaned, no compatibility path is written.
