## Context

UZE already has `marketplace.json` + `plugins/**` at the repo root and `acquisition::marketplace` (pure `parse_manifest` + `resolve_plugin_source`) used for the embedded official marketplace via `bootstrap`. `Store` is authoritative for bytes, `Engine` discovers capabilities, `Integration` delivers natively. Claude's `marketplace add` → `plugin install name@marketplace` has proven the UX in dogfood, but UZE only exposes `add <path|git>` for a single package. No registry for marketplace discovery sources exists; `hiukky/ai` (marketplace.json with `std|flow`) cannot be consumed as `flow@ai` without per-plugin path.

See proposal.md Why for motivation.

## Goals / Non-Goals

**Goals:**
- Registry for marketplace discovery sources (`~/.uze/state/marketplaces.json`, generic `Git`/`Local`)
- `plugin install name@marketplace` that converges into existing `PackageSource` → `acquisition` → `Store` → `native projection`
- Keep `add <path>` as shortcut, `Store` unaware of marketplace, `uze-official` as pre-registered embedded

**Non-Goals:**
- Marketplace federation/search, version resolver, sparse checkout, TUI redesign, `uze.lock` (future)
- Changing `PackageSource::Git` host semantics (remains `Git`, not `GitHub`)

## Decisions

- **Registry location `state/marketplaces.json` (not Store):** Discovery is not installed bytes; mirrors `state/integrations.json` pattern. Alternative `Store` considered but rejected (Store must stay harness/marketplace-agnostic).
- **Reuse `PackageSource::Git` with `subdirectory` for marketplace plugin entries:** `marketplace.json` plugin `source: "./plugins/flow"` maps to `Git{url, subdirectory: Some("plugins/flow")}` or `Local{path}`. No new `PackageSource` variant; `acquisition` already handles subdirectory via `MaterializedPackage::retarget`. Alternative new `Marketplace` variant rejected (would duplicate Git logic).
- **Convergence, not new pipeline:** `plugin install` resolves registry → `PackageSource` → calls existing `add_plugin`/`install_materialized` path. No second Store write path.
- **UX mirroring Claude `marketplace add` / `plugin install name@marketplace`:** `marketplace` noun for discovery, `plugin` noun for installable unit. `add` kept as backward-compatible shortcut that creates a direct `Local`/`Git` source without registry.

## Risks / Trade-offs

- [Stale marketplace manifest] → Mitigation: `marketplace add` validates `marketplace.json` at registration; `plugin install` re-reads manifest and fails clearly if plugin missing
- [Git marketplace needs clone to discover] → Mitigation: use `acquisition` scratch clone (same as `add` Git), not persistent cache; `update` re-resolves
- [Registry drift vs embedded] → Mitigation: `uze-official` is conceptual pre-registered (no registry entry needed), `marketplace list` merges embedded + `marketplaces.json`
- [Duplicate marketplace names] → Mitigation: `marketplace add` rejects duplicate name; `marketplace remove` blocks while plugins from that marketplace are installed
