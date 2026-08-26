## Context

The Store owns canonical package bytes; the Engine composes those bytes into
vendor-neutral `Resource`s; integrations are the only vendor-aware layer; and
typed receipts plus inspect-before-detach protect user files. Hooks must follow
that path instead of becoming a side channel that writes arbitrary settings.

Current vendor research establishes the portable intersection:

| Semantic event | Claude | Codex | AGY | OpenCode |
| --- | --- | --- | --- | --- |
| before a tool | `PreToolUse` | `PreToolUse` | `PreToolUse` | `tool.execute.before` |
| after a tool | `PostToolUse` | `PostToolUse` | `PostToolUse` | `tool.execute.after` |
| agent stop | `Stop` | `Stop` | `Stop` | no equivalent (degraded/unsupported) |

Claude and Codex have command-hook JSON contracts. AGY has command hooks with
camelCase payloads and `allow`/`ask`/`deny`. OpenCode's plugin API supplies
mutable pre/post tool callbacks where a failure blocks the intercepted tool;
there is no declarative JSON hook file, so it needs generated source.

## Goals / Non-Goals

Goals: one authored `hooks.json`, command handlers with JSON stdin/stdout,
tool aliases plus explicit native names, ordered handler execution, explicit
compatibility diagnostics, generated artifacts that are rebuildable, and
receipt-safe merge/removal.

Non-goals: vendor prompt/agent/http/MCP hook handlers, arbitrary OpenCode SDK
plugins, lifecycle events without demonstrated equivalence, a TypeScript
compiler requirement, silently weakening a safety denial, or a new core
dependency on a harness.

## Canonical manifest and ABI

The root `hooks.json` schema is:

```json
{
  "hooks": {
    "PreToolUse": [{
      "id": "protect-env",
      "matcher": "shell|file.write|native:Write",
      "hooks": [{ "type": "command", "command": "${PLUGIN_ROOT}/scripts/check", "timeout": 10 }]
    }]
  }
}
```

`id` is optional and derived deterministically from event/group order when
absent. The initial canonical events are `PreToolUse`, `PostToolUse`, and
`Stop`. A matcher is a `|`-separated list of portable aliases or a
`native:<name>` escape hatch. Only `type: command` is canonical. Timeout is
seconds, bounded to 1..300 and defaults to 30.

Every adapter sends a normalized JSON object on stdin:

```json
{
  "version": 1,
  "event": "pre_tool_use",
  "tool": { "portable": "shell", "native": "Bash" },
  "input": {},
  "context": { "cwd": "/path", "session_id": "optional" }
}
```

stdout is either empty (observe/allow) or one JSON object:
`{"decision":"allow|ask|deny","reason":"...","input":{...}}`.
`input` is honored only where the target supports a safe pre-tool rewrite.
Invalid stdout, launch failure, non-zero exit except the canonical deny exit,
and timeout are fail-open for observational hooks but fail-closed for a
declared pre-tool `deny`/`ask` effect only when the target can enforce it;
otherwise the plan is `degraded` and attach requires an explicit diagnostic.
Adapters cap stdout at 64 KiB, preserve stderr for diagnostics, and inject
`PLUGIN_ROOT` as the canonical package root. Handlers are sequential in
manifest order; the first deny wins.

## IR and compatibility

`uze-core::hook` owns parsed `HookManifest`, `PortableHook`, `HookEvent`,
`HookMatcher`, `CommandHook`, `HookEffect`, `HookCompatibility`, and the alias
table. A Hook resource contains one serialized IR entry and a stable
resource-name identity. Core never maps to vendor tool names.

An integration exposes a Hook adapter capability declaration covering event,
effect, matcher translation, input transformation, ordering, and handler
type. It computes compatibility from all of those axes, yielding native,
adapted, degraded, or unsupported with a reason and produced artifacts.

## Delivery and ownership

- Claude: generate the plugin `hooks/hooks.json` form, retaining command,
  matcher, timeout, and `${PLUGIN_ROOT}` expansion.
- Codex: generate its current `hooks.json` command form with the supported
  events and command fields; use its native Hook source, not a Claude claim.
- AGY: generate named `hooks.json` entries, camelCase bridge command payload,
  translated native tool names, and native decisions.
- OpenCode: generate an owned `.opencode/plugins/uze-hooks-<package>.ts`
  bridge plus a managed config entry. The source embeds the normalized IR,
  uses `tool.execute.before`/`after`, invokes commands sequentially, maps
  denial to the documented tool error, and has no TypeScript compilation
  step. `Stop` is reported degraded because OpenCode exposes no stop hook.

Each generated artifact is identified by a receipt-owned selector/fingerprint.
Merges add only a UZE namespaced entry; inspect verifies that exact entry;
detach removes only a matching entry and then an empty UZE-created directory.
Foreign files, entries, order, and plugins are never changed.

## Risks / Trade-offs

- Vendor hook APIs evolve rapidly: keep source evidence and conformance
  fixtures per integration; never upgrade a route without observed evidence.
- A safety hook cannot be silently made observational: downgrade becomes a
  visible `degraded` or `unsupported` plan.
- OpenCode needs generated JavaScript/TypeScript: the bridge is small,
  deterministic, no-dependency source rather than a general plugin runtime.
- Windows command quoting differs: retain `commandWindows` only as a target
  adaptation and test generated paths/escaping.

## Verification

Unit-test parse/validation/aliases/IR/compatibility/emitters/bridge; test
merge, fingerprints, safe detach, paths with spaces, deny/ask/allow,
transforms, sequential multiple handlers, timeout and malformed output; then
run deterministic suite and all four real-harness conformance verticals.
