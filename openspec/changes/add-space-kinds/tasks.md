> **Two rounds.** Groups 1-7 are the first round: two kinds of space,
> shipped in `v0.0.0-alpha.6`. Dogfooding found the choice in the wrong
> place, so groups 8-14 move isolation onto the agent and remove the
> kind. The ticked boxes above stay ticked — that work happened, and this
> file is the record of what was done as much as of what is left. What
> those groups built and this round removes is named where it is removed.

## 1. A space can be created from the keyboard

- [x] 1.1 `Action::NewSpace` gets a default chord in `uze-keys` and a `perform` arm in the workspace client that opens `RootPicker::opened_in` exactly as `WorkspaceHit::NewSpace` does.
- [x] 1.2 Keymap test for the chord; `docs/keymap.md` lists it. `docs/keymap.md` enumerates no action's chord (the Keys screen is the listing), so the chord is covered by `the_new_space_chord_opens_the_picker_the_pointer_opens` and no line was added.

## 2. The tenant record

- [x] 2.1 `AgentId` extracted from `TaskId` in `uze-core`; `TaskId::branch()` becomes `Task::generated_branch()`; conversations key on `AgentId`.
- [x] 2.2 `project/tenant.rs`: `Tenant { id, harness, root, created_at_unix, ended_at_unix }`; `TaskStore.tenants`; `AgentRecord::Tenant` with the root as its own directory; `SCHEMA_VERSION` bumps.
- [x] 2.3 The store of a tenant is keyed by the space's canonical root, the same key every project state uses; a tenant over a directory that is not a repository has a store like any other.
- [x] 2.4 `Workspace::end_abandoned_tenants(root, echoed)`: ends every live tenant of that root no echoed identifier names; needs no repository. The client's occupancy reconcile calls it for every space root beside `release_abandoned_tasks`, which keeps skipping roots without a repository.
- [x] 2.5 `Workspace::name_task` refuses a tenant by type, with a reason saying the work is not named.
- [x] 2.6 Core and application tests: a tenant creates no directory and no branch; two tenants share a root; closing the last pane ends the tenant; a live tenant survives the sweep; a tenant of a plain directory ends; a tenant is refused naming; a tenant's conversation resumes.
- [x] 2.7 `uze status` and every CLI listing that enumerates tasks states what it does with tenants: listed under their own heading, never as tasks. No CLI command enumerates tasks today (`uze agent task name` is the only task command), so there is no listing to change.

## 3. The space's kind

- [x] 3.1 `uze-terminal`: `SpaceKind` on `Space`, `SpaceSeed`, `PersistedSpace` and `CreateSpace`; `Session::open_space` becomes `space_for(root, kind)`; `PROTOCOL_VERSION` bumps.
- [x] 3.2 Architecture rule in `tests/architecture/layering.rs`: `crates/uze-terminal/src/runtime.rs` never names `SpaceKind`. The one new rule; no per-crate variants.
- [x] 3.3 Runtime tests: the kind round-trips a restart; same root and other kind creates; same root and same kind reuses.

## 4. Placement by kind

- [x] 4.1 `Placement { Slot, Tenant }` in `uze-application`; `place_new_agent(root, kind) -> Result<Placement>`; every former fallback returns `Err` with its reason; `AgentPlacement::unisolated` is removed.
- [x] 4.2 `PlacementRequest::New { from, kind }`; `spawn_agent_placement` maps the wire kind to the placement kind; `absorb_placement` opens nothing on `Err` and stamps the identifier on `Ok`.
- [x] 4.3 `RootProfile { repository: bool, has_commit: bool }` read model in `uze-application`, asked off the frame through a `spawn_root_profile`/`absorb_root_profile` pair when the picker's landed candidate changes, cached per path.
- [x] 4.4 The picker: a chip row under the directory line choosing the kind; both chips shown until the profile answers, worktree selected by default; `chosen()` returns root and kind; creating a worktree space over a root whose profile says no repository or no commit is refused with the reason, never created.
- [x] 4.5 Placement tests: a cap reached refuses and starts nothing; a non-repository root is a tenant by choice; the operator's tree is never a fallback; a worktree space over a plain directory is refused.

