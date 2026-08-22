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
| Extension (package) | SUPPORTED, exact coverage | `gemini extensions link --consent` | SOURCE_CONFIRMED (0.56.0) + TESTED (8 tests) |
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
Store plugin (gemini-extension.json present)
   │
   ▼
gemini extensions link <path> --consent   (no copy, no catalogue)
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

Without a `gemini-extension.json`, Skills and MCP decompose individually:
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

- Extension receipt: `IntegrationOwned{kind:"linked-extension", selector: <name>, detail: {source_path, package_root}}`.
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
- No behavioral (real-model, real-CLI) proof exists for any Gemini surface —
  everything here is DOCUMENTED/SOURCE_CONFIRMED (the module doc comment's
  0.56.0 claim) or TESTED (unit tests against synthetic JSON), never
  EMPIRICAL in the sense Claude/Codex's Skills attachment is.
- `provision.rs` has zero unit tests (npm install/verify logic untested).

## Evidence

- Tests: 17/17 passing — `gemini::extension::extension_tests` (6 tests:
  drift, conflict, ambiguous-name, absent, disabled-still-owned,
  name-from-manifest), `gemini::extension::gemini_native_coverage_tests` (8
  tests: full declaration, subset, Store-extra-skill, Store-extra-MCP,
  missing `mcpServers` field, malformed manifest, unexpected field shape,
  partial-coverage-plus-fallback coexistence), and `gemini::mcp::mcp_tests`
  (3 tests: matched, command/args drift, unexpected state drift).
- Real harness version referenced: Gemini CLI **0.56.0** (module doc
  comment). A `gemini` 0.56.0 binary is present in this session's
  environment but was not exercised live for this fix: `gemini_exact_coverage`
  is a pure function already validated against the exact real fixture
  manifest shape
  (`e2e/fixtures/gemini-native-conformance/gemini-extension.json`), so a live
  `gemini extensions link` run would add side-effect risk without adding
  coverage-computation evidence.
- Source: `docs/adr/013-adopt-native-projection-principle.md` (native
  projection hierarchy, names Gemini's link-not-copy explicitly),
  `docs/adr/010-*.md` (npm/Homebrew install is DOCUMENTED, not verified
  here).

## Next

1. Add a behavioral (real Gemini CLI, isolated `$HOME`) conformance probe —
   Gemini is the only one of the four peer integrations with zero such
   record in the ADR history.
2. Add unit tests for `provision.rs` (currently untested).
3. Decide whether the missing pre-setup Skill fallback (Codex/OpenCode have
   `FilesystemProjection`, Gemini does not) is intentional or an oversight.
