# Research notes — multi-capability plugin portability (2026-08-20)

Evidence grades: **OFFICIAL** = current vendor docs; **SOURCE_CONFIRMED** =
current upstream source; **EMPIRICAL** = local CLI inspected on this machine;
**UNKNOWN** = not established. This is research only; it contains no claim
that a harness behavior was behaviorally verified by UZE.

## Answer to the phase question

> Can UZE install a plugin once, preserve its original/open representation,
> expose it natively to plugin-aware harnesses, and decompose it safely into
> capabilities for harnesses that do not understand the same plugin envelope?

**Yes.** The next slice must distinguish envelope support from component
support: Codex understands Agent Plugins in current source; Claude's native
plugin requires its original Claude envelope; OpenCode does not understand
the Agent Plugins envelope but natively consumes Agent Skills and supports
MCP through a different configuration primitive. Therefore one Store copy can
feed all three without source-to-source conversion.

## OpenCode research (current)

| Concern | Finding | Evidence | UZE consequence |
|---|---|---|---|
| Plugin system | Yes: global/project JS/TS modules and npm packages; V2 API can transform agents, commands, skills, integrations, and tools. | OFFICIAL | This is executable extension code, not a declarative AP envelope. Do not generate it. |
| Marketplace | No official first-class plugin marketplace/catalog found. `opencode plugin <module>` installs an npm module; docs describe npm/local distribution. | OFFICIAL + EMPIRICAL (CLI 1.18.19) | No UZE marketplace-source registration path. |
| Agent Plugins support | No. Two current upstream feature requests remain open. | SOURCE_CONFIRMED | Deliberate adversarial envelope target. |
| Global/user Skills | `~/.config/opencode/skills`, `~/.claude/skills`, and `~/.agents/skills` all discovered. | OFFICIAL | Existing UZE/Codex `~/.agents/skills` references are native OpenCode discovery. |
| Project Skills | `.opencode/skills`, `.claude/skills`, `.agents/skills`, searched up to git worktree. | OFFICIAL | Project fallback is possible but not preferred for UZE's user-scope North Star. |
| MCP | Local/remote MCP native, automatic tool availability; configuration is OpenCode-specific. | OFFICIAL + EMPIRICAL `opencode mcp` CLI | Translate AP MCP config safely; classify `ADAPTABLE`. |
| Hooks | Yes, through plugin module hooks; V2 has lifecycle/scoped registration and is beta. | OFFICIAL | Future research only; semantics are not equivalent to Claude/Codex. |
| Agents | Yes, primary/subagents with permissions/tool restrictions, global/project config. | OFFICIAL + EMPIRICAL `opencode agent` CLI | Future separate capability analysis. |
| Commands | Yes, markdown/config commands globally or per project; plugin API can transform commands. | OFFICIAL | Vendor convenience primitive; not a reason to revive portable UZE Actions. |
| Scope | Global and project for Skills and plugins; config precedence applies to MCP/agents/commands. | OFFICIAL | Deliver user-scope first; document project only as fallback. |
| Extension API | Yes; V2 is beta, package plugins are npm packages with default export. | OFFICIAL | Never call this “native Agent Plugin.” |

