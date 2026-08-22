# Codex Integration

Peer integration for OpenAI Codex CLI. Transparent attachment: Agent Skills
via a persistent user-scope symlink (`~/.agents/skills/<name>`), MCP via
`codex mcp add` (global only — no `--scope` flag exists), and a native
plugin path via a UZE-generated local marketplace catalogue
(`~/.agents/plugins/marketplace.json` in the Store, referenced by
`codex plugin marketplace add`).

## Support

| Surface | Status | Delivery | Evidence |
|---|---|---|---|
| Native Package | Supported, exact coverage | `.codex-plugin/plugin.json` → generated catalogue → `codex plugin add` | CODE_FACT + TESTED (11 tests) + EMPIRICAL (config/install only) |
| Skills | Supported | Persistent symlink, `~/.agents/skills/<name>` | EMPIRICAL (behavioral, ADR-006) |
| MCP | Supported (config), unproven behaviorally | `codex mcp add` → `~/.codex/config.toml` | EMPIRICAL (configuration), UNKNOWN (discovery), gap (behavioral) |
| Context (AGENTS.md) | Native, out of this crate's scope | Codex reads `AGENTS.md` directly | DOCUMENTED |
| Agents | Not implemented (also a real Codex vendor gap) | — | DOCUMENTED |
| Hooks | Not implemented (research-only project-wide) | — | DOCUMENTED |
| Commands | Not implemented | — | DOCUMENTED |
| Runtime Integration | None | Passthrough (trait default) | CODE_FACT |

## Delivery

```
Store package (.codex-plugin/plugin.json present)
        │
        ▼
store/.agents/plugins/marketplace.json   (derived catalogue, rebuildable)
        │
        ▼
codex plugin marketplace add <root>   (once, if not already registered)
        │
        ▼
codex plugin add <name>@uze-local
        │
        ▼
Codex's own plugin cache
        │
        ▼
one IntegrationOwned receipt (kind: "marketplace-plugin")
        │
        ▼
provided = discovered ∩ declared (skills dir, mcpServers file) — see Native package
```

Without a `.codex-plugin/plugin.json`, Skills and MCP decompose individually
through the same symlink/`codex mcp add` mechanisms described above.

## Native package

`package_exposure_plan` checks whether `.codex-plugin/plugin.json` exists,
then computes a real intersection via `codex_exact_coverage`
(`codex/plugin.rs`): the manifest's `skills` field names one directory whose
entire subtree is covered (component-wise `Path::starts_with`, not a string
prefix — `skills-extra` is never mistaken for inside `skills`), and its
`mcpServers` field names one external file (typically `.mcp.json`, distinct
from the portable root-level `mcp.json`) holding the standard
`{"mcpServers": {...}}` shape — a server is covered iff its name appears
there. Either field can independently be absent, empty, malformed, escape the
package root (`..`, an absolute path), or point at a file that can't be
read/parsed — each case degrades to "no coverage for that field" rather than
erroring; the package still installs natively, just with a smaller (possibly
empty) `provided_resource_identities`. Undeclared resources fall through to
individual attachment (Skill symlink / `codex mcp add`), never silently
dropped. `attach_package` then ensures the derived catalogue is registered as
a Codex marketplace and runs `codex plugin add <id>@uze-local`, idempotent
via a pre-check against `codex plugin list --json`.

## Fallbacks

- **Skills**, pre-setup: `ExposureMechanism::FilesystemProjection` into the
  caller's own workspace (`.agents/skills/<name>`), cleaned up per-session.
- **MCP**: no pre-setup fallback exists (`Unsupported` until `uze setup`
  completes) — this is a documented, accepted gap (ADR-007), not specific to
  this integration.

## Runtime

None. `runtime_contribution`/`supports_runtime_integration` are not
overridden — Codex gets the shim/dispatch machinery for free (ADR-014) but
does nothing with it, and nothing here anticipates that it should.

## Lifecycle

| Artifact | Receipt kind | Inspect | Detach |
|---|---|---|---|
| Skill symlink | `SymlinkReference` (standard) | Standard | Standard |
| MCP entry | `VendorConfigEntry` | `codex mcp get --json`; absence = exit 1 + stable stderr string, any other non-zero stays `Blocked` | `codex mcp remove` |
| Native plugin | `IntegrationOwned{kind:"marketplace-plugin"}` | `codex plugin marketplace list --json` + `codex plugin list --json`, checked before every destructive call (ADR-009) | `codex plugin remove` |

MCP inspection is fully structured (`--json`), unlike Claude's raw-file read
— a genuine advantage of Codex's CLI surface.

## Limitations

- **`.codex-plugin/plugin.json`'s `skills`/`mcpServers` fields are singular
  strings, not arrays** — one shared skills directory, one shared MCP
  manifest file — so partial declaration is coarser-grained than Claude's
  per-entry arrays: Codex can't declare "these two of three skills" without
  moving the undeclared one physically outside the shared directory. Within
  that shape, coverage is exact (`codex_exact_coverage`, 11 tests).
- `codex plugin marketplace add`/`codex plugin add` failures surface as a
  single `ExposureUnavailable` string; no structured retry/diagnosis.
- No override of `exposure_name_candidates` (uses the fully-qualified-only
  trait default), unlike Claude/OpenCode which both try the bare name first
  for Skills. Not confirmed whether this is deliberate.

## Evidence

- Tests: 13 (`codex::mcp::mcp_tests`: 1, `codex::plugin::plugin_tests`: 1,
  `codex::plugin::codex_native_coverage_tests`: 11 — full declaration,
  subset, Store-extra-skill, Store-extra-MCP, manifest-references-missing
  file, malformed MCP file, unexpected field shape, `..` escape, absolute
  path, empty declaration, partial-coverage-plus-fallback coexistence), all
  passing. `tests/plugin_first_vertical_slice.rs` re-confirmed green under
  the new intersection logic.
- Real harness version last empirically validated (per ADRs): Codex CLI
  0.148.0. A `codex` 0.148.0 binary is present in this session's environment
  but was not exercised live for this fix: `codex_exact_coverage` is a pure
  function already validated against the exact real fixture manifest shape
  (`e2e/fixtures/plugin-first-conformance/.codex-plugin/plugin.json`), so a
  live `codex plugin add` run would add side-effect risk without adding
  coverage-computation evidence.
- Sources: ADR-005, ADR-006, ADR-007, ADR-008, ADR-009, ADR-013,
  `docs/capabilities/agents.md`.

## Next

1. Close Codex MCP behavioral verification (blocked today by
   `codex exec`'s approval gate in non-interactive mode — ADR-007).
2. Confirm or drop the fully-qualified-only `exposure_name_candidates`
   choice deliberately, with a doc comment either way.
