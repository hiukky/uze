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

**A tenant is a record of its own, not a `Task` variant.** The first draft
put `work: Work::Isolated | InPlace` inside `Task`. That makes
`Task { work: InPlace, state: Ready }` representable and forces a match
into every function that takes `&Task`. Instead:
`Tenant { id: AgentId, harness, root, created_at_unix, ended_at_unix }` in
`TaskStore.tenants`, under the same lock and the same per-repository
sweep. `AgentId` is what `TaskId` is today, with `branch()` moved onto
`Task`, so task and tenant share exactly identity and conversation and
nothing else. `landing`, `checkout`, `evaluate_tasks`, `deliver`,
`release` and `collect` are untouched. The name: "seat" was the rule
`add-portable-worktree-policy` removed and would read as its return;
"tenant" says what the agent is to the directory.

**`Option<CheckoutId>` stays.** With tenants out of `Task`, "never had a
checkout" no longer needs representing; "lost its checkout" is what
`None` plus the state already mean, and "lost while a pane still sits in
it" is an observation (`slot_path(...).is_dir()`), not a record.

**The kind lives on the terminal's `Space`, opaque to the server.** A
space's identity is not its root — `create_space` allows several spaces
per root by design — and the terminal mints space ids afresh on restore,
so no stable key exists outside the terminal for a domain-side registry to
hang on. `Space.kind` sits beside `label`, which the server also persists
and never reads. The proof is a rule in `tests/architecture/layering.rs`:
`crates/uze-terminal/src/runtime.rs` may not name the kind's vocabulary.
`Session::open_space` becomes `space_for(root, kind)`.

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
`tab_agent(tab)`; and two layouts, `Tree` and `Flat`, each with
`measure()` and `draw()` in the same `impl`, so `tree_rows` and the
drawing loop cannot disagree. The rule is the code extension's: the mode
decides who is asked, never what the state means. Flat measures
`1 + max(agents, 1) + 1`. The extraction lands first, behaviour-preserving.

**Selection in the flat layout is the bar, and only the bar.** The bar
replaces the `Selected` glyph rather than joining it: two encodings of
one fact is the drift `agent_tab_status` exists to prevent. `Working` and
`Completed` keep the status column. The drag drop-indicator, which today
draws a bar in the same column, becomes an insertion hairline between
rows in the flat layout (`Symbol::TreeDivider` in `Token::Accent`): a
one-row item wants a line where it will land, not a mark on its edge, and
a token named *selected* is not spent on a drop target. The comment in
`render.rs` that chose block fill over a bar is extended with the
per-kind rule. `Symbol::BarMedium` and `Token::Accent` over
`Token::SurfaceRaised` are the management modal's exact vocabulary; no
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

**No LikeC4 update.** No container or component is added or removed. The
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
  kind] → `space_for(root, kind)`; the scenario is in the spec.
- [The sidebar extraction moves the foot-section tests] → Three tests
  budget the foot from `tree_rows`; they are re-derived from `measure()`
  in the extraction PR, before any behaviour changes.
- [`caption_color_of` in the sidebar tests assumes a caption row] →
  Flat rows have none; the helper takes the layout's row offset.
- [Old task-state files and old terminal state] → Pre-1.0: versions bump,
  stale state is cleaned, no compatibility path is written.
