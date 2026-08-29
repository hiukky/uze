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

**Amendment.** The first cut of this decision also qualified every
*generated* native manifest's `name` field with the marketplace
(`marketplace--plugin`), reasoning it was needed to avoid a collision inside
UZE's own single generated vendor marketplace. Real-machine testing against
Claude Code (installing two same-named plugins from different marketplaces,
`--plugin-dir`-loading both into one session, then invoking the colliding
command) showed two things at once: first, that manifest `name` is exactly
what Claude turns into the invocation prefix, so unconditionally qualifying
it changed *every* plugin's slash command from `/git:commit` to
`/one--git:commit`, not just the rare colliding pair — a DX regression for
the overwhelmingly common case where no collision exists at all. Second,
that the collision this was guarding against is real: with two same-named
plugins simultaneously active, Claude resolves `/git:commit` to whichever
loaded first, silently, with no error and no sign the second one exists.
Store-level coexistence (`git@one` and `git@two` sharing no bytes or
receipts) was solving the wrong layer — the two plugins never needed to
share storage, but at most one of them may ever answer to a given
invocation name at a time. The Decision below folds in the fix: a second,
independent **active name** reservation on top of the unchanged Store
layout.

## Decision

The Store owns **plugins**, not packages, under this layout:

```text
~/.uze/store/plugins/<marketplace>/<plugin>
```

`PackageId` remains the Rust type during the staged naming refactor, but its
canonical value is `plugin@marketplace`. It is the key for Store registration,
Engine resource origin, receipts, inspection, update, and removal.

Marketplace installation and project-lock installation must pass their
resolved marketplace into Store ingestion. Direct source installation is
assigned the explicit `local` namespace. A plain plugin lookup is allowed
only when it resolves to exactly one installed identity; otherwise it fails
and requires `plugin@marketplace`.

This is a clean pre-1.0 break. UZE provides no migration, compatibility
reader, or fallback for `store/packages` or its old registry keys.

**Active name (amendment).** A package's local invocation name — what every
harness's generated manifest `name` field carries, and what
`qualified_capability_name`'s `<plugin>:<capability>` label (ADR-026) is
built from — is its own bare `plugin_name()` by default, tracked
independently of the Store identity as `Registration.active_name` (`None` =
default; `Some(alias)` only once an operator has explicitly chosen one).
Exactly one installed package may hold a given active name at a time,
checked at the one chokepoint every install passes through
(`UzeStore::ingest_with_active_name`): a second package requesting an
already-active name is refused with `PluginNameCollision`, never silently
shadowed. The Application layer (never the Store — a pure data layer with no
UX) offers a `NameCollisionAuthority` the same way `TrustAuthority` offers
a trust decision: `Abort` (the default — refuse, matching every existing
call site unchanged), `Alias(name)` (install under an explicit different
local name, so both plugins stay active side by side), or `Replace` (detach
and remove the existing active plugin first — only once that is proven
`Safe`, the exact rule `remove_plugin` already enforces — then let the new
install claim the freed name). An `update` re-resolves and reinstalls under
the same marketplace-qualified id but must restore whatever active name the
package already had; it is a version change, never a re-namespacing.

## Consequences

`git@one` and `git@two` can coexist as **bytes** — installed,
attached-or-not, removed or updated independently, sharing no filesystem
state or receipts — but only one of them is ever the live `/git:*` in any
harness at a time. Store paths reflect the product vocabulary users see:
plugins. Listing and removal must show both an active name and an origin
(the qualified id) rather than collapsing them, since the two can now
legitimately differ.

Every source that reaches the Store needs a marketplace namespace. A
marketplace name is now security-relevant path input and must be validated at
the same construction boundary as plugin names. Existing pre-change local
Store state is deliberately unsupported. A registration written before the
active-name field existed deserializes as `None` — implicitly active under
its own bare name, exactly what was already true of it — so no migration is
needed for that either.

## Implementation Plan

- Update `crates/uze-core/src/home.rs` and `store.rs` to expose `plugins_dir`,
  construct qualified identities, and write the nested path.
- Pass marketplace identity through `crates/uze-application` marketplace and
  project lifecycle flows; keep direct source installs in `local`.
- Update generated Claude and Codex catalogues/manifests to use the package's
  bare/active local name (not a marketplace-qualified one) and paths beneath
  `plugins/`.
- Update assertions that observe Store paths or installed IDs.
- Do not add a legacy migration or retain `packages_dir`/`package_dir` aliases.
- Add `UzeStore::ingest_with_active_name`/`active_name_for`/
  `find_by_active_name`/`set_active_name` and the `naming::NameCollisionAuthority`
  boundary; thread it through `add_plugin`/`install_from_marketplace`/
  `plugin_install`'s `_resolving` variants and the CLI's `--alias`/`--replace`
  flags and interactive prompt.

## Verification

- [x] The Store writes `store/plugins/<marketplace>/<plugin>`.
- [x] The same plugin name from two marketplaces creates two registrations
  and two independent directories.
- [x] An invalid marketplace name fails before plugin bytes are written.
- [x] An ambiguous unqualified lookup fails and a qualified removal affects
  only the requested marketplace plugin.
- [x] Installing a second same-named plugin under a different marketplace,
  with no resolution given, fails with `PluginNameCollision` rather than
  silently shadowing the active one.
- [x] `alias` lets both coexist active, addressable by their own local names;
  `replace` safely removes the existing one first and aborts untouched if
  that removal is not `Safe`.
- [x] `update` restores an aliased plugin's active name across its
  remove-then-reinstall cycle.
- [x] Generated Claude/Codex manifests carry the plain active name, verified
  against real Claude Code (`claude plugin install`, `--plugin-dir`) rather
  than assumed.
- [ ] The full Rust test suite and formatting gate pass.

Source change: marketplace-qualified-store-identity
