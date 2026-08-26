## Context

See [proposal.md](proposal.md). `CapabilityKind::Agent` already exists for
import evidence, but package composition, routing, delivery, lifecycle, and
the support UI treat it as unimplemented. This change crosses the Core,
Application/Integration boundary, all four integration verticals, generated
support reporting, and isolated real-harness conformance.

## Goals / Non-Goals

**Goals:**

- Preserve one vendor-neutral `agents/<name>.md` definition in the Store and
  derive every harness artifact from it.
- Treat an Agent as a capability-level concern, so one unsupported or adapted
  target never prevents independent Skill or MCP delivery.
- Retain receipt-based inspect-before-detach safety and explicit route
  evidence.

**Non-Goals:**

- Define a common agent runtime, cross-harness delegation protocol, or tool
  permission schema.
- Convert arbitrary vendor-only agent directories into canonical UZE agents.
- Translate vendor-only model, tool, or sandbox policy beyond the portable
  Markdown subset.
- Add Hooks portability as part of this change.

## Decisions

### Canonical package surface is Markdown agent definitions

`agents/<name>.md` is the sole portable Agent surface. The file body is the
agent's prompt and its frontmatter carries portable metadata only where that
metadata is structurally compatible. The Store keeps it verbatim; integrations
produce wrapper files or configuration under UZE-managed derived locations.

This matches the Markdown definition surfaces documented by Claude, OpenCode,
and Antigravity and avoids inventing a UZE-specific manifest. JSON-only or
configuration-only agents are intentionally out of scope because translating
their tool/model/permission semantics would be lossy.

### Route every agent at capability level

The Engine discovers one Resource per agent definition. Integrations declare
their route in `HarnessCapabilities` and create a per-resource exposure plan.
This follows the existing Skill/MCP fallback architecture and means packages
may still use native package delivery for the resources it can cover while
Agents receive their own projection.

### Harness-specific projections

- Claude Code: generate/install its plugin-native `agents/<name>.md` content
  in the existing package generation or agent-specific artifact path.
- OpenCode: materialize a managed configuration-scope Markdown agent in the
  documented `agents/` discovery root.
- Antigravity CLI: generate its documented Markdown agent representation in
  the agent discovery root or generated plugin envelope, preserving only
  documented portable fields.
- Codex: generate a documented standalone TOML custom-agent file with the
  required `name`, `description`, and `developer_instructions` fields; expose
  it in `~/.codex/agents/` through a receipt-owned reference. This is
  Generated Native, not an adapter.

The concrete path and schema for each projection must be proven by the
respective real-harness vertical before the route is marked Native. Codex
generation must not introduce model, tool, or approval privileges absent from
the portable definition.

### Identity, naming, and lifecycle follow existing exposure machinery

Agent resource identity derives from package identity plus canonical agent
name. The existing collision naming machinery is reused where a global
discovery root needs a stable qualified label. Every generated file,
directory, or configuration entry has a typed receipt and participates in
existing inspect, detach, reconcile, and drift paths.

This is an architecturally significant format and projection choice; the
corresponding ADR artifact records it.

## Risks / Trade-offs

- [Different frontmatter schemas] → Project only a small verified portable
  subset; retain full canonical bytes and call out unsupported fields.
- [Codex custom-agent schema evolves] → Generate only its stable required
  fields and hold Native status to real-harness conformance.
- [Global discovery-root collisions] → Reuse qualified naming and inspect
  existing artifacts before mutation.
- [Vendor version changes] → Pin each vertical's evidence in its fixture and
  make conformance failures block matrix promotion.

## Migration Plan

1. Add discovery and routing behind the existing Agent capability kind.
2. Implement and test each delivery vertical and its receipt lifecycle.
3. Add real-harness conformance scenarios and promote only verified routes.
4. Regenerate the README matrix and expose the same status in the TUI.

Rollback removes derived projections through their receipts; Store package
bytes remain unchanged, so no package migration is necessary.
