# Hooks — deep research

Companion to [landscape.md](landscape.md). This document carries Parts 3–8 of
the M3 research brief: the hook format landscape, the semantic matrix, the
portability classification, the (rejected-for-now) semantic model, the
format-vs-semantics question, and the generated-code/trust assessment.

Research date 2026-08-21. Sources are cited inline; anything not directly
verified against official docs or current source is marked **UNVERIFIED** or
**UNKNOWN** and must not be treated as a portability claim.

## Part 3 — Hook format landscape

| | Claude Code | Codex | OpenCode | Gemini CLI |
|---|---|---|---|---|
| **Declared where** | `~/.claude/settings.json` (user), `.claude/settings.json` (project, shareable), `.claude/settings.local.json` (project, personal), managed/org policy settings, plugin `hooks/hooks.json`, Skill/Subagent YAML frontmatter | `~/.codex/hooks.json` or `[hooks]` in `~/.codex/config.toml` (user); `.codex/hooks.json`/`.codex/config.toml` (project); plugin-bundled `hooks/hooks.json`; `requirements.toml` (admin/managed) | `.opencode/plugins/` (project), `~/.config/opencode/plugins/` (global), or an npm package in `opencode.json`'s `"plugin"` array | `~/.gemini/settings.json` (user), `.gemini/settings.json` (project), extension-bundled `hooks/hooks.json` inside `gemini-extension.json` |
| **Lives in a distributable package?** | Yes — plugin root `hooks/hooks.json` | Yes — plugin-bundled `hooks/hooks.json` (structurally confirmed in source) | Yes, but as **executable code**, not declarative config — the plugin file *is* the hook | Yes — extension `hooks/hooks.json` |
| **Format** | Declarative JSON | Declarative TOML or JSON | Executable TypeScript/JavaScript module | Declarative JSON |
| **Handler kinds** | `command` (shell/exec), `http` (POST), `mcp_tool`, `prompt` (LLM eval), `agent` (experimental) | `Command` (`command`, `command_windows`, `timeout_sec`, `async`, `additional_context_limit`), `Prompt`, `Agent` — source-confirmed struct names | A JS callback function receiving a context object (`{project, client, $, directory, worktree}`) | `type: "command"` — shell command only; no other handler kind documented |
| **Lifecycle events** | ~35 named events (session/turn/tool/subagent/context/worktree/elicitation/display categories) | `pre_tool_use`, `post_tool_use`, `permission_request`, `session_start`, `session_end`, `user_prompt_submit`, `stop`, `compact` (pre/post via matcher) — confirmed by source file listing. `SubagentStart`/`SubagentStop` claimed by secondary sources, **not found in the source `events/` directory** — treat as unconfirmed | Two parallel-looking APIs: a "hooks object" (`tool.execute.before/after`, `chat.message`, `chat.params`, `permission.ask`, `stop`, `experimental.*`) and a generic `event({event})` bus covering ~20 observational event types (session/message/file/lsp/etc.) | `SessionStart`, `SessionEnd`, `BeforeAgent`, `AfterAgent`, `BeforeTool`, `AfterTool`, `BeforeModel`, `BeforeToolSelection`, `AfterModel`, `PreCompress`, `Notification` |
| **Matching/filtering** | `matcher` field: regex/pipe-alternation on tool name for tool events; fixed vocabulary for others (e.g. `startup|resume|clear`) | `MatcherGroup` (regex on tool/session-source/compaction-trigger) — source-confirmed type, exact matching semantics unverified | In-code `if` conditions inside the handler — no declarative matcher | `matcher`: regex on tool name for `BeforeTool`/`AfterTool`; event-name-only for the rest |
| **Command execution model** | exec form (`command`+`args`, no shell) or shell form (shell-interpreted); path placeholders `${CLAUDE_PROJECT_DIR}`, `${CLAUDE_PLUGIN_ROOT}` | Shell command with OS-specific override (`command_windows`) | N/A — runs in-process; has full `$` (Bun shell) access from the host process, not a spawned/sandboxed one | Shell command; `type: "command"` only |
| **Environment** | Inherits host process env + `CLAUDE_PROJECT_DIR`, `CLAUDE_PLUGIN_ROOT`/`CLAUDE_PLUGIN_DATA` (plugin hooks), `CLAUDE_CODE_REMOTE`, `CLAUDE_EFFORT`; `OTEL_*` stripped | **UNKNOWN** — not source-verified this pass | N/A (in-process; full host access) | **UNKNOWN** — not found in fetched docs |
| **stdin/stdout protocol** | One JSON object on stdin (`session_id`, `hook_event_name`, event-specific fields); JSON out with `continue`, `additionalContext`, `updatedPrompt`, `updatedInput`, `hookSpecificOutput.permissionDecision`; output capped 10,000 chars | **UNVERIFIED** — the only fetched summary describing an exact schema turned out to blend in unrelated Claude-Code-plugin material (see Codex research note below); structural existence of a schema (`schema.rs`, `output_parser.rs`) is source-confirmed, exact fields are not | N/A — direct JS function arguments/return, no serialization boundary | JSON on stdin (`session_id`, `transcript_path`, `cwd`, `hook_event_name`, `timestamp` + event fields); JSON out, parsed only on exit 0; stderr = rejection reason on block |
| **Exit code semantics** | `0` success; `2` blocking error (stderr = reason shown to the model); other = non-blocking warning; timeout = non-blocking cancellation | **UNVERIFIED** (plausibly similar — unconfirmed) | No exit codes (in-process) — block = throw an `Error` from the handler; a thrown error aborts the call | `0` = apply JSON output; `2` = "System Block", stderr = rejection reason; other = non-fatal warning |
| **Timeout** | Default 600s (`command`/`http`/`mcp_tool`), 30s (`prompt`), 60s (`agent`); per-event overrides exist; configurable | `Command.timeout_sec` field confirmed in source; default value unverified | **Not documented anywhere found** — no evidence of an execution deadline; a hanging hook likely hangs the host process | Per-hook `timeout` field, default 60000ms |
| **Async/sync** | Sync by default; `async: true` backgrounds it; `asyncRewake: true` wakes the model on a late blocking result; matching hooks for one event run in parallel | `Command.async` field confirmed in source | All hooks are async JS functions (`Promise`-based) by nature of the runtime | Sync for tool/agent/model events (blocking); `SessionEnd`/`PreCompress` explicitly async/best-effort |
| **Can block/veto** | Yes, on a documented subset of events (table below) | Claimed for `pre_tool_use`/`permission_request`/`stop` — **UNVERIFIED** | Yes via throwing, for `tool.execute.before`/`stop` — but **`permission.ask` is defined and not currently triggered** ([issue #7006](https://github.com/anomalyco/opencode/issues/7006)), and `tool.execute.before` **does not cover subagent-issued tool calls** ([issue #5894](https://github.com/sst/opencode/issues/5894)) | Yes, via `decision: "deny"` on `BeforeAgent`/`AfterAgent`/`BeforeTool`/`AfterTool` |
| **Can mutate input** | Yes — `PreToolUse` only, via `updatedInput` | Claimed for `pre_tool_use` — **UNVERIFIED** | Yes — `tool.execute.before` can rewrite `output.args` directly | Yes — `BeforeTool` via `hookSpecificOutput.tool_input`; `BeforeModel`/`BeforeToolSelection` can rewrite the LLM request itself |
| **Can mutate output / add context** | Yes on many events via `additionalContext` | Plausible (`additional_context_limit` field exists in source) — **UNVERIFIED shape** | Yes — `experimental.chat.system.transform`, `experimental.session.compacting` can push into system/context | Yes — widely, incl. `AfterModel` rewriting streamed response chunks, `AfterAgent` clearing context entirely |
| **Permissions/trust model** | Trust granted at plugin-install / settings-file granularity; managed/org hooks cannot be disabled by lower scopes; subagent-frontmatter hooks require a one-time workspace-trust dialog; no per-hook consent step | Dedicated TUI hook-review surfaces exist in source (`startup_hooks_review.rs`, `hooks_browser_view.rs`); `allow_managed_hooks_only` confirmed in docs; exact per-hook trust/hash flow **UNVERIFIED** | **No separate trust gate found.** Loading a plugin (local file or npm-declared, auto-installed via Bun at startup) grants full host access — filesystem, SDK client, and shell (`$`) — with no intermediate "installed but not yet trusted" state | Explicit doc warning ("Hooks execute arbitrary code with your user privileges"); **content-fingerprint re-trust**: a changed project-level hook command triggers a fresh warning even if the name is unchanged; extension install has a `--consent` flag, relationship to hook-specific consent unconfirmed |
| **Inspect/list/remove/update surface** | `/hooks` (read-only browser: event, matcher, type, source); debug log; `disableAllHooks: true`; removing a plugin removes its hooks | Dedicated hook-browser TUI exists (source-confirmed); exact CLI commands unverified | **No CLI found** — purely file/config-driven; no `opencode plugin list`-equivalent for hooks specifically | `gemini extensions` CLI family; `hooksConfig.enabled`/`hooksConfig.disabled` (array of names) |

**Format is confirmed secondary to semantics here, exactly as the brief warned
against conflating.** Claude Code (JSON) and Gemini CLI (JSON) share a format
*and*, on inspection, a strikingly similar semantic shape (subprocess,
stdin/stdout JSON, `decision`-style block, field-level mutation, matcher
regex, timeout, async). Codex uses a different format (TOML) but is
*structurally* the same family (subprocess, matcher, timeout, async fields
all exist) — its behavioral semantics are simply unverified, not different.
OpenCode is the one genuine outlier, and the reason is not its format
(TS/JS vs JSON) — it is that there is **no subprocess boundary at all**: no
stdin/stdout contract to normalize, because there is no process to hand a
contract to. That is a semantic difference, not a syntactic one.

## Part 4 — Hook semantic matrix

Normalized event names for analysis only (not proposed as Core vocabulary).

### SessionStart / SessionEnd

| Property | Claude Code | Codex | OpenCode | Gemini CLI |
|---|---|---|---|---|
| Event exists | Yes (`SessionStart`, `SessionEnd`) | Yes (`session_start`, `session_end`) | Yes (`event` bus: `session.created`, `session.idle`, `session.deleted`, etc. — observational only) | Yes (`SessionStart`, `SessionEnd`) |
| Observe | Yes | Yes | Yes | Yes |
| Block | No | UNVERIFIED, plausibly no (n/a for a start event) | No | No |
| Mutate input | n/a | n/a | n/a | n/a |
| Add context | Yes (`additionalContext`) | UNVERIFIED | No | Yes (`additionalContext`, `SessionStart` only) |
| Structured payload | Yes | Likely, UNVERIFIED shape | Yes (`event.type` union) | Yes (`source`: startup/resume/clear) |
| Matcher/filter | `startup\|resume\|clear\|compact\|fork` | Matcher group exists, values UNVERIFIED | None (event-type switch in code) | `startup\|resume\|clear` |
| Async | Yes | `Command.async` field exists | Yes (JS Promise) | `SessionEnd` explicitly async/best-effort |

### BeforeTool / PreToolUse

| Property | Claude Code | Codex | OpenCode | Gemini CLI |
|---|---|---|---|---|
| Event exists | Yes (`PreToolUse`) | Yes (`pre_tool_use`) | Yes (`tool.execute.before`) | Yes (`BeforeTool`) |
| Observe | Yes | Yes | Yes | Yes |
| Block | **Yes**, exit 2 | Claimed, **UNVERIFIED** | **Yes**, throw — but does **not** cover subagent-issued calls (documented bug) | **Yes**, `decision: "deny"` |
| Mutate input | **Yes**, `updatedInput` replaces/merges tool args | Claimed, **UNVERIFIED** | **Yes**, direct mutation of `output.args` | **Yes**, `hookSpecificOutput.tool_input` merges/overrides |
| Add context | Yes, `additionalContext` | UNVERIFIED | No (this event) | Not on this event (use `AfterTool`) |
| Structured payload | Yes (`tool_name`, `tool_input`) | Likely, shape UNVERIFIED | Yes (`input: {tool, args}`) | Yes (`tool_name`, `tool_input`, `mcp_context`) |
| Matcher/filter | Regex on tool name | Matcher group on tool name (type-confirmed) | In-code only | Regex on tool name |
| Exit-code semantics | 0/2/other | UNVERIFIED | throw = block; no other code | 0/2/other |
| Async | Yes | Yes (field exists) | Yes | Sync (blocking by design) |
| Timeout | Configurable, default 600s | Field exists, default UNVERIFIED | **None documented** | 60s default |

### AfterTool / PostToolUse

| Property | Claude Code | Codex | OpenCode | Gemini CLI |
|---|---|---|---|---|
| Event exists | Yes (`PostToolUse`, `PostToolUseFailure`) | Yes (`post_tool_use`) | Yes (`tool.execute.after`) | Yes (`AfterTool`) |
| Observe | Yes | Yes | Yes | Yes |
| Block | No (already happened) | n/a | No | **Yes** — `decision: "deny"` can hide/replace the real result before the model sees it (Gemini treats "after" as still interceptable) |
| Mutate input | n/a | n/a | n/a | n/a |
| Add context | Yes | UNVERIFIED | No (community docs show read-only use here) | Yes — `additionalContext`, `tailToolCallRequest` |
| Structured payload | Yes | Likely | Yes | Yes (`tool_response`: llmContent/returnDisplay/error) |
| Async | Yes | Yes (field exists) | Yes | Sync |

**Note the asymmetry**: Claude Code treats `PostToolUse` as strictly
observational (the action already happened); Gemini CLI's `AfterTool` can
still veto/replace the result before the model consumes it. Same event name
("after the tool ran"), materially different semantic — exactly the kind of
divergence the brief asked not to paper over.

### BeforePrompt / UserPromptSubmit

| Property | Claude Code | Codex | OpenCode | Gemini CLI |
|---|---|---|---|---|
| Event exists | Yes (`UserPromptSubmit`) | Yes (`user_prompt_submit`) | Partial — `chat.message`/`chat.params` hooks observe/adjust, no dedicated named event | Yes (`BeforeAgent`) |
| Block | **Yes** | Claimed, UNVERIFIED | UNVERIFIED | **Yes**, `decision: "deny"` |
| Mutate input | **Yes**, `updatedPrompt` | UNVERIFIED | Likely via `chat.params` (UNVERIFIED shape) | No direct prompt rewrite found on this event (context injection only) |
| Add context | Yes | UNVERIFIED | Yes, `experimental.chat.system.transform` | Yes, `additionalContext` appended to prompt |

### PermissionDecision

| Property | Claude Code | Codex | OpenCode | Gemini CLI |
|---|---|---|---|---|
| Event exists | Yes (`PermissionRequest`, `PermissionDenied`) | Yes (`permission_request`) | Yes (`permission.ask`), **but not currently triggered by the runtime** ([#7006](https://github.com/anomalyco/opencode/issues/7006)) | Folded into `BeforeTool`'s `decision` field; separate `Notification` event of `type: "ToolPermission"` for observability |
| Block | Via JSON `decision`, not exit code | Plausible, UNVERIFIED | **Broken today** — defined but inert | Via `BeforeTool`/`BeforeAgent` `decision: "deny"` |

### Stop / TurnEnd

| Property | Claude Code | Codex | OpenCode | Gemini CLI |
|---|---|---|---|---|
| Event exists | Yes (`Stop`, `StopFailure`, `SubagentStop`) | Yes (`stop`) | Yes (`stop(input)`) | No direct equivalent found (`AfterAgent` is the closest — turn-level, not stop-specific) |
| Block | **Yes** — can prevent stopping | Claimed, UNVERIFIED | **Yes**, throw | n/a (no equivalent event) |

### Agent/Subagent start/end

| Property | Claude Code | Codex | OpenCode | Gemini CLI |
|---|---|---|---|---|
| Event exists | Yes (`SubagentStart`, `SubagentStop`) | **Claimed by secondary sources only — not found in source `events/` directory listing; treat as unconfirmed** | No dedicated event found (subagents are a separate concept from plugin hooks in OpenCode's docs) | No dedicated hook event found (subagent lifecycle is a separate, non-hook-gated concept per [agents.md](agents.md)) |

### File change

| Property | Claude Code | Codex | OpenCode | Gemini CLI |
|---|---|---|---|---|
| Event exists | Yes (`FileChanged`) | Not found | Yes (`file.edited`, `file.watcher.updated` on the `event` bus) | Not found as a hook event |

## Part 5 — Portability test

For each intersection found, whether a package requesting this semantic can
be realized by both harnesses without loss:

| Semantic requirement | Claude ↔ Gemini | Claude ↔ Codex | Claude ↔ OpenCode |
|---|---|---|---|
| `BeforeTool` + observe only | **LOSSLESS** — both are subprocess/JSON, matcher on tool name, no block needed | **UNKNOWN** — structurally plausible, behavior unverified | **LOSSY** — OpenCode's equivalent is in-process JS; delivering "observe only" without executing arbitrary generated code means, at best, a thin fixed-shape bridge (Part 7), not the package's original logic running unmodified |
| `BeforeTool` + block (veto) | **LOSSLESS**, pending a real conformance spike on merge semantics | **UNKNOWN** | **LOSSY at best** — the intended veto path (`permission.ask`) is documented broken; the working substitute (`tool.execute.before` throw) has a documented subagent-call bypass, so a package that assumed complete coverage would silently lose protection for subagent-issued calls |
| `BeforeTool` + mutate input | **LOSSY-to-LOSSLESS, unresolved** — both support it, but Claude's `updatedInput` (replace) vs. Gemini's `hookSpecificOutput.tool_input` (merge) semantics need a real spike before calling this lossless | **UNKNOWN** | **IMPOSSIBLE without generated code** — no declarative path exists at all |
| `AfterTool` + add context | **LOSSLESS** | **UNKNOWN** | **LOSSY** — same executable-code gap as above |
| `Stop` + block | **IMPOSSIBLE on the Gemini side** — no equivalent event found | **UNKNOWN** | **LOSSLESS in principle** (OpenCode has `stop(input)` + throw), but still requires generated code to deliver without running the package's own JS |
| Any hook, package-native delivery | **LOSSLESS** (both have `hooks/hooks.json` in their package format) | **UNKNOWN whether Codex's plugin format currently accepts third-party-authored hook bundles the same way** | **IMPOSSIBLE as declarative delivery** — OpenCode packages a hook *as code*, so "package-native delivery" for OpenCode already means "ship the JS," which is a different distribution problem than the JSON-config case |

## Part 6 — Possible semantic model

Illustrative only, evaluated against the evidence above, per field:

```
HookIntent {
    event
    requirements {
        observe
        block
        mutate_input
        mutate_output
    }
    action
}
```

| Field | Evidence from ≥2 harnesses | Who can represent it | Loss if removed |
|---|---|---|---|
| `event` | Yes — Claude, Gemini, (structurally) Codex all key hooks by a named lifecycle point | Claude, Gemini directly; Codex plausibly, OpenCode only via in-code event-name switch | Cannot route at all without it |
| `requirements.observe` | Yes — universal baseline across all four | All four | n/a, this is the floor |
| `requirements.block` | Yes — Claude, Gemini both real; Codex claimed; OpenCode real-but-buggy for the subagent case | Claude, Gemini cleanly; Codex unverified; OpenCode only with the documented gap disclosed | A package that needs a real veto and gets silently downgraded to observe-only is exactly the Part 17 fail-closed failure mode |
| `requirements.mutate_input` | Yes — Claude (`updatedInput`), Gemini (`hookSpecificOutput.tool_input`), OpenCode (`output.args`); Codex claimed | Claude, Gemini, OpenCode (each with different merge-vs-replace mechanics); Codex unverified | This is the single most vendor-specific-shaped field even where the *capability* exists — a portable model would need to normalize merge vs. replace semantics, which this research has not yet proven safe |
| `requirements.mutate_output` (add context) | Yes — Claude, Gemini, OpenCode (`experimental.*`) all support some form of context injection | All three with a real mechanism; Codex plausible | Loses the single most commonly-used non-blocking hook use case (adding context) |
| `action` (what the hook actually does) | This is where the model breaks down: `action` is either a `command` invocation (Claude/Codex/Gemini) or a JS callback (OpenCode) — these are not the same *kind* of thing, only both "the hook's logic" | Cannot be represented uniformly without either (a) restricting to command-only packages, which excludes OpenCode entirely, or (b) generating code, which Part 7/8 below argues against as an automatic default | Removing `action` from the model entirely and treating it as opaque, integration-owned payload (exactly how `Capability.payload` already works) is the honest choice today |

**Conclusion:** every field except `action` clears the two-harness bar. The
`action` field is where format-vs-semantics actually collapses back into a
real difference (subprocess command vs. in-process code are genuinely
different execution models, not just different serializations of the same
one). A `HookIntent`-shaped struct with `event` + `requirements` is
defensible for the Claude↔Gemini subset specifically, carrying `action` as
opaque payload exactly like `Capability` already does — **not** a general
four-harness model. This is not proposed for implementation now; it is the
shape a future Claude↔Gemini spike should validate against real conformance
runs before any Core change.

## Part 7 — Format vs. semantics: should UZE translate?

Evaluated against the four options in the brief:

- **A. Native pass-through only.** Correct default for all four harnesses
  today, and the only strategy compatible with OpenCode without a trust
  regression (Part 8). Costs nothing new in Core.
- **B. Semantic adapter** (`HookIntent` → per-vendor representation). Only
  arguably justified for the Claude↔Gemini subset (Part 6), and only after a
  real conformance spike resolves the `mutate_input` merge-semantics
  question — not justified today from documentation research alone.
- **C. Static generated bridge** (vendor event → original package executable,
  no UZE business logic). This is the interesting middle ground for Claude→
  Gemini specifically, since both are already "call an external command with
  a JSON contract" — a generated bridge here would be closer to a config
  translation (rename fields, remap event names) than to code generation.
  For OpenCode, "bridge" stops being config translation and starts requiring
  a real JS shim that calls out to the package's original (non-JS) hook
  executable — this is possible in principle (spawn a subprocess from JS)
  but crosses into Part 8's generated-code trust question, and is explicitly
  the kind of thing that should not happen automatically without a
  conformance-verified adapter template, not an ad hoc UZE-authored one.
- **D. Runtime bridge / daemon.** Not evidenced as necessary by anything
  found — every harness's hook mechanism is either already a subprocess
  (Claude/Codex/Gemini) or already in-process to that harness (OpenCode). No
  finding calls for a UZE-owned long-lived process.

**UZE should not translate the same intention into different mechanisms as a
general policy.** It may, for a narrowly-scoped and conformance-verified
Claude↔Gemini config bridge (option C), generate the minimal glue that maps
one JSON shape to another — never code that decides anything.

## Part 8 — Generated code / trust boundary

Applies specifically to any future option B/C work, since OpenCode's format
makes "adapt" and "generate code" the same question there.

| Question | Answer |
|---|---|
| Who owns the generated code? | UZE, if it authors it — this is new: M2's trust model always attributes execution to the *package*. A UZE-generated bridge would be the first UZE-authored code a harness executes. |
| Where does it live? | Would need to live in the Store alongside the package (derivable, not user-edited), analogous to how `republish_packages` already treats derived harness-owned views: reconstructible, safe to delete/regenerate, never a second source of truth. |
| Is it derived/rebuildable? | Must be, for the same reason `republish_packages`'s doc comment already states for catalogs: "must be reconstructible from the package alone... safe to delete and regenerate at any moment." A generated hook bridge should follow the identical contract. |
| Does it enter the ledger? | Yes, as an `AttachmentReceipt` — it is a managed side effect exactly like a symlink or vendor config entry, and needs its own inspectable `ManagedArtifact` variant so drift/removal work the same way. |
| How is drift inspected? | Content-hash comparison against what the generator would currently produce from the package + target event mapping — same pattern `ManagedUserScopeReference`/`ManagedVendorConfig` already use, just applied to generated text instead of a symlink target. |
| How is it removed? | Same as any other managed artifact: `detach_receipt`, safe-by-inspection. No new removal primitive needed if the receipt shape carries enough to regenerate. |
| How does UZE avoid generating arbitrary logic? | By construction, if option C is followed strictly: the generator only ever emits field remapping (event name, matcher, payload shape) between two *already-command-shaped* hook declarations. It must refuse — not degrade, refuse — the moment a target requires code (OpenCode) rather than a declarative rename. This is the fail-closed principle from Part 17 applied to code generation specifically. |
| How does it update when the package changes? | Regenerate on every `uze add`/`uze update` of that package, same lifecycle point `republish_packages` already uses. |
| Node/runtime versions? | Not applicable under the option-C-only scope above — a config-remapping generator emits JSON/TOML text, not executable JS, so it has no runtime dependency of its own. This constraint is itself an argument for staying inside option C and refusing option-D-shaped OpenCode bridges. |

**Comparison to existing derived artifacts:** Codex's marketplace catalog and
Gemini's link workflow are both already "UZE writes something the harness
reads," and both are already scoped as derived, regenerable, receipt-free
views (`republish_packages`) precisely because they carry **no interpretation
of package semantics** — they are catalog entries pointing at package
content, not translations of package behavior. A generated hook bridge is a
step further: it *would* carry a semantic translation (event A means event
B), which is why it needs a real `AttachmentReceipt`, not the catalog
pattern. **This is judged an acceptable strategy only under option C's strict
scope (declarative remap, refuse-on-code-requirement) — not evaluated as
acceptable for anything resembling option D, and not recommended for
implementation in M3.**
