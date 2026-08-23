## 1. Read model (Application)

- [x] 1.1 `overview_workspace` returns `ProjectOverview { environment, memory, declared_plugins, installed_plugins, missing_plugins }` + `OverviewMarketplace { name, package_count, invalid_packages, state }`; file-level fields (`agents.lock`/`agents.json` rows, `.agents/` count, per-package listing, `Invalid`) removed from the projection
- [x] 1.2 `ProjectEnvironmentState { NotConfigured, Invalid, InstallRequired, Ready }` derived in `project_overview` (valid lock + all declared installed → Ready; never Ready with a missing declared plugin)
- [x] 1.3 `MemoryState { None, Ready, Issue }` via pure `derive_memory(agents_md, portability)` truth table, portability from `context_inspect`
- [x] 1.4 `MarketplaceState { Valid, InvalidManifest }`; invalid-package count kept distinct from manifest validity
- [x] 1.5 Remove `count_local_resources` from `uze-core::workspace` (no longer part of the projection)

## 2. TUI

- [x] 2.1 PROJECT column renders `Environment` / `Memory` / `Plugins` from the Application states, verbatim
- [x] 2.2 MARKETPLACE column renders `Name` / `Plugins` / `Status`
- [x] 2.3 `i install` gated on `ProjectEnvironmentState::InstallRequired` only
- [x] 2.4 Workspace section always stacked (PROJECT block above MARKETPLACE block), width-capped at 36 cells; project-only, marketplace-only and empty states per kind
- [x] 2.5 Indicator semantics: `✓` verified healthy, `!` attention, `×` error, `—` absent (quantities uncolored unless divergent)

## 3. Tests

- [x] 3.1 Application: plain dir, AGENTS.md-only, valid+installed, valid+nothing, partial, malformed, unsupported version, empty lock, ready-never-with-missing, memory truth table, marketplace valid/invalid/missing-sources, hybrid, nested
- [x] 3.2 TUI: render states verbatim, install action gating, no PROJECT for marketplace-only, no MARKETPLACE for consumer-only, wide/narrow layout, no mutation on render, no-workspace creates nothing, e2e install flips state to Ready
- [x] 3.3 Pre-existing `ExitStatus::from_raw` test-compile issue in `uze-integrations` fixed (was blocking `--workspace` gates)

## 4. Validation

- [x] 4.1 `cargo fmt --check`
- [x] 4.2 `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [x] 4.3 `cargo test --workspace --no-fail-fast`
- [x] 4.4 `openspec validate --all --strict`
- [x] 4.5 `git diff --check`
