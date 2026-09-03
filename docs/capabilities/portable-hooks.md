# Portable Hooks

One authored declaration, one handler contract, four harnesses (ADR-033,
ADR-040). A package ships a root `hooks.json` plus plain scripts; UZE
compiles that, at install time, into each harness's own hook form plus a
small wrapper vendored inside the delivered artifact. **Nothing UZE puts on
the execution path is UZE**: a delivered hook keeps working after the `uze`
binary is removed.

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

- **Events**: the semantic events are `PreToolUse`, `PostToolUse`, and
  `Stop`. No other event is canonical.
- **Matcher**: `|`-separated portable tool aliases or an explicit
  `native:<tool>` escape hatch. Omitting the matcher matches every tool.
- **Effect**: `observe` (default), `allow`, `ask`, `deny`, or `transform`.
  `transform` is only valid on `PreToolUse`, and is not deliverable today
  (see [Known limitations](#known-limitations)).
- **Handlers**: only `type: command`. `timeout` is seconds, bounded to
  1..300, default 30. `command` may use the `${PLUGIN_ROOT}` placeholder;
  UZE resolves it at generation time and also exports `PLUGIN_ROOT`.
- **`id`** is optional; absent ids are derived deterministically from
  event and group order (`pre_tool_use-0`, `stop-1`, …).

Malformed, duplicate, or unsafe declarations are rejected before any
attachment; nothing is projected silently.

> **Commands are shell command lines.** A handler's `command` is executed
> as a user would type it — `${PLUGIN_ROOT}/scripts/check` therefore
> requires the script to be executable, or the command must say so
> (`sh ${PLUGIN_ROOT}/scripts/check`). A non-executable script fails the
> handler and follows the declared effect's fail-open/fail-closed rule — a
> `deny` hook that cannot run denies.

## Handler contract

A handler reads the hook context from its environment and answers with its
exit code. It never parses a harness payload and never writes harness JSON.

| Variable | Meaning |
|---|---|
| `HOOK_HARNESS` | the delivering harness's id (`claude`, `codex`, `antigravity`, `opencode`) |
| `HOOK_EVENT` | `pre_tool_use` \| `post_tool_use` \| `stop` |
| `HOOK_TOOL` | the portable alias that matched; empty for a tool the vocabulary does not bind |
| `HOOK_TOOL_NATIVE` | the harness's own tool name (`Bash`, `exec_command`, `run_command`, `bash`) |
| `HOOK_CWD` | the workspace directory; may be empty |
| `HOOK_INPUT` | the tool input, as JSON, for anything the alias does not name |
| `PLUGIN_ROOT` | the package root the handler was delivered from |
| `HOOK_<FIELD>` | one per portable field of the matched alias — see the vocabulary below |

| Exit code | Meaning |
|---|---|
| `0` | allow — the next handler of the group runs |
| `3` | deny — the reason is read from stderr, and no later handler runs |
| anything else, a failure to start, or a timeout | a handler failure, resolved by the group's effect |

A handler failure is **fail-open** for `observe`/`allow` (the tool proceeds
and the failure is reported) and **fail-closed** for `deny`/`ask` (the tool
is denied and the reason names the failure). A safety hook that cannot be
evaluated is never weakened into a no-op.

```sh
#!/bin/sh
# The whole contract: read a variable, choose an exit code.
case "$HOOK_COMMAND" in
  *.env*|*id_rsa*)
    echo "blocked: $HOOK_COMMAND touches a secret file" >&2
    exit 3 ;;
esac
exit 0
```

Handlers of one group run **sequentially in manifest order** and the first
denial stops the rest — on every harness, whatever order the harness itself
would have used.

## Portable tool vocabulary

Each alias names the portable fields it guarantees; each harness names the
tool it matches and the native input field every portable field is read
from. This one table drives the matchers, the generated wrappers and the
compatibility verdicts.

| Alias | Fields | Claude Code | Codex | Antigravity CLI | OpenCode |
|---|---|---|---|---|---|
| `shell` | `HOOK_COMMAND` | `Bash` / `command` | `exec_command` / `cmd` | `run_command` / `CommandLine` | `bash` / `command` |
| `file.read` | `HOOK_PATH` | `Read` / `file_path` | `Read` / `file_path` | `view_file` / `AbsolutePath` | `read` / `filePath` |
| `file.write` | `HOOK_PATH` | `Write` / `file_path` | `Write` / `file_path` | `write_to_file` / `TargetFile` | `write` / `filePath` |
| `file.edit` | `HOOK_PATH` | `MultiEdit`, `Edit` / `file_path` | `Edit` / `file_path` | `replace_file_content` / `TargetFile` | `edit` / `filePath` |
| `search.files` | `HOOK_QUERY` | `Grep` / `pattern` | `Grep` / `pattern` | `grep_search` / `Query` | `grep` / `pattern` |
| `search.web` | `HOOK_QUERY` | `WebSearch` / `query` | `WebSearch` / `query` | `search_web` / `query` | `web_search` / `query` |
| `agent.spawn` | — | `Task` | — | — | `task` |
| `agent.message` | — | — | — | — | — |

Antigravity's and Codex's names come from the schemas those harnesses
declare to the model, captured with the Lab's `--discovery` mode. A
`native:<tool>` matcher bypasses the table entirely: the handler receives
`HOOK_TOOL_NATIVE` and `HOOK_INPUT`, with `HOOK_TOOL` empty.

## Delivery per harness

| Harness | Delivered artifact | Route |
|---|---|---|
| Claude Code | one merged entry per group in `~/.claude/settings.json`, `command` = the generated `hooks/exec` with the group's arguments (exec form: no shell parsing) | native |
| Codex | one merged entry per group in `~/.codex/hooks.json`, one quoted shell line invoking the same wrapper | native |
| Antigravity CLI | one named entry per group merged into the shared `~/.gemini/config/hooks.json` (the document root *is* the named-hook map), keyed `<package>:<group-id>` — grouped (`matcher` + `hooks`) for the tool events, a flat handler list for `Stop` — whose command is the shared `hooks/exec` wrapper by absolute path | native (shared vendor file); execution needs a signed-in session — an API-key session runs no hook at all (#893). The Lab measures both the vendor's execution gate (`hooks > vendor`) and whether the harness loads what UZE delivered (`hooks > delivery`) live, every run |
| OpenCode | one generated `Plugin.define` plugin, `<config root>/plugins/hooks-<package>.ts`, auto-discovered — the plugin *is* the wrapper, with the package's groups as data | adapted (`observe`/`allow` only; `deny`/`ask` unsupported, `Stop` never claimed) |

The `sh` wrapper is one file per harness, byte-identical for every package,
and depends on `sh` and `jq`. Claude, Codex and Antigravity each keep one
copy under `$UZE_HOME/state/attachments/<harness>/hooks/exec` — a shared
vendor config file has no plugin root to resolve against, so every entry
names the wrapper by absolute path.

Compatibility is semantic, per event and effect. A `Stop` hook is never
represented as a tool callback: on OpenCode it is Degraded with the reason
stated, and it is not attached. `deny`/`ask` are Unsupported on OpenCode V2
— its tool hooks see the input but cannot block, and its only decision point
(`permission.evaluate`) carries the action's resources rather than the tool
input — so they are never fabricated.

## Compatibility matrix

Legend: **native** = the harness's own mechanism with the canonical
semantics preserved · **adapted** = delivered, semantics degraded (reason
stated) · **—** = not expressible.

| | Claude Code | Codex | Antigravity CLI | OpenCode V2 |
|---|---|---|---|---|
| wrapper runtime | `sh` + `jq` | `sh` + `jq` | `sh` + `jq` | Bun (embedded) |
| plugin root | absolute path | absolute path | absolute path (cwd is the `hooks.json` directory) | `import.meta.url` |
| exec form (no shell parsing) | yes (`command` + `args`) | no (shell line) | no (shell line) | n/a |
| matcher | native, regex on the tool name | native | native, regex (`"*"` matches all) | in-plugin |
| a group's handlers | run **in parallel** natively → sequential inside `exec` | sequential inside `exec` | sequential inside `exec` | sequential inside the plugin |
| `PreToolUse` observe/allow | native | native | native (signed-in session) | native (`execute.before`) |
| `PreToolUse` deny | native (JSON + exit 2) | native (JSON + exit 2) | native (`decision: deny`, signed-in session) | — |
| `PreToolUse` ask | native (`permissionDecision: ask`) | rendered as a denial | native (signed-in session) | — |
| `PreToolUse` transform | — (needs a stdout convention) | — | — | — |
| `PostToolUse` | native, observe only | native, observe only | native (`{}`), signed-in session | native (`execute.after`) |
| `Stop` | native (exit 2 prevents the stop) | native (must print `{}`) | native, signed-in session | — |
| a denial's exit status | 2 (the documented block signal) | 2 | **0** — the decision is the stdout document, and any non-zero exit is logged as a *failed* hook | n/a |
| fail-closed on handler failure | via `exec` (natively, exit ≠ 2 **runs the tool**) | via `exec` | via `exec` | via the plugin |
| handler context | `HOOK_*` environment | `HOOK_*` environment | `HOOK_*` environment | `HOOK_*` environment |

## Lifecycle safety

- Every projection is receipt-owned: config entries by exact content
  identity, the generated wrapper by comparison against the template UZE
  writes, the OpenCode plugin as a whole owned file. Inspection compares
  the exact managed content, removal refuses drift, and foreign hooks,
  plugins, files, entries and ordering are never changed.
- All generated artifacts are derived: safe to delete and regenerate from
  the Store (`uze plugin install` / `uze plugin update` rebuilds them). The
  shared wrapper is removed with the last entry that needs it.
- Existing installs are re-projected on the next install/update: nothing is
  migrated in place, and a receipt-owned entry from a previous release is
  replaced, never duplicated.
- `uze plugin inspect` lists hooks with their per-harness delivery;
  `uze doctor` reports attachment health, the route each hook took, and a
  delivered wrapper whose `jq` is missing; the TUI harness matrix shows the
  per-harness verdict.

## Known limitations

- **`transform` is not deliverable.** Rewriting the tool input needs a
  channel for the handler to answer on, which an exit code is not. A
  `transform` group degrades on every harness — stated, never silently
  attached — until its own change defines that channel.
- **`jq` is the shell wrapper's dependency.** It is not declarable by a
  package yet (plugin `requirements` is its own change); `uze doctor`
  reports it missing, and until it is installed a `deny` group denies while
  an `observe` group proceeds and reports.
- **Windows has no wrapper template.** A PowerShell wrapper is future work;
  until then hooks there take the packager-runtime fallback route, which
  speaks the same contract but keeps working only while `uze` is installed.
- **Antigravity ran delivered hooks only in a signed-in session through
  1.1.24.** Its hook entries load and list correctly in either mode
  (`hooks_manager: loaded N named hooks`), but the executor reads
  `enable_json_hooks` — field 17 of the model backend's
  `CustomizationConfig`, switched server-side by the `json-hooks-enabled`
  feature flag. Through 1.1.24 that config reached the CLI only over the
  CloudCode backend it speaks when signed in to a Google account; a session
  running on `GEMINI_API_KEY` never received it and ran no hook at all —
  not UZE's, not the vendor's own format at the vendor's own path. That was
  the vendor's own bug,
  [antigravity-cli#893](https://github.com/google-antigravity/antigravity-cli/issues/893),
  with #78 recording that the API-key path is unsupported. On 1.1.25 the
  Lab measured the same vendor-format deny hook firing under the API key.
  **If your hooks are silent on an older build, check how the CLI is
  authenticated before suspecting delivery.** The Lab's Antigravity
  vertical runs signed in (a synthetic identity against its own CloudCode
  plane) and proves the gate open there, and asserts the same control hook
  on the API-key mode every run, so a regression shows as a red check.
- **Antigravity does not load a plugin's `hooks.json` (1.1.24)**, which is
  why UZE delivers its hooks into the shared `~/.gemini/config/hooks.json`
  instead. The harness reads `hooks.json` from its shared customization
  roots but never from a plugin directory: `agy plugin validate` counts a
  plugin's hooks, the plugin is listed with a `hooks` component and enabled
  in `config.json`, and the session still reports `loaded 0 named hooks from
  0 hooks.json file(s)` — the file is never opened. The vendor's own shipped
  plugin guide documents the opposite ("Hooks defined in
  `plugins/<name>/hooks.json` are registered and run during the agent's
  lifecycle"). The Lab measures it live each run (`hooks > delivery`), so if
  a later build starts reading plugin hooks the delivery can move back.
- **OpenCode V2 cannot block.** Its tool hooks carry the input but no block
  signal; `deny`/`ask` are diagnosed before attach.
- **Antigravity's two entry shapes are not interchangeable.** Its docs
  give the tool events a `matcher` and a `hooks` group, and `Stop` a flat
  list of handler objects. A `Stop` written in the grouped form is parsed
  as invalid and dropped in silence — `agy plugin validate` reports
  nothing, and only `--log-file` names the reason
  ([antigravity-cli#925](https://github.com/google-antigravity/antigravity-cli/issues/925),
  1.1.24). UZE emits each event in its own shape.
- **Codex requires the `[features].hooks` flag** in `~/.codex/config.toml`
  (verified against codex-cli 0.150.0).
- **`tool.execute.before` does not cover subagent-issued tool calls** on
  OpenCode ([sst/opencode#5894](https://github.com/sst/opencode/issues/5894)).
