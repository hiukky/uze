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

An ADR that absorbed others carries a `Consolidates:` line under its Status
naming them, and the absorbed records are listed under
[Consolidated records](#consolidated-records) below.

## Rules

- **Numbered sequentially**, one shared sequence for the whole repo.
  Numbers are never reused and never renumbered — code comments,
  `docs/architecture/invariants.md`, and OpenSpec changes cite them.
- **An ADR is written when an OpenSpec change is archived**, not while the
  approach could still change. The `operations.archive` guidance in
  `openspec/config.yaml` is the trigger: a decision flagged in that
  change's `design.md` that actually held up through implementation earns
  a record. Writing the ADR up front produces a decision that a later
  slice contradicts, and then two ADRs where one belonged. Use the `adr`
  skill to record one ad hoc — a decision made outside an OpenSpec change,
  or backfilling one that predates this convention.
- **Only for decisions that clear the bar**: a new external dependency, a
  technology or pattern choice with long-term consequences, a boundary
  that would be expensive to move later. Routine implementation choices
  don't need one.
- **Stable once accepted, but not frozen.** Don't quietly rewrite a
  Decision to match what the code does now — that erases the reason the
  boundary exists. When a later change *reverses* a decision, write a new
  ADR and mark the old one superseded. When a later change *continues*
  one — the same decision, refined or extended — fold it into the existing
  record instead of starting a new number, so one topic stays one ADR.
- **Consolidation is periodic and deliberate.** When several ADRs turn out
  to be one decision told in installments, or one records an approach that
  was never implemented, merge them into the surviving record, delete the
  absorbed files, and log the merge under
  [Consolidated records](#consolidated-records). The surviving ADR keeps
  its own number; every reference to an absorbed number is repointed in
  the same change. Full history stays in git and in
  `openspec/changes/archive/`.

## Index

- [001 — Adopt open standards and model only the residual gap](001-adopt-open-standards-over-competing-formats.md)
- [004 — Implement UZE in Rust as a layered Cargo workspace](004-implement-the-uze-core-in-rust.md)
- [005 — Establish peer harness integrations around a harness-agnostic core](005-establish-peer-harness-integrations.md)
- [006 — Attach UZE packages through persistent user-scope skill references](006-attach-uze-packages-through-persistent-user-scope-skill-references.md)
- [007 — Attach MCP servers through generated vendor configuration](007-attach-mcp-servers-through-generated-vendor-configuration.md)
- [009 — Manage harness attachments with receipts and safe reconciliation](009-manage-harness-attachments-with-receipts-and-safe-reconciliation.md)
- [010 — Provision supported harnesses through official, integration-owned routes](010-provision-supported-harnesses-through-official-routes.md)
- [013 — Native Projection: delivery precedence per capability per harness](013-adopt-native-projection-principle.md)
- [014 — Claude Code runtime projection via native command shim](014-claude-code-runtime-projection-via-native-command-shim.md)
- [016 — Project Agent Environment: AGENTS.md, .agents/, and agents.lock](016-project-agent-environment.md)
- [018 — Cache expensive read paths with fingerprint + TTL invalidation](018-cache-harness-detection-with-fingerprint-ttl-invalidation.md)
- [019 — Explicit Project/Machine boundary in the CLI command grammar](019-explicit-project-machine-boundary-in-cli-command-grammar.md)
- [026 — Stable namespaced invocation labels](026-stable-namespaced-invocation-labels.md)
- [027 — Antigravity CLI is the Google-family v0 harness; Gemini CLI removed](027-antigravity-primary-google-family-harness.md)
- [029 — Projection conflicts are detected at naming time](029-projection-conflicts-at-naming-time.md)
- [030 — Skill + Invocation Policy replace the canonical Command](030-skill-plus-invocation-policy.md)
- [031 — Adopt a canonical portable Agent capability](031-adopt-a-canonical-portable-agent-capability.md)
- [032 — Marketplaces: manifest, discovery registry, and the embedded official marketplace](032-restore-marketplace-manifest.md)
- [033 — Adopt a canonical portable Hook capability](033-adopt-portable-hook-capability.md)
- [034 — Adopt GitHub Releases as the official Linux distribution channel](034-adopt-github-releases-linux-distribution.md)
- [035 — Adaptive-result registry and version provenance as the conformance evidence-integrity contract](035-adaptive-result-registry-and-version-provenance.md)
- [036 — Qualify Store plugins by marketplace](036-qualify-store-plugins-by-marketplace.md)
- [037 — Adopt bounded environment maintenance](037-adopt-bounded-environment-maintenance.md)
- [038 — Adopt a local terminal runtime server](038-adopt-local-terminal-runtime-server.md)
- [039 — Adopt syntect for diff syntax highlighting](039-adopt-syntect-for-diff-syntax-highlighting.md)
- [040 — Compile portable hooks into the delivered artifact](040-compile-portable-hooks-into-the-delivered-artifact.md)

## Consolidated records

2026-09-01 — 39 records reduced to 25. Six topics had been told in
installments across separate numbers, and two recorded approaches that were
never implemented. Each absorbed record's substance lives in the survivor
named below; numbers are retired, never reused. Original text is in git
history and in the OpenSpec change each one came from.

| Absorbed | Into | Why |
| --- | --- | --- |
| 002 — Scope capability model to standards gap | [001](001-adopt-open-standards-over-competing-formats.md) | Its fixed primitive list (Action/Subagent/Hook/Policy) never became the model. The durable part is the *bar* a new capability kind must clear, now stated in 001. |
| 003 — Compose effective agent environments; ACP at the Client-Agent boundary | [001](001-adopt-open-standards-over-competing-formats.md) | ACP was never implemented — no code, no dependency, no harness required it; UZE's runtime boundary became the shim of 014. The "adopt before invent" precedence survives in 001 and is made concrete by 013. |
| 011 — Split UZE into a layered Cargo workspace | [004](004-implement-the-uze-core-in-rust.md) | Same decision as choosing Rust: what UZE is implemented in, and how it is layered. |
| 022 — Remove the dead foreign Claude plugin importer | [005](005-establish-peer-harness-integrations.md) | A cleanup whose durable clause — no foreign importer retained speculatively — belongs with the core/integration boundary it qualifies. |
| 008 — Adopt Plugin First, Capability Aware delivery | [013](013-adopt-native-projection-principle.md) | First statement of the delivery precedence 013 owns. |
| 020 — Generated Native Package Projection | [013](013-adopt-native-projection-principle.md) | Adds one level to that precedence. |
| 021 — Extend generated native projection to Codex and Gemini | [013](013-adopt-native-projection-principle.md) | Same level, more harnesses — and it named Gemini, removed by 027. |
| 017 — Reproducible agent dependency lock | [016](016-project-agent-environment.md) | The `agents.lock` schema for the artifact 016 introduced. |
| 024 — Cache attachment inspection with fingerprint + TTL | [018](018-cache-harness-detection-with-fingerprint-ttl-invalidation.md) | The same caching contract applied to the second expensive read path. |
| 025 — Commands as a first-class capability | [030](030-skill-plus-invocation-policy.md) | Superseded: `Command` is not a canonical capability. |
| 028 — Claude Command explicit invocation via generated frontmatter | [030](030-skill-plus-invocation-policy.md) | Superseded with 025, by invocation policy. |
| 012 — Marketplace contract for the embedded default plugin | [032](032-restore-marketplace-manifest.md) | One of three parts of a single marketplace decision. |
| 015 — Marketplace as discovery registry, plugin as installable unit | [032](032-restore-marketplace-manifest.md) | Second part. Its Store-neutrality clause was later narrowed by 036. |
| 023 — Marketplace manifest is `agents.json` | [032](032-restore-marketplace-manifest.md) | Reverted before any consumer existed; 032 records both the rename and the revert. |
