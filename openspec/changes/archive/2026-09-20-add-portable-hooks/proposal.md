## Why

UZE currently recognizes `CapabilityKind::Hook` only for import compatibility and deliberately routes it nowhere. Plugin authors therefore cannot ship one deterministic automation policy across Claude Code, Codex, Antigravity CLI, and OpenCode, despite each harness now exposing a tool-lifecycle extension point.

## What Changes

- Add `hooks.json` as the canonical, declarative Hook capability and a command-only portable ABI.
- Discover, validate, normalize, plan, attach, inspect, detach, and diagnose portable hooks through the existing Store/Engine/IntegrationPort lifecycle.
- Emit native hook artifacts for Claude Code, Codex, and Antigravity CLI; generate a receipt-owned OpenCode bridge without requiring an author-maintained TypeScript build.
- Add semantic tool aliases, compatibility verdicts, user-config-safe merges, fixtures, examples, migration guidance, and conformance coverage.

## Capabilities

### New Capabilities

- `portable-hooks`: canonical Hook manifest, command ABI, aliases, compatibility planning, projection, lifecycle safety, and diagnostics.

### Modified Capabilities

- `plugin`: installed packages may contribute canonical Hooks in addition to Skills, MCP servers, Agents, and instructions.
- `doctor`: reports Hook delivery compatibility and managed-artifact health.

## Impact

`uze-core` capability discovery, exposure/receipt contracts, routing diagnostics, all four integration verticals, application read models, CLI/TUI surfaces, tests/fixtures, conformance lab, marketplace example, architecture documentation, and ADR index. No compatibility alias is added for vendor-specific hook formats.
