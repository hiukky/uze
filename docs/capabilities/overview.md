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
| `/uze:init` Skill | **Implemented** — the official agentic orchestrator (see [uze-skill.md](uze-skill.md)) |
| Hooks | **Research-only** — not implemented |
| Agents / Subagents | **Research-only** — not implemented |
| Memory | Future — would land inside the Context Manager boundary (see [context-manager.md](context-manager.md)) |

## Research-only capabilities

Hooks and Agents are deliberately not implemented: their `CapabilityKind`
variants are recognized only by `uze-core::importers` and route to zero
integrations. The 2026-08-21 M3 capability research established the durable
findings below; the full vendor matrices from that research were not kept as
permanent documentation — any implementation work must re-verify against
current vendor behavior.

- **Agents**: no shared cross-vendor contract (isolation, nesting, and
  package-native scope all differ). Codex cannot declare subagents inside a
  plugin manifest at all today (open vendor gap). Native pass-through is the
  only defensible strategy.
- **Hooks**: the only plausible portable subset is the declarative JSON
  subprocess pair (Claude Code ↔ Gemini CLI), never conformance-verified;
  OpenCode's hooks are in-process code with no subprocess boundary, so any
  adaptation would require generating code — the first UZE-authored code a
  harness executes, crossing UZE's trust boundary. Known traps: OpenCode's
  `permission.ask` hook is defined but does not currently fire
  ([anomalyco/opencode#7006](https://github.com/anomalyco/opencode/issues/7006)),
  and `tool.execute.before` does not cover subagent-issued tool calls
  ([sst/opencode#5894](https://github.com/sst/opencode/issues/5894)).
- **Fail closed**: a capability request no harness can honestly satisfy
  routes PARTIAL/UNSUPPORTED — never a best-effort translation that silently
  drops semantics. One capability routing Unsupported never suppresses
  delivery of the others.
- **Trust**: if Hook delivery ever ships, `executable_capabilities`
  (`crates/uze-core/src/trust.rs`) should surface hook commands so the
  single acquisition trust prompt lists every process a package can cause to
  run, not only MCP servers.