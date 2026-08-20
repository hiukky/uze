# Adopt AGENTS.md, Agent Skills, MCP, and Agent Plugins instead of inventing competing formats

Status: Accepted

## Context

UZE's premise is that a developer's agent environment should be portable
across coding-agent harnesses. The obvious failure mode, named explicitly
in the product brief, is UZE becoming just another proprietary format that
merely relocates the duplication problem into `.uze/` instead of solving
it. Primary-source research (2026-08-19) confirmed that four relevant
primitives already have genuinely multi-vendor open standards: AGENTS.md
(project instructions, stewarded by OpenAI, now under the Linux
Foundation's Agentic AI Foundation), Agent Skills (`SKILL.md`, ~45 listed
adopters including all four of our target harnesses), MCP (tool/resource/
prompt exposure, current spec `2026-07-28`), and Agent Plugins v1.0
(skill+MCP bundling, ratified 2026-08-06, multi-vendor authorship though
disputed between sources — see `research-notes.md`). A decision was
needed on whether UZE should model these primitives itself (for
consistency with the rest of its capability graph) or defer to the
external standards entirely.

## Decision

We will consume AGENTS.md, Agent Skills, MCP, and Agent Plugins directly
as external standards, and will not invent UZE-specific formats that
compete with any of them. Where a harness doesn't yet read a standard
natively (e.g. Claude Code and AGENTS.md), UZE bridges via the harness's
own native format rather than asking the standard to change. UZE's
capability graph represents these items by reference to their standard
form (byte-for-byte preserved payload, see `resource-import` spec), not
by re-serializing them into an internal schema.

Alternative considered: model every primitive — including Skills and MCP
— uniformly inside a single UZE-defined schema, for internal consistency.
Rejected because it would mean UZE re-implements consensus that four
major vendors have already reached, adds a translation step with no
functional benefit for standard-covered primitives, and directly
contradicts the brief's "standards first" principle and its explicit
non-goal of becoming "a replacement for MCP / Agent Skills / Agent
Plugins / AGENTS.md."

## Consequences

Easier: UZE's import/classification logic for Skills, MCP servers, and
plugin-bundled resources is thin — verify presence and preserve content,
not parse-and-regenerate. Adoption risk is shared with the wider
ecosystem rather than borne alone.

Harder: UZE now has an external dependency on standards it doesn't
control, including one (Agent Plugins) whose authorship and adoption are
not yet fully confirmed, and one (AGENTS.md) where at least one major
harness's advertised support doesn't match its own documentation. UZE
must track these standards' evolution and cannot unilaterally fix a gap
in them — it can only report the gap honestly (see the companion ADR
"Scope UZE's capability model to the confirmed standards gap" and the
`capability-classification` spec's `unknown`/`degraded` outcomes) or fall
back to filesystem projection.
