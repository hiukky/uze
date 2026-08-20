# Research notes — broader 2026 coding-agent ecosystem

**Date:** 2026-08-20. This is research for the Conformance Lab change; it is
not authorization to implement another harness integration.

Evidence labels: **OFFICIAL** means vendor documentation/specification;
**SOURCE_CONFIRMED** means current first-party source; **UNKNOWN** means that
UZE must not infer support from similar directory names.

## Executive result

**UZE ARCHITECTURE GENERALIZES WITH MINOR EXTENSIONS.**

The broader ecosystem reinforces the existing separation:

```text
Package       = distribution unit
Capability    = compatibility unit
Integration   = harness delivery strategy
Provenance    != compatibility
```

No relevant harness requires a vendor-to-vendor converter, a Store that owns
harness artifacts, a lifecycle-aware Router, or a presentation layer that
talks directly to integrations. The two bounded future extensions are:

1. format-aware import for *recognized* external envelopes beyond root Agent
   Plugins; and
2. an integration-owned generic native-package mechanism/receipt, because the
   current `NativePluginMarketplace` mechanism is Codex-shaped.

Neither is a Core redesign. Until an envelope is supported, UZE can preserve
it as package bytes and only expose safely recognized portable capabilities.

## Classification matrix

These labels are research output, not Core enums. A `NATIVE_PLUGIN` harness
may still receive a given package through capability fallback.

| Harness | Delivery class | L2 class | Short rationale |
|---|---|---|---|
| Claude Code | `NATIVE_PLUGIN` | `L2_CONFORMANCE_POSSIBLE` | Native plugin/marketplace and headless CLI; non-Claude local gateways are explicitly outside Anthropic support. |
| Codex | `NATIVE_PLUGIN` | `L2_CONFORMANCE_POSSIBLE` | Native plugin/marketplace and headless CLI; local Responses route needs a pinned behavioral spike. |
| OpenCode | `CAPABILITY_ADAPTER` | `L2_CONFORMANCE_READY` | Standard Skills/MCP plus headless CLI and direct OpenAI-compatible provider. |
| Cursor | `NATIVE_PLUGIN` | `L2_CONFORMANCE_POSSIBLE` | Direct Agent Plugins support and headless CLI; generic local endpoint evidence is insufficient. |
| Windsurf | `IDE_EXTENSION_REQUIRED` | `CONTRACT_ONLY` | Documented plugins are IDE integrations; no safe headless/local-model package surface established. |
| Gemini CLI | `NATIVE_PLUGIN` | `NOT_CURRENTLY_TESTABLE` | Rich native extension and headless CLI, but no official zero-vendor llama/OpenAI-compatible path. |
| GitHub Copilot CLI | `NATIVE_PLUGIN` | `L2_CONFORMANCE_READY` | Plugin CLI, isolated `COPILOT_HOME`, offline mode, and documented local OpenAI-compatible BYOK. |
| Cline | `CAPABILITY_ADAPTER` | `L2_CONFORMANCE_POSSIBLE` | Headless CLI and OpenAI-compatible provider documented; container spike still required. |
| Roo Code | `CAPABILITY_ADAPTER` | `CONTRACT_ONLY` | MCP/rules exist, but maintained CLI/local-conformance surface is not stable enough. |

## Ecosystem and package matrix

