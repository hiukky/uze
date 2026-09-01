# Marketplaces: manifest, discovery registry, and the embedded official marketplace

Status: Accepted
Consolidates: ADR-012 (marketplace contract for the embedded default
plugin), ADR-015 (marketplace as discovery registry, plugin as installable
unit), ADR-023 (marketplace manifest is `agents.json`, since reverted) —
see the "Consolidated records" section of `README.md`.

## Context

UZE needed a way to distribute plugins that mirrors what users already know
from harness plugin marketplaces, without becoming a second Store or a
remote service. Three questions had to be settled together: what the
manifest file is called, how a user-added marketplace is registered and
resolved, and how UZE ships its own official plugin on a fresh machine.

The filename question was answered twice. A prior decision (ADR-023) renamed
the manifest to `agents.json` for family consistency with `AGENTS.md` and
`agents.lock`. That was wrong: it conflated distribution discovery with the
project agent-environment files, and contradicted the marketplace
specification's own terminology. It was reverted before any production
consumer existed.

## Decision

### 1. The manifest is exactly `marketplace.json`

Schema: `{name, plugins: [{name, source, description, keywords}]}` with
optional `owner`.

Every UZE-owned marketplace root uses it — the repository root, the embedded
official marketplace, local and Git acquisition inputs, workspace detection,
fixtures, and conformance seeds. **`agents.json` is not accepted as an
alias.** A root containing only that file is not a marketplace and receives
the normal missing-manifest failure naming `marketplace.json`. UZE does not
rename it, warn about it, or persist a compatibility setting. Accepting both
filenames was rejected: it makes discovery precedence, diagnostics,
fixtures, and documentation permanently less deterministic, for a transition
nobody needs.

This does not rename vendor-owned catalogue files
(`.claude-plugin/marketplace.json`, `.agents/plugins/marketplace.json`) or
UZE state (`~/.uze/state/marketplaces.json`), and does not touch
`AGENTS.md`, `agents.lock`, or package manifests.

### 2. Marketplace is discovery; plugin is the installable unit

User-added marketplaces are a registry in `~/.uze/state/marketplaces.json`
(`name → Git | Local`), validated against `marketplace.json` at
registration time. `plugin install <name>@<marketplace>` resolves the
registry entry's source plus the manifest's plugin `source` into a
`PackageSource` (Git with `subdirectory`, or Local) and then reuses the
existing acquisition → Store → native projection pipeline.

A marketplace copies no bytes until a plugin is installed, so it never
becomes a second source of truth. Acquisition stays generic (`Git`, not
`GitHub`). `uze market remove` is blocked while plugins from that
marketplace are installed. Registry drift is validated at install time.

`uze-core::acquisition::marketplace` is a pure, deterministic primitive —
`parse_manifest` + `resolve_plugin_source` — that answers "which plugins
exist, and where" for any local directory holding that shape, with no
opinion on how the directory got there and no knowledge of any specific
plugin name.

### 3. This repository is the official marketplace, embedded in the binary

The repo root is itself a marketplace: `marketplace.json` plus `plugins/**`.
`PackageSource::Embedded { id }` resolves against a build-time snapshot of
those files — a `build.rs`-emitted table of `include_bytes!` keyed by
relative path, with no per-file or per-plugin code — narrowing the
materialized package's root to the resolved subdirectory exactly as Git's
`subdirectory` already does. It is pre-registered as `uze-official`.

`bootstrap::DEFAULT_PLUGIN_IDS` is a separate, small list of plugin *names*:
product policy over what installs on a fresh `UZE_HOME`, independent of what
the marketplace happens to offer.

### 4. Bootstrap and update are distinct operations

`ensure_default_plugins`, run before every CLI dispatch, installs a default
plugin **only when it is completely absent**. It never touches an installed
plugin's content, however it compares to the embedded snapshot. A plugin's
`update_available` — a directory-tree comparison against a fresh scratch
extraction, then discarded — is surfaced as a read-only fact for an explicit
`update_plugin` to act on. Nothing is ever applied automatically.

## Consequences

Easier: adding a second official plugin means adding files under `plugins/`
and one entry in `marketplace.json` — no Rust changes anywhere, proven by a
test resolving two marketplace fixtures with the same primitive and no
special-casing. Store, Engine, Router, and every `IntegrationPort` stay
provably marketplace-neutral (a scan test forbids the marketplace type names
outside `uze-core::acquisition` and `uze-application::bootstrap`).
Observational commands (`doctor`, `list`, `inspect`, `status`,
`context inspect`) never mutate installed plugin content as a side effect.

Harder: there is one more state file to manage, and the first
`plugin install` from a Git marketplace pays a clone. A default plugin that
would introduce a new executable capability is not installed silently even
non-interactively — it reports `TRUST_REQUIRED` like any other package.

Deliberately out of scope: a remote registry, marketplace search, a plugin
version resolver, and sparse Git checkout (the manifest contract is shaped
to allow one later without touching Store, Engine, or integrations).
