# Marketplace as Discovery Registry, Plugin as Installable Unit

Status: Superseded in part by [ADR-036](036-qualify-store-plugins-by-marketplace.md)

## Context

UZE already has a marketplace contract (`marketplace.json` + `plugins/**`) for the embedded official marketplace, but no registry for user-added marketplaces (e.g., `hiukky/ai`). `uze add <path>` only handles a single package source, so `flow@ai` cannot be resolved without a per-plugin path. Mirroring Claude's `marketplace add` → `plugin install name@marketplace` requires a discovery registry that does not become a second Store.

## Decision

We will store marketplace discovery sources as a registry in `~/.uze/state/marketplaces.json` (`name → Git|Local`), with `marketplace.json` validation at registration. `plugin install name@marketplace` will resolve the marketplace entry's `source` + `marketplace.json` plugin `source` into a `PackageSource` (Git with `subdirectory` or Local) and then reuse the existing `acquisition → Store → native projection` pipeline. `Store` will remain unaware of marketplace; `add <path>` remains as direct-source shortcut. The embedded official marketplace is treated as pre-registered `uze-official`.

## Consequences

Marketplace becomes discovery only (no byte copy until plugin install), `acquisition` stays generic (`Git` not `GitHub`), and plugin install converges. Registry drift is validated at install time; `marketplace remove` is blocked while plugins from that marketplace are installed. Adds a new state file to manage and a `Git` clone on first `plugin install` from a Git marketplace, but avoids a second source of truth.

Source change: openspec/changes/marketplace-registry-and-plugin-install/
