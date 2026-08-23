# Extend Generated Native Projection to Codex and Gemini

Status: Accepted

Refines: [ADR-020 (Generated Native Package Projection)](020-generated-native-package-projection.md),
which itself refines [ADR-013 (Native Projection Principle)](013-adopt-native-projection-principle.md) §2.

## Context

ADR-020 proved Generated Native Package for Claude and explicitly scoped
Codex and Gemini out: "each has its own manifest shape and its own
safely-synthesizable subset to prove empirically before implementing." Until
this milestone, a canonical UZE package with no vendor envelope reached
Claude as a synthesized native plugin, but reached Codex and Gemini only
through capability-level decomposition — even though both harnesses already
had a proven EXPLICIT native envelope route (`.codex-plugin/plugin.json`,
`gemini-extension.json`), each with its own exact-coverage discipline
(`codex_exact_coverage`, `gemini_exact_coverage`) predating this milestone.

This left the v0 North Star only partially proven:
`CANONICAL PACKAGE PORTABILITY` was 4/4, but the harness-by-harness
"most-native-safe-representation" claim was Claude-only. An author shipping
one vendor-neutral `plugin.json` plus `skills/<name>/SKILL.md` got Claude's
native plugin experience for free, but had to either accept Codex/Gemini
capability-level delivery or author `.codex-plugin/plugin.json`/
`gemini-extension.json` by hand to get their native experiences too —
exactly the asymmetry the v0 thesis exists to remove.

Auditing each harness's already-implemented EXPLICIT format (this
milestone's own audit, not repeated here) confirmed the same conclusion
ADR-020 reached for Claude: no Core or Engine change is required. Both
`generatable`/`generated_exact_coverage`/`materialize_*`/receipt-shape
functions are a direct structural port of Claude's `claude/generate.rs`,
adapted only to each harness's own manifest shape — never to a shared
abstraction, per ADR-013's "no universal projection abstraction" discipline.

## Decision

Generated Native Package/Extension now applies to Codex and Gemini as well
as Claude, completing the hierarchy ADR-020 introduced:

```
Explicit Native Package/Extension  >  Generated Native Package/Extension  >  Native Capability  >  Safe Adaptation  >  Unsupported
```

**Package authors are not required to author vendor-specific envelopes for
supported harnesses when UZE can safely synthesize them.** This now holds
for Claude, Codex, and Gemini uniformly: a package with a conventional
`skills/` directory and/or a root `mcp.json`, and no vendor envelope of its
own, is delivered as a native plugin (Claude, Codex) or native extension
(Gemini) on every harness that supports one — not decomposed into
capability-level shims merely because the author didn't hand-author three
different vendor manifests.

**Vendor-specific envelopes remain an escape hatch for richer vendor
semantics.** Explicit still always wins, unconditionally, on every harness:
presence of the envelope file alone decides the branch, not its validity — a
malformed but present explicit envelope still takes the explicit route on
Codex and Gemini exactly as it already did on Claude, never silently
substituted with a generated one.

**Per-harness adaptation, not a shared abstraction.** Codex's generated
manifest (`.codex-plugin/plugin.json`) declares `skills` as a single
directory string and `mcpServers` as an external-file reference
(`./.mcp.json`, symlinked to the package's own `mcp.json` — never a byte
copy) — matching its EXPLICIT format's own shape, confirmed against the real
`e2e/fixtures/plugin-first-conformance/.codex-plugin/plugin.json` fixture.
Gemini's generated manifest (`gemini-extension.json`) declares `mcpServers`
inline, matching ITS explicit format's shape; Gemini's Skill coverage is
purely structural on both the explicit and generated routes (Gemini's
manifest schema has no `skills` field at all), a genuine asymmetry from
Claude's and Codex's manifest-declared-directory convention that was
confirmed, not assumed, against the pre-existing `gemini_exact_coverage`
implementation and its own test suite.

**Generated artifacts follow the same Derived Artifact discipline as
Claude's**, co-located under the same `$UZE_HOME/state/attachments/<integration>/generated/`
convention (`.../codex/generated/<package-id>/`,
`.../gemini/generated/<package-id>/`) — never inside the Store. Codex's
generated marketplace root is the generated directory itself (not the
Store), because Codex resolves a catalogue entry's `source.path` relative to
whatever root it was pointed at via `plugin marketplace add` and rejects
paths escaping that root (confirmed empirically against Codex 0.148.0) —
generated package directories therefore live directly under the generated
root, mirroring Claude's own `"./<id>"` catalogue convention exactly. Gemini
needs no marketplace/catalogue at all (unchanged from ADR-013's original
audit): `gemini extensions link` points directly at whichever directory
(Store-owned for explicit, UZE-generated for generated) carries the
manifest.

**A second, dedicated marketplace, mirroring Claude's.** Codex publishes
generated packages through `uze-local-generated`, distinct from the
existing explicit-only `uze-local` — same rationale as ADR-020: a generated
envelope must never be confused with, or silently override, an
author-provided one. Gemini needs no second marketplace (it has none to
begin with), only a second receipt `kind`
(`linked-extension-generated` vs `linked-extension`) for the same reason.

