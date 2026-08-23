# Gemini CLI Integration

**Status label: EXPERIMENTAL / CONFORMANCE.** The module's own doc comment
says this plainly: Gemini "exists to falsify or confirm the vendor-neutral
core, not to claim v0 support." Everything below was confirmed against a
**pinned Gemini CLI 0.56.0** in a conformance container. Treat this
integration as the least-proven of the four — it has real, working code and
real unit tests, but (unlike Claude and Codex) no recorded real-CLI
behavioral proof-token run anywhere in `docs/adr/*.md`.

## Support

| Surface | Status | Delivery | Evidence |
|---|---|---|---|
| Extension (explicit) | SUPPORTED, exact coverage | `gemini extensions link --consent` (linked from the Store) | SOURCE_CONFIRMED (0.56.0) + TESTED (8 tests) |
| Extension (generated) | SUPPORTED, exact coverage | `gemini extensions link --consent` (linked from a UZE-generated directory, ADR-021) | TESTED (15 tests) + EMPIRICAL — real-binary dogfood (Gemini CLI 0.56.0): attach → inspect (Matched) → detach (Missing) → reinstall (Matched) |
| Skills | SUPPORTED, native | `ManagedUserScopeReference` → shared `~/.agents/skills` | DOCUMENTED (Gemini's own discovery docs) |
| MCP | SUPPORTED, adapted | `gemini mcp add --scope user --transport stdio` | TESTED (logic only) |
| Context (AGENTS.md→GEMINI.md) | OUT OF SCOPE for this crate | — | — |
| Agents | NOT_IMPLEMENTED | `CapabilityKind::Agent` recognized only by `uze-core::importers`, never routed here | CODE_FACT |
| Hooks | NOT_IMPLEMENTED | Research-only project-wide (`docs/capabilities/hooks.md`) | CODE_FACT |
| Commands | NOT_IMPLEMENTED | Research-only project-wide (`docs/capabilities/commands.md`) | CODE_FACT |

No behavioral (real model call) proof-token run for Gemini is recorded in
any ADR — contrast Claude (`docs/adr/006-*.md`, real `claude -p` run, real
proof token) and Codex (same ADR, real `codex exec` run). Gemini's evidence
ceiling today is DOCUMENTED/SOURCE_CONFIRMED + TESTED, not EMPIRICAL.

## Delivery

```
Store plugin (gemini-extension.json present)                    [Explicit, ADR-013]
   │
   ▼
gemini extensions link <Store package path> --consent   (no copy, no catalogue)
   │
   ▼
Gemini's own extension registry (~/.gemini/extensions, Gemini-owned)
   │
   ▼
IntegrationOwned{kind:"linked-extension"} receipt
   │
   ▼
Skills/MCP inside the extension: provided = discovered ∩ declared
```

```
Store plugin (no explicit envelope, but skills/ dir and/or root mcp.json present)  [Generated, ADR-021]
   │
   ▼
$UZE_HOME/state/attachments/gemini/generated/<id>/gemini-extension.json
   (UZE-synthesized: name=package id, mcpServers copied inline from the
    package's own root mcp.json — Gemini's schema embeds servers directly,
    unlike Codex's external-file reference — skills/ symlinked from the Store)
   │
   ▼
gemini extensions link <GENERATED dir path> --consent   (still no catalogue —
   Gemini needs none for either route; only the linked directory differs)
   │
   ▼
IntegrationOwned{kind:"linked-extension-generated", detail.origin:"generated"} receipt
   (detail.source_path = the GENERATED dir, matching what
    `gemini extensions list` actually reports as installMetadata.source)
```

Without either kind of envelope, Skills and MCP decompose individually:
Skill → `ManagedUserScopeReference` into the **shared** `~/.agents/skills`
root (same directory Codex and OpenCode also write into); MCP → adapted
stdio entry in `~/.gemini/settings.json`.

## Native package

`gemini extensions link` was chosen deliberately over `install`: `install`
copies the package into `~/.gemini/extensions`, which would make a second
copy of bytes the Store already owns and break install-once. `link` keeps
the Store the single copy (code comment cites this as "confirmed
empirically against 0.56.0").

`gemini extensions list --output-format=json` is a real, source-confirmed
vendor quirk: it writes its JSON payload to **stderr**, not stdout, leaving
stdout empty. `extension::gemini_json` handles this by preferring stdout and
falling back to stderr.

**Generated Native Extension** (`gemini/generate.rs`, ADR-021): the
simplest of the three integrations' generated tiers — Gemini needs no
marketplace/catalogue for either route, so `attach_generated_extension`
just links straight at whichever directory (Store-owned for explicit,
UZE-generated for generated) carries the manifest.
`generatable()`/`generated_exact_coverage()` use the same capability-based
eligibility rule as Claude's/Codex's; Skill coverage stays purely
structural on the generated route too, for the same reason it already is
on the explicit route (Gemini's schema has no `skills` field at all — see
Limitations below). An explicit envelope, even malformed, always wins.

## Fallbacks

- Skill: none. If `uze setup` hasn't completed for Gemini, `skill_exposure_plan`
  returns `Unsupported` — there is no session-scoped adapted fallback the way
  Codex/OpenCode have (`FilesystemProjection`). This is a real asymmetry
  worth knowing, not necessarily a bug.
- MCP: `Unsupported` before setup, same as every other harness (no
  per-session conformance-probe fallback exists for MCP anywhere in UZE).

## Runtime

None. `GeminiIntegration` never overrides `runtime_contribution`/
`supports_runtime_integration` — it gets the default passthrough. Gemini
reads its own `GEMINI.md`, not `AGENTS.md`, natively; whatever bridge exists
for that gap lives in `uze-application`'s persistent-bridge mechanism,
outside this crate.

## Lifecycle

- Extension receipt: `IntegrationOwned{kind:"linked-extension", selector: <name>, detail: {source_path, package_root}}` (explicit) or
  `IntegrationOwned{kind:"linked-extension-generated", selector: <package id>, detail: {source_path, package_root, origin:"generated"}}` (generated) — both inspected through the same `inspect_linked_extension`, marketplace/source-root-agnostic.
- Ownership proof = identity (name) **and** source **and** install type
  (`"link"`) together — matching only the name is explicitly insufficient
  (`a_same_named_extension_from_another_source_is_drift`,
  `a_differently_installed_extension_is_a_conflict`).
- Two extensions answering to one name → `Conflict`, never guessed at
  (`two_extensions_answering_to_one_name_are_ambiguous`).
- Enablement (`isActive`) is **deliberately excluded** from the ownership
  proof — a disabled-but-UZE-linked extension stays `Matched`, not
  `Drifted`, because (a) it isn't reliably observable (`isActive` stays
  `true` after `gemini extensions disable` on 0.56.0; the real toggle lives
  in an unparsed `extension-enablement.json`) and (b) even if observable, a
  user disabling something UZE still owns is a preference, not drift —
  treating it as drift would block `uze remove` from ever detaching it.
  Tested (`a_disabled_extension_is_still_owned_by_uze`).
- MCP receipt/inspect/detach follows the same shape as Claude/Codex/OpenCode:
  read-only inspection against `~/.gemini/settings.json`, mutation only
  through `gemini mcp add/remove`.

## Limitations

- **Skill coverage is convention-based, not manifest-declared.**
  `gemini-extension.json` has no `skills` field at all (confirmed by
  `e2e/fixtures/gemini-native-conformance/gemini-extension.json`, which
  omits it even though its sibling `skills/uze-plugin-first/SKILL.md`
  exists) — `gemini_exact_coverage` (`gemini/extension.rs`) treats a skill as
  covered iff its directory lives directly under the extension root's fixed
  `skills/` subdirectory (component-wise, not a string prefix), never a
  declared path. `mcpServers` **is** declared, inline as a name-keyed object
  (unlike Codex's external-file reference) — a server is covered iff its
  name appears there. Either field degrades to "no coverage" rather than
  erroring on a missing/malformed manifest or an unexpected `mcpServers`
  shape; undeclared resources fall through to individual attachment, never
  silently dropped (8 tests, `gemini::extension::gemini_native_coverage_tests`).
- No behavioral (real-model) proof exists for any Gemini surface — real-CLI
  proof for package delivery now does (see Evidence below), but running a
  real model turn through a Gemini-attached MCP/Skill remains DOCUMENTED/
  SOURCE_CONFIRMED (the module doc comment's 0.56.0 claim) or TESTED (unit
  tests against synthetic JSON), never EMPIRICAL the way Claude's/Codex's
  Skills attachment is.
- `provision.rs` has zero unit tests (npm install/verify logic untested).

## Evidence

- Tests: 32/32 passing — `gemini::extension::extension_tests` (6 tests:
  drift, conflict, ambiguous-name, absent, disabled-still-owned,
  name-from-manifest), `gemini::extension::gemini_native_coverage_tests` (8
  tests: full declaration, subset, Store-extra-skill, Store-extra-MCP,
  missing `mcpServers` field, malformed manifest, unexpected field shape,
  partial-coverage-plus-fallback coexistence), `gemini::generate::generated_native_tests`
  (15 tests, mirroring Claude's/Codex's generated-native matrix), and
  `gemini::mcp::mcp_tests` (3 tests: matched, command/args drift,
  unexpected state drift).
- Real harness version empirically validated live this milestone: Gemini
  CLI **0.56.0**, isolated `HOME`/`UZE_HOME`, no credentials. A full
  generated-extension lifecycle (attach → inspect Matched → detach Missing
  → reinstall Matched) was run against the real binary — the first
  real-CLI proof recorded for this integration (see ADR-021); the explicit
  route's `extensions link`/`extensions list` was separately confirmed live
  via the same run and via `tests/shared_agent_skill_root_naming.rs`'s
  original (now-faked-for-CI) real-binary run.
- Source: `docs/adr/013-adopt-native-projection-principle.md` (native
  projection hierarchy, names Gemini's link-not-copy explicitly), ADR-021
  (generated extension, real-CLI proof), `docs/adr/010-*.md` (npm/Homebrew
  install is DOCUMENTED, not verified here).

## Next

1. Add a behavioral (real Gemini CLI, isolated `$HOME`) conformance probe
   for an actual **model** turn through an attached MCP/Skill — package
   delivery itself now has real-CLI proof (see Evidence), but no real model
   call has exercised it yet.
2. Add unit tests for `provision.rs` (currently untested). Caution: this
   milestone confirmed `uze setup`'s provisioning step calls the real
   `npm install -g @google/gemini-cli@latest` even when the harness is
   already present (`present → Update`, not a no-op) — any future
   dogfood/manual verification of `uze setup` itself (not just
   attach/inspect/detach) should isolate `$PATH`, not just `$HOME`, to
   avoid mutating the operator's real global npm state.
3. Decide whether the missing pre-setup Skill fallback (Codex/OpenCode have
   `FilesystemProjection`, Gemini does not) is intentional or an oversight.
