# Antigravity CLI Integration

**Status: primary Google-family v0 harness** (validated against `agy`
**1.1.19** in an isolated `$HOME`). Full audit/evidence:
`docs/architecture/antigravity-compatibility.md`; decision: ADR-027.

| Surface | Status | Mechanism | Evidence |
|---|---|---|---|
| Plugin (explicit) | SUPPORTED, exact coverage | `agy plugin install <Store package path>` — the canonical `plugin.json` (name + description) **is** the vendor manifest (extra fields tolerated) | PROVEN — real-binary dogfood: attach → `agy plugin list` shows import → inspect MATCHED → remove → unregistered → reinstall MATCHED |
| Plugin (generated) | SUPPORTED, exact coverage | canonical `mcp.json` → generated envelope (`mcp_config.json` translation: `url`/`httpUrl` → `serverUrl`) installed from `$UZE_HOME/state/attachments/antigravity/plugins/<id>/` | PROVEN — real-binary dogfood + `agy plugin validate` (skills + mcpServers processed) |
| Skills | SUPPORTED, native | via plugin (package-level) or `ManagedUserScopeReference` → `~/.gemini/antigravity-cli/skills/<label>` (CLI-documented global skills root) | DOCUMENTED (root) + TESTED (lifecycle/drift) |
| Commands | ADAPTED, documented | the vendor's own commands→Skills conversion (no custom-command primitive; no explicit-only mechanism) | PROVEN (conversion output) + TESTED |
| MCP | SUPPORTED, adapted | `agy mcp add <name> <command> [args…]` → `~/.gemini/config/mcp_config.json` | PROVEN (add/list/remove/disable) + TESTED (inspection) |

## Delivery

```
Store canonical package (plugin.json + skills/ + commands/ [+ mcp.json])
        │
        ├─ no canonical MCP surface ──▶ agy plugin install <Store path>   [Explicit]
        │                                → staged byte copy at
        │                                  ~/.gemini/config/plugins/<name>/
        │                                → import_manifest.json registration
        │                                → 1 receipt (content fingerprint)
        └─ canonical MCP surface ──────▶ generated envelope (mcp_config.json
                                          translation) → agy plugin install   [Generated]
```

The staged tree is a **Derived Artifact** (ADR-013 §4): the Store stays the
single source of truth, the staged copy is rebuilt from the Store on
attach, its content fingerprint is the ownership proof, and it is removed
through the official `agy plugin uninstall` verb. There is no link verb in
Antigravity (symlinks are dereferenced — verified 1.1.19), so the vendor
always stages bytes; UZE never *reads* from the staged copy and never lets
it become authoritative.

## Decisions worth stating

- **`plugin install` copies, and UZE accepted that.** The alternative —
  hand-writing `import_manifest.json` and placing the plugin under
  `config/plugins/` ourselves — would reimplement a vendor private format.
  The cost is a byte copy; the mitigation is the derived-artifact
  discipline plus the fingerprint receipt. A same-named foreign import is
  refused (install merges, so overwriting would clobber user state).
- **`mcp.json` is not read by the plugin system** — the vendor file is
  `mcp_config.json`, so MCP-bearing canonical packages take the generated
  route; the translation `url`/`httpUrl` → `serverUrl` is the vendor's own
  documented legacy-migration rule.
- **Commands are Adapted**, not Native: conversion to Skills loses the
  explicit-only property (Skills are model-discoverable). The capability
  declaration says so.
- **Context is Native**: `AGENTS.md` and `GEMINI.md` are both parsed
  (official docs: "identical workspace context rules"), so UZE
  generates no bridge file.
- **No runtime shim**; internal invocations always resolve the real `agy`
  outside `$UZE_HOME/shims`.

## Not yet implemented (documented, never faked)

- Subagents (`agents/` — vendor format is JSON `agent.json`) and hooks
  (`hooks.json`) are supported by the native plugin
  format but are future UZE surfaces, exactly as with every other harness.
- Workspace-level discovery (`.agents/plugins/`, `.agents/skills/`,
  `.agents/mcp_config.json`) is a project-scope concern outside UZE's
  machine-scope integration; workspace MCP config was not observable
  headlessly (`agy mcp list` shows global only).
