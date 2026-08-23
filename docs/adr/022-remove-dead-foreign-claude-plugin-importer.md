# Remove the Dead Foreign Claude Plugin Importer

Status: Accepted

Refines: the acquisition-import requirement in
`openspec/changes/validate-universal-agent-environment/specs/resource-import/spec.md`
("Keep foreign importers separate from harness integrations"). That
requirement is not reversed by this ADR — see Decision below — but the one
concrete importer it produced is removed as dead code.

## Context

`crates/uze-core/src/importers/claude_plugin.rs` defined
`ClaudePluginImporter`: a `ForeignImporter` recognizing a package that
ships only a foreign `.claude-plugin/plugin.json`, with no canonical
`plugin.json` of its own, and converting it into UZE's canonical
`ImportedBundle` shape. It existed by deliberate original design — the
`resource-import` spec this ADR refines required exactly this: "An
importer MAY know vendor-specific paths and manifests, but a harness
integration SHALL consume the effective environment rather than act as a
source or destination of a conversion" — so that `ClaudeIntegration`
(delivery-time) would never also be responsible for parsing a
foreign-authored bundle (acquisition-time).

The Integration Capability Contracts Audit and the Integration
Conformance Suite implementation that followed it (this repo's own
session history) established, by tracing actual call sites rather than
assuming from the spec text, that this importer was never wired into any
real flow:

- `Store::ingest` — the only acquisition path `uze add`/`uze plugin
  install` reaches — calls `AgentPluginImporter` exclusively
  (`crates/uze-core/src/store.rs`).
- `import_bundle()`, the function that tried `ClaudePluginImporter` before
  falling back to `AgentPluginImporter`, was called from nowhere except
  its own two unit tests in `importers.rs`.
- No CLI command, `uze-application` code path, or marketplace acquisition
  flow ever reached `ClaudePluginImporter`.

So the code was simultaneously: (a) genuinely dead — zero production
reachability, confirmed by a full reachability trace, not a grep alone —
and (b) a real violation of this crate's own stated invariant ("no
harness name appears in `uze-core`," `crates/uze-integrations/README.md`),
since `ClaudePluginImporter`, its file name, and the string literal
`"claude-plugin"` all named a specific vendor inside the crate that is
supposed to have none.

## Decision

Remove `ClaudePluginImporter`, `import_bundle()`, and the now-unreachable
`checked_root()` helper it alone called. `AgentPluginImporter` — the live,
canonical importer `Store::ingest` actually depends on — and the
`ForeignImporter` trait it implements are unchanged and unaudited beyond
confirming they remain the real, load-bearing path.

**The original architectural principle is preserved, not reversed**:
foreign-format import, if it returns, still belongs structurally separate
from harness delivery — an importer converting a vendor's own artifact
into canonical form is a different concern from an `IntegrationPort`
projecting canonical content out to a vendor, and conflating them was
never the fix under consideration here. What changes is narrower and more
specific:

> Foreign-format importing remains a separate concern from harness
> delivery, but no foreign importer is retained in production until a
> real acquisition/reverse-discovery flow requires it.

Prefer removing speculative/dead architecture over wiring dead code
merely to justify code that already exists. `ClaudePluginImporter` was
not deleted *because* it named Claude — a correctly-used foreign importer
naming a vendor would be legitimate under the principle above, exactly as
originally designed. It was deleted because it did nothing, and nothing
&mdash; not even a scaffold with vendor knowledge in it &mdash; is free to
keep once confirmed unreachable.

If foreign/reverse import returns, it should be designed against a real
acquisition requirement, with its own deliberate boundary decided at that
time (`importers/`, `translators/`, `foreign_formats/`, or an acquisition
adapter possibly living outside pure canonical Core are all options worth
weighing then, not now) — not resurrected merely because a historical
spec once anticipated it.

## Consequences

**Easier:** `uze-core` production logic is now genuinely, verifiably
vendor-neutral with zero exceptions — `tests/integration_conformance.rs::
core_never_names_a_vendor_harness` no longer needs to carve out an
exception for `importers/` (it previously did, narrowly, specifically for
this file); the crate's own "no harness name appears in `uze-core`" claim
is now literally true rather than aspirational.

**Unchanged:** the v0 canonical acquisition contract (`plugin.json` via
`AgentPluginImporter`), Generated Native Package/Extension delivery
(ADR-020/ADR-021, which solves canonical → vendor projection and was
never affected by this — foreign import is the reverse direction and a
wholly separate concern), and every other `uze-core` production path.

**Harder:** nothing identified — the removed code had no production
consumer to migrate.

## Implementation

- **Removed:** `crates/uze-core/src/importers/claude_plugin.rs` (whole
  file); `import_bundle()` and `checked_root()` in
  `crates/uze-core/src/importers.rs`; the `ClaudePluginImporter`
  re-export in `crates/uze-core/src/importer.rs` (the facade).
- **Rewritten, not removed:** `importers.rs`'s two unit tests
  (`imports_skills_without_changing_their_bytes`,
  `rejects_parent_directory_manifest_reference`) still express live,
  load-bearing invariants (byte-preserving import, unsafe-manifest-path
  rejection) — they now call `AgentPluginImporter` directly instead of
  the removed multi-importer dispatcher, losing no coverage.
- **Unchanged:** `AgentPluginImporter`, `ForeignImporter`,
  `import_from_manifest`, `bundle_item`, `validate_references`,
  `validate_reference` — all still live, all still exercised by the real
  `Store::ingest` path.

### Verification

- [x] `cargo test --workspace` green after removal — no caller broke.
- [x] `tests/integration_conformance.rs::core_never_names_a_vendor_harness`
      strengthened: the `importers/` scope-narrowing it previously needed
      is gone, and the test still passes with zero exceptions.
- [x] The two rewritten `importers.rs` tests still pass, now against the
      live `AgentPluginImporter` path directly.

Source: this milestone's Path Safety + Foreign Importer Cleanup audit
(see the session's final report for the full reachability trace and
before/after `uze-core` vendor-neutrality classification).
