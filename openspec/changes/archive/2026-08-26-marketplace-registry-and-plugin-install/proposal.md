## Why

UZE already delivers a single plugin package via `uze add <path|git>` through acquisition → Store → native projection, but a marketplace (e.g., `hiukky/ai` with `marketplace.json` + `plugins/flow|std`) has no CLI surface: `uze add /path/to/marketplace` fails with `bundle manifest is missing`, and `hiukky/ai` cannot be consumed as `flow@ai`. Following the Claude `marketplace add` → `plugin install name@marketplace` pattern, UZE needs a discovery registry (marketplace as source) and a plugin install that converges into the existing pipeline, without re-opening Store/Engine/Projection.

## What Changes

- `uze marketplace add <path|https://...>` registers a marketplace discovery source in `~/.uze/state/marketplaces.json` (git/local, generic `Git` source, not GitHub-specific)
- `uze marketplace list|remove <name>` manages the registry
- `uze plugin install <name>@<marketplace>` resolves the marketplace entry → `PackageSource` → `acquisition` → `Store` → native projection (same path as `uze add`)
- `uze plugin list|remove|update <name>` (and `list` showing `marketplace` column) for marketplace-installed plugins
- `uze add <path>` remains as shortcut/backward-compatible for direct package source
- Embedded official marketplace (`plugins/uze`) becomes a pre-registered entry `uze-official` (conceptual, no new persistence beyond current bootstrap)

## Capabilities

### New Capabilities
- `marketplace`: registry of marketplace discovery sources (add/list/remove, git/local, marketplace.json resolution)
- `plugin`: install/update/remove/list of plugins via `name@marketplace` through the existing acquisition → Store → projection pipeline

### Modified Capabilities
- (none) — existing `add`/`Store`/`Engine` behavior unchanged; new commands are additive. `add` remains shortcut.

## Impact

- CLI: new `marketplace` and `plugin` command groups
- Core: `acquisition::marketplace` already exists; new `marketplace` registry in `uze-core::state` (or new module) + `PackageSource` handling for marketplace-resolved plugins
- Store/Engine/Integration unchanged (vendor-neutral)
- TUI: future `Marketplace → plugins` view can read registry
- Docs: marketplace as discovery source, plugin as installable unit, convergence into native projection
