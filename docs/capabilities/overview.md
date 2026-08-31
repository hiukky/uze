# Capabilities

The canonical capability set is **one Skill kind** (`AgentSkill`) whose
portable semantics are its invocation policy (`invoke: {model, user}`,
ADR-030 — see [skill-invocation-policy.md](skill-invocation-policy.md)),
**MCP servers**, and **project instructions/context** (see
[context-manager.md](context-manager.md)). Store bytes stay verbatim; every
harness-specific encoding is a derived artifact. Per-harness delivery
details, evidence and limitations live in each integration's README
(`crates/uze-integrations/src/<harness>/README.md`), with the cross-harness
view in `crates/uze-integrations/README.md`.

## Delivery status

| Capability | Status |
|---|---|
| Skills | **Implemented** — all four harnesses (Claude, Codex, OpenCode, Antigravity) |
| MCP | **Implemented** — all four |
| Instructions / context | **Implemented** — Context Manager (see [context-manager.md](context-manager.md)) |
| Skill invocation policy | **Implemented** — native per harness where the vendor has the mechanism; ADAPTED on Antigravity (see [skill-invocation-policy.md](skill-invocation-policy.md)) |
| Official UZE Skills | **Implemented** — `/uze:init` for project context and `/uze:worktree` for concurrent-worktree coordination (see [uze-skill.md](uze-skill.md)) |
| Hooks | **Implemented** — portable command hooks (ADR-033); see [portable-hooks.md](portable-hooks.md) |
| Agents / Subagents | **Implemented** — native pass-through per harness (ADR-031) |
| Memory | Future — would land inside the Context Manager boundary (see [context-manager.md](context-manager.md)) |

## Research-only capabilities

None remain implemented-adjacent: Agents (ADR-031) and Hooks (ADR-033)
ship as canonical capabilities with per-harness delivery, and their
vendor matrices are re-verified continuously by the Conformance Lab rather
than by static research notes. Known traps from the 2026-08-21 M3 research
stay tracked, because they constrain what a projection may honestly claim:

- **OpenCode hooks**: `permission.ask` is defined but does not currently
  fire ([anomalyco/opencode#7006](https://github.com/anomalyco/opencode/issues/7006)),
  so `ask` is never claimed for OpenCode; `tool.execute.before` does not
  cover subagent-issued tool calls
  ([sst/opencode#5894](https://github.com/sst/opencode/issues/5894)), a
  documented gap the bridge inherits.
- **Fail closed**: a capability request no harness can honestly satisfy
  routes PARTIAL/UNSUPPORTED — never a best-effort translation that silently
  drops semantics. For Hooks this is per-event/per-effect
  (ADR-033): a `Stop` hook is never represented as a tool callback, and an
  `ask` or `transform` effect is only attached where the harness preserves
  it. One capability routing Unsupported never suppresses delivery of the
  others.
- **Trust**: `executable_capabilities`
  (`crates/uze-core/src/trust.rs`) surfaces hook commands so the
  single acquisition trust prompt lists every process a package can cause to
  run, not only MCP servers.