| Harness | Package / manifest / marketplace | Scope and extension surface | Evidence |
|---|---|---|---|
| Claude Code | `.claude-plugin/plugin.json`; local/Git/URL marketplace and `claude plugin` CLI. | User, project, local and managed scope; Skills, MCP, agents, hooks and legacy commands in a plugin. | [Plugins](https://code.claude.com/docs/en/plugins), [marketplaces](https://code.claude.com/docs/en/plugin-marketplaces) — OFFICIAL |
| Codex | `.codex-plugin/plugin.json`; personal/local/Git/NPM marketplace flow. Current source recognizes root Agent Plugin packages too. | Skills, `.mcp.json`, apps and vendor components; package hooks require real conformance before a portability claim. | [OpenAI plugins](https://developers.openai.com/plugins/build/plugins), [Codex source](https://github.com/openai/codex) — OFFICIAL/SOURCE_CONFIRMED |
| OpenCode | Native plugin is executable JS/TS module, local or npm-configured; no declarative Agent Plugin loader established. | Global/project modules/config; Skills, MCP, commands, agents and hooks exist separately. | [Plugins](https://opencode.ai/docs/plugins/), [V2 API](https://opencode.ai/v2/docs/build/plugins) — OFFICIAL |
| Cursor | Root Agent Plugins `plugin.json` loads unchanged; Cursor-specific `.cursor-plugin/plugin.json` adds vendor extras. Marketplace and symlinked local development are documented. | User/workspace/team scope; Rules, agents, commands and hooks are Cursor semantics. | [Plugins](https://prod.cursor.com/docs/plugins), [Skills](https://prod.cursor.com/docs/skills), [Rules](https://prod.cursor.com/docs/rules) — OFFICIAL |
| Windsurf | Current documented plugins are IDE/editor integrations; no agent-package manifest or local agent catalog established. | MCP, rules/memories, workflows and hooks have their own IDE/workspace surfaces. | [Plugins](https://docs.windsurf.com/plugins) — OFFICIAL |
| Gemini CLI | `gemini-extension.json`; install from local/Git, update/enable/disable, symlink link workflow; experimental registry can point at URL/path. | Skills, MCP, `GEMINI.md`, hooks, commands, policies and preview agents. | [Extension reference](https://github.com/google-gemini/gemini-cli/blob/main/docs/extensions/reference.md), [configuration](https://github.com/google-gemini/gemini-cli/blob/main/docs/reference/configuration.md) — OFFICIAL |
| GitHub Copilot CLI | `plugin.json` plus `.plugin`, `.github/plugin` and `.claude-plugin` locations; marketplace/Git/local CLI lifecycle. | Skills, MCP, hooks, commands, agents and extensions; global state can be isolated with `COPILOT_HOME`. | [Plugin reference](https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-plugin-reference) — OFFICIAL |
| Cline | Native package is executable TS/JS plugin (`package.json.cline.plugins`), installed from marketplace/Git/npm/local paths. | Global/project Skills, MCP, rules, agents and SDK hooks/tools/commands. | [Plugins](https://docs.cline.bot/customization/plugins), [SDK plugins](https://docs.cline.bot/sdk/plugins) — OFFICIAL |
| Roo Code | No current portable package envelope established. | Global/project MCP, modes and rules; source lifecycle is currently uncertain. | [MCP](https://github.com/RooCodeInc/Roo-Code-Docs/blob/main/docs/features/mcp/overview.md) — SOURCE_CONFIRMED |

## Capability matrix

`P` means present in the harness-native package, `A` means capability-level
adapter is the honest UZE starting point, and `?` remains unproven.

| Harness | Skills | MCP | instructions | Hooks | Commands | Agents | Semantically safe UZE reading |
|---|---:|---:|---:|---:|---:|---:|---|
| Claude Code | P | P | `CLAUDE.md` | P | P / legacy | P | Native only for Claude envelope; otherwise Skill/MCP fallback. |
| Codex | P | P | `AGENTS.md` | P* | Skills-first | P | Native AP/Codex package where proven; otherwise fallback. |
| OpenCode | A | A | ✓ | ✓ (JS) | ✓ | ✓ | Skills/MCP; do not synthesize/execute JS plugin. |
| Cursor | P / AP | P / AP | `AGENTS.md` / Rules | P | P | P | Agent Plugins Skill/MCP directly; extras remain Cursor-only. |
| Windsurf | ? | A | ✓ | ✓ | workflows | ? | No safe adapter until official package/config evidence is sufficient. |
| Gemini CLI | P | P | `GEMINI.md` (configurable) | P | P | P preview | Preserve extension; adapt only proven portable components. |
| Copilot CLI | P | P | ✓ | P | P | P | Native compatible plugin if supplied; otherwise Skills/MCP adapter. |
| Cline | A | A | ✓ | ✓ (SDK) | ✓ | ✓ | Skills/MCP only; SDK plugin is opaque executable code. |
| Roo Code | A | A | ✓ | ? | modes | modes | Contract research only. |

\* Codex package Hooks are not yet UZE conformance evidence.

### Convergence

The strongest portable convergence is **Agent Skills + MCP**, followed by a
growing `AGENTS.md`/instruction convention. Hooks, agents and commands share
names but not a behavior contract: event timing, permissions, isolation,
memory, vetoes, input/output schemas and trust differ. Commands in particular
are trending toward Skills in Claude/Codex and remain vendor convenience rather
than a new UZE portable primitive.

Executable JS/TS plugin APIs (especially OpenCode and Cline) are a separate
trust boundary. UZE may preserve and later natively attach them; it must never
run, transform or recreate their code while installing a portable package.

## Provider, headless and container matrix

| Harness | Headless / isolation | Provider route | Local-model result |
|---|---|---|---|
| Claude Code | `claude -p`, JSON; isolated HOME is viable. | `ANTHROPIC_BASE_URL`, Anthropic Messages protocol. | Possible lab spike via `/v1/messages`, but non-Claude gateway is unsupported vendor behavior. [Gateway](https://code.claude.com/docs/en/llm-gateway) |
| Codex | `codex exec`; isolated HOME/config viable. | Responses API/provider configuration. | Candidate `/v1/responses`; current [llama.cpp server](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md) exposes it, but tools/streaming need spike. |
| OpenCode | `opencode run`; config/home can be disposable. | OpenAI-compatible `/v1/chat/completions`. | Direct llama.cpp candidate and L2 ready. [Providers](https://opencode.ai/docs/providers) |
| Cursor | `cursor-agent -p` documented. | BYOK cloud providers documented; generic local URL not established. | Possible only after a vendor/empirical route spike. [Headless](https://docs.cursor.com/en/cli/headless) |
| Windsurf | No suitable headless harness CLI found. | No local provider contract found. | Contract-only; do not force IDE into Docker. |
| Gemini CLI | `gemini -p`, JSON/JSONL; isolated home/container viable. | `GOOGLE_GEMINI_BASE_URL` speaks Gemini API. | No official direct llama/OpenAI route; local Gemma is routing, not agent inference. [Configuration](https://github.com/google-gemini/gemini-cli/blob/main/docs/reference/configuration.md) |
| Copilot CLI | Programmatic/headless plus `COPILOT_HOME`. | BYOK supports local OpenAI Chat Completions and offline mode. | Direct llama.cpp candidate and L2 ready. [BYOK](https://docs.github.com/en/copilot/how-tos/copilot-cli/customize-copilot/use-byok-models) |
| Cline | Headless/JSON CLI with `--data-dir`. | OpenAI-compatible provider. | Candidate after Docker install/isolation spike. [CLI](https://docs.cline.bot/cli/cli-reference) |
| Roo Code | CLI/release surface insufficiently stable. | Compatible-provider docs are IDE-oriented. | Contract-only. |

`LiteLLM` remains test infrastructure only. It is not a product dependency. A
gateway is a fallback for an exact protocol mismatch, not assumed Compose
topology. Gemini would require a Gemini-protocol gateway, which is too much
unsupported machinery for first zero-vendor L2 evidence.

## Integration and Core impact

| UZE abstraction | Broader ecosystem result |
|---|---|
| `PackageId` / `StoredPackage` | Correct install-once ownership. Identity currently comes only from AP `plugin.json`; a future importer registry can derive identity from supported vendor manifests without creating a UZE manifest. |
| `Resource` / `Capability` | Correct for Skills/MCP; other existing kinds are placeholders, not a portability promise. Add discoverers only after semantic proof. |
| `EffectiveEnvironment` | Correct aggregate of package-origin resources. No change. |
| `PackageExposurePlan` | Correct anti-duplication boundary. Native mechanism should later be generalized as integration-owned, not be made vendor-aware in Core. |
| `CapabilityRouter` | Correctly remains capability-only. No import, lifecycle or vendor schema belongs here. |
| `IntegrationPort` | Correct owner of detect/plan/attach/inspect/detach. A new peer implements strategies and receipt inspection. |
| `AttachmentReceipt` / `ManagedArtifact` | Correct safety pattern. Native vendor extension install needs an additive typed receipt containing selector, source and enabled/installed evidence. |
| Application layer | Sufficient façade; only composition registration changes after an integration is approved. |

### Gaps that must remain explicit

1. Store intake is Agent-Plugin-only today; native Gemini/Cursor/Copilot/Cline
   envelopes need an additive recognized importer before native delivery.
2. MCP is stdio-focused. HTTP/SSE/OAuth/headers/trust need their own resource
   and receipt semantics before UZE claims them.
3. `AGENTS.md`, `GEMINI.md`, Rules and memories are not safely interchangeable.
4. Hooks, agents and commands remain deliberately unsupported portable
   capabilities.

## L2 recommendation

Use the lab first to test the **existing** product integrations, not to add an
integration and a lab simultaneously:

1. **OpenCode** first — existing integration, headless CLI and documented
   direct OpenAI-compatible local inference.
2. **Codex** second only after a small `/v1/responses` spike. It exercises the
   current native-package path.
3. **Claude Code** is optional experimental L2 after those two; L3 remains
   authoritative because Anthropic does not support local non-Claude upstreams.

**GitHub Copilot CLI** is the strongest next ecosystem expansion when a
Copilot integration is separately approved: it offers native plugin lifecycle,
headless execution, isolated state and an official local OpenAI-compatible
route. Gemini is a high-value future package adversary, not a first L2 target.

## Verdict

**UZE ARCHITECTURE GENERALIZES WITH MINOR EXTENSIONS.**

Adding any researched harness can stay at the intended boundary:

```text
new Integration + strategies + typed receipts + tests
```

and, only when its external envelope should be native-installed:

```text
recognized importer + integration-owned native package mechanism
```

No evidence calls for changing Store ownership, CapabilityRouter semantics,
Package model, EffectiveEnvironment or the application API.
