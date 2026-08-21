# Model a marketplace contract for the embedded default plugin

Status: Accepted

## Context

The previous "builtin `uze` plugin" bootstrap hardcoded exactly one plugin
id: `bootstrap::materialize` was a `match id { "uze" => ... }`, and adding a
second official plugin would have meant a new match arm per plugin, per
lookup function. It also conflated two questions that should be independent
— "which plugins does the official UZE marketplace offer" and "which of
those install by default" — and, more seriously, its freshness check
(`is_current`) called `update_plugin` (a full detach + reinstall) from
inside `ensure_default_plugins`, which every CLI command runs before
dispatch, including read-only ones. Diagnostic commands could silently
rewrite installed plugin content.

## Decision

The repository itself is the official UZE marketplace: `marketplace.json`
at the root plus `plugins/**`. `uze-core::acquisition::marketplace` is a
pure, deterministic primitive — `parse_manifest` + `resolve_plugin_source`
— that answers "which plugins exist, and where" for any local directory
holding that shape, with no opinion on how the directory got there. It has
no knowledge of any specific plugin name.

`PackageSource::Embedded { id }` is kept as-is (no new source type, no
persisted-shape migration) but its resolution stops being hardcoded:
`uze-application::bootstrap` extracts a build-time-generated, generic
snapshot of `marketplace.json` + `plugins/**` (a `build.rs`-emitted table
of `include_bytes!` calls, keyed by relative path — no per-file, per-plugin
code) and resolves a requested plugin name against it via the Core
primitive, narrowing `MaterializedPackage`'s root to the resolved
subdirectory the same way Git's `subdirectory` already does
(`MaterializedPackage::retarget`).

`bootstrap::DEFAULT_PLUGIN_IDS` is a separate, small list of plugin
*names* — product policy over what installs on a fresh `UZE_HOME`,
independent of what the marketplace happens to offer.

Bootstrap and update are now distinct operations. `ensure_default_plugins`
— run before every CLI dispatch — only installs a default plugin that is
completely **absent**; it never touches an already-installed plugin's
content, no matter how it compares to the embedded snapshot. A plugin's
`update_available` (a directory-tree comparison against a fresh scratch
extraction, then discarded) is surfaced as a read-only fact on
`PluginSummary` for an explicit `update_plugin` to act on later — never
applied automatically.

## Consequences

Adding a second official plugin means adding files under `plugins/` and one
entry in `marketplace.json` — no Rust code changes anywhere, proven by a
test resolving two marketplace fixtures with the same primitive and no
special-casing. Store, Engine, Router and every `IntegrationPort` remain
provably marketplace-neutral (a scan test forbids the marketplace type
names outside `uze-core::acquisition` and `uze-application::bootstrap`).
Observational commands (`doctor`, `list`, `inspect`, `status`, `context
inspect`) no longer mutate installed plugin content as a side effect —
verified by a snapshot test comparing `packages.json`/`attachments.json`
before and after a repeat `ensure_default_plugins` call. A default plugin
that would introduce a new executable capability is not installed silently
even non-interactively; it reports `TRUST_REQUIRED` like any other package,
proven with an MCP-bearing fixture materialized as an `Embedded` source.
Existing installations need no migration: `PackageSource::Embedded`'s
persisted shape (`{ id: String }`) is unchanged.

What this does not do: no remote registry, no marketplace search, no TUI
marketplace surface, no plugin version resolver, no Package→Plugin
rename/refactor, no sparse Git checkout implementation (the manifest
contract is shaped to allow one later without touching Store/Engine/
Integration, but none is built).

Source change: openspec/changes/ (none filed — see conversation record;
this ADR is the durable trace for a change made ad hoc).
