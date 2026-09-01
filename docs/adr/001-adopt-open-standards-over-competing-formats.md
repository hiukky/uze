# Adopt open standards and model only the residual gap

Status: Accepted
Consolidates: ADR-002 (scope capability model to the standards gap),
ADR-003 (compose effective agent environments; ACP at the Client-Agent
boundary) — see the "Consolidated records" section of `README.md`.

## Context

UZE's premise is that a developer's agent environment should be portable
across coding-agent harnesses. The obvious failure mode, named explicitly
in the product brief, is UZE becoming just another proprietary format that
merely relocates the duplication problem into `.uze/` instead of solving
it. Primary-source research (2026-08-19) confirmed that four relevant
primitives already have genuinely multi-vendor open standards: AGENTS.md
(project instructions, stewarded by OpenAI, now under the Linux
Foundation's Agentic AI Foundation), Agent Skills (`SKILL.md`, adopted by
all four target harnesses), MCP (tool/resource/prompt exposure), and Agent
Plugins v1.0 (skill+MCP bundling).

Two questions followed. First, whether UZE should model those primitives
itself for internal consistency, or defer to the standards entirely.
Second, where UZE's own model starts and stops for whatever the standards
*don't* cover — drawn too wide it re-creates the standards it just agreed
not to compete with; drawn too narrow it leaves real interoperability gaps
unaddressed.

## Decision

**Consume the standards directly; never invent a competing format.**
AGENTS.md, Agent Skills, MCP, and Agent Plugins are consumed as external
standards. UZE's capability graph represents them by reference to their
standard form — byte-for-byte preserved payload — not by re-serializing
them into an internal schema. Where a harness doesn't read a standard
natively (e.g. Claude Code and AGENTS.md), UZE bridges through that
harness's own native format rather than asking the standard to change.

**Model only the residual gap, and only on evidence.** A capability kind
earns a place in UZE's own model only when it is genuinely uncovered by
AGENTS.md / Agent Skills / MCP / Agent Plugins as they exist today. That
bar, not a fixed list, is the durable rule: the set has already changed
under it (the original Action/Subagent/Hook/Policy proposal was replaced —
`Command` was retired as a canonical kind by ADR-030, `Agent` was admitted
by ADR-031, `Hook` by ADR-033).

**Adopt before inventing.** For any capability on any harness, prefer in
order: a direct open standard, then a native harness capability that
leaves the portable core intact, then an explicit adapter, and otherwise
report the capability as unsupported with a rationale. ADR-013 refines
this into the concrete per-capability delivery precedence UZE implements.

**A portable core plus optional enhancements.** UZE resolves an *effective
agent environment*: a portable project core (`AGENTS.md`, Agent Skills,
MCP) plus separately identified optional harness enhancements. The two are
never conflated — an enhancement that cannot be delivered is reported, not
silently emulated.

Alternatives rejected: modelling every primitive, Skills and MCP included,
inside a single UZE-defined schema for internal consistency (it
re-implements consensus four vendors already reached, and contradicts the
brief's explicit non-goal of replacing MCP / Agent Skills / Agent Plugins
/ AGENTS.md); and standardizing the Client-Agent runtime boundary on ACP
with the official Rust SDK's `Proxy`/`Conductor` (proposed by ADR-003 and
never implemented — no target harness required it, and UZE's actual
runtime boundary became the PATH shim of ADR-014; A2A remains equally out
of scope).

## Consequences

Easier: import and classification for Skills, MCP servers, and
plugin-bundled resources stay thin — verify presence and preserve content,
not parse-and-regenerate. Every future addition to the capability model
has one bar to clear rather than a negotiation. Adoption risk is shared
with the wider ecosystem rather than borne alone.

Harder: UZE depends on standards it does not control and cannot
unilaterally fix a gap in one — it can only report the gap honestly or
fall back to projection. If a standard later expands to cover part of the
residual set (Agent Plugins adding hooks, say), a UZE capability kind may
need to be retired in favor of it; that migration is not planned here
because it depends on standards evolution outside UZE's control.
