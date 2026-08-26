# Portable Hooks

One authored declaration, one command ABI, four harnesses (ADR-033).
A package ships a root `hooks.json` plus plain scripts; UZE projects that
into each harness's own hook mechanism and keeps every derived artifact
receipt-owned.

## Canonical manifest

`hooks.json` at the package root:

```json
{
  "hooks": {
    "PreToolUse": [{
      "id": "protect-env",
      "matcher": "shell|file.write|native:Write",
      "effect": "deny",
      "hooks": [{ "type": "command", "command": "${PLUGIN_ROOT}/scripts/check", "timeout": 10 }]
    }]
  }
}
```

- **Events**: the initial semantic events are `PreToolUse`, `PostToolUse`,
  and `Stop`. No other event is canonical.
- **Matcher**: `|`-separated portable tool aliases (`shell`, `file.read`,
  `file.write`, `file.edit`, `search.files`, `search.web`, `agent.spawn`,
  `agent.message`) or an explicit `native:<tool>` escape hatch. Omitting the
  matcher matches every tool.
- **Effect**: `observe` (default), `allow`, `ask`, `deny`, or `transform`.
  `transform` is only valid on `PreToolUse`.
- **Handlers**: only `type: command`. `timeout` is seconds, bounded to
  1..300, default 30. `command` may use the `${PLUGIN_ROOT}` placeholder;
  UZE expands it and also injects `PLUGIN_ROOT` as an environment variable.
- **`id`** is optional; absent ids are derived deterministically from
  event and group order (`pre_tool_use-0`, `stop-1`, …).

Malformed, duplicate, or unsafe declarations are rejected before any
attachment; nothing is projected silently.

> **Commands are shell command lines.** A handler's `command` is executed
> through the system shell exactly as a user would type it — `${PLUGIN_ROOT}
> /scripts/check` therefore requires the script to be executable, or the
> command must say so explicitly (`sh ${PLUGIN_ROOT}/scripts/check`). A
> non-executable script fails the handler and follows the declared effect's
> fail-open/fail-closed rule — a `deny` hook that cannot run denies.

## Command ABI

Every handler receives one normalized JSON object on stdin:

```json
{
  "version": 1,
  "event": "pre_tool_use",
  "tool": { "portable": "shell", "native": "Bash" },
  "input": {},
  "context": { "cwd": "/path", "session_id": null }
}
```

stdout is either empty (observe/allow) or one JSON object:

```json
{ "decision": "allow|ask|deny", "reason": "...", "input": {} }
```

`input` is honored only where the target supports a safe pre-tool rewrite
(OpenCode's bridge today). Rules that hold on every harness:

- Handlers run sequentially in manifest order; the first deny stops the
  rest and blocks the intercepted tool where the target supports blocking.
- stdout is capped at 64 KiB; stderr is preserved for diagnostics.
- Exit `0` + valid JSON is a decision; exit `3` is a canonical hard deny;
  any other non-zero exit, a timeout, or malformed output is **fail-open**
  for `observe`/`allow` hooks and **fail-closed** (a deny) for declared
  `deny`/`ask`/`transform` effects.
- The runtime wrapper is the `uze` binary's internal `hook-exec` command,
  emitted into managed hook configuration with an absolute executable path
  and per-handler timeouts.

## Delivery per harness

| Harness | Mechanism | Route |
|---|---|---|
| Claude Code | merged `hooks` entries in `~/.claude/settings.json` (plugin `hooks/hooks.json` group form) | native |
| Codex | merged entries in `~/.codex/hooks.json` | native |
| Antigravity CLI | named-entry `hooks.json` inside the UZE-generated native plugin | native (package-level) |
| OpenCode | owned, regenerable `.opencode/plugins/uze-hooks-<package>.ts` bridge + one `plugin` entry in `opencode.json` | adapted (bridge) |

Compatibility is semantic, per event and effect. A `Stop` hook is never
represented as a tool callback: on OpenCode it is Degraded with the reason
stated, and it is not attached. An `ask` effect is Unsupported where the
target cannot preserve a real ask (Claude today; OpenCode, whose thrown
error is a hard denial). A `transform` is only attached where the target
preserves a safe input rewrite (OpenCode's bridge).

## Lifecycle safety

- Every projection is receipt-owned (`HookConfigEntry`); inspection compares
  the exact managed content, removal refuses drift/conflicts, and foreign
  hooks, plugins, entries, and ordering are never changed.
- All generated artifacts are derived: safe to delete and regenerate from
  the Store (`uze plugin install` / `uze plugin update` rebuilds them).
- `uze plugin inspect` lists hooks as resources with their per-harness
  delivery; `doctor` reports attachment health; the TUI harness matrix
  shows the per-harness verdict.

## Known limitations

- Vendor payload/decision contracts are implemented as documented mappings
  and proven by the Conformance Lab against the real harness binaries; a
  claim of native behavior on a specific version requires the lab's
  observed evidence.
- `tool.execute.before` does not cover subagent-issued tool calls
  ([sst/opencode#5894](https://github.com/sst/opencode/issues/5894)).
- Windows command quoting uses the POSIX wrapper form; `cmd`-specific
  quoting is a future target adaptation.