## 1. Marketplace Registry

- [x] 1.1 Add `marketplaces.json` registry in `uze-core::state` (`name → PackageSource::Git|Local`) with add/list/remove validation (`marketplace.json` must exist and be well-formed)
- [x] 1.2 Implement `uze marketplace add <path|https://...>` (generic Git, not GitHub-specific; Local path canonicalized) and `marketplace remove` (blocked while plugins from that marketplace are installed)
- [x] 1.3 Make `marketplace list` merge `marketplaces.json` + pre-registered `uze-official` (embedded) with plugin counts

## 2. Plugin Install via Marketplace

- [x] 2.1 Implement `uze plugin install <name>@<marketplace>` resolving `marketplace.json` plugin `source` → `PackageSource` (Git with `subdirectory` or Local) → `acquisition` → `Store`
- [x] 2.2 Reuse existing `install_materialized` pipeline so `Store`/`Engine`/`Integration` remain unaware of marketplace (convergence)
- [x] 2.3 Record installed plugin provenance as `{name, marketplace}` for `plugin list` marketplace column and `update` re-resolution
- [x] 2.4 Keep `uze add <path>` as direct-source shortcut (no registry required)

## 3. Plugin Lifecycle

- [x] 3.1 Implement `plugin list` (with marketplace), `plugin remove <name>`, `plugin update <name>@<marketplace>` via registry-resolved source
- [x] 3.2 Ensure `plugin remove` respects ADR-009 (detach only Matched) and `marketplace remove` is blocked while plugins remain
- [x] 3.3 Add `plugin` and `marketplace` CLI help and shell completions

## 4. Convergence and Native Projection

- [x] 4.1 Verify `plugin install flow@ai` and `add /path/to/flow` converge to same Store bytes and same native projection (Claude/Codex/Gemini/OpenCode)
- [x] 4.2 Update `bootstrap` to treat `uze-official` as pre-registered (no functional change, just conceptual alignment)

## 5. Validation

- [x] 5.1 Add L0 tests for `marketplace add/list/remove` and `plugin install` (local + Git with subdirectory, idempotency, blocked remove)
- [x] 5.2 Dogfood with `hiukky/ai` marketplace: `marketplace add` → `plugin install flow@ai` → `claude plugin list` → `plugin remove` → `marketplace remove`
- [x] 5.3 Run `cargo fmt --check && cargo clippy --workspace --all-targets --all-features -- -D warnings && cargo test --workspace && openspec validate --all --strict`
- [x] 5.4 Confirm `docs/adr/015-marketplace-as-discovery-registry.md` exists
