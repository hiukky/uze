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

Goals: one authored `hooks.json`, command handlers with an environment/exit-code
contract,
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

Every delivery hands the handler the same normalized context as environment
(the `HOOK_*` set: `HOOK_HARNESS`, `HOOK_EVENT`, `HOOK_TOOL`,
`HOOK_TOOL_NATIVE`, `HOOK_CWD`, `HOOK_INPUT`, the matched alias's portable
fields such as `HOOK_COMMAND`/`HOOK_PATH`, and `PLUGIN_ROOT` as the canonical
package root). Nothing arrives on stdin and nothing is parsed from stdout —
`native-first-hooks` replaced that ABI, because the harness payload is read
and the harness's decision document is written by the delivered wrapper, not
by the handler.

The decision is the handler's exit code: `0` allows, `3` denies with the
reason on stderr, and anything else — a launch failure, a timeout, any other
status — is a handler failure. A failure is fail-open for `observe`/`allow`
and fail-closed for `deny`/`ask`/`transform`, and a target that cannot
enforce the declared effect yields a `degraded` plan whose attach requires an
explicit diagnostic. The reason read back from stderr is bounded, each
handler is bounded by its own declared timeout, handlers run sequentially in
manifest order, and the first deny wins.

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
