# Make Antigravity CLI the Google-family v0 harness

## Why

Google transitioned its terminal agent from Gemini CLI to Antigravity CLI
(`agy`) in mid-2026 — a Go rewrite sharing the Antigravity 2.0 harness, with
an official migration path. UZE's fourth integration was Gemini CLI, chosen
to falsify the vendor-neutral core against a differently shaped native
delivery. The Google-family v0 target is now Antigravity, and the product
decision is that the Gemini CLI integration is **removed** from the
codebase (no legacy code path remains; history is preserved in the ADRs and
the migration audit record). The migration is an audit-driven port — not a
string rename.

## What Changes

- New `AntigravityIntegration` (`crates/uze-integrations/src/antigravity/`):
  independent `IntegrationPort` implementation — no inheritance, no wrapper.
- Native delivery: `agy plugin install` — the canonical `plugin.json` **is**
  a valid Antigravity plugin manifest, so a canonical package ships no
  Antigravity-specific file and takes the explicit route from the Store; a
  package with canonical `mcp.json` gets a generated envelope translating
  `mcp_config.json` (vendor's own `url`/`httpUrl` → `serverUrl` migration
  rule).
- The staged plugin tree at `~/.gemini/config/plugins/<name>/` is a Derived
  Artifact: rebuilt from the Store on attach, ownership proven by content
  fingerprint, removed via the official `agy plugin uninstall`; a foreign
  same-name import is refused (vendor install merges, never overwrite).
- Commands are **Adapted** (not Native): Antigravity's only command
  representation is commands→Skills conversion and Skills are
  model-discoverable with no explicit-only mechanism — the semantic loss is
  declared, never hidden.
- Context is **Native**: Antigravity reads `AGENTS.md` (and `GEMINI.md`), so
  no `@AGENTS.md` bridge is generated; the Claude bridge remains the only
  one.
- Provisioning: official installer (invoked exactly as documented; the
  docs' `--skip-aliases`/`--skip-path` flags are rejected by the current
  script — noted in ADR-027) + `agy update`; detection `agy --version`;
  post-install verification falls back to the documented
  `~/.local/bin/agy` destination.
- Composition: Antigravity is a first-class peer (Claude, Codex, OpenCode,
  Antigravity). **Gemini CLI integration removed**: module, tests, fixtures,
  e2e spec, composition and bridge entry deleted. Zero `uze-core` changes,
  zero `IntegrationPort` changes.
- Docs/ADR/OpenSpec updated; V0 matrix lists the four v0 harnesses.

**BREAKING**: none to remaining supported surfaces. `IntegrationPort`,
`ExposureMechanism`, receipts and CLI grammar are unchanged. Users of the
removed integration see it disappear from `doctor`/`setup`/inspection, as
documented in ADR-027.

## Capabilities

### New Capabilities
- `antigravity-google-family-harness`: first-class Antigravity CLI
  integration (native plugin delivery, exact coverage, adapted Commands,
  `agy mcp add` fallback) and removal of the replaced integration from the
  v0 surface, with the documented compatibility map (official docs +
  real-binary 1.1.19 evidence).

### Modified Capabilities
- (none beyond the removal above.)

## Impact

- `crates/uze-integrations/src/antigravity/{provision,plugin,generate,skills,commands,mcp}.rs` + `antigravity.rs` — new.
- `crates/uze-integrations/src/gemini/` — removed.
- `crates/uze-application/src/application.rs` — composition (both
  constructors), native-instruction set, bridge list.
- `src/shim.rs`, `src/main.rs`, `crates/uze-core/*` comments — cleaned to
  the live harness set.
- Tests: shared conformance suite, North Star, command conformance,
  invocation labels, shim boundary, context suites, cli lifecycle — updated
  (Gemini legs removed, Antigravity legs added).
- `e2e/src/harness.rs` — Antigravity `HarnessSpec`; Gemini spec removed.
- `docs/adr/027-*`, `docs/architecture/antigravity-compatibility.md`,
  README compatibility tables, integration READMEs, AGENTS.md, capability
  docs.
- No Store/Engine/Router/Core changes; no new CLI commands.