**A genuine implementation defect found and fixed by real-binary dogfood,
not by unit tests alone**: the first draft of Codex's generated receipt
recorded `package_root` as the canonical Store package path — correct for
the EXPLICIT route, where Codex installs directly from the Store, but wrong
for the GENERATED route, where Codex actually installs from the derived
directory. Real `codex plugin list --json` output (via an isolated-`HOME`
dogfood run against a real, locally installed Codex 0.148.0) reported the
derived directory as the installed plugin's `source.path`, which
`inspect_codex_plugin`'s existing source-comparison logic then correctly
flagged as Drifted against the wrong recorded path — a false-positive drift
that would have blocked every `uze remove`/reinstall cycle for a Codex
generated package. Claude's own `inspect_claude_plugin` never hit the
analogous bug because it deliberately never compares source path at all
(`let _ = package_root;`, a documented, load-bearing decision this milestone
did not change). Fixed by recording the actually-installed directory in the
receipt instead of the Store root — see `codex/generate.rs`'s
`generated_package_receipt`. This is exactly why spec §17 calls for real
binaries "where available": this class of drift-detection bug is invisible
to a fully mocked process boundary, since a mock only ever answers what the
test told it to.

## Consequences

**Easier:** an author's one canonical package now reaches Claude, Codex, and
Gemini as each harness's own native plugin/extension without ever authoring
`.codex-plugin/plugin.json` or `gemini-extension.json` by hand — the exact
same experience ADR-020 first proved for Claude alone. `V0 NATIVE-FIRST
NORTH STAR` moves from PARTIAL to PROVEN (see the final report).

**Harder:** Codex now has two marketplaces with package-level native
delivery (explicit `uze-local` + generated `uze-local-generated`), the same
`uze doctor`/`uze plugin marketplace list` honesty obligation ADR-020
already imposed for Claude. `uze inspect` distinguishes all four
explicit-vs-generated pairs (Claude, Codex ×2 kinds; Gemini ×2 kinds) via
the same `detail.origin`/`kind` convention — still no new Core type.

**Unchanged:** Store canonical/source-of-truth semantics; Engine vendor
neutrality; `AttachmentReceipt`/ADR-009 reconciliation and safe-removal
semantics (a generated Codex/Gemini receipt detaches through the same
`inspect_codex_plugin`/`remove_plugin` and `inspect_linked_extension`/
`run_gemini` functions the explicit route already used, marketplace/source
-root-agnostic); no new Core enum, no vendor-schema knowledge leaking into
`uze-core`; OpenCode still has no package-level native concept and is not
expected to grow a fake one (spec non-goal, unchanged from ADR-020).

## Implementation

- **Affected paths:** `crates/uze-integrations/src/codex/generate.rs` (new,
  mirrors `claude/generate.rs`), `crates/uze-integrations/src/codex.rs`
  (`package_exposure_plan`/`attach_package`/`republish_packages`/
  `publication`/`inspect_receipt`/`detach_receipt` branch explicit-vs
  -generated, identical shape to Claude's);
  `crates/uze-integrations/src/gemini/generate.rs` (new, no
  catalogue/marketplace concept, simpler than Codex's or Claude's),
  `crates/uze-integrations/src/gemini.rs` (same branching, `attach_package`
  splits into `attach_explicit_extension`/`attach_generated_extension`).
- **Pattern:** identical to ADR-020's — generation stays read-only inside
  `package_exposure_plan`; materialization happens only inside
  `attach_package`.
- **Avoid:** a shared cross-integration "generation" abstraction in Core or
  a common crate — each integration's `generate.rs` is a structural port,
  not a shared implementation, matching ADR-013's explicit rejection of a
  universal projection abstraction.

### Verification

- [x] Codex and Gemini each generate a native package/extension for
      single-Skill, single-MCP, and combined Skill+MCP packages with no
      explicit envelope, with exact coverage.
- [x] An explicit envelope, even malformed, always takes precedence over
      generation on both harnesses; generation is never attempted when an
      explicit envelope file is merely present.
- [x] Generation never writes into the Store package directory on either
      harness; both are deterministic and idempotent across rebuilds.
- [x] Detaching a generated receipt on either harness removes only its own
      derived directory, never Store bytes.
- [x] Real-binary dogfood (Codex 0.148.0, Gemini CLI 0.56.0, isolated
      `HOME`/`UZE_HOME`, no credentials) proves attach → inspect → Matched
      → detach → reinstall for both, and caught the `package_root` receipt
      defect described above before it shipped.
- [x] `tests/north_star_flow_fixture.rs` proves the vendor-neutral `flow`
      fixture reaches Claude, Codex, and Gemini as Generated Native
      Package/Extension, and OpenCode as Native Capability — the full v0
      North Star matrix, not Claude alone.

Source: this milestone's Codex/Gemini native-projection completion (see the
session's final report for the full per-harness verdict grid, test battery,
and dogfood transcript).