## 5. The sidebar

- [x] 5.1 Extraction, behaviour-preserving: `AgentRow::{Task(TaskView), Tenant(TenantView)}` read model in `uze-application` with one `tab_agent(tab)` replacing every caller of `tab_task`; per-agent layout rows built once; `Tree` layout with `measure()` and `draw()` in one `impl`; the scroll bound derived from `measure()`. Every existing sidebar test passes unchanged except the scroll-bound test, re-derived from `measure()`.
- [x] 5.2 One two-row item for both kinds — status and label over the branch or directory, the header lighter than the block; a worktree space draws it on a tree, a workspace space flat with no connectors, and the header toggle swaps the caption for the harness id in both.
- [x] 5.3 Selection in a flat space: `Symbol::BarMedium` in `Token::Accent` down both rows of the selected agent, beside its status glyph.
- [x] 5.4 TestBackend tests for the flat shape: two rows per agent over the root's branch, no connector, no branch on the header; a directory caption outside a repository; the status glyphs; the bar on the selected item only; no task mark and no deliver button; hits land; `step_agent` walks the agents; the header toggle naming each harness; an empty workspace space's caption row; a tenant whose harness exited is not an agent row.

## 6. The projected text and the Skill

- [x] 6.1 `WorktreePolicy::instructions()` describes both readers: inside `.worktrees/` isolated and committing on its own branch; elsewhere on the operator's branch, committing there, never switching, resetting or stashing it. "already isolated" stays in the isolated clause.
- [x] 6.2 `plugins/uze/skills/worktree/SKILL.md` rewritten for both cases.
- [x] 6.3 `conformance/contract/isolation.py`'s hand-kept copy of the declaration updated; `tests/projection/worktree_policy.rs` covers the second reader.
- [x] 6.4 `conformance/contract/continuity.py` writes the task document by hand: its `schema_version` and the `tenants` field follow the new schema.

## 7. Spec, invariants and docs

