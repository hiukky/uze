# OpenCode Integration

OpenCode does not consume any external plugin envelope — there is no
`.opencode-plugin/plugin.json` equivalent UZE reads. It decomposes every
package into individual Agent Skill / MCP capability attachments, delivered
through two native OpenCode surfaces: the shared `~/.agents/skills`
discovery directory and the global `mcp` object in `opencode.json`.

## Support

| Capability | Status | Delivery | Evidence |
|---|---|---|---|
| Plugin (package-level) | Unsupported (by design) | — no native envelope exists to consume | CODE_FACT |
| Skills | Supported | Native — managed symlink in `~/.agents/skills` | EMPIRICAL (OpenCode 1.18.18, 2026-08-20, real behavioral proof-token run — see ADR-006) |
| MCP | Supported | Adapted — direct write to `opencode.json`'s `mcp.<name>` | TESTED (config-level); no behavioral/CLI-discovery probe recorded for OpenCode in any ADR |
| Instructions/Context | Native (outside this crate) | Reads `AGENTS.md` directly, no bridge needed | DOCUMENTED (ADR-014) |
| Agents | Not implemented | `CapabilityKind::Agent` is import-only, routed to no integration | CODE_FACT |
| Hooks | Not implemented | `CapabilityKind::Hook` is import-only, routed to no integration | CODE_FACT |
| Commands | Not implemented | Research-only, not modeled as a capability at all | DOCUMENTED (`docs/capabilities/commands.md`) |
| Runtime projection | None | `runtime_contribution`/`supports_runtime_integration` never overridden — inherits passthrough default | CODE_FACT |

## Delivery

```
Store plugin
   │
   ├── Skill → managed symlink in ~/.agents/skills/<name>   (route: Native, once `uze setup` ran)
   │           └── pre-setup fallback: FilesystemProjection into the caller's cwd (route: Adaptable)
   │
   └── MCP   → direct write into opencode.json's `mcp.<name>` object (route: Adaptable)
   ↓
capability receipts (no package-level receipt — there is no native envelope)
```

No marketplace/catalogue step exists for OpenCode: `package_exposure_plan`
is never overridden, so `IntegrationPort`'s default (`None`) always applies
and every resource routes through per-capability `exposure_plan` instead.

## Native package

None. This is a deliberate architectural fact, not a gap: OpenCode has no
documented external plugin manifest UZE could preserve-and-consume the way
Claude's `.claude-plugin/plugin.json` or Codex's `.codex-plugin/plugin.json`
are consumed. Package coverage
(`provided_resource_identities`) is not applicable here.

## Fallbacks

- **Skill**, pre-`uze setup`: `ExposureMechanism::FilesystemProjection` —
  session/workspace-scoped, not persistent, same conformance-probe category
  as the other harnesses' fallbacks (ADR-005).
- **MCP**, pre-`uze setup`: no fallback at all — `Unsupported` (matches
  every other harness's MCP behavior; ADR-007's stated gap).

## Runtime

None. `runtime_contribution` and `supports_runtime_integration` are never
overridden on `OpenCodeIntegration`, so both fall through to the
`IntegrationPort` trait defaults (pure passthrough, `false`). OpenCode gets
the shim/dispatch/bypass machinery Claude uses for free but does nothing
with it today (ADR-014 explicitly anticipates this).

## Lifecycle

| Receipt | Inspect | Detach | Drift-safe |
|---|---|---|---|
| `VendorConfigEntry` (MCP only — Skills use the shared `SymlinkReference` path via `ManagedUserScopeReference::attach()`/standard detach) | Reads `opencode.json`, checks `mcp.<name>` against the receipt's recorded command/args/transport/cwd/env/enabled | Re-inspects immediately before mutating (ADR-009); removes only the matched key, preserves every other `mcp` entry and top-level config key | Yes — `mcp_inspection_tolerates_unrelated_fields_and_detaches_only_owned_entry` asserts a `foreign` entry and an `unrelated` top-level key both survive detach |

