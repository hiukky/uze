## 1. Core: `project_lock` and `project_root`

- [x] 1.1 Add `noyalib` dependency to `crates/uze-core/Cargo.toml` (replacing deprecated `serde_yaml`)
- [x] 1.2 Create `crates/uze-core/src/project_lock.rs` with `ProjectLock`, `LockedMarketplace`, `LockedPlugin`, parse/serialize YAML, `parse_plugin_marketplace_spec()`
- [x] 1.3 Create `crates/uze-core/src/project_root.rs` with `resolve_project_root()` (walk upward: `agents.lock` > `AGENTS.md` > `.git`) — a redundant duplicate check preceding the walk was found and removed during review (the walk's first iteration already covers `cwd`)
- [x] 1.4 Add error variants to `crates/uze-core/src/error.rs`: `UnsupportedLockVersion`, `MalformedLock`, `MarketplaceSourceConflict`, `MarketplaceMismatch` — plus `InvalidPluginSpec`, added during review to replace a misused `ExposureUnavailable` in `parse_plugin_marketplace_spec` and `plugin_install` (an unrelated error variant meaning "no exposure route," reused generically for spec-parsing failures)
- [x] 1.5 Update `crates/uze-core/src/lib.rs` with new modules

## 2. Application: `project_environment` use cases

- [x] 2.1 Create `crates/uze-application/src/application/project_environment.rs` with `project_environment()`, `plan_project_environment()`, `add_project_plugin()`, `remove_project_plugin()`, `install_project_environment()`.
      **Correction (review, 2026-08-22): this was marked done while `install_project_environment` was a literal stub returning `InstallReport::NotImplemented`, and the CLI reported "Environment installed" regardless** — a false completion, not just an incomplete one. Now genuinely implemented: resolves each missing locked plugin's source directly from the lock (not the global registry), acquires it, and installs it through `install_materialized`. Verified end-to-end (`tests/project_agent_environment.rs::install_project_environment_reproduces_a_lock_on_a_fresh_machine`, and manually via the real CLI: `marketplace add` → `flow@market` → fresh `UZE_HOME` → `uze install` → `uze status` correctly shows the plugin installed).
- [x] 2.2 Reuse existing `authorize→acquire→ingest→republish→attach` lifecycle from `lifecycle/install.rs`.
      **Correction: also false as originally marked** — `install_project_environment` didn't call any of it (it was a stub); only `add_project_plugin` did. Now both do, via `install_materialized`, and `authority` (previously accepted as `_authority` and silently ignored — the trust-boundary gap task 6.7 worried about) is now genuinely threaded through and enforced per plugin.
- [x] 2.3 Add read model types: `ProjectEnvironment`, `ProjectEnvironmentPlan`.
      **`ProjectPluginHealth`/`ProjectPluginState` did not exist despite being marked done** — `ProjectPluginState` was never built (no concrete need identified beyond a boolean); `ProjectPluginHealth { plugin, installed }` now exists and backs `uze status`'s lock section (see §4).
- [x] 2.4 Wire `project_environment` module in `crates/uze-application/src/application.rs`
- [x] 2.5 *(added during review)* `add_project_plugin` now populates `resolved.revision` from what acquisition actually observed (`Provenance.resolved`, via `ResolvedSource::lock_revision()`/`ResolvedPlugin::from_resolved_source()`), instead of always writing `None`. `resolved.version` remains `None` — no code in this crate parses a plugin manifest's `version` field yet; populating it is unstarted, not silently faked.
- [x] 2.6 *(added during review)* Extracted `resolve_locked_plugin_source` — the marketplace/plugin source resolution logic `add_project_plugin` and `install_project_environment` both need — into one shared method, removing duplicated (and previously slightly divergent) copies.

## 3. CLI: shorthand, `install`, `remove` disambiguation

- [x] 3.1 Add `uze <plugin>@<marketplace>` shorthand in `src/main.rs` (parse before `Command::from`, requires `@`)
- [x] 3.2 Add `uze install` command (consumer of `agents.lock`).
      **Correction:** the handler unconditionally printed "Environment installed" even when the result was `NotImplemented` (i.e., always, since nothing else was possible) — fixed alongside 2.1; the spinner/message now distinguishes `NoChanges` ("Already up to date") from `Installed`.
- [x] 3.3 Disambiguate `uze remove <plugin>`: if lock present + plugin in lock → project; else → global
- [x] 3.4 Add `--trust` flag to `uze <plugin>@<marketplace>` and `uze install`
- [x] 3.5 Update CLI help text and shell completions

## 4. Doctor / `status` extension

- [x] 4.1 Extend `StatusReport` with lock state.
      **Correction: false as originally marked** — `StatusReport` had no lock-related field at all; `render_status()` had no lock output. Now added: `StatusReport.project_lock: ProjectLockStatus` (`Absent` / `Malformed { reason }` / `Present { plugins: Vec<ProjectPluginHealth> }`), computed by `UzeApplication::project_lock_status` — deliberately infallible (a load/parse error becomes `Malformed`, not a `status`-command failure), simpler than the originally-planned `lock_present: bool` + `lock_error: Option<String>` pair (one enum instead of two independently-nullable fields covers the same states without an invalid combination being representable).
- [x] 4.2 Update `render_status()` in `src/main.rs` to display lock state and plugin health — now genuinely does, per-plugin (`installed` / `missing (run 'uze install')`).
- [x] 4.3 Ensure `desired ≠ actual` is diagnosticable — a locked-but-not-installed plugin now shows in `uze status` distinctly from an installed one, without being folded into `issues`/"unhealthy".

## 5. ADRs and OpenSpec

- [x] 5.1 `docs/adr/016-project-agent-environment.md` exists.
- [x] 5.2 The `agents.lock` schema is recorded (now `docs/adr/016-project-agent-environment.md`).
- [x] 5.3 `openspec/changes/project-agent-environment/` exists with `.openspec.yaml`, `proposal.md`, `design.md`, `tasks.md`.
- [x] 5.4 `specs/project-agent-environment/spec.md` — **rewritten during review**: the original used a non-conforming `REQ-PAE-NNN`/`**MUST**` format with no delta headers, and `openspec validate --strict` failed on it (`No delta sections found`). Converted to proper `## ADDED Requirements` / `### Requirement:` / `#### Scenario:` (WHEN/THEN) format, same substance, with one scenario (attach failure still persisting the lock) dropped because it no longer matches actual behavior (any `install_materialized` failure, not just an ingest failure, now leaves the lock untouched — see §2.1/2.2).
- [x] 5.5 `specs/agents-lock/spec.md` — same conversion. One requirement (`REQ-LOCK-008`, a `NonReproducibleMarketplace` warning in `plan_project_environment` for `path`-sourced marketplaces) was dropped rather than ported: it was never implemented, and porting it into proper delta format would have asserted it as current behavior. Tracked as a real gap in §6 below instead of a spec claim nothing backs.
- [x] 5.6 *(added during review)* `adr` artifact: mirrored the existing `docs/adr/016`/`017` into `openspec/changes/project-agent-environment/adr/` so `openspec status` reports this change's planning as complete (it previously showed `adr` unchecked — the two ADRs existed but were never linked into this change's own artifact tracking).