Sources: [OpenCode Skills](https://opencode.ai/docs/skills),
[OpenCode MCP](https://opencode.ai/docs/mcp-servers/),
[OpenCode Plugins](https://opencode.ai/docs/plugins/),
[OpenCode V2 plugin API](https://opencode.ai/v2/docs/build/plugins),
[OpenCode Commands](https://opencode.ai/docs/commands/),
[OpenCode Agents](https://opencode.ai/docs/agents/), and current open
[Agent Plugins request #40993](https://github.com/anomalyco/opencode/issues/40993).

### OpenCode semantic caveats

- AP Skills are discovered natively from `~/.agents/skills`, but OpenCode's
  discovery is its own runtime behavior. This supports `NATIVE` capability
  attachment, not `NATIVE` plugin-envelope attachment.
- AP `mcp.json` describes `stdio` as tokenized command + args; OpenCode
  documents a `local` type with one command array and OpenCode environment/
  working-directory semantics. Mapping is a controlled adaptation, not a
  byte-for-byte native load.
- The OpenCode V2 plugin API could inject transforms, but using it would turn
  UZE into an OpenCode-plugin author and introduce beta executable code.
  It is explicitly rejected for this slice.
- Current UI docs and installed CLI establish an npm plugin command but no
  authoritative general marketplace registry/catalog. Re-evaluate before any
  future registry claim.

## Claude native strategy

Claude Code has a native plugin system and local/Git/URL marketplaces.
`claude plugin marketplace add <source>` and
`claude plugin install <plugin>@<marketplace> --scope user` are available in
the locally inspected Claude Code 2.1.237 CLI (**EMPIRICAL**); its documents
define `.claude-plugin/plugin.json`, Skills, Agents, Hooks, `.mcp.json`, and
the marketplace catalog (**OFFICIAL**).

Agent Plugins root `plugin.json` direct support was not established. Thus
Claude's native strategy is conditional: a package supplies its original
Claude compatibility envelope and UZE preserves/attaches it as one native
Claude plugin. Otherwise UZE uses existing per-capability attachments. This
is native plugin first without pretending the AP core itself is Claude-native.

Sources: [Claude plugins](https://code.claude.com/docs/en/plugins),
[Claude plugin reference](https://code.claude.com/docs/en/plugins-reference),
[Claude marketplaces](https://code.claude.com/docs/en/plugin-marketplaces).

## Codex native strategy

Codex's official documentation establishes a local marketplace at
`.agents/plugins/marketplace.json`, `codex plugin marketplace add`, and
`codex plugin add`. The local CLI 0.148.0 exposes those commands
(**EMPIRICAL**). Current official source change `#36544` recognizes valid
root Agent Plugins manifests during discovery/packing/install, while the
native `.codex-plugin/plugin.json` remains a compatibility overlay
(**SOURCE_CONFIRMED**).

Therefore a root Agent Plugin must be attempted natively first through a
local UZE-backed Codex marketplace. Its native Skill/MCP behavior still needs
the future isolated-home probe. Fallback retains ADR-006/ADR-007 mechanisms.

Sources: [OpenAI Docs: package and marketplace plugins](https://developers.openai.com/plugins/build/plugins),
[Codex source #36544](https://github.com/openai/codex/commit/2b5bdcf).

## Plugin source/import model

Sources may be local paths, Git repositories/revisions, or a specific Claude
or Codex marketplace selector. These answer “where did this package come
from?”, not “which harness can consume it?”. UZE needs source provenance as
metadata and one preserved package root. An imported Codex-marketplace plugin
may carry an AP root, a Claude overlay, both, or neither; compatibility is
derived from those stored artifacts.

| Source | Minimum provenance | Initial handling |
|---|---|---|
| local path | absolute/canonical path + observed manifest/version | already supported input; extend receipt only |
| Git repo | URL, ref/SHA, subdirectory, observed version | data model now; fetch deferred |
| Codex marketplace | marketplace id, package selector, selected revision/cache identity | import from a resolved local package root; remote discovery deferred |
| Claude marketplace | marketplace id, package selector, installed cache/source identity | import from resolved root; remote discovery deferred |
| Agent Plugin source | root AP manifest schema/name/version + source above | preferred portable-core detection |

Store invariant: preserve bytes/full validated tree and provenance receipt;
derive compatibility graph in memory/state. Do not copy only selected
components or rewrite vendor manifests.

## Current-model impact

| Current part | Adequate now | Gap for slice | Minimal next change |
|---|---|---|---|
| `PackageId` / `StoredPackage` | one installed identity and root | registry remembers only source path; terms Package/Plugin implicit | retain one identity, add provenance + envelope facts |
| `UzeStore` | installs AP core Skill/MCP | drops Claude overlay/assets/other original files | full validated-tree preservation |
| `UzeEngine` | derives Resources | flattens before package-native decision | add package plan before existing resource plans |
| `Resource` / `Capability` | component compatibility records | no parent component-consumption state | retain; add derived component IDs/consumption only |
| `IntegrationPort` | resource fallback strategy | no `attach_plugin` inquiry | additive plugin support/attachment plan above it |
| `CapabilityRouter` | four truthful component outcomes | not yet asked about native package consumption | leave unchanged until plan needs it |
| `report` | explicit per-resource routes | no plugin-grouped view | derive plugin detail ViewModel/report later |

`Package == Plugin` is sufficient for the first slice. “Resource” remains a
material component; “Capability” remains compatibility semantics; an
“Attachment” is operational integration state, not a new core identity type.

## Local marketplace and TUI proposal

Local marketplace means the UZE Store plus compatibility/attachment facts.
Vendor local marketplaces are delivery mechanisms and can point at a UZE
stored package after a future integration registers them. UZE does not become
a hosted registry.

The future Rust TUI should use a thin application facade around Store/Engine/
report/integrations and map outputs into a ViewModel. A `ratatui` renderer
would display Plugin list, Harness health, and selected plugin component
matrix. All actions call the same facade used by `uze list/add/remove/inspect/
setup/doctor`; no TUI-specific compatibility logic or duplicated business
rules.

## Risks

1. Native package installation may cache/copy, requiring explicit refresh and
   version awareness.
2. Store full-tree copying requires strict containment/symlink and executable
   trust handling.
3. Claude plugin overlay and AP core can drift; the fixture must test their
   paths/config consistently.
4. OpenCode MCP adaptation must not transform secrets or broaden approvals.
5. OpenCode V2 executable plugin API is beta and deliberately excluded.
6. Shared Skill discovery roots require non-destructive namespaced ownership.
7. A failed authenticated behavioral call is an environment block, not an
   incompatibility verdict.

## Result

**PLUGIN-FIRST PORTABILITY READY FOR IMPLEMENTATION.** The implementation
scope is constrained and falsifiable: preserve one external dual-envelope
multi-capability package, choose native attachment first, then prove OpenCode
component fallbacks explicitly. It does not authorize Hooks, Agents, Commands,
TUI, registry, or generalized marketplace work.

## Implementation outcome — 2026-08-20

The approved vertical slice is now deterministically proven in
`tests/plugin_first_vertical_slice.rs`. One external fixture preserves its
portable `plugin.json`/`mcp.json` plus source-provided Codex
`.codex-plugin/plugin.json`/`.mcp.json` tree under one PackageId. The Store
generates only the documented Codex local marketplace catalog. Codex's native
package plan consumes both derived resource identities, preventing duplicate
fallback attachments. Claude has no envelope in this fixture and therefore
uses the previously proven per-capability paths. OpenCode receives a native
global Skill reference and an adaptable, conflict-safe global MCP config
entry; it does not receive a generated OpenCode plugin.

This proves planning and deterministic delivery, not authenticated vendor
runtime behavior. The remaining native Codex CLI probe is deliberately
opt-in; login/quota/approval failures remain `BLOCKED_BY_ENVIRONMENT`.
