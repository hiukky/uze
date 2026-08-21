## Why

UZE proved two capability-level tracer bullets: Agent Skills through a
managed user-scope reference (ADR-006) and MCP through managed vendor
configuration (ADR-007). Both are sound fallback mechanisms, but neither
answers the more fundamental distribution question: when an external package
is already a plugin, should UZE expose the package as a native plugin before
decomposing it into capabilities?

Current ecosystem evidence says **yes, with material boundaries**. Agent
Plugins 1.0 now defines a portable package for Skills and MCP. Claude Code,
Codex, Cursor, and Gemini CLI each have a broader native package/envelope;
Cursor directly loads Agent Plugins and Codex source now supports their root
manifest. Native package attachment can therefore preserve semantics which a
capability-by-capability path cannot, while the existing attachment paths
remain necessary for harnesses and vendor-only components outside the
portable core.

This is research/design only. It deliberately does not implement Commands,
Hooks, Agents, Cursor, a marketplace, or any model refactor.

## What Changes

- Preserve Phase C's asymmetric-capability research as durable evidence:
  Commands are native in Claude plugins but deliberately unsupported as a
  standalone Codex prompt primitive; Hooks are semantically close but not
  proven identical; Agents remain partially comparable.
- Establish and evaluate the proposed architectural principle:
  **Plugin First, Capability Aware**.
- Compare the current plugin/marketplace surfaces of Claude Code, Codex,
  Cursor, OpenCode, Windsurf, and Gemini CLI, including their actual package
  formats and installation scope.
- Analyze Agent Plugins 1.0 as the canonical *external portable envelope*
  for its defined core, not as a replacement for vendor extensions or a UZE
  plugin specification.
- Identify how the present Package/Resource/Capability model would need to
  evolve if the principle is accepted, without changing it now.
- Recommend a third harness based on a real capability mismatch rather than
  assuming Cursor is adversarial.

## Non-Goals

- No `uze-plugin.json`, `uze.yaml`, or UZE-owned plugin manifest.
- No ADR yet. An ADR is conditional on a user-reviewed decision after this
  research, not a consequence of opening this change.
- No Store, CapabilityRouter, `IntegrationPort`, CLI, attachment, or test
  changes.
- No new harness integration; in particular no Cursor implementation.
- No runtime proxy, daemon, registry, cloud account, `uze.dev`, or remote
  synchronization design.

## Proposed Principle (pending review)

> **Plugin First, Capability Aware**: a plugin is the preferred unit of
> distribution; standards remain canonical; capabilities explain and route
> compatibility; an integration uses native plugin attachment whenever it is
> actually supported, then decomposes only the remaining components through
> an explicit fallback or reports them unsupported.

The product model remains setup/install-time composition, not an asserted
runtime proxy:

```text
setup/install time: UZE -> harness integration -> native plugin/resources
runtime:            harness -> its installed native plugin/resources
```

## Impact

This change adds only its own research/design artifacts. If accepted later,
the candidate affected modules are documented in `design.md`; no source file
is modified in this phase.
