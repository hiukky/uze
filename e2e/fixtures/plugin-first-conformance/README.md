# UZE Plugin First conformance package

One external package, installed once. `plugin.json` and `mcp.json` preserve
the Agent Plugins portable core; `.codex-plugin/plugin.json` plus `.mcp.json`
are the source-provided Codex-native envelope. No harness-specific copy is
created by UZE. Claude and OpenCode intentionally receive the portable Skill
and MCP capabilities through their own delivery strategies.

For L2, the test-only E2E runner copies this package into a disposable
run directory and replaces only `__UZE_SKILL_PROOF__` in `SKILL.md`. MCP uses
its independently generated proof through `UZE_MCP_CONFORMANCE_PROOF` inherited
by the fixture process. The Store's source package is never modified.
