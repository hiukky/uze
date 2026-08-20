# Scope UZE's capability model to the confirmed standards gap: Action, Subagent, Hook, Policy

Status: Superseded by ADR-003

## Context

Given ADR "Adopt AGENTS.md, Agent Skills, MCP, and Agent Plugins instead
of inventing competing formats," UZE still needs an internal
representation for whatever those standards don't cover — but the
product brief (§15) left the primitive list open-ended ("this must remain
minimal... do not invent abstractions simply because vendors use
different names"), without a concrete boundary. Research surfaced two
independent, converging sources for that boundary: the MCP "Skills over
MCP" working group's charter explicitly defers "plugin/bundle packaging"
to a separate effort, and the Agent Plugins v1.0 spec itself explicitly
excludes hooks, subagents, commands, and permissions as "too
client-specific for v1." Both standards agree on the same exclusion list
independently. Memory is not addressed by any standard and has
inconsistent, often absent, native support across harnesses. A concrete
decision was needed on where UZE's own capability model starts and stops,
since drawing it too wide re-creates the standards UZE just agreed not to
compete with, and drawing it too narrow leaves real interoperability gaps
unaddressed.

## Decision

We will scope UZE's capability model (internal representation) to exactly
four primitive kinds not covered by any current standard: **Action**
(commands/workflows), **Subagent**, **Hook**, and **Policy**
(permissions), plus **Memory** as a fifth category that is explicitly
degraded-by-default rather than modeled with the same confidence as the
other four, given the total absence of any standard and highly
inconsistent native support. Skills, MCP servers, and plugin-bundled
resources are represented by reference to their standard form (per the
sibling ADR), not re-modeled here. This is the complete primitive set the
`resource-import`, `capability-classification`, and `harness-projection`
specs operate over.

Alternative considered: use the product brief's full open-ended primitive
list (Instruction, Skill, Agent, Action, Hook, Tool, Policy, Context) as
the model boundary from the start. Rejected because Instruction, Skill,
and Tool are already fully covered by AGENTS.md/Agent Skills/MCP
respectively (per the sibling ADR) — including them in UZE's own model
would duplicate, not complement, the standards layer. "Context" was left
out entirely: no research finding identified it as a distinct capability
with harness-specific representation requiring modeling, as opposed to
being an emergent property of Instructions plus Memory.

## Consequences

Easier: the capability model has a small, evidence-grounded surface (four
confirmed-gap primitive kinds plus one explicitly-uncertain one) instead
of an open-ended list that would need to be re-justified per primitive.
Every future addition to the model has a natural bar to clear: "is this
genuinely uncovered by AGENTS.md/Agent Skills/MCP/Agent Plugins as they
exist today?"

Harder: if a future standard expands to cover part of this residual set
(e.g. Agent Plugins v2 adding hooks or subagents), UZE's model overlaps
with it and a primitive kind may need to be retired from UZE's own model
in favor of the standard — a migration this ADR does not attempt to plan
for now, since it depends on standards evolution outside UZE's control.
Memory's degraded-by-default treatment also means the PoC will likely
report most memory items as `unsupported` rather than portable, which is
an honest but limited outcome for a capability the original brief treated
as a first-class goal (§21) — that ambition is deferred, not abandoned.
