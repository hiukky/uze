# Native Projection: delivery precedence per capability per harness

Status: Accepted
Consolidates: ADR-008 (Plugin First, Capability Aware delivery), ADR-020
(Generated Native Package Projection), ADR-021 (extend generated native
projection to Codex and Gemini) — see the "Consolidated records" section of
`README.md`.

## Context

ADR-006 and ADR-007 proved Agent Skill and MCP attachment separately, one
resource at a time. That resource-first model could not express the
difference between a package a harness installs as one native plugin and a
package whose portable components must be delivered individually — and it
copied only portable files, losing any vendor envelope the source provided.

The harnesses are genuinely asymmetric about this. Some accept a
source-provided vendor envelope through their own plugin mechanism; some
have no package-level concept at all and only discover capabilities. UZE
needed one precedence rule that produces the most native available delivery
on each harness without inventing a UZE plugin format, and without a package
author having to hand-author a different vendor manifest per harness.

## Decision

Adopt **Native Projection**, a delivery precedence evaluated per capability
per harness:

```
Explicit Native Package  >  Generated Native Package  >  Native Capability
                         >  Safe Adaptation  >  Unsupported
```

"Native" means the harness offers an officially supported mechanism that
preserves canonical semantics — not that the same physical primitive exists
on every harness.

### 1. The Store preserves the canonical source verbatim

The Store keeps the complete validated source tree. It never mutates on
behalf of a harness, never knows a harness name, and never writes a harness
artifact. It never creates a UZE plugin format.

### 2. Explicit Native Package

The package ships the harness's own envelope (`.claude-plugin/plugin.json`,
`.codex-plugin/plugin.json`, and the equivalent per vendor). It always wins,
unconditionally, on every harness. **Presence of the envelope file alone
decides this branch — not its validity**: a malformed but present explicit
envelope still takes the explicit route (empty coverage, not a crash, and
never a silent fallthrough to generation).

The owning integration decides package-level delivery via
`package_exposure_plan` and declares exactly which discovered resources the
envelope actually delivers
(`provided_resource_identities = discovered ∩ declared`). Undeclared
resources are not marked provided and continue through normal per-resource
fallback — no silent disappearance.

### 3. Generated Native Package

A vendor envelope the integration deterministically synthesizes from
canonical UZE capabilities, when the source package has no explicit envelope
of its own. **A package author is not required to author vendor-specific
envelopes for supported harnesses when UZE can safely synthesize them**: a
package with a conventional `skills/` directory and/or a root `mcp.json` is
delivered as a native package on every harness that has one, rather than
decomposed into capability-level shims merely because the author didn't
hand-author three vendor manifests. Explicit envelopes remain the escape
hatch for richer vendor semantics.

**Eligibility is capability-based, not resource-count-based.** One Skill
alone, or one MCP server alone, already qualifies. There is no minimum
bundle size and no author opt-in marker: native wins because the harness
supports a native representation for that content, not because bundling
crossed a threshold. A count floor or an opt-in flag would be artificial
semantics with no basis in what the harness can consume, and would push back
onto the author a responsibility UZE exists to absorb.

**Synthesis is structural, never semantic.** A generated envelope declares
only what the integration can losslessly project — a whole conventional
`skills/` directory, an `mcp.json`'s `mcpServers` object verbatim, and
name/version/description already present in the canonical manifest. Nothing
is translated, reinterpreted, or invented. `provided_resource_identities`
stays an exact `discovered ∩ (safely representable)` intersection: a
capability UZE cannot safely represent is never claimed as covered and
continues through per-resource fallback.

**Per-harness adaptation, not a shared abstraction.** Each generated
manifest matches that vendor's *own explicit format's* shape — a single
`skills` directory string here, an external `./.mcp.json` reference there,
inline `mcpServers` elsewhere, and no `skills` field at all on a harness
whose schema has none (Skill coverage is then purely structural). These
asymmetries are confirmed against each vendor's real behavior, not assumed.

