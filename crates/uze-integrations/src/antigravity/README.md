# Antigravity CLI Integration

**Status: primary Google-family v0 harness** (validated against `agy`
**1.1.21** in an isolated `$HOME`). Full audit/evidence:
`docs/architecture/antigravity-compatibility.md`; decision: ADR-027.

| Surface | Status | Mechanism | Evidence |
|---|---|---|---|
| Plugin (explicit) | SUPPORTED, exact coverage | `agy plugin install <Store package path>` — the canonical `plugin.json` (name + description) **is** the vendor manifest (extra fields tolerated) | PROVEN — real-binary dogfood: attach → `agy plugin list` shows import → inspect MATCHED → remove → unregistered → reinstall MATCHED |
| Plugin (generated) | SUPPORTED, exact coverage | canonical `mcp.json` → generated envelope (`mcp_config.json` translation: `url`/`httpUrl` → `serverUrl`) installed from `$UZE_HOME/state/attachments/antigravity/plugins/<id>/` | PROVEN — real-binary dogfood + `agy plugin validate` (skills + mcpServers processed) |
| Skills | SUPPORTED, native (default policy) | via plugin (package-level) or `ManagedUserScopeReference` → `~/.gemini/antigravity-cli/skills/<label>` (CLI-documented global skills root) | DOCUMENTED (root) + TESTED (lifecycle/drift) |
| Skill invocation policy | NATIVE model-only; ADAPTED user-only | `disable-slash-command: true` preserves `model=true,user=false`; no model-discovery suppression exists for `model=false,user=true` | PROVEN (agy 1.1.21) + TESTED |
| MCP | SUPPORTED, adapted | `agy mcp add <name> <command> [args…]` → `~/.gemini/config/mcp_config.json` | PROVEN (add/list/remove/disable) + TESTED (inspection) |

## Delivery

```
Store canonical package (plugin.json + skills/ [+ mcp.json])
        │
        ├─ no canonical MCP surface ──▶ agy plugin install <Store path>   [Explicit]
        │                                → staged byte copy at
        │                                  ~/.gemini/config/plugins/<name>/
        │                                → import_manifest.json registration
        │                                → 1 receipt (content fingerprint)
        └─ canonical MCP surface ──────▶ generated envelope (mcp_config.json
                                          translation) → agy plugin install   [Generated]
```

The staged tree is a **Derived Artifact** (ADR-013 §5): the Store stays the
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
- **Invocation policy is asymmetric**: `disable-slash-command: true`
  natively preserves model-only Skills; Antigravity still has no way to
  preserve user-only Skills because they remain model-discoverable. Any
  package containing a non-default Skill is decomposed so an unchanged
  plugin tree cannot bypass the per-skill policy wrapper or duplicate it.
- **Context is Native**: `AGENTS.md` and `GEMINI.md` are both parsed
  (official docs: "identical workspace context rules"), so UZE
  generates no bridge file.
- **No runtime shim**; internal invocations always resolve the real `agy`
  outside `$UZE_HOME/shims`.

## Not yet implemented (documented, never faked)

- Subagents (`agents/` — vendor format is JSON `agent.json`) are supported
  by the native plugin format but are a future UZE surface, exactly as
  with every other harness. Hooks are delivered (ADR-033/ADR-040: the
  generated plugin's `hooks.json`, named entries at the document root, each
  group's matcher translated and its handlers run by the `hooks/exec`
  wrapper vendored inside that same plugin — no `uze` on the execution
  path); note
  that AGY executes `hooks.json` hooks only when `enable_json_hooks`
  (field 17 of the backend's `CustomizationConfig`, switched server-side by
  the `json-hooks-enabled` feature flag) is set. That config reaches the CLI
  only over the CloudCode backend it speaks when signed in to a Google
  account; a Gemini API-key session never receives it. The conformance Lab
  serves the flag and the harness consumes it, yet still observes hooks
  loaded and listed but never run (1.1.22, 1.1.24) — which the Antigravity
  vertical measures with a vendor-format control hook before judging UZE's.
- Workspace-level `.agents/mcp_config.json` discovery is a project-scope
  concern outside UZE's machine-scope integration; it was not observable
  headlessly (`agy mcp list` shows global only). `.agents/skills/` is a
  separate case: official docs (antigravity.google/docs/cli/plugins, 2026)
  now confirm `agy` reads it directly per-workspace, no UZE involvement
  needed — see `AntigravityIntegration::discovers_project_agents_directory`.