- [x] 7.1 `openspec/changes/add-portable-worktree-policy/design.md`: the non-goal becomes the decision it turned into, with why it was a non-goal; its `adr/isolate-concurrent-agents-at-launch.md` evolves in place. Its spec was edited in place when this change was proposed, and its task 9.5 points here.
- [x] 7.2 `docs/architecture/invariants.md`: "Every agent is isolated" scoped to worktree spaces; new property "a tenant never acquires a slot and never creates a branch", tied to its test.
- [x] 7.3 `web/content/docs/workspace.mdx` describes both kinds before any journey that names it in `proves:` changes.
- [x] 7.4 `journeys/journey.py`: a `tenants: { count, live }` check reading the store's `tenants` array; `tasks` stays as it is.
- [x] 7.5 Journeys: `04-workspace/06-an-agent-on-the-operators-branch.yml` (chip gesture with its `expect`; process alive with cwd at the project; `.worktrees/*` count 0; one Git worktree; `agent/*` branches count 0; `tasks: { count: 0 }`; `tenants: { count: 1, live: 1 }`; a commit made from a shell in the root appears in the branch's log) and a `06-recovery` scene restarting a tenant, mirroring the task continuity scene (kill by home, reopen, click the agent; the harness log carries the resume argument; the recorded and resumed identifiers are equal; one transcript; process cwd at the project; `tenants: { count: 1, live: 1 }`). `04-an-agents-changes-are-its-own` keeps its worktree-only claim stated in prose.
- [x] 7.6 Gate: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace --no-fail-fast`, `journey validate`, `openspec validate --all --strict`.

## 8. One record per agent

- [x] 8.1 `Agent { id, harness, created_at_unix, ended_at_unix, isolation: Option<Isolation> }` in `uze-core`, with `Isolation { checkout, branch, base, base_commit, target, state, published_as, published_request, request_branch, request_asked_at_unix }`; `TaskStore.agents` replaces `tasks` and `tenants`; `SCHEMA_VERSION` bumps.
- [x] 8.2 Delete `Tenant` and `AgentRecord` (undoes 2.2): one record answers for an identifier, so there is nothing to unify and nothing to match on.
- [x] 8.3 Every reader of `store.tasks`/`store.tenants` reads one collection; readiness, delivery, naming and the sweep take an isolated agent and say so in their signatures.
- [ ] 8.4 Core tests: an agent with no isolation has no branch, no readiness and no delivery; `Ready` cannot be expressed without an isolation; a document of the previous schema is set aside by the rule already in `locked_reporting`.

## 9. Isolation as an action

- [ ] 9.1 `Workspace::isolate(agent, carry: Carry) -> Result<AgentPlacement>`: acquires the slot as the policy says, writes the isolation against the same identity under the document's lock, and answers the placement to relaunch from. Built from `place_in_slot` and `resume_task`; neither is duplicated.
- [ ] 9.2 The branch is cut from the root's current `HEAD`; `Carry::{Nothing, CopyOfChanges}` decides whether the root's uncommitted changes are copied into the checkout. The root's working tree is never written to.
- [ ] 9.3 Refusals leave the agent exactly where it is: no repository, no commit to branch from, cap reached, Git refusing, the record unwritable.
- [ ] 9.4 Application tests: the identity survives; the conversation record survives; a refusal changes nothing; a dirty root is copied from, never moved from; the root's tree is byte-identical after either answer.

## 10. A space is a root

- [ ] 10.1 Remove `SpaceKind` from `Space`, `SpaceSeed`, `PersistedSpace` and `CreateSpace` (undoes 3.1); `space_for(root)`; `PROTOCOL_VERSION` bumps. Delete the architecture rule from 3.2 with the vocabulary it guarded.
- [ ] 10.2 The persisted workspace declares its version and is read by `upgrade-resilience`'s rule: a document that carries a kind per space is the previous version, set aside with the operator told what was done.
- [ ] 10.3 Runtime tests: one root names one space; the space round-trips a restart; a persisted workspace of the previous version is recovered from and reported.

## 11. The column

- [ ] 11.1 Two groups per space: the root's agents, then the isolated ones, with a blank row between them only when both groups have an agent.
- [ ] 11.2 A colour per group, from the two tokens the kinds wore; the selection tint reads the agent's group rather than the space's kind (undoes what the kind gave `kind_hue`).
- [ ] 11.3 The isolated row keeps the tree connector; the root's row does not. Dragging to reorder is confined to a group.
- [ ] 11.4 `Isolate` in the agent's context menu, offered only where it can be honoured; the prompt for a dirty root; the row moves between groups when it lands.
- [ ] 11.5 TestBackend tests: both groups with their separator; one group with none; the row moving on isolation; the action absent outside a repository.

## 12. The picker asks only where

- [ ] 12.1 Remove the kind row, the kind `⇄`, `choose_kind`, `slots_available` and `RootProfile`; the `⇄` on a space header keeps its own job.
- [ ] 12.2 Remove the repository-reach filter and its cache: with no kind, every directory can be a space and the listing hides nothing.
- [ ] 12.3 Picker tests: every directory is offered; picking one creates a space; the prompt has one question.

## 13. The project's default

- [ ] 13.1 `worktrees.default: isolated | in-place` in `agents.yaml`, parsed with the rest of the policy and defaulting to in-place.
- [ ] 13.2 The placement at launch reads it; an agent launched under `isolated` is placed in a slot and the action has nothing to offer.
- [ ] 13.3 Manifest and application tests: declared, undeclared, and an unreadable value refused with the rest of the policy.

## 14. Docs, journeys and the gate

- [ ] 14.1 `docs/architecture/invariants.md`: isolation is an agent's property; the operator's tree is never written to by an isolation; one root, one space.
- [ ] 14.2 `web/content/docs/workspace.mdx` and the `worktree` Skill rewritten for one space and an action; `conformance/contract/isolation.py`'s copy of the projected text follows.
- [ ] 14.3 Journey: an agent in the root is isolated — same identity, same conversation, checkout created, the operator's tree untouched, the row in the other group.
- [ ] 14.4 Journey: a workspace persisted by the previous release is opened by this build.
- [ ] 14.5 Retire the journeys and tests whose claim was the two kinds, replacing the claim rather than deleting the coverage.
- [ ] 14.6 Gate: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace --no-fail-fast`, `journey validate`, `openspec validate --all --strict`.
