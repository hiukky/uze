# Agents / Subagents — research

Companion to [landscape.md](landscape.md). Part 10 of the M3 brief.

## Semantic-gap matrix

| | Claude Code | Codex | OpenCode | Gemini CLI |
|---|---|---|---|---|
| Definition format | Markdown + YAML frontmatter | `.toml` files | JSON config or Markdown+YAML frontmatter | Markdown + YAML frontmatter |
| Package-native | Plugin `agents/` dir — but **cannot use `hooks`, `mcpServers`, or `permissionMode` frontmatter** when package-shipped (must be copied to `.claude/agents/`/`~/.claude/agents/` to unlock those) | **No** — cannot be declared inside `plugin.json` today ([open issue](https://github.com/openai/codex/issues/28491)); user/project `.toml` files only | Not documented as package-shippable; reads as project/user config | Yes, extension `agents/` dir |
| Tool allowlist/denylist | Both (`tools:`/`disallowedTools:`), incl. MCP-scoped patterns and subagent-spawn restriction | Unresearched this pass | Rich `allow/ask/deny` grammar per agent, incl. glob bash rules | `tools:` allowlist, supports `*` and `mcp_*` wildcards |
| Model selection | Per-agent `model` field, `inherit`, env-var override | Unresearched | Per-agent, subagent inherits invoker's if unset | Per-agent `model` field, defaults to parent |
| Memory | Own persistent memory dir per agent (`user\|project\|local`), separate from parent | Unresearched | Not documented as a distinct mechanism | Not documented as a distinct mechanism |
| Isolation | Two real modes: fresh-context subagent vs. **fork** (inherits entire parent conversation incl. history/cache); separate `isolation: worktree` for git-scoped work | Unresearched | Each runs in its own context/conversation loop | Each runs in its own separate context/conversation loop |
| Nesting/delegation | Default max depth 3 (configurable, settable to 1 to disable), concurrency limit 20 | Unresearched | Three-tier model: primary / subagent / system; delegation via Task tool | **Nesting explicitly disallowed** — subagents cannot call other subagents (recursion protection) |
| Delegation trigger | Explicit invocation | Unresearched | `@mention` or Task tool | Automatic (description matching) or explicit `@agent` |

## Portability assessment

None of the four rows above ("isolation," "nesting," "package-native," even
"what a subagent's frontmatter is allowed to configure") land on a shared
contract. The clearest single finding is Codex's: **a real, open, unresolved
gap** — subagents cannot be declared inside a Codex plugin manifest at all
today, so "package ships a subagent" is not yet expressible for that harness
regardless of what UZE does. Claude Code's own package format has a *partial*
version of the same problem: a plugin-shipped agent silently loses three
frontmatter capabilities (`hooks`, `mcpServers`, `permissionMode`) that a
locally-authored one keeps — a real, disclosed loss even within one vendor's
own package format, not a cross-vendor one.

Gemini CLI's flat, explicitly-no-nesting model and Claude Code's
fork-vs-fresh-context distinction are not reconcilable into one shared
"isolation" vocabulary without picking a side and losing the other's real
semantic (Claude's fork mode, which shares conversation history/cache, has no
equivalent restriction *or* capability in Gemini's model — it is not merely
"more nested," it is a different kind of thing).

| | Portable candidate? | Partial candidate? | Vendor-specific? |
|---|---|---|---|
| Verdict | No | Only for the crudest "package declares tool-scoped delegate with a prompt" shape, and even that hits Codex's package-format gap | Isolation, nesting, memory, and package-native scope are each vendor-specific today |

**Recommendation:** Agents/Subagents stay research-only for M3, consistent
with [landscape.md](landscape.md) Part 12's `CORE_MODEL_INSUFFICIENT`
assessment. Native pass-through (deliver a package's agent definitions inside
its native envelope, where the harness's own package format supports it at
all) is the only currently-defensible strategy. Do not attempt a portable
`CapabilityKind::Agent` requirement shape before Codex's package-format gap
closes and a real conformance spike resolves whether "isolation" can be
represented as anything less crude than a vendor-specific enum.
