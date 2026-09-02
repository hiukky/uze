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
- [x] 3.2 Apply the seat rule at both agent-creation sites, falling back to the seat when isolation is impossible. *Superseded by 9.*
- [x] 3.3 Cover the seat decision: shells excluded, isolated checkouts excluded, movement within the primary retaining the seat, and the shared slug. *Superseded by 9.*

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

Sections 1–6 are the first iteration, on `main`. Sections 7–12 evolve it
to the slot model; each is independently mergeable and leaves the gate
green. Tests run against real repositories through `uze_testkit::git`.

## 7. Write lock and task model

- [x] 7.1 Put the repository write lock behind `uze_git::write`, inter-process, keyed on the common directory, with a bounded wait and a clear timeout message; `read` never takes it.
- [x] 7.2 Cover the lock: concurrent worktree creation does not collide, a panic inside the critical section releases it, a lock left by a dead process is reclaimed.
- [x] 7.3 Add `project/task`: generated identifier, label derived from the prompt, `base`/`base_commit` carried without behaviour, state persisted under `UzeHome`'s `state/` layout keyed on the core's own project id (the one prompt history already uses), atomic writes, schema version.
- [x] 7.4 Cover task state: identity survives relabelling, state survives checkout removal, a kill mid-write leaves the previous or the new file.
- [x] 7.5 Remove `discover_linked_worktrees`, which lost its last consumer.

## 8. Slots and adoption

- [x] 8.1 Add `project/checkout`: generated slot identifiers, acquire (free slot first: switch to `agent/<id>` from the base, reset, clean without `-x`; create only when none is free), release (free or parked), the optional cap.
- [x] 8.2 Reconcile at space start: adopt checkouts without a task (parked when holding work, free otherwise), mark tasks without a checkout from where their branch stands, adopt legacy `agent/<name>` checkouts without renaming branches, prune last.
- [x] 8.3 Safe removal: prune branches fully reachable from the target; remove the directory of a clean slot idle beyond the declared age, keeping its branch.
- [x] 8.4 Cover slots: ignored artifacts survive reuse, a previous task's edits never reach the next, a dirty orphan is parked and every file preserved, an unintegrated branch outlives its directory, a new directory appears only when none is free, prune runs after adoption.
- [x] 8.5 Absorb the existing `worktree` module: keep primary resolution, ignore-file maintenance and the projected text; drop the seat.

## 9. Every agent is isolated

- [x] 9.1 Replace the seat decision in the application's launch placement with slot acquisition; no branch to the primary except the impossible-isolation fallback, which the read model reports.
- [x] 9.2 Remove the isolation marker and `push_isolation_marker`; keep `isolated_checkout` for the caption and the diff overlay; show the fallback warning on the tab.
- [x] 9.3 Cover placement: the first agent is isolated, three agents get three distinct checkouts and none is the primary, the operator's uncommitted work survives agents running, a shell tab creates no checkout, a repository without a commit launches in place with the warning, the diff overlay scopes to the tab's checkout.

- [x] 9.4 Keep listing a shell tab whose foreground process is a known harness, but as an unmanaged harness: name and real directory, no task state, no delivery action. Managed means the tab was created by `+ agent`, which today the tab's generated label and the slot its pane sits in say; an explicit task identifier on the tab arrives with the terminal protocol bump.

## 10. Readiness and delivery

- [x] 10.1 Evaluate readiness from Git (commits ahead of the base, clean tree) when a managed agent's pane goes quiet — the activity signal behind today's `Completed` status — and on demand; no harness signal required. An end-of-turn hook may trigger the same evaluation later, as an optional sharpening.
- [x] 10.2 Add `project/landing`: one task at a time under the write lock — rebase in the task's checkout, gate on the rebased commits, deliver by `completion` (`handoff` marks ready; `merge` fast-forwards the target, refusing on overlap with the operator's uncommitted changes; `pr` pushes under the readable name and opens the pull request with the available forge CLI, whose absence is a lock read-time error).
- [x] 10.3 On conflict or gate failure: leave the rebase paused, write the files and the target's range into the owning agent's pane, keep the target untouched, re-evaluate on the next quiet or on demand.
- [x] 10.4 Application surface: task list read model with state, one delivery service, the preserved-work list at space start; TUI: sidebar label and state, `i`/`I`, the preserved-work list with resume, deliver and discard.
- [x] 10.5 Cover delivery: handoff never touches the target, merge advances it linearly after the gate, gate runs after the rebase, a gate failure and a conflict both leave the target untouched and return to the owner, the second task sees the first, overlap with the operator's dirty primary refuses, pr pushes and opens the request against a fake forge CLI, only the operator discards.

