## Why

UZE recognizes package `agents/` content during acquisition but discards it
from the effective environment, leaving reusable subagents unavailable in all
four supported harnesses. The current harnesses now provide documented agent
surfaces, so UZE can make agents portable with an explicit, evidence-backed
projection instead of keeping the matrix cell as roadmap.

## What Changes

- Introduce `agents/<name>.md` as UZE's canonical Agent capability.
- Discover canonical agent definitions from installed packages, preserve their
  bytes in the Store, and route them independently of Skills and MCP.
- Project agents natively to Claude Code, OpenCode, and Antigravity CLI.
- Provide a safe generated adapter for Codex CLI and report its adapted route
  and semantic limits honestly.
- Extend receipts, inspection, lifecycle safety, TUI compatibility rows,
  generated harness matrix, deterministic tests, and real-harness conformance
  coverage for the new capability.

## Capabilities

### New Capabilities

- `portable-agents`: Portable discovery, routing, delivery, lifecycle, and
  support reporting for package-defined agent profiles across supported
  harnesses.

### Modified Capabilities

- None.

## Impact

Affected areas include `uze-core` capability discovery and resource identity,
all four modules in `uze-integrations`, the shared exposure/receipt model,
the TUI and harness-matrix generator, fixtures, integration/acceptance tests,
and the four conformance verticals. No new runtime dependency is required.
