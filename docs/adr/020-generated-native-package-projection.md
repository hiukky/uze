# Generated Native Package Projection

Status: Accepted

Status note: [ADR-021](021-extend-generated-native-projection-to-codex-and-gemini.md)
extends this decision's Codex/Gemini scope limitation (see this ADR's
"Scope of this decision" paragraph below) — both now also have Generated
Native Package/Extension, completing the hierarchy this ADR introduced.

Refines: [ADR-013 (Native Projection Principle)](013-adopt-native-projection-principle.md) §2.
Note on numbering: authored in a worktree isolated from a concurrent session
that independently claimed `019` for an unrelated CLI-grammar change; `020`
avoids that collision on reconciliation.

## Context

ADR-013 established `Native Package > Native Capability > Safe Adaptation >
Unsupported` and defined Native Package as "the package ships the harness's
own envelope." In practice this made an author-supplied vendor manifest
(`.claude-plugin/plugin.json`, `.codex-plugin/plugin.json`,
`gemini-extension.json`) the *only* route to package-level native delivery.
A canonical UZE package with no such manifest — including the common case of
one Skill and nothing else — was unconditionally decomposed into
capability-level shims (a symlinked skills-dir entry, an MCP config entry),
even when a harness's own native package format could trivially represent
exactly that content.

This directly contradicts the v0 product thesis: *a plugin author should not
need to know every harness format; UZE owns porting and delivery.* An author
who ships one vendor-neutral `plugin.json` plus `skills/<name>/SKILL.md`
should not need to also learn and author `.claude-plugin/plugin.json` just to
get Claude's native plugin experience (native discovery/identity, one
package-level receipt, package-level lifecycle, `claude plugin list/details`
truthfully showing the plugin) rather than a same-effect-different-appearance
capability shim.

