# Qualify Store plugins by marketplace

Status: Accepted

## Context

The Store previously keyed an installed plugin only by the `name` from its
`plugin.json` and stored it at `~/.uze/store/packages/<plugin>`. Marketplace
resolution happened before ingestion, then its provenance was recorded
separately. Consequently, installing `git@one` and `git@two` attempted to
claim the same Store key and directory. The second install failed even though
the two marketplace entries are distinct plugins.

Claude Code treats the marketplace as part of installed-plugin identity:
its installed registry uses `plugin@marketplace` and its cache is partitioned
by marketplace, plugin, and version. UZE needs the equivalent invariant in
its canonical Store, not merely in a harness projection.

This supersedes the statement in ADR-015 that the Store remains unaware of
marketplace identity. That separation prevents a correct installed identity.

## Decision

The Store owns **plugins**, not packages, under this layout:

```text
~/.uze/store/plugins/<marketplace>/<plugin>
```

`PackageId` remains the Rust type during the staged naming refactor, but its
canonical value is `plugin@marketplace`. It is the key for Store registration,
Engine resource origin, receipts, inspection, update, and removal. The
unqualified plugin name remains the external `plugin.json` name and is used
only where a vendor manifest needs a slug; generated native projections use a
safe deterministic `marketplace--plugin` name to avoid collisions inside a
single UZE-owned vendor marketplace.

Marketplace installation and project-lock installation must pass their
resolved marketplace into Store ingestion. Direct source installation is
assigned the explicit `local` namespace. A plain plugin lookup is allowed
only when it resolves to exactly one installed identity; otherwise it fails
and requires `plugin@marketplace`.

This is a clean pre-1.0 break. UZE provides no migration, compatibility
reader, or fallback for `store/packages` or its old registry keys.

## Consequences

`git@one` and `git@two` can coexist, be attached independently, and be
removed or updated without sharing filesystem state or receipts. Store paths
now reflect the product vocabulary users see: plugins.

Every source that reaches the Store needs a marketplace namespace. A
marketplace name is now security-relevant path input and must be validated at
the same construction boundary as plugin names. Existing pre-change local
Store state is deliberately unsupported.

## Implementation Plan

- Update `crates/uze-core/src/home.rs` and `store.rs` to expose `plugins_dir`,
  construct qualified identities, and write the nested path.
- Pass marketplace identity through `crates/uze-application` marketplace and
  project lifecycle flows; keep direct source installs in `local`.
- Update generated Claude and Codex catalogues/manifests to use namespaced
  native plugin names and paths beneath `plugins/`.
- Update assertions that observe Store paths or installed IDs.
- Do not add a legacy migration or retain `packages_dir`/`package_dir` aliases.

## Verification

- [x] The Store writes `store/plugins/<marketplace>/<plugin>`.
- [x] The same plugin name from two marketplaces creates two registrations
  and two independent directories.
- [x] An invalid marketplace name fails before plugin bytes are written.
- [x] An ambiguous unqualified lookup fails and a qualified removal affects
  only the requested marketplace plugin.
- [ ] The full Rust test suite and formatting gate pass.

Source change: marketplace-qualified-store-identity
