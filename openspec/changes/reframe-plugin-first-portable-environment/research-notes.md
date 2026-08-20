# Research notes — Plugin First, Capability Aware (2026-08-20)

This is evidence for a design decision, not authorization to implement it.
Evidence grades: **OFFICIAL** = vendor/spec documentation; **SOURCE_CONFIRMED**
= current upstream source or shipped CLI source behavior; **EMPIRICAL** = a
real-harness observation recorded by UZE; **UNKNOWN** = not established by
the sources examined. Dates and versions matter: Codex CLI locally inspected
at `0.148.0`; findings are current on 2026-08-20.

## Executive answer

> Can UZE treat a plugin as the primary portable unit, use native
> plugin/marketplace support whenever available, and fall back to
> capability-level compatibility only when necessary?

**Yes, with important gaps.** It can for the portable Agent Plugins core
(Skills + MCP) and for native vendor envelopes it understands. It cannot
truthfully promise whole-plugin portability for Hooks, Agents, Commands,
Rules, UI, marketplace policy, or trust because Agent Plugins 1.0 does not
standardize them and native envelopes differ. “Plugin First” is therefore a
preference and planning boundary, not an equivalence claim.

## 1. Agent Plugins 1.0 — what it standardizes

The published Agent Plugins 1.0 specification defines a self-contained
directory with a required root `plugin.json` and **exactly two portable
component types**: immediate-child Agent Skills in `skills/` and MCP server
configuration in root `mcp.json`. The root manifest has a closed schema;
client-specific metadata belongs under reverse-domain `extensions`, and
client-specific files belong in a same-named top-level extension directory.
It also specifies package-path containment, MCP transport/configuration,
`${PLUGIN_ROOT}` / `${PLUGIN_DATA}`, and per-component failure boundaries.

It does **not** standardize Agents, Hooks, Commands, Rules/instructions,
marketplace/catalog manifests, package lifecycle, trust review, OAuth/secret
references, UI, or how clients expose a skill. Those are vendor extensions
or client policy, not omissions UZE should fill with a new format.

