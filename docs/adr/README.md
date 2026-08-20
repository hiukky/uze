# Architecture Decision Records

This directory answers **"why is the system this way?"** — the durable
record of significant, hard-to-reverse decisions. It complements two other
parts of this repo:

- `openspec/changes/` answers **"what are we changing now?"** (proposal,
  specs, design, tasks for one in-flight change).
- `docs/architecture/likec4/` answers **"how is the system organized?"**
  (the current structure, as a diagrammable model).

## Format

Each ADR is `NNN-kebab-title.md`, numbered sequentially (never reused,
never renumbered), using this structure (Michael Nygard style):

- **Status** — `Accepted`, or `Superseded by docs/adr/NNN-new-title.md`
- **Context** — the situation that forced a choice
- **Decision** — what was decided, stated firmly
- **Consequences** — what becomes easier or harder as a result

## Rules

- **Numbered sequentially**, one shared sequence for the whole repo.
- **Immutable once accepted.** Don't edit a Decision after the fact - if a
  later change reverses or replaces it, write a *new* ADR and mark the old
  one `Superseded by docs/adr/NNN-new-title.md`. The history of what was
  decided when is part of the value.
- **Only for decisions that clear the bar**: a new external dependency, a
  technology/pattern choice with long-term consequences, a boundary that
  would be expensive to move later. Routine implementation choices don't
  need one.
- Most ADRs are created automatically as part of an OpenSpec change (the
  `adr` artifact, when the change's design.md contains a qualifying
  decision) - see `openspec/config.yaml`. Use `/std:adr` to record one
  ad hoc (a decision made outside an OpenSpec change, or backfilling a
  decision that predates this convention).

## Index

- [001 — Adopt open standards over competing formats](001-adopt-open-standards-over-competing-formats.md)
- [002 — Scope capability model to standards gap](002-scope-capability-model-to-standards-gap.md) — superseded
- [003 — Compose effective agent environments and use ACP at the Client-Agent boundary](003-compose-effective-agent-environments-and-use-acp-at-the-client-agent-boundary.md)
- [004 — Implement the UZE core in Rust](004-implement-the-uze-core-in-rust.md)
- [005 — Establish peer harness integrations](005-establish-peer-harness-integrations.md)
