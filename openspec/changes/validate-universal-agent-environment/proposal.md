## Why

AI coding harnesses expose similar capabilities through different project
conventions and runtime integrations. The original PoC correctly identified
that re-creating those conventions in a UZE-specific format would move, not
solve, the portability problem; however, it still made importing and
projecting harness-specific bundles the architectural center.

Before implementation, UZE must validate the narrower thesis: **an agentic
project can be portable when its effective environment is composed from
existing standards first, then progressively enhanced by a selected runtime**.
UZE's own responsibility is to discover, resolve, explain, and—only when
needed—adapt that composition. It is not to become a universal protocol or
configuration translator.

## What Changes

- Establishes **adopt before invent** as the architectural rule: use open
  standards before native capabilities, and native capabilities before an
  adapter or generated harness artifact.
- Defines the portable project core as `AGENTS.md` for instructions, Agent
  Skills for reusable capabilities, and MCP for tools, resources, and
  external context. UZE consumes these standards directly and preserves their
  representations; it does not create competing equivalents.
- Adds ACP as the preferred interface specifically at the **Client ↔ Agent**
  runtime boundary. ACP capability negotiation is reused for sessions,
  prompts, streaming, tool activity, permission requests, diffs, and other
  protocol capabilities rather than duplicated by UZE.
- Defines the ACP integration order as: native ACP, then an official or
  well-maintained ACP adapter, then a minimal explicit integration adapter.
  ACP is not required of every harness and does not replace the project,
  standards, or MCP layers.
- Recasts import/projection as compatibility fallbacks. The normal path is
  discovery and resolution of a project-owned portable core; parsing a plugin
  bundle or writing a harness-specific directory is allowed only when it is
  necessary, safe, and visibly reported.
- Separates protocol capabilities from harness/project capabilities. ACP
  advertises the former; UZE classifies the latter as `STANDARD`, `NATIVE`,
  `ADAPTABLE`, or `UNSUPPORTED` and never silently converts them.
- Makes progressive enhancement explicit: a portable core may gain optional
  Claude Code, Codex, Cursor, or OpenCode enhancements without contaminating
  the core. Windsurf/Devin Desktop remains outside the active matrix; no new
  target is added.
- Recognizes A2A as a possible future **Agent ↔ Agent** standard without
  adding multi-agent orchestration to this MVP.
- Adds the standards coverage / remaining-gap analysis, revised architecture
  diagrams, and an ADR that supersedes the old single capability-graph
  boundary.

## Capabilities

### New Capabilities
- `resource-import`: Discover and resolve a project-owned portable core
  (`AGENTS.md`, Agent Skills, MCP) without converting it; safely import a
  declarative bundle only as a compatibility fallback.
- `capability-classification`: Distinguish ACP-negotiated protocol
  capabilities from project/harness capabilities, and classify only the
  latter as `STANDARD`/`NATIVE`/`ADAPTABLE`/`UNSUPPORTED` with evidence.
- `harness-projection`: Select an ACP runtime path when the interaction is
  Client ↔ Agent and apply explicit, minimal optional enhancements only after
  the portable core has been resolved.
- `compatibility-report`: Explain the effective agent environment, selected
  runtime path, enhancement outcomes, and standards coverage without hidden
  conversion.

### Modified Capabilities
- None — this remains the first change in the project; the listed capability
  specs are revised before implementation rather than modifying a baseline
  specification.

## Impact

- No implementation code is changed; this change remains planning and
  architecture work only.
- Supersedes the capability-graph boundary in `docs/adr/002-*.md` with a
  new permanent ADR that records standards-first composition, progressive
  enhancement, and ACP's limited-but-preferred role.
- Revises the LikeC4 model from a universal projector to a project
  composition layer that sits beside—not in—the Client ↔ Agent ACP path.
- Defines the future PoC around resolving one representative project across
  Claude Code, Codex, Cursor, and OpenCode, with fallbacks and remaining gaps
  reported explicitly.