- [x] 10.6 Rebase a live task onto the target automatically when the target has moved and the task's pane is quiet with a clean tree, through the same path as delivery's rebase; never under a dirty tree. No manual mode.

- [x] 10.7 In pr mode, resolve the target's tip from the remote-tracking branch after a fetch under the lock, and take "integrated" from the forge (request merged) rather than from reachability, so a squash merge still closes the task and prunes its branch; the operator's local target is never pulled.

## 11. Materialisation

- [x] 11.1 Grow the lock's `worktrees` block: `completion` gains `pr`; add `target` (default: the primary's checked-out branch at task creation), `link`, `setup`, `gate`, `slots`, all optional; `link` validated at read time (relative, inside the repository, ignored); unknown policy fields rejected by name, top level still tolerant.
- [x] 11.2 Materialise on acquire: link, then setup through the bounded subprocess helper with the transcript capturing output; a missing link target warns; a setup failure warns and launches.
- [x] 11.3 Decide and record whether `setup` inherits the consent boundary of remote executable capabilities. Decided: it does not — `agents.lock` is the project's own file, in the trust class of its Makefile; recorded in `design.md`.
- [x] 11.4 Cover the lock and materialisation: round-trip, no block still loads, escaping and tracked links rejected at read time, linked file is a symlink, missing target warns but launches, setup failure surfaced but not fatal.

## 12. Projection, Skill, invariants, dogfood

- [ ] 12.1 Extend the projected text: already isolated, commit on your own branch, never write the target, delivery is UZE's; keep the no-top-level-worktree property and the content-keyed region identity.
- [ ] 12.2 Rewrite the `worktree` Skill for the slot model and drop its integration guidance.
- [ ] 12.3 Rewrite the "Concurrent work isolation" section of `docs/architecture/invariants.md`: every agent isolated, primary is the operator's, nothing holding work removed automatically, delivery serialized with the gate on rebased commits, target written only in deliver, identity immutable, replaced lock field rejected — each tied to its test.
- [ ] 12.4 Dogfood on this repository: three agents with the primary dirty, deliver all three, force a conflict, kill UZE mid-delivery and restart; the primary's uncommitted edit survives every step.
- [ ] 12.5 Run the gate: formatting, clippy, the workspace suite, strict OpenSpec validation.

## 13. End-to-end proof of the engine

The slot and delivery engine is harness-independent, so its end-to-end
proof does not live in the Conformance Lab, whose contract is what every
harness must prove. It is an L3 group of its own.

- [ ] 13.1 Give `FakeHarness` a scripted-agent mode: a harness binary on `PATH` that follows a per-launch script (commit files, go quiet, wait for text in its pane, resolve a paused rebase, exit), so an agent's behaviour is deterministic and needs no model.
- [ ] 13.2 Drive the real terminal server through the client protocol from `tests/acceptance/`, the way the TUI does — create a space, create agents, read task state, trigger delivery — with real Git and an isolated `$UZE_HOME`, no PTY driver and no container.
- [ ] 13.3 Prove the engine end to end: three agents in three slots with the primary dirty and untouched; deliver all three in `merge` and see a linear target; a scripted conflict returned to its owner and resolved from the pane; `pr` against a fake forge CLI; the server killed mid-delivery and restarted with nothing lost and the target never half-written; legacy checkouts adopted; a dirty orphan parked.
- [ ] 13.4 Move the three checks of the Lab's `harnesses/uze` vertical (client reaches its prompt, `doctor` sees the provisioned environment, context delivered) into this group and retire the vertical: UZE is not a harness and the Lab's contract names none.
- [ ] 13.5 In the Lab, add one scene per real harness under the contract: the harness starts in the slot's directory and works there; it reads the projected declaration; scripted to "isolate", it creates no top-level worktree; text written into its pane reaches the model, visible in the request the synthetic provider receives.
