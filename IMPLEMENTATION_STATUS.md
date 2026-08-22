# Project Agent Environment Implementation Status

## Completed

### Core Implementation
- ✅ `project_lock` module with YAML serialization (using `noyalib` instead of deprecated `serde_yaml`)
- ✅ `project_root` module with deterministic root resolution
- ✅ Error types for lock validation (UnsupportedLockVersion, MalformedLock, MarketplaceSourceConflict, MarketplaceMismatch)
- ✅ Unit tests for project_lock and project_root

### Application Layer
- ✅ `project_environment()` - read-only project state observation
- ✅ `plan_project_environment()` - read-only change planning
- ✅ `add_project_plugin()` - add plugin to project lock
- ✅ `remove_project_plugin()` - remove plugin from project lock
- ✅ `install_project_environment()` - install from lock (stub)

### CLI Integration
- ✅ `uze <plugin>@<marketplace>` shorthand syntax
- ✅ `uze install` command
- ✅ `uze remove` disambiguation (project vs global)
- ✅ `--trust` flag support
- ✅ Help text and render functions

### Documentation
- ✅ ADR-016: Project Agent Environment
- ✅ ADR-017: Reproducible Agent Dependency Lock
- ✅ OpenSpec change proposal
- ✅ OpenSpec design document
- ✅ OpenSpec tasks breakdown
- ✅ OpenSpec requirements (project-agent-environment)
- ✅ OpenSpec requirements (agents-lock)

### Validation
- ✅ All tests pass (57 tests)
- ✅ Code formatted (cargo fmt)
- ✅ No clippy warnings
- ✅ Build succeeds

## Remaining Work

### Integration Tests (Priority: High)
- [ ] Test `add_project_plugin` creates lock deterministically
- [ ] Test `install_project_environment` fresh-machine repro
- [ ] Test `remove_project_plugin` removes from lock, not Store
- [ ] Test `plan_project_environment` read-only behavior
- [ ] Test trust boundary (lock never bypasses consent)
- [ ] Test idempotency (repeated `uze flow@ai` no diff)
- [ ] Test malformed/unsupported lock handling
- [ ] Test global commands never touch lock
- [ ] Test Store/Engine/Integration lock-neutrality
- [ ] Test offline scenarios

### Implementation Gaps (Priority: Medium)
- [ ] Complete `install_project_environment` implementation (currently stub)
  - Need to resolve marketplace sources from lock
  - Need to acquire packages from resolved sources
  - Need to handle trust requirements
  - Need to persist resolved revisions to lock
- [ ] Implement `resolved.revision` population in `add_project_plugin`
  - Currently stores `None`, should store actual commit hash
- [ ] Implement `resolved.version` population from plugin.json
- [ ] Add `integrity` field support (reserved but not implemented)

### Doctor/Status Extension (Priority: Low)
- [ ] Extend `StatusReport` with project lock information
- [ ] Update `render_status()` to display lock state
- [ ] Ensure `desired ≠ actual` is diagnosticable

### Dogfooding (Priority: Low)
- [ ] Test with real marketplace (e.g., `hiukky/ai`)
- [ ] Verify fresh-machine repro workflow
- [ ] Document any UX issues

## Architecture Decisions

### YAML Serialization
- Chose `noyalib` over deprecated `serde_yaml`
- `noyalib` is actively maintained, zero unsafe code
- Compatible API via `compat-serde-yaml` feature

### Lock Format
- Version 1 schema with explicit version field
- Deterministic serialization (BTreeMap ordering)
- Separate `marketplaces` and `plugins` sections
- Support for git, path, and embedded sources
- Reserved fields for future (integrity, version)

### Project Root Resolution
- Priority: agents.lock > AGENTS.md > .git
- Walks up directory tree
- Falls back to current directory
- No git assumption (works in non-git projects)

### Trust Boundary
- Lock never bypasses consent
- `authorize()` always called for executable capabilities
- Fresh machine requires explicit trust

## Files Modified

### New Files
- `crates/uze-core/src/project_lock.rs` (332 lines)
- `crates/uze-core/src/project_root.rs` (115 lines)
- `crates/uze-application/src/application/project_environment.rs` (290 lines)
- `docs/adr/016-project-agent-environment.md`
- `docs/adr/017-reproducible-agent-dependency-lock.md`
- `openspec/changes/project-agent-environment/.openspec.yaml`
- `openspec/changes/project-agent-environment/proposal.md`
- `openspec/changes/project-agent-environment/design.md`
- `openspec/changes/project-agent-environment/tasks.md`
- `openspec/changes/project-agent-environment/specs/project-agent-environment/spec.md`
- `openspec/changes/project-agent-environment/specs/agents-lock/spec.md`

### Modified Files
- `crates/uze-core/Cargo.toml` (added noyalib dependency)
- `crates/uze-core/src/lib.rs` (added project_lock and project_root modules)
- `crates/uze-core/src/error.rs` (added 4 new error variants)
- `crates/uze-application/src/application.rs` (added project_environment module and re-exports)
- `src/main.rs` (added Install command, shorthand parsing, render_install)

## Next Steps

1. **Integration Tests**: Write comprehensive integration tests to validate the implementation
2. **Complete install_project_environment**: Implement the full installation logic
3. **Populate resolved fields**: Store actual commit hashes and versions in the lock
4. **Dogfooding**: Test with real marketplaces and document any issues
5. **Doctor Extension**: Extend status command to show lock state

## Known Limitations

1. `install_project_environment` is a stub - needs full implementation
2. `resolved.revision` and `resolved.version` are not populated yet
3. No integrity hash support (reserved for future)
4. No offline mode handling yet
5. Trust requirements not fully validated in install flow
