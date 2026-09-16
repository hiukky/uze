## 1. The terminal carries an environment per tab

- [ ] 1.1 `CreateTab` gains `env: Vec<(String, String)>`; `Tab` reports `env`; the persisted tab keeps it with its command as one launch, with no default for a file written before; `PROTOCOL_VERSION` bumps.
- [ ] 1.2 `CreateTab` refuses an environment without a command, an environment over the documented bound on entries and total size, and a variable name that is empty or contains `=`.
- [ ] 1.3 `PaneRuntime` keeps its `Launch` (the command and its environment), applies the environment after stripping the inherited launch variables, and carries none when a pane is respawned as a plain shell (`spawn_pane(pane, Launch::Shell)`).
- [ ] 1.4 The server's restore path zips `env` with `command` and applies it on respawn; the fallback to a shell carries nothing.
- [ ] 1.5 `uze_terminal::launch` owns the vocabulary of launch-stamped variables: `AGENT_IDENTITY_VARIABLE` beside `UZE_PANE`, and the strip list is built from it. The server never reads the value.
- [ ] 1.6 Runtime tests: the environment reaches the first process; it survives a restart; a shell respawn carries none; a shell request with an environment is refused; an oversized environment is refused; a server started with the identity variable set spawns a clean shell.

## 2. Core verifies a claim and never names the variable

- [ ] 2.1 `TaskStore::agent(id) -> Option<AgentRecord<'_>>` with one `own_directory()` per variant; `conversation::owner_of(home, Claim { id, cwd })` answers only when the store names `id` and `cwd` lies inside that directory.
- [ ] 2.2 `continuity::plan`, `refresh` and `record_observed` take the claim, or nothing; none reads the environment.
- [ ] 2.3 Core tests: verified claim resumes; unknown id ignored; wrong directory ignored; two agents recorded over one directory resolve by identifier alone.
- [ ] 2.4 Remove `worktree::isolated_checkout` from every identity path; keep it where it answers a directory question (slot occupancy, captions, `space_root`).
- [ ] 2.5 Architecture test: `crates/uze-core` and `crates/uze-application` never name the identity variable.

## 3. The shim owns the identity it launches

- [ ] 3.1 The shim builds the claim from its environment and working directory, and applies the ownership rule: an identity accompanied by a `UZE_SHIM_PID` other than its own pid is absent. The sanction for `src/shim.rs` in `tests/architecture/layering.rs` names `uze_terminal` as well as core, with the reason.
- [ ] 3.2 Shim tests: a stamp with no owner is taken; a stamp owned by another pid is ignored and the launch is ordinary; the operator's own arguments are untouched either way.

## 4. The client stamps at launch and binds by the echo

- [ ] 4.1 `open_agent_tab` passes the placed agent's identifier in `CreateTab.env`; the record is written before the tab opens (already true for tasks).
- [ ] 4.2 The pane→task binding reads `Tab.env` from the session; `bind_pane_tasks` and `slot_claims` are removed; `pane_checkouts` stays for slot occupancy.
- [ ] 4.3 `spawn_occupancy_reconcile` carries the identifiers live tabs echo alongside the directories their panes hold; `release_abandoned_tasks` accepts both.
- [ ] 4.4 `spawn_conversation_refresh` builds the claim from the tab's echoed identifier and the tab's launch directory, never from the probed pane cwd.
- [ ] 4.5 TestBackend tests: a tab bound by echo draws its task; a pane that lost its checkout is still bound; a shell beside an agent binds to nothing; a tab whose harness exited and left a shell is not an agent row.

## 5. The agent's own surface

- [ ] 5.1 `Workspace::name_task(claim, name)` resolves by the claim; the refusal reads "this process is not an agent UZE launched".
- [ ] 5.2 `uze agent task name` builds the claim from the inherited identity and its working directory, without the ownership rule: a child of the harness is the normal caller.
- [ ] 5.3 Acceptance: naming from inside the agent's process works; naming from a shell with no stamp refuses; a stamp naming another task from the wrong directory refuses.

## 6. Provenance, Lab and docs

- [ ] 6.1 `tests/acceptance/session_continuity.rs`: the relaunch scene lays a stamp down; a hand launch inside a slot with no stamp launches untouched; a nested launch inside a stamped pane launches untouched.
- [ ] 6.2 `tests/integrations/runtime_boundary.rs` gains the nested-launch case.
- [ ] 6.3 `conformance/contract/continuity.py`: the relaunch command carries the stamp (`UZE_AGENT=<id> <launcher> <args>`, twice, no `UZE_SHIM_PID`) with the id equal to the slot's checkout name; `isolation.py` unchanged.
- [ ] 6.4 `docs/architecture/invariants.md`: the identity property, the ownership property and the nested-launch property, each tied to its test; ADR-047 is not edited (the ADR at archive supersedes its mechanism).
- [ ] 6.5 User docs: the behaviour change for a harness started by hand inside an isolated checkout, where continuity is described.
- [ ] 6.6 Gate: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace --no-fail-fast`, `openspec validate --all --strict`.