OpenCode is the only integration that writes its vendor config file
**directly** (`attach_mcp_config`/`detach_receipt` parse-and-rewrite JSON)
rather than shelling out to an official CLI verb the way Claude
(`claude mcp add/remove`) and Codex (`codex mcp add`, `codex plugin ...`)
do. `attach_mcp_config` refuses to overwrite a same-named entry it doesn't
already own (`Some(_) => Err(...)` when the existing value differs from
what UZE would write), which is OpenCode's own collision safety net for
this direct-write approach — confirmed by ADR-008's stated consequence
("OpenCode configuration conflicts fail rather than overwrite unrelated
user entries").

**ADR-009 compliance note:** ADR-009 states mutation should prefer a
harness's own CLI/API, with direct file editing "permitted only inside its
integration when no sufficient structured API exists." No ADR in this
repository states outright that OpenCode lacks a `mcp add`-equivalent CLI
verb — the direct-write choice is DOCUMENTED as deliberate and
collision-safe (ADR-008), but the specific justification "no CLI exists"
is not itself sourced anywhere. Treat this as an unverified assumption
carried by the implementation, not a disproven one.

## Runtime binary aliasing (Provisioning)

OpenCode's v2 installer produces a binary named `opencode2`; UZE's
canonical invocation is `opencode` with no suffix. `provision()`:

1. `resolve_opencode_binary()` — tries `opencode --version` first, falls
   back to `opencode2 --version`.
2. Runs the official install/upgrade script (or `<binary> upgrade` if
   already present) through the injected `ProcessRunner`.
3. `ensure_opencode_alias()` — if `opencode` still doesn't resolve, creates
   a symlink from either `opencode2`'s own directory or `~/.local/bin`
   pointing `opencode → opencode2`. Idempotent and non-destructive: a
   correct symlink is left alone, a stale one is repaired, a real
   (non-symlink) file at the alias path is never touched.
4. Only reports `Verified` once `opencode --version` itself succeeds — not
   merely `opencode2`.

No other integration in this crate has an equivalent binary-name-migration
workaround.

## Limitations

- No package-level delivery exists or is planned; every capability is
  decomposed individually.
- MCP attachment writes the vendor config file directly instead of through
  an OpenCode CLI verb — collision-safe by construction, but the "no CLI
  exists" premise behind that choice is undocumented (see Lifecycle above).
- The binary-alias mechanism (`ensure_opencode_alias`) is only exercised in
  tests when `opencode`/`opencode2` genuinely exists on the test machine's
  real `PATH` — see Evidence.
- Skill naming policy (bare-name-first, like Claude) is deliberately
  claimed for Codex too, since the two share the same
  `~/.agents/skills` discovery root — see
  `shared_agent_skill_root`/`exposure_name_candidates`'s doc comments. A
  bug in that shared-claim logic would manifest as a naming collision
  across two integrations at once, not just here.

## Evidence

- Tests: `cargo test -p uze-integrations --lib opencode` — 5 passed, 0
  failed (`opencode::lifecycle_tests` ×2, `opencode::mcp::mcp_tests` ×1,
  `opencode::provision::provision_tests` ×2).
- Skills transparent attachment: EMPIRICAL, OpenCode 1.18.18, real
  behavioral proof-token run via `opencode/deepseek-v4-flash-free`,
  2026-08-20 (ADR-006). No later re-validation recorded against a `v2`
  (`opencode2`-named) install — the alias mechanism this integration's
  `provision.rs` exists for postdates that evidence.
- MCP: no CONFIGURATION/DISCOVERY/BEHAVIORAL conformance run is recorded
  for OpenCode anywhere in this repository's ADRs (unlike Claude/Codex,
  which both have dated real-harness runs in ADR-007). MCP support here is
  TESTED at the unit level only.
- No `opencode`/`opencode2` binary is installed in the environment this
  audit ran in — no live re-validation was possible this pass.

## Next

1. Record a real OpenCode MCP conformance run (configuration/discovery
   tier at minimum), matching what ADR-007 already did for Claude/Codex —
   currently the only harness with zero dated MCP evidence.
2. Confirm or refute whether OpenCode exposes any CLI verb for MCP
   registration; if one exists, ADR-009 would call for using it instead of
   direct JSON writing.
3. Re-verify Skills against an actual `opencode2`-named v2 install — the
   existing EMPIRICAL evidence predates the binary-alias code this
   integration now depends on.
4. `provision_dispatches_install_or_update_consistently_with_detected_state`
   silently skips on a machine with neither `opencode` nor `opencode2` on
   `PATH` (true for CI) — it has never actually asserted `Verified` in that
   environment; a fully mocked alias-resolution seam would close this gap.
