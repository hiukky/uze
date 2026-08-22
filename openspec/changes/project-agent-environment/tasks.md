## 1. Core: `project_lock` and `project_root`

- [x] 1.1 Add `noyalib` dependency to `crates/uze-core/Cargo.toml` (replacing deprecated `serde_yaml`)
- [x] 1.2 Create `crates/uze-core/src/project_lock.rs` with `ProjectLock`, `LockedMarketplace`, `LockedPlugin`, parse/serialize YAML, `parse_plugin_marketplace_spec()`
- [x] 1.3 Create `crates/uze-core/src/project_root.rs` with `resolve_project_root()` (walk upward: `agents.lock` > `AGENTS.md` > `.git`)
- [x] 1.4 Add error variants to `crates/uze-core/src/error.rs`: `UnsupportedLockVersion`, `MalformedLock`, `MarketplaceSourceConflict`, `MarketplaceMismatch`
- [x] 1.5 Update `crates/uze-core/src/lib.rs` with new modules

## 2. Application: `project_environment` use cases

- [x] 2.1 Create `crates/uze-application/src/application/project_environment.rs` with `project_environment()`, `plan_project_environment()`, `add_project_plugin()`, `remove_project_plugin()`, `install_project_environment()`
- [x] 2.2 Reuse existing `authorize→acquire→ingest→republish→attach` lifecycle from `lifecycle/install.rs`
- [x] 2.3 Add read model types: `ProjectEnvironment`, `ProjectEnvironmentPlan`, `ProjectPluginHealth`, `ProjectPluginState`
- [x] 2.4 Wire `project_environment` module in `crates/uze-application/src/application.rs`

## 3. CLI: shorthand, `install`, `remove` disambiguation

- [x] 3.1 Add `uze <plugin>@<marketplace>` shorthand in `src/main.rs` (parse before `Command::from`, requires `@`)
- [x] 3.2 Add `uze install` command (consumer of `agents.lock`)
- [x] 3.3 Disambiguate `uze remove <plugin>`: if lock present + plugin in lock → project; else → global
- [x] 3.4 Add `--trust` flag to `uze <plugin>@<marketplace>` and `uze install`
- [x] 3.5 Update CLI help text and shell completions

## 4. Doctor / `status` extension

- [x] 4.1 Extend `StatusReport` with `project_plugins: Vec<ProjectPluginHealth>`, `lock_present: bool`, `lock_error: Option<String>`
- [x] 4.2 Update `render_status()` in `src/main.rs` to display lock state and plugin health
- [x] 4.3 Ensure `desired ≠ actual` is diagnosticable, not collapsed into "unhealthy"

## 5. ADRs and OpenSpec

- [x] 5.1 Create `docs/adr/016-project-agent-environment.md`
- [x] 5.2 Create `docs/adr/017-reproducible-agent-dependency-lock.md`
- [x] 5.3 Create `openspec/changes/project-agent-environment/` with `.openspec.yaml`, `proposal.md`, `design.md`, `tasks.md`
- [x] 5.4 Create `openspec/changes/project-agent-environment/specs/project-agent-environment/spec.md` with MUST/SHOULD scenarios
- [x] 5.5 Create `openspec/changes/project-agent-environment/specs/agents-lock/spec.md` with schema and determinism scenarios

## 6. Tests

- [x] 6.1 Unit tests for `project_lock` parse/serialize/determinism (already in `project_lock.rs`)
- [x] 6.2 Unit tests for `project_root` resolution (already in `project_root.rs`)
- [ ] 6.3 Integration tests for `add_project_plugin` (creates lock deterministically)
- [ ] 6.4 Integration tests for `install_project_environment` (fresh-machine repro)
- [ ] 6.5 Integration tests for `remove_project_plugin` (removes from lock, not Store)
- [ ] 6.6 Integration tests for `plan_project_environment` (read-only, zero writes)
- [ ] 6.7 Integration tests for trust boundary (lock never bypasses consent)
- [ ] 6.8 Integration tests for idempotency (repeated `uze flow@ai` no diff)
- [ ] 6.9 Integration tests for malformed/unsupported lock (blocks, no overwrite)
- [ ] 6.10 Integration tests for global commands (never touch lock)
- [ ] 6.11 Integration tests for Store/Engine/Integration lock-neutrality
- [ ] 6.12 Integration tests for offline scenarios (Store hit vs miss)

## 7. Validation

- [x] 7.1 Run `cargo fmt --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --workspace`
- [ ] 7.2 Run `openspec validate --all --strict` (if available)
- [x] 7.3 Confirm `docs/adr/016-project-agent-environment.md` and `docs/adr/017-reproducible-agent-dependency-lock.md` exist
- [ ] 7.4 Dogfood: `uze flow@ai` → `git add agents.lock` → `git commit` → fresh machine → `git clone` → `uze install` → verify environment
