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
  with every other harness.

  **Hooks are delivered into the shared `~/.gemini/config/hooks.json`**
  (ADR-033/ADR-040), not into the generated plugin: one named entry per
  canonical group, keyed `<package>:<group-id>`, grouped with the translated
  matcher for the tool events and flat for `Stop`, whose command is the
  generated `hooks/exec` wrapper under
  `$UZE_HOME/state/attachments/antigravity/hooks/exec` — absolute, because
  the harness runs a hook with its cwd set to the directory holding
  `hooks.json`, and no `uze` sits on the execution path. The document root
  *is* the named-hook map, so UZE owns exactly its own keys: a hand-written
  hook in the same file is never read, rewritten or removed, and drift or an
  unreadable file blocks the mutation. This is the same shape as Codex's
  shared `hooks.json` delivery.

  It is delivered there because **the harness does not read a plugin's
  `hooks.json`.** The vendor's own plugin guide says hooks in
  `plugins/<name>/hooks.json` are "registered and run during the agent's
  lifecycle"; on 1.1.24 they are not. `agy plugin validate` counts them, the
  plugin is listed with a `hooks` component and enabled in `config.json`,
  and the session still reports `loaded 0 named hooks from 0 hooks.json
  file(s)` — it never opens the file. The Conformance Lab measures that live
  every run (`hooks > delivery`), so if a later build starts reading plugin
  hooks, the check says so and the delivery can move back.

  One vendor gate still stands in front of every delivered hook: **AGY
  executes `hooks.json` hooks only in a signed-in session.** The executor
  reads `enable_json_hooks` (field 17 of the backend's
  `CustomizationConfig`, switched server-side by the `json-hooks-enabled`
  feature flag), and that config reaches the CLI only over the CloudCode
  backend it speaks when signed in to a Google account. A `GEMINI_API_KEY`
  session loads the same hooks, lists them under `/hooks`, and runs none of
  them — for any event. Nothing UZE delivers changes that; the mode does.
  Vendor bug
  [google-antigravity/antigravity-cli#893](https://github.com/google-antigravity/antigravity-cli/issues/893)
  ("hooks loaded but never executed when authenticated via GEMINI_API_KEY"),
  alongside #78 recording that the Gemini API-key path is unsupported at
  all. The Lab runs the vertical signed in and asserts UZE's own hooks there
  (deny relayed, tool blocked, first-deny-wins, allow executes), and keeps
  one declared check on the API-key mode so #893 stays visible.
- Workspace-level `.agents/mcp_config.json` discovery is a project-scope
  concern outside UZE's machine-scope integration; it was not observable
  headlessly (`agy mcp list` shows global only). `.agents/skills/` is a
  separate case: official docs (antigravity.google/docs/cli/plugins, 2026)
  now confirm `agy` reads it directly per-workspace, no UZE involvement
  needed — see `AntigravityIntegration::discovers_project_agents_directory`.
