# ADR 027: Antigravity CLI is the Google-family v0 harness; Gemini CLI removed

## Status

Accepted.

## Context

Google transitioned its terminal agent from Gemini CLI to Antigravity CLI
(`agy`) in mid-2026: Gemini CLI stopped serving Google AI Pro/Ultra and free
tiers on June 18, 2026, and Antigravity CLI (a Go rewrite sharing the
Antigravity 2.0 harness) is the migration target. UZE's fourth integration
was Gemini CLI — deliberately chosen to falsify the vendor-neutral core
against a differently shaped native delivery (`extensions link`, no
catalogue). Keeping it as the Google-family v0 target no longer reflects
where users are. The migration was executed as audit → map → migrate/reuse
safely → prove behavior → replace Gemini as the v0 surface, and the final
product decision is that the Gemini CLI integration is removed entirely (no
legacy code path remains; its history survives only in ADRs and the
migration audit record).

## Decision

1. **Antigravity CLI is the Google-family v0 harness** (after Claude,
   Codex, OpenCode) through a new, independent `AntigravityIntegration`
   implementing the unchanged `IntegrationPort`. No inheritance, no wrapper;
   independent integrations sharing small, proven-neutral helpers.
2. **The Gemini CLI integration is removed from the codebase** — module,
   tests, fixtures, e2e spec, composition and bridge entry deleted. Its
   historical record survives in the ADRs that documented it and in
   `docs/architecture/antigravity-compatibility.md` (the migration audit).
3. **The canonical UZE package stays unchanged.** The canonical
   `plugin.json` (name + description) **is** a valid Antigravity plugin
   manifest (extra fields tolerated — verified against agy 1.1.19), so the
   North Star package ships no Antigravity-specific file and takes the
   explicit native route straight from the Store. The only translated
   surface is MCP: the plugin system reads `mcp_config.json`, never
   canonical `mcp.json`, so a package with a canonical MCP surface is
   delivered through a generated plugin carrying a translated
   `mcp_config.json` (the `url`/`httpUrl` → `serverUrl` mapping is the
   vendor's own documented legacy-migration rule).
4. **The staged plugin tree is a Derived Artifact.** `agy plugin install`
   stages a byte copy at `~/.gemini/config/plugins/<name>/` and registers it
   in `import_manifest.json`; there is no link verb (symlinks are
   dereferenced — verified). The Store stays authoritative: UZE rebuilds the
   staged copy from the Store on attach, records a deterministic content
   fingerprint as its ownership proof, never reads from the copy, and
   removes it through the official `agy plugin uninstall` on detach. A
   registration existing without a UZE receipt is foreign state and is
   refused rather than overwritten (install merges; stale files survive —
   verified).
5. **Commands are `Adapted`, deliberately.** Antigravity has no
   custom-command primitive: its official migration path converts commands
   to Skills, and Skills are model-discoverable with no observable
   explicit-only mechanism. Per ADR-030, Native requires preserved canonical
   semantics, and the explicit-only property degrades; user invocation stays
   native (slash command), body/description/identity are preserved, and the
   degradation is declared in `capabilities()` — never hidden.
6. **Context is Native.** Official Antigravity docs state workspace context
   rules are identical: `AGENTS.md` and `GEMINI.md` are both parsed. UZE
   generates no `@AGENTS.md` bridge for Antigravity (it is in the
   native-instruction set). With the Gemini integration removed, the only
   remaining bridge is Claude Code's.
7. **Vendor terminology is preserved: Plugin, never Extension.** No
   internal function, receipt kind, or doc string calls Antigravity
   packages "extensions".
8. **No shim.** `runtime_contribution` stays passthrough; no PATH shim is
   created; internal invocations resolve the real `agy` outside
   `~/.uze/shims` (the shared recursion-hazard rule).

## Consequences

- `AntigravityIntegration`: id `antigravity`, aliases `agy` /
  `antigravity-cli`; detection `agy --version`; provisioning via the
  official installer (invoked exactly as documented; its own PATH export
  append to shell rc files is vendor behavior — the docs' documented
  `--skip-aliases`/`--skip-path` flags are rejected by the current script,
  so UZE cannot suppress them) and `agy update`; post-install verification
  resolves the binary at its documented `~/.local/bin/agy` destination
  even when the current shell has no fresh rc files; native Skills + MCP
  via plugin, adaptable Command, adapted MCP fallback via `agy mcp add`.
- Exact coverage: structural (skills/, commands/ — converted at load — and
  declared/translated MCP), pure functions, mirroring the other
  integrations' intersection discipline, with partial-coverage tests.
- Real-binary dogfood (agy 1.1.19, isolated HOME): attach → MATCHED,
  generated route (MCP translation) → MATCHED with vendor
  validate-confirmed plugin, drift → blocked detach, clean removal →
  unregistered, reinstall → MATCHED.
- `uze-core`: **zero changes**. `IntegrationPort`: **unchanged**.
- Documentation ordering reads Claude, Codex, OpenCode, Antigravity.

## References

- `docs/architecture/antigravity-compatibility.md` (audit map + evidence log)
- `crates/uze-integrations/src/antigravity.rs` (integration composition root)
- Antigravity CLI docs: Plugins & Skills, Migration (gcli-migration), MCP,
  Installation & Auth, Skills
