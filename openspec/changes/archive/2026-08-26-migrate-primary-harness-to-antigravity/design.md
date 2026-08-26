# Design

## Problem

The Google-family v0 harness is now Antigravity CLI (`agy`). The prior
Google-family integration was removed from the codebase after parity was
proven — no legacy code path remains.

## Constraints and evidence

The design is evidence-driven. Full map: `docs/architecture/
antigravity-compatibility.md` (official docs + real `agy` 1.1.19 in
isolated `$HOME`). Key facts that shaped the design:

1. **The canonical manifest is the vendor manifest.** Antigravity's
   `plugin.json` (name pattern `^[a-zA-Z0-9-_]+$`, description optional,
   extra fields tolerated) is exactly the canonical UZE manifest, so a
   canonical package needs zero Antigravity-specific files and takes the
   explicit route from the Store. There is no generated envelope in the
   common case — the strongest possible portability result.
2. **`mcp.json` is not read by the plugin system.** The vendor file is
   `mcp_config.json`, so canonical MCP requires a generated envelope whose
   only real content transformation is the vendor's own documented
   legacy-migration mapping (`url`/`httpUrl` → `serverUrl`).
3. **No link verb exists.** `agy plugin install` stages a byte copy
   (symlinks are dereferenced) and registers it in
   `import_manifest.json`. UZE therefore treats the staged tree as a
   rebuildable Derived Artifact with a content fingerprint as ownership
   proof — the Store stays authoritative, the copy is never read from, and
   a foreign same-name registration is refused rather than merged over
   (the vendor's install merges; stale files survive).
4. **Commands become Skills.** The vendor's official migration path
   converts commands to Skills and has no explicit-only mechanism. Command
   therefore routes **Adapted** — user invocation native, body/identity
   preserved, the model-discoverability degradation declared. No policy
   file is invented.
5. **Context is native.** `AGENTS.md` and `GEMINI.md` are both parsed
   (official docs), so Antigravity joins the native-instruction set and no
   bridge is generated. The Claude bridge remains the only one.

## Shape

```
canonical UZE package (plugin.json + skills/ + commands/ [+ mcp.json])
        │
        ├─ valid plugin name, no canonical MCP ──► Explicit: agy plugin install <Store path>
        │
        └─ valid plugin name, canonical MCP ─────► Generated: derived envelope
                                                   (plugin.json, symlinks, translated
                                                   mcp_config.json) → agy plugin install
```

All other delivery goes through the unchanged capability machinery:
Skills/Commands as managed references into `~/.gemini/antigravity-cli/skills`
(with a generated wrapper carrying the stable namespaced label), MCP through
`agy mcp add` + `mcp_config.json` inspection.

## What was reused / kept separate

- Reused unchanged: `crate::shared::{process, provision}` (run_quiet,
  capture, `provision_cli`), `qualified_exposure_name_candidates`,
  `default_exposure_name_candidates`, Core receipt/inspection machinery,
  derived-artifact conventions.
- Kept separate (verified divergent, not merely different constants):
  plugin install/inspect/uninstall (copy semantics, JSON shape, fingerprint
  ownership), command representation (SKILL.md; Native vs Adapted), MCP
  config (`mcp_config.json` + `disabled` + `serverUrl`).
- No inheritance; every integration is an independent `IntegrationPort`
  implementation (per the brief: prefer independent implementations
  sharing small helpers).

## Core impact

None. `uze-core` and `IntegrationPort` are untouched — this migration is
another independent proof of the vendor-neutral abstraction. Any
vendor-specific enum, Store change, or Engine branch was found unnecessary
and therefore never added.