**Generated envelopes are published separately from explicit ones.** Where
a harness uses a catalogue, generation gets its own dedicated marketplace
(e.g. `uze-local-generated` alongside `uze-local`); where it does not, it
gets its own receipt `kind`. Either way the two lifecycles stay structurally
separate: a generated envelope can never be mistaken for, or silently
override, an author-provided one, and each can be inspected, rebuilt, or
torn down independently.

### 4. Native Capability, then Safe Adaptation

When no package-level route exists, fall back to **Native Capability**
(`ManagedUserScopeReference` with `route: Native`, direct Standard
consumption — ADR-006, ADR-007), then **Safe Adaptation** (a shim, a
generated catalogue, a vendor CLI). `Unsupported` always carries a
rationale, never a silent translation.

### 5. Derived Artifact

An artifact created from the Store's source of truth for delivery to a
harness. Properties: **not authoritative, rebuildable from the Store alone,
inspectable via an official harness surface, safely removable** (ADR-009).
It may be a copy, a generated catalogue or manifest, a symlink, or a
harness-owned cache. `AttachmentReceipt::IntegrationOwned` (with its `kind`,
`selector`, and `detail` map) already represents the lifecycle — no new
ledger type and no new `PackageKind` enum in the Core.

Generated artifacts are UZE-owned and live under
`$UZE_HOME/state/attachments/<integration>/generated/<package-id>/` —
**never inside the Store**. A generated envelope references Store bytes by
symlink, or inlines a value read from the Store; it never byte-copies files
that could then drift.

*Amended 2026-09-02.* A symlink only delivers when the consumer
dereferences it. `codex plugin add` stages the envelope into
`~/.codex/plugins/cache/` without following symlinks (verified against
codex-cli 0.149.0 through 0.152.1: a symlinked skill directory and a
symlinked `.mcp.json` are simply absent from the cache, so the skill never
reached the model and the server never reached `codex mcp list`). The Codex
envelope therefore mirrors Store bytes as real files. What keeps it from
drifting is the property that always mattered — it is rebuilt wholesale
from the Store on every materialization — not the physical link.
Antigravity's installer dereferences (verified 1.1.19), so its envelope
keeps the symlink.

### 6. Native ≠ zero-copy

`Store → derived marketplace.json → <vendor> plugin install → vendor-owned
cache copy` does not violate install-once, because the cache is not a source
of truth: `rm cache && reinstall` rebuilds byte-identically from the Store.
This matches the official marketplaces' own cache behavior.

### 7. Physical location ≠ ownership

A derived catalogue may live physically under `store/` so a relative
`source: "./..."` stays valid, while being owned and rebuilt solely by the
integration that publishes it. Physical placement never confers Store
ownership.

Vendor-specific manifest parsing — `skills`, `mcpServers`, path
normalization, `..` rejection, duplicate deduplication, malformed handling —
belongs to the owning integration. Core and Engine stay vendor-neutral and
never generate harness artifacts.

## Consequences

Easier: a package author writes one canonical `plugin.json` plus
`skills/`/`mcp.json` and gets the native plugin experience on every harness
that has one — vendor `plugin list` shows it, package-level receipt and
lifecycle apply, and there is no duplicate standalone delivery for what the
package already covers. Adding a harness is an integration concern: it
declares which of the five levels it can honor.

Harder: two catalogues now exist per harness with package-level native
delivery (explicit + generated), which is a real, visible fact that
`uze doctor` and `uze inspect` must explain honestly rather than hide.
Inspection must distinguish Explicit / Generated / Capability / Adapted in
its evidence text — it does so through
`ManagedArtifact::IntegrationOwned`'s existing `kind`/`detail.origin`
fields, with no new central type. And a generated route's receipt must
record the *derived* directory as the installed source, not the Store path;
recording the Store path produces a false-positive drift that blocks every
remove/reinstall cycle — a defect real-binary dogfood caught and unit tests
alone did not.