An audit (this milestone's own audit phase, not repeated here) established
that `PackageExposurePlan`, `ManagedArtifact::IntegrationOwned`, and
`AttachmentReceipt` are already general enough to represent this without any
Core or Engine change: the "envelope present → package-level" gate was
self-imposed inside each `IntegrationPort::package_exposure_plan`
implementation, never enforced by Core. `ManagedArtifact::IntegrationOwned`'s
free-form `detail` map was already sufficient to record provenance
(`origin: "generated"`) without a new Rust type.

## Decision

Refine the hierarchy:

```
Explicit Native Package  >  Generated Native Package  >  Native Capability  >  Safe Adaptation  >  Unsupported
```

**Explicit Native Package**: unchanged from ADR-013 — a vendor envelope
supplied by the package author. Always wins; never overwritten, never
normalized, never displaced by generation. Presence of the envelope file
alone decides this branch — not its validity. A malformed but present
explicit envelope still takes the explicit route (matching the existing
`claude_exact_coverage` malformed-manifest precedent: empty coverage, not a
crash, and never a silent fallthrough to generation).

**Generated Native Package**: a vendor envelope an Integration
deterministically synthesizes itself from canonical UZE capabilities, when
the source package has no explicit envelope of its own. Both explicit and
generated are Native delivery; generated is never canonical and is always a
Derived Artifact (ADR-013 §4): non-authoritative, rebuildable from the Store
alone, inspectable, safely removable, never a second source of truth.

**Eligibility is capability-based, not resource-count-based.** A package
qualifies for generation the moment it has at least one capability UZE can
safely, structurally represent for that harness — one Skill alone, or one
MCP server alone, already qualifies. There is no minimum bundle size and no
opt-in marker required in the canonical `plugin.json`. Native wins because
the harness supports a native package representation for that content, not
because bundling has crossed some threshold — introducing either a
resource-count floor or an author opt-in flag would be artificial semantics
with no basis in what the harness can actually consume, and would push a
responsibility back onto the author that UZE exists to absorb.

Safe synthesis remains structural, not semantic, per ADR-013's existing
discipline: a generated envelope declares only what the Integration can
losslessly project — a package's whole conventional `skills/` directory, an
`mcp.json`'s `mcpServers` object verbatim, and name/version/description
already present in the package's own canonical manifest. Nothing is
translated, reinterpreted, or invented. `provided_resource_identities`
remains an exact `discovered ∩ (safely representable)` intersection —
partial coverage still applies unchanged: a capability UZE cannot safely
represent (an unsupported `CapabilityKind`, or a resource physically outside
the conventional location a generated manifest declares) is never silently
claimed as covered, and continues through the normal per-resource fallback.

**Generated artifacts are UZE-owned, never inside the Store.** They live
under `$UZE_HOME/state/attachments/<integration>/generated/<package-id>/` —
the same convention already used for per-Skill shims — reusing that
directory's established Derived Artifact discipline rather than introducing
a new top-level `$UZE_HOME` subtree. A generated envelope references the
Store's own resource bytes by symlink (skills) or verbatim inline copy from
a Store-read value (MCP server definitions) — never a byte copy of files
that could drift from the Store.

**A generated envelope is published through a second, dedicated
marketplace**, distinct from the one an explicit envelope publishes through
(for Claude: `uze-local-generated` alongside the existing `uze-local`). This
keeps the two lifecycles structurally separate — a generated envelope can
never be mistaken for, or silently override, an author-provided one — and
lets each be inspected, rebuilt, or torn down independently.

## Consequences

**Easier:** a package author writes one canonical `plugin.json` plus
`skills/`/`mcp.json` and gets Claude's native plugin experience —
`claude plugin list/details` shows it, package-level receipt/lifecycle,
no duplicate standalone Skill/MCP delivery for what the generated package
already covers — without ever authoring `.claude-plugin/plugin.json`. This
is now a real, empirically proven behavior change: a package that previously
decomposed (any Skill/MCP-only package with no vendor envelope) now
generates instead. Existing tests that encoded "no envelope → always
decompose" as an architectural assumption needed updating to encode the new
one; tests whose actual subject was capability-level naming/collision
resolution moved to an integration with no package-level native concept at
all (OpenCode) so they keep testing what they were built to test without
incidentally also asserting something about package-level generation.

**Harder:** two marketplaces now exist per harness with package-level
native delivery (explicit + generated), which is a real, visible
`claude plugin marketplace list` fact an operator or `uze doctor` output
needs to explain honestly, not hide. `uze inspect` must distinguish
Explicit/Generated/Capability/Adapted in its evidence text — it does so via
`ManagedArtifact::IntegrationOwned`'s existing `kind`/`detail.origin`
fields, no new central type.

**Unchanged:** Store canonical/source-of-truth semantics; Engine vendor
neutrality (no Claude-specific logic exists outside
`crates/uze-integrations/src/claude/`); `AttachmentReceipt`/ADR-009
reconciliation and safe-removal semantics (a generated package's receipt is
detached exactly like an explicit one — same `inspect_claude_plugin`/
`remove_claude_plugin` functions, marketplace-root-agnostic); no new Core
enum, no vendor-schema knowledge leaking into `uze-core`.

**Scope of this decision:** applies now to Claude only (the proven tracer
bullet). Codex and Gemini generated-native delivery are explicitly
NOT_IMPLEMENTED as of this ADR — each has its own manifest shape and its own
safely-synthesizable subset to prove empirically before implementing, per
ADR-013's own precedent of not extrapolating one harness's proof onto
another's unverified surface. OpenCode has no package-level native concept
at all and is not expected to grow a fake one merely for symmetry (spec
non-goal, unchanged).

## Implementation

- **Affected paths:** `crates/uze-integrations/src/claude/generate.rs`
  (new: `generatable`, `generated_exact_coverage`,
  `materialize_generated_package`, `write_generated_catalogue`,
  `generated_package_receipt`), `crates/uze-integrations/src/claude.rs`
  (`package_exposure_plan` falls back to generation when no explicit
  envelope is present; `attach_package`/`republish_packages`/
  `publication`/`inspect_receipt`/`detach_receipt` branch on
  explicit-vs-generated).
- **Pattern:** generation is read-only inside `package_exposure_plan` (it
  computes what *would* be covered, never materializes anything — a plan
  can be requested from `uze inspect`'s read-only path without side
  effects); materialization happens only inside `attach_package`, the one
  method the trait already reserves for package-level writes.
- **Avoid:** writing into the Store; a resource-count or opt-in-marker
  eligibility gate; semantic translation between vendor tool-permission/
  hook formats; a new Core type distinguishing explicit from generated
  (the existing `detail` map already suffices).

### Verification

- [x] A single-Skill, single-MCP, and combined Skill+MCP package with no
      explicit envelope each generate a native Claude package with exact
      coverage.
- [x] Multiple Skills in one conventional directory are all covered.
- [x] An explicit envelope, even a malformed one, always takes precedence
      over generation and is never displaced by it.
- [x] An unsupported-only capability (no Skill, no MCP) never produces a
      native package; a package mixing a safe Skill with an unsupported
      capability generates a package covering only the Skill, and the
      unsupported capability still routes through normal fallback.
- [x] Generation never writes into the Store package directory.
- [x] Generation is deterministic and idempotent across repeated rebuilds.
- [x] Detaching a generated package's receipt removes only its own derived
      directory, never Store bytes.
- [x] One cross-harness conformance test
      (`tests/north_star_flow_fixture.rs`) proves the vendor-neutral `flow`
      fixture (one `plugin.json`, one `skills/commit/SKILL.md`, no vendor
      manifests of any kind) reaches Claude as Generated Native Package
      while Codex/Gemini/OpenCode correctly fall back to Native Capability.

Source: this milestone's native-projection audit and tracer-bullet
implementation (see the session's final report for empirical Claude tracer
evidence, receipt/lifecycle behavior, and remaining gaps before v0).
