# UZE Plugin First conformance package

One external package, installed once. `plugin.json` and `mcp.json` preserve
the Agent Plugins portable core; `.codex-plugin/plugin.json` plus `.mcp.json`
are the source-provided Codex-native envelope. No harness-specific copy is
created by UZE. Claude and OpenCode intentionally receive the portable Skill
and MCP capabilities through their own delivery strategies.
