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
- [x] 5.2 `docs/adr/017-reproducible-agent-dependency-lock.md` exists.
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
- [x] 7.3 `docs/adr/016-project-agent-environment.md` and `docs/adr/017-reproducible-agent-dependency-lock.md` exist.
- [ ] 7.4 Dogfood exactly as described (`git add agents.lock` → `git commit` → fresh clone → `uze install`) was not run as a literal end-to-end git-clone scenario; the equivalent was verified without git (two separate `UzeHome`s sharing the same `agents.lock` on disk, both in the automated test and manually via the real CLI binary — see §2.1). The literal clone-based dogfood remains undone.

## 8. Known gaps (honest as of this review, not aspirational)

- `install_project_environment`'s "atomicity" is coarser than originally specified: any failure inside `install_materialized` (not just an ingest failure) aborts the whole call, matching `add_project_plugin`'s own behavior but not literally the old spec's "attach fails, lock still persists" scenario (removed from the spec — see §5.4).
- `plan_project_environment`'s `trust_required`, `delivery_changes`, `offline_unavailable`, and `conflicts` fields are and remain empty — each would require materializing a missing package just to inspect it without installing it, which nothing in this codebase does today. Documented in the function's own doc comment, not silently stubbed.
- No `NonReproducibleMarketplace` warning for `path`-sourced marketplaces (dropped from the spec, §5.5).
- `resolved.version` (a plugin manifest's own `version` field) is never populated — no manifest-version parsing exists anywhere in `uze-core` yet.
