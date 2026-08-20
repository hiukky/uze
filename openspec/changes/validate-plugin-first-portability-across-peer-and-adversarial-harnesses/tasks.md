## 1. Research and design

- [x] 1.1 Verify current OpenCode plugin, package, Skills, MCP, Hooks,
      Agents, Commands, scope, and extension surfaces from official docs,
      source evidence, and installed CLI help.
- [x] 1.2 Define source/provenance handling independently from compatibility.
- [x] 1.3 Define Claude native, Codex native, and OpenCode component fallback
      strategies for one external multi-capability plugin.
- [x] 1.4 Define the multi-capability conformance fixture and evidence tiers.
- [x] 1.5 Assess minimal current-model, local-marketplace, and TUI impacts.

## 2. Completed implementation boundary

- [x] 2.1 Implement only the approved vertical slice; do not implement a TUI,
      Hooks, Agents, Commands, or a remote marketplace.
- [x] 2.2 Record the accepted architectural result in ADR-008.

## 3. Deferred implementation, pending review

- [x] 3.1 Preserve a complete validated external plugin tree and provenance
      receipt in the UZE Store.
- [x] 3.2 Add package-aware native attachment planning with consumed-component
      accounting, preserving existing resource fallbacks.
- [x] 3.3 Add the immutable dual-envelope Skill + MCP external fixture and
      deterministic plan tests.
- [x] 3.4 Implement Codex native marketplace planning; Claude correctly stays
      on fallback because the fixture has no Claude-native envelope.
- [x] 3.5 Implement OpenCode Skill native discovery and MCP adaptation.
- [x] 3.6 Run a real isolated-home Codex native-plugin configuration/install
      probe; behavioral invocation remains an opt-in future check and
      auth/approval/quota stays BLOCKED_BY_ENVIRONMENT.
- [ ] 3.7 Add package-level inspect/report data before any TUI renderer.
