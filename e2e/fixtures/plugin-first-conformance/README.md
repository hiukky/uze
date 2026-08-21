# UZE Plugin First conformance package

One external package, installed once. `plugin.json` and `mcp.json` preserve
the Agent Plugins portable core; `.codex-plugin/plugin.json` plus `.mcp.json`
are the source-provided Codex-native envelope. No harness-specific copy is
created by UZE. Claude and OpenCode intentionally receive the portable Skill
and MCP capabilities through their own delivery strategies.

For L2, the test-only E2E runner copies this package into a disposable
run directory, replaces `__UZE_SKILL_PROOF__` in `SKILL.md` and
`__UZE_MCP_FIXTURE_BINARY__` in both MCP manifests, and declares the
independently generated MCP proof as a `--proof` argument. The Store's source
package is never modified.

## Why the MCP server is named `conformance`

The name a model finally sees is composed, not the one written here:

```text
uze- <package>            - <server>     _ <tool>
uze- plugin-first-conformance - conformance _ uze_conformance   = 60 characters
```

OpenAI rejects a tool call whose function name exceeds **64 characters**
(`Invalid 'messages[..].tool_calls[0].function.name': string too long`), so
this budget is real and is spent by the package name, the server name and the
tool name together. Naming the server `uze-plugin-first-conformance` — the
obvious, descriptive choice — produces 77 characters and fails at the provider
the moment a model actually calls the tool. Discovery still passes, which is
why the limit is easy to reintroduce by accident.

This is a fixture-level workaround for a product-level gap: UZE composes entry
names without checking them against any provider's limit. Renaming this server
back will break `uze-conformance behavior`.