## 6. Tests

- [x] 6.1 Unit tests for `project_lock` parse/serialize/determinism (already in `project_lock.rs`)
- [x] 6.2 Unit tests for `project_root` resolution (already in `project_root.rs`)
- [x] 6.3 Integration tests for `add_project_plugin` (creates lock deterministically) — `tests/project_agent_environment.rs::add_project_plugin_creates_a_deterministic_lock` (also covers repeat-add idempotency at the lock-byte level) and `::add_project_plugin_populates_resolved_revision_for_a_local_marketplace_plugin`.
- [x] 6.4 Integration tests for `install_project_environment` (fresh-machine repro) — `::install_project_environment_reproduces_a_lock_on_a_fresh_machine` (separate `UzeHome`, asserts the plugin is genuinely acquired and installed) and `::install_project_environment_is_a_no_op_once_everything_is_installed`, `::install_project_environment_with_no_lock_is_a_no_op`.
- [x] 6.5 Integration tests for `remove_project_plugin` (removes from lock, not Store) — `::remove_project_plugin_removes_from_lock_but_not_from_the_store`, `::remove_project_plugin_reports_no_lock_and_not_in_lock_distinctly`.
- [x] 6.6 Integration tests for `plan_project_environment` (read-only) — covered indirectly (every test above relies on `plan`'s output driving `install` correctly); no dedicated "asserts zero filesystem writes" test was added. Left as a gap rather than claimed done.
- [ ] 6.7 Integration tests for trust boundary (lock never bypasses consent) — **partially covered**: `AlwaysTrust` is exercised throughout (proving the authority parameter is genuinely consulted, not ignored, per §2.2's fix), but no test exercises `NoTrustAuthority`/`TrustDenied` against a plugin that actually declares an executable capability — no such fixture exists yet. Real gap, not claimed done.
- [x] 6.8 Integration tests for idempotency (repeated add, no diff) — `::add_project_plugin_creates_a_deterministic_lock`.
- [x] 6.9 Integration tests for malformed/unsupported lock (blocks, no overwrite) — `::malformed_lock_is_reported_not_panicked_on`, `::unsupported_lock_version_is_reported_not_panicked_on`.
- [x] 6.10 Integration tests for global commands (never touch lock) — `::global_add_plugin_never_touches_the_project_lock`.
- [ ] 6.11 Integration tests for Store/Engine/Integration lock-neutrality — not added; `tests/vendor_neutral_core.rs` covers the broader vendor-neutrality invariant but nothing there specifically asserts lock-neutrality. Real gap.
- [ ] 6.12 Integration tests for offline scenarios (Store hit vs miss) — not added; `plan_project_environment`'s `offline_unavailable` field is itself an unimplemented stub (see design.md), so there is nothing yet to test here.

## 7. Validation

- [x] 7.1 `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace --no-fail-fast` all pass (re-verified 2026-08-22 after the review's changes).
- [x] 7.2 `openspec validate project-agent-environment --strict` passes (it did not, before §5.4/5.5's rewrite).
- [x] 7.3 `docs/adr/016-project-agent-environment.md` exists.
- [ ] 7.4 Dogfood exactly as described (`git add agents.lock` → `git commit` → fresh clone → `uze install`) was not run as a literal end-to-end git-clone scenario; the equivalent was verified without git (two separate `UzeHome`s sharing the same `agents.lock` on disk, both in the automated test and manually via the real CLI binary — see §2.1). The literal clone-based dogfood remains undone.

## 8. Known gaps (honest as of this review, not aspirational)

- `install_project_environment`'s "atomicity" is coarser than originally specified: any failure inside `install_materialized` (not just an ingest failure) aborts the whole call, matching `add_project_plugin`'s own behavior but not literally the old spec's "attach fails, lock still persists" scenario (removed from the spec — see §5.4).
- `plan_project_environment`'s `trust_required`, `delivery_changes`, `offline_unavailable`, and `conflicts` fields are and remain empty — each would require materializing a missing package just to inspect it without installing it, which nothing in this codebase does today. Documented in the function's own doc comment, not silently stubbed.
- No `NonReproducibleMarketplace` warning for `path`-sourced marketplaces (dropped from the spec, §5.5).
- `resolved.version` (a plugin manifest's own `version` field) is never populated — no manifest-version parsing exists anywhere in `uze-core` yet.

## 9. Manifest/lock split (added 2026-09-05)

ADR-017 recorded the split as a deferred non-goal; §9 is that deferral
being paid. No migration code — a pre-split `agents.lock` is rejected as
malformed, naming its two replacements.

- [x] 9.1 Two models, as `crates/uze-integrations/src/shared/toml_config.rs` does for Codex: serde for reading (typed, `deny_unknown_fields`), a document patch for writing so untouched bytes — comments included — survive. The lock keeps a plain serde round-trip; it is generated and has nothing to preserve.
- [x] 9.2 **Passed** (5/5, `crates/uze-core/tests/spike_cst.rs`, since folded into the module's own tests): round-trip is byte-identical; `set` on `worktrees.completion` changes only that value; `insert_entry` adds a plugin keeping the neighbour's comment, its indentation and its inline style; a splice producing invalid YAML is rejected leaving the document untouched. One wart for the wrapper to absorb: removing the last entry of a mapping leaves `plugins:\n  {}`. Original spike text: spike `noyalib`'s `cst` module before writing anything by hand — it sits behind the `std` feature, so it is already available. Its `Document` claims byte-identical round-trip, path-targeted `set`, and comment read/write, and it already re-parses after an edit and rejects the splice if the result is invalid, leaving the document untouched — the guarantee 9.1 asks for, already built. Take a real `agents.yaml` with comments, `set` each of the three paths UZE writes, and assert every byte outside the edit is unchanged. One hour, and it decides 9.3.
- [x] 9.3 *(not needed — 9.2 passed)* Would have been: write the surgical editor by hand with the same guarantee: parse to a struct, compute the struct the edit should produce, apply the text edit, re-parse, and discard unless the result equals the expected struct exactly.
- [x] 9.4 Guards live in `manifest::edit::reject_shapes_we_will_not_edit`, refusing anchors, aliases, merge keys, tabs and document separators before an edit is attempted, with a comment-aware scan so prose mentioning `<<:` is not mistaken for one. Original: UZE writes at most `plugins.<name>`, `marketplaces.<name>` and `worktrees.completion`. Guard the exotic YAML neither implementation should touch — anchors and aliases, merge keys, multiple documents, a flow mapping at the top level, tabs — refusing before attempting rather than after mangling, and naming the key to edit by hand. Never fall back to re-serializing the whole file.
- [x] 9.5 `crates/uze-core/src/project/manifest/edit.rs` is the only file naming `noyalib::cst`, exposing `ManifestDocument` with `set`/`upsert`/`remove`/`append_block`/`save`. It absorbs the empty-mapping wart 9.2 found: removing a mapping's last entry removes the parent key rather than leaving `{}`.
- [ ] 9.6 Keep `noyalib`, and bump 0.0.28 → current. Re-evaluated on evidence rather than signals: 43.6k lines of source against 65.5k of tests, 351 vendored `yaml-test-suite` cases, `#![forbid(unsafe_code)]` with zero `unsafe` in the tree. Still true and still the risk: `0.0.x` across 33 releases, one author, renamed once from `serde_yml`, and scope sprawl (a `robotics` module in a YAML library). Record the exception where `AGENTS.md` asks for it — "refuse `0.0.x` unless the reason is written down" — with 9.5 as the mitigation. Revisit if 9.2 fails or the crate goes quiet.
- [x] 9.7 *(rejected, recorded so it is not rediscovered)* `yamlpath`/`yamlpatch` (zizmor, v1.30, MIT) pulls `tree-sitter` + `tree-sitter-yaml` — a C grammar with a build script — into a crate with five dependencies, onto a release matrix that already needed a workaround for one C dependency (`release.yml:206`). Disproportionate to three write paths, and unnecessary: 9.2 passed.
- [ ] 9.8 *(manifest half done, lock half open)* `project/manifest.rs` carries the typed schema, validation, and the writers (`ensure_exists`, `declare_plugin`, `undeclare_plugin`). Still to do: `project_lock.rs` becoming the derived half — that is 9.12/9.13/9.14 below.
- [x] 9.9 `WorktreePolicy` left the lock. `REPLACED_KEYS` in `project_lock.rs` now rejects both `worktrees_dir` and `worktrees`, naming `agents.yaml`; `reject_unignored_links` moved to `manifest`; the two consumers (`services/tasks.rs::policy`, `application/context.rs::worktree_policy`) read the manifest; this repository's own declaration was split into `agents.yaml` + a policy-free `agents.lock`.
- [x] 9.10 A manifest plugin entry carrying `revision`/`integrity`/`version` is rejected, and `explain` replaces serde's wording with a message naming `agents.lock` as where resolution belongs.
- [x] 9.11 `workspace.rs` and `project_root.rs` anchor on `agents.yaml`. The reasoning is in the module doc: the lock is derived, and a derived file cannot be what identifies a project — a project that declared but never resolved is still a workspace.
- [x] 9.12 `resolved` is flattened into the lock entry (`#[serde(flatten)]`: `revision`, `version`, `integrity`) and `RequestedPlugin` echoes the declaration it was resolved from — marketplace, git, ref: only what can change a resolution, so a comment or a key's position cannot make a lock look stale.
- [x] 9.13 `digest::tree_sha256` (SHA-256 over sorted, length-prefixed path+content pairs) is recorded from the Store's own bytes on add and verified on install, before ingest and before any harness sees anything. A mismatch is `UzeError::IntegrityMismatch`, naming both digests and saying nothing was installed. A source with no stable revision records no pin rather than one that would be wrong by the next save.
- [x] 9.14 `project_lock::stale_against(manifest, lock)` compares the manifest's declarations against each entry's `requested` echo — no network, no re-resolution, so `status` still answers in a tunnel. A plugin the lock has never seen is stale; an entry predating the echo is deliberately *not* reported, since calling every existing project out of date for a reason nobody can act on is worse than saying nothing.
- [x] 9.15 `add` declares in `agents.yaml` first and records the resolution in the lock second; `remove` undeclares and regenerates, deleting the lock when nothing is left to reproduce (`project_lock::remove_lock`). `declared_marketplace_for` maps a resolved `MarketplaceSource` back to the declaration that produced it, so a person reads what they asked for rather than what resolution made of it.
- [ ] 9.16 Reject the pre-split lock format with an error naming `agents.yaml` and the regeneration step; delete this repo's own `agents.lock` and re-author it as `agents.yaml` + regenerated lock.
- [ ] 9.17 Update `tests/_fixtures/scenarios/malformed-lock/` and every fixture writing an `agents.lock`; add a fixture for the pre-split shape.
- [x] 9.18 `tests/lifecycle/manifest_and_lock.rs` (8 tests, product-level through `UzeApplication`): reading a project creates nothing; `install` writes the commented default and no lock when there is nothing to resolve; `install` twice is byte-identical; a manifest somebody authored is never rewritten; the declared behavior reaches the `AGENTS.md` projection; a lock still carrying the policy is refused naming `agents.yaml`; a typo is named. Plus 34 unit tests in `manifest`/`manifest::edit`.
- [ ] 9.19 `openspec validate project-agent-environment --strict` and `add-portable-worktree-policy --strict` pass; `make check` clean.

## 10. Creation and discoverability (added 2026-09-05)

Reframed from the original amendment: nothing is created on arrival.
`tasks.rs:222` already defaults an undeclared policy to `Handoff`, so a
project with no manifest works today — what was missing was
discoverability, which §11's popup provides.

- [x] 10.1 *(dropped — no new command)* `uze init` was never asked for; `uze install` is already an explicit act of setting a project up, so it creates the manifest. One fewer command on the surface. The "created by intent" rule is unchanged: what it rules out is creation on *opening* the client, not creation by a command the person typed.
- [x] 10.2 `manifest::ensure_exists` writes the built-in default spelled out and commented (`handoff | merge | pr`), so the knobs are discoverable by opening the file. Idempotent, and it never touches a manifest somebody wrote. Called from `install` only — never from the client, inspection or planning.
- [x] 10.3 Pinned by `tests/lifecycle/manifest_and_lock.rs::policy_scope`: a manifest inside an isolated checkout does not override the primary's. It held by construction before — nothing stopped a future caller passing a worktree's own path, which is why it is now a test.
- [x] 10.4 Pinned by the same module: the same repository with an undeclared policy resolves identically under two different `UZE_HOME`s, so two developers project the same `AGENTS.md`.
- [x] 10.5 `project_lock::remove_lock` deletes the lock when the last entry goes, rather than leaving one that declares nothing.

## 11. Policy in the agent context popup (added 2026-09-05)

`delivery_policy()` (`crates/uze-application/src/application/services/tasks.rs:280`)
already returns `completion`/`target`/`gate` — this is a presentation and
write-path task, not a new read model.

- [x] 11.1 `DeliveryPolicyView` carries `PolicySource` (`Declared` / `BuiltInDefault`) with an `attribution()` naming `agents.yaml` or `default`. Without it, showing a reader `handoff` teaches nothing: they cannot tell whether anyone chose it.
- [ ] 11.2 Render it in the space/tab context menu, which is already the right shape: `MenuAction`'s own doc says a third action is "a variant plus a match arm here and in `dispatch_menu_action`, not restructuring the popup". Add `SetCompletion(CompletionBehavior)` × 3, marking the one in force and attributing it via `PolicySource`. `src/` reaches `uze-application` only (`tests/architecture/layering.rs`).
- [ ] 11.3 Write the manifest on click, creating it when absent, and state the consequence before writing: the tracked file being changed and the reconciliation `AGENTS.md` still needs.
- [ ] 11.4 Bind a change to tasks created after it; a live task keeps the policy it launched under, and the popup shows both.
- [ ] 11.5 The write runs off the render thread like every other unbounded workspace operation (`spawn_*`/`absorb_*` pair; `orchestrator/` may not name `WorkspaceHost`).
- [ ] 11.6 Tests: attribution of an undeclared policy; click creates the manifest; a live task keeps its launch policy; a stale projection is reported.
