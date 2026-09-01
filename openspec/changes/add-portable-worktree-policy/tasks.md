## 1. Canonical model

- [x] 1.1 Add the vendor-neutral `worktree` module in `uze-core`: fixed layout constants, completion vocabulary, and the rendered declaration.
- [x] 1.2 Add the isolation primitives — primary-checkout resolution, seat membership, checkout creation with prune, collision-suffixing, and ignore-file maintenance.
- [x] 1.3 Read linked worktrees from Git's own registry rather than by spawning Git, so budgeted commands stay budgeted.
- [x] 1.4 Cover the primitives against real repositories: primary resolution from inside an isolated checkout, suffixing, ignore idempotence, unborn HEAD.

## 2. Lock schema

- [x] 2.1 Replace `worktrees_dir` with a `worktrees` block carrying `completion` only.
- [x] 2.2 Reject the replaced key by name rather than ignoring it and silently dropping a declared policy.
- [x] 2.3 Update the Git diff overlay, the only prior consumer of the removed field.

## 3. Launch-time isolation

- [x] 3.1 Key the terminal runtime on the resolved workspace root instead of the launch directory, and resolve it once for both the server and prompt history.
- [x] 3.2 Apply the seat rule at both agent-creation sites, falling back to the seat when isolation is impossible.
- [x] 3.3 Cover the seat decision: shells excluded, isolated checkouts excluded, movement within the primary retaining the seat, and the shared slug.

## 4. Projection and reporting

- [x] 4.1 Project the declaration into a marker-owned region of `AGENTS.md`, keyed on the rendered content so it stays editable.
- [x] 4.2 Report the region and completion behavior from `context inspect|plan|reconcile` and render them in the CLI.
- [x] 4.3 Cover the projection: never triggering a harness's own isolation, drift refused, edits superseding.

## 5. Deletions

- [x] 5.1 Remove the configurable directory, branch prefix, base ref, isolation triggers, and integration authority — the layout is infrastructure, and the seat rule replaced the triggers.
- [x] 5.2 Remove `WorktreeCapabilities`, `assess`, and the per-harness route. UZE isolates before a harness starts, so there is nothing for a harness to preserve or lose.
- [x] 5.3 Remove the deviation report, which described a configured directory being violated.

## 6. Package and documentation

- [x] 6.1 Rewrite the `uze` package's `worktree` Skill: it no longer decides whether to isolate, and covers what UZE cannot — subagents, coordination, integration.
- [x] 6.2 Record the guarded properties in `docs/architecture/invariants.md`, each tied to the test that proves it.
- [x] 6.3 Dogfood on this repository: declare the policy and reconcile.
- [x] 6.4 Run the gate: formatting, clippy, the deterministic suite, strict OpenSpec validation. Green for everything this change touches. Two pre-existing reds remain, in code this change does not modify:
  - `src/ui/orchestrator.rs` — `AgentTabStatus`, its methods, and `WorkspaceModel::agent_tab_status` are dead code failing `clippy --all-targets -D warnings`. Uncommitted work not yet wired to a call site; left untouched.
  - `uze-core::acquisition::tests::a_missing_or_non_directory_source_is_rejected` — asserts a path is absent while its helper resolves to `uze_testkit::temp::scratch`, which creates the directory. Broken premise on committed `main`.

## 7. Deferred, by decision

- [ ] Removing a kept checkout at close (clean *and* no commits absent from the primary).
- [ ] Resuming an idle checkout whose tab was closed.
- [ ] Focused-pane ↔ checkout association and a sync action in the Git diff overlay.
- [ ] `pull_request` completion; merge automation beyond refusing a dirty primary.
- [ ] Detecting a human editor occupying the seat, which UZE cannot see.