- **Source:** [Agent Plugins 1.0 normative specification](https://agent-plugins.org/specification)
  (§§4–9), **OFFICIAL**.
- **External-envelope conclusion:** suitable as UZE's canonical *external
  portable envelope* only for Skills/MCP; insufficient as a universal
  plugin model. UZE additionally needs provenance/install state, attachment
  state, evidence/confidence, external envelope detection, and a derived
  compatibility graph. It should not recreate the portable manifest.

## 2. Plugin convergence matrix

Terms: **native** means the harness's documented package/envelope; **AP 1.0**
means direct Agent Plugins root-envelope support. “No” means no documented
feature, not a claim the concept is impossible through custom code.

| Concern | Claude Code | Codex | Cursor | OpenCode | Windsurf | Gemini CLI |
|---|---|---|---|---|---|---|
| Plugin system | Native Claude plugin | Native plugin | Native Cursor plugin + AP 1.0 | Native in-process JS/TS module | No agent-package system found; IDE plugins are editor integrations | Native extension |
| Open Agent Plugins | No direct support found | Yes (current source; 0.147+ change) | Yes, loads unchanged | No; open support request | No evidence | No evidence |
| Marketplace | Native catalog | Native catalog + universal directory | Native curated/team marketplace | npm ecosystem, not a plugin catalog | No agent marketplace | Extension gallery |
| Local marketplace source | Yes, local path/catalog | Yes, local marketplace source | Local plugin-development directory; team marketplace is cloud-admin | No catalog; local module directories/config | No | Local extension install/link, not catalog |
| Git marketplace source | Yes | Yes | Git repository publication/team import | npm package / local module, not marketplace | No | Git repo extension install |
| User/global plugins | Yes | Yes | Yes | Yes | No package form | Yes (global enabled default) |
| Project plugins | Yes | Yes | Yes | Yes | No package form | Disable/enable by workspace; install is global |
| Skills in package | Yes | Yes | Yes/AP 1.0 | No package bundle; native skills separately | UNKNOWN | Yes |
| MCP in package | Yes | Yes | Yes/AP 1.0 | No package bundle; native config/plugins can transform | Native MCP separately | Yes |
| Hooks in package | Yes | Yes; trust review required | Yes (Cursor-only format) | Plugin code hooks | Native `hooks.json` separately | Yes |
| Agents in package | Yes | No documented Codex plugin component | Yes (Cursor-only format) | Native agents separately/plugin may transform | UNKNOWN | Yes, preview |
| Commands/actions in package | Yes, legacy; Skills preferred | No standalone prompt resource; plugin UX via Skills | Yes (Cursor-only format) | Native commands separately/plugin may transform | Workflows are slash-triggered, not package | Yes |
| Rules/instructions in package | Skills/agents/hook context, no distinct Rules folder | Skills; no documented Rules package surface | Yes (Cursor-only format) | Config/agents/plugin transforms | Rules/ memories separately | Extension `GEMINI.md`/context file |
| Install CLI/API | `claude plugin` CLI | `codex plugin` CLI | UI/Customize; local directory | config/npm/local module | N/A | `gemini extensions` CLI |
| Symlink support | Skills-dir experimentally yes; marketplace normally cache-copy | Yes (local marketplace examples and source) | Yes for local development | Local module directly loaded (symlink not specifically documented) | UNKNOWN | `extensions link` explicitly |
| Plugin lifecycle | marketplace cache, enable/update/reload | cache, enable/config, marketplace refresh | install user/workspace/team; reload/restart | loaded/reloaded module; V2 beta scopes registrations | N/A | copy/install, update, enable/disable, restart |

### Evidence and semantic qualification per harness

**Claude Code — OFFICIAL + EMPIRICAL.** Claude plugins use
`.claude-plugin/plugin.json`; root directories can include Skills, legacy
Commands, Agents, Hooks, `.mcp.json`, LSP, monitors, executables, and
settings. It supports local/Git/URL marketplaces with
`.claude-plugin/marketplace.json`, but normally copies installed plugins to a
cache. The previously completed UZE research empirically proved the separate
skills-directory symlink path. Claude's plugin-component documentation is
official, but no documentation was found that it directly loads a root Agent
Plugins manifest; classify AP 1.0 direct support **UNKNOWN/unsupported for
planning**, not assumed.

Sources: [create plugins](https://code.claude.com/docs/en/plugins),
[plugin reference](https://code.claude.com/docs/en/plugins-reference),
[marketplaces](https://code.claude.com/docs/en/plugin-marketplaces).

**Codex — OFFICIAL + SOURCE_CONFIRMED.** Codex's native manifest is
`.codex-plugin/plugin.json`; official docs describe bundled Skills, MCP,
and Hooks, native marketplace CLI, local/Git/NPM marketplace entries, copied
cache installs, and plugin-scoped MCP policy. The current upstream commit
`#36544` explicitly recognizes a valid root Agent Plugins manifest for
discovery/packing/installation while retaining the legacy native path. Its
own test also guards that a root Agent Plugin is not an executor-plugin
overlay. This establishes direct AP 1.0 package support at **SOURCE_CONFIRMED**
level; an UZE real-harness probe is still required before claiming
behaviorally verified native attachment.

Sources: [OpenAI plugin package docs](https://developers.openai.com/plugins/build/plugins),
[Codex AP 1.0 source change #36544](https://github.com/openai/codex/commit/2b5bdcf),
and local `codex plugin --help` (**EMPIRICAL** availability of CLI 0.148.0).

**Cursor — OFFICIAL.** Cursor has two explicit formats: AP 1.0 at root
`plugin.json` (Skills + MCP only), which it says loads without changes, and
Cursor Plugin at `.cursor-plugin/plugin.json` (adds Rules, Agents, Commands,
Hooks, Variables). It supports user/project scope and symlinked local plugin
development. Cursor is therefore *not* adversarial for the AP 1.0 core;
vendor extras remain non-portable.

Sources: [Cursor plugins](https://cursor.com/docs/plugins),
[Cursor plugin reference](https://cursor.com/docs/reference/plugins).

**OpenCode — OFFICIAL + SOURCE-ecosystem finding.** Its native “plugin” is
an in-process JS/TS module loaded from global/project directories or npm,
not a declarative distributable directory envelope. Its V2 plugin API is
beta and can transform agents, commands, integrations, references, skills,
and tools. Agents, commands, MCP, and skills are independently configurable;
documentation does not describe an AP 1.0 package loader. The upstream open
issue requesting AP 1.0 support is corroborating but not a feature contract.

Sources: [plugins](https://opencode.ai/docs/plugins/),
[V2 plugin API](https://opencode.ai/v2/docs/build/plugins),
[commands](https://opencode.ai/docs/commands/),
[agents](https://opencode.ai/docs/agents/),
[MCP](https://opencode.ai/docs/mcp-servers/),
[AP 1.0 support request](https://github.com/anomalyco/opencode/issues/40993).

**Windsurf — OFFICIAL, narrow.** Current documentation establishes MCP,
workspace/user/system Hooks with veto by exit code, rules/memories, and
slash-invoked Workflows. Its documented “Windsurf Plugins” are IDE editor
plugins, not an agent-plugin package/catalog. Skills, Agents, AP 1.0, and a
local agent marketplace were not established; classify them **UNKNOWN**.
This makes it a meaningful but poorly documented/closed target, not the next
recommended conformance harness.

Sources: [Windsurf plugins](https://docs.windsurf.com/plugins),
[Cascade Hooks](https://docs.windsurf.com/en/windsurf/cascade/hooks),
[Workflows](https://docs.windsurf.com/en/plugins/cascade/workflows).

**Gemini CLI — OFFICIAL.** A proprietary `gemini-extension.json` bundle can
contain MCP, context, custom TOML Commands, `hooks/hooks.json`, Agent Skills,
preview subagents, and policy files. It installs from a Git URL or local
path, copies the source, supports update/enable/disable, and has an explicit
`extensions link` symlink development flow. No AP 1.0 loader or marketplace
catalog-source format was found. It is a robust secondary adversarial target
but its package is not a drop-in portable standard envelope.

Sources: [Gemini extensions](https://github.com/google-gemini/gemini-cli/blob/main/docs/extensions/index.md),
[extension reference](https://github.com/google-gemini/gemini-cli/blob/main/docs/extensions/reference.md).

## 3. Agent Plugin ↔ vendor format mapping

| Mapping | Relationship | Evidence | Consequence for UZE |
|---|---|---|---|
| AP 1.0 ↔ Claude Plugin | **Subset-compatible by content, not identical envelope.** Shared Skill structure and MCP concept; Claude requires/uses `.claude-plugin/plugin.json`, `.mcp.json`, and vendor component semantics. | OFFICIAL | Preserve Claude package as Claude; extract AP core only when it truly conforms. |
| AP 1.0 ↔ Codex Plugin | **Supported portable envelope plus proprietary superset.** Codex native envelope is `.codex-plugin/plugin.json`; current source recognizes root AP manifest for installation. Native extras include Hooks, apps/UI/marketplace metadata. | SOURCE_CONFIRMED + OFFICIAL | Prefer root AP attachment when an input is AP 1.0; retain Codex envelope for Codex-only additions. |
| AP 1.0 ↔ Cursor Plugin | **Direct support plus proprietary superset.** Cursor documents AP root packages loading unchanged; its own envelope adds Rules/Agents/Commands/Hooks/Variables. | OFFICIAL | Cursor would validate native AP path, not an adversarial core fallback. |
| AP 1.0 ↔ OpenCode Plugin | **Different abstraction.** OpenCode plugins are executable modules and do not establish root-manifest support. | OFFICIAL | Decompose portable components or later build a carefully scoped adapter; never relabel as native AP plugin. |
| AP 1.0 ↔ Gemini Extension | **Different proprietary envelope.** Similar folders do not imply identical hooks/commands/agents. | OFFICIAL | Translate only after semantic and trust analysis, with explicit loss reporting. |

## 4. Marketplace/native discovery mapping

| Harness | Discovery/install model | Can UZE be a local source? | Result |
|---|---|---|---|
| Claude | Add local/Git/URL marketplace; install selected package; cache-copy default. | **Yes, OFFICIAL** local marketplace path. | Feasible future experiment, vendor catalog required. |
| Codex | `codex plugin marketplace add` accepts local/Git source, then `codex plugin add`; local catalog can point at store paths; cache/install lifecycle. | **Yes, OFFICIAL** local marketplace; AP root package support SOURCE_CONFIRMED. | Strongest future native-source experiment alongside Claude. |
| Cursor | local plugin dev directory and hosted/team marketplace. Team marketplace is admin/cloud plan surface. | Local load **yes**; local catalog registration **not established**. | Direct package attachment may work; “UZE source” needs separate evidence. |
| OpenCode | Global/project modules or npm packages, loaded directly/configured. | Not a marketplace source. | Capability/config or module-adapter path. |
| Windsurf | No agent-plugin catalog found. | No evidence. | Capability-level only, if targeted. |
| Gemini | `extensions install <Git URL|local path>` and `extensions link`; no catalog found. | Direct local extension is possible, local marketplace source is not. | Conversion/envelope experiment, not a source registry. |

## 5. Semantic—not structural—comparison

### Commands/actions (Phase C reinterpretation)

Phase C's conclusion survives. Claude plugin `commands/` is documented but
legacy; Claude recommends `skills/` for new work. Codex removed
`~/.codex/prompts/*.md` and directs users toward Skills; the contemporary
Codex plugin model's user-facing workflow is Skill/plugin invocation rather
than a portable Commands directory. Cursor, OpenCode, and Gemini retain
their own command primitives, each with distinct invocation, argument,
agent, and shell-expansion behavior.

**Interpretation:** `Command` is not currently a portable UZE capability to
abstract by default. UZE must not invent an `Action` only to emulate a
primitive Codex consciously retired. Revisit only if an open standard gains
semantic adoption or a concrete user package needs an explicitly lossy
adaptation. Skills are the ecosystem's more credible portable replacement.

### Hooks (Phase C reinterpretation)

Claude and Codex have strong lifecycle convergence and both package Hooks;
Codex accepts blocking and rewritten tool input, while Claude has its own
event/matcher/output contract. Cursor, Gemini, OpenCode, and Windsurf also
have hook concepts, but their event names, timing, permissions, trust review,
input/output JSON, failure rules, ordering, and veto semantics vary. Gemini
adds policy tiers; Codex requires trust review for plugin hooks; Windsurf
uses exit code 2 for pre-hook veto.

**Interpretation:** portable Hooks are a plausible *future extension
proposal*, not a current portable capability. Only a semantic compatibility
matrix plus empirical tests can justify a common model.

### Agents/subagents (Phase C reinterpretation)

Claude plugin agents have rich declarative fields (tools/disallowed tools,
memory, model, background, and worktree isolation). Codex has rich runtime
orchestration but does not document a parallel plugin-agent component.
Cursor, OpenCode, and Gemini have their own agent definitions, permission
and lifecycle models; Gemini subagents remain preview.

**Interpretation:** Agents are not a portable component today. They need
capability-by-capability reporting, not a forced common `Agent` adapter.

## 6. UZE architectural impact

The existing `ManagedUserScopeReference` and `ManagedVendorConfig` tracer
bullets remain useful, proven fallbacks. Plugin-first inserts a package-level
decision *before* current flattening:

```text
Plugin in UZE Store
  |
  +-- integration can consume this exact envelope natively?
  |      yes -> attach package; mark consumed components
  |      no  -> derive components
  |
  +-- per remaining component: native capability -> safe adaptation -> unsupported
```

The compatibility report should eventually be explicit per plugin and per
component rather than calculate a deceptive percentage:

```text
Plugin foo
  Claude: native Claude envelope / Skill native / MCP native / Hook native
  Codex:  Agent Plugins native / Skill native / MCP native / Hook native
          Agent and Command: not claimed portable
  OpenCode: no native envelope / Skill adaptable / MCP adaptable / others explicit
```

No score is proposed.

## 7. Risks

1. **Young standard:** AP 1.0 deliberately has a small surface; source and
   vendor adoption can change quickly.
2. **Vendor extensions:** reverse-domain namespaces preserve data but do not
   make it portable or semantically comparable.
3. **Marketplace divergence:** catalogs, cache copies, updates, source types,
   enablement, scopes, and reloads vary; “one local source” is not one
   manifest.
4. **Trust/security:** Hooks execute code; MCP can expose privileged tools
   and OAuth; plugin installation must retain each harness's review,
   approvals, and credential boundaries.
5. **Undocumented behavior:** retain evidence grades and add real-harness
   probes before `VERIFIED` claims.
6. **Duplicate attachment:** native package attach plus fallbacks can cause
   duplicate skills/MCP unless the plan returns consumed components.
7. **Runtime-proxy overreach:** static/package installation should remain the
   default; introduce a bridge only for a demonstrated runtime need.

## Verdict

**PLUGIN-FIRST VIABLE WITH IMPORTANT GAPS.**

It improves UZE's model because it preserves existing standard/plugin
packages and lets native harness support win where it genuinely exists. It
does not simplify the whole system into one universal envelope. Capability
analysis and the two existing attachment mechanisms remain essential for
vendor extensions, incomplete clients, and truthful graceful degradation.
