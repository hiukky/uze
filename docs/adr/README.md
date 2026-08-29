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
- [006 — Attach UZE packages through persistent user-scope skill references](006-attach-uze-packages-through-persistent-user-scope-skill-references.md)
- [007 — Attach MCP servers through generated vendor configuration](007-attach-mcp-servers-through-generated-vendor-configuration.md)
- [008 — Adopt Plugin First, Capability Aware delivery](008-adopt-plugin-first-capability-aware-delivery.md)
- [009 — Manage harness attachments with receipts and safe reconciliation](009-manage-harness-attachments-with-receipts-and-safe-reconciliation.md)
- [010 — Provision supported harnesses through official, integration-owned routes](010-provision-supported-harnesses-through-official-routes.md)
- [011 — Split UZE into a layered Cargo workspace](011-split-uze-into-layered-cargo-workspace.md)
- [012 — Model a marketplace contract for the embedded default plugin](012-model-a-marketplace-contract-for-the-embedded-default-plugin.md)
- [013 — Adopt Native Projection Principle](013-adopt-native-projection-principle.md)
- [014 — Claude Code Runtime Projection via Native Command Shim](014-claude-code-runtime-projection-via-native-command-shim.md)
- [015 — Marketplace as Discovery Registry, Plugin as Installable Unit](015-marketplace-as-discovery-registry.md)
- [016 — Project Agent Environment](016-project-agent-environment.md)
- [017 — Reproducible Agent Dependency Lock](017-reproducible-agent-dependency-lock.md)
- [018 — Cache harness detection with fingerprint + TTL invalidation](018-cache-harness-detection-with-fingerprint-ttl-invalidation.md)
- [019 — Explicit Project/Machine Boundary in the CLI Command Grammar](019-explicit-project-machine-boundary-in-cli-command-grammar.md)
- [020 — Generated Native Package Projection](020-generated-native-package-projection.md)
- [021 — Extend Generated Native Projection to Codex and Gemini](021-extend-generated-native-projection-to-codex-and-gemini.md)
- [022 — Remove the Dead Foreign Claude Plugin Importer](022-remove-dead-foreign-claude-plugin-importer.md)
- [023 — Marketplace Manifest Is agents.json](023-marketplace-manifest-is-agents-json.md) — superseded (ADR-032)
- [024 — Cache Attachment Inspection with Fingerprint + TTL Invalidation](024-cache-attachment-inspection-with-fingerprint-ttl-invalidation.md)
- [025 — Commands as a First-Class Capability](025-commands-as-first-class-capability.md) — superseded (ADR-030)
- [026 — Stable Namespaced Invocation Labels](026-stable-namespaced-invocation-labels.md)
- [027 — Antigravity CLI is the Google-family v0 harness; Gemini CLI removed](027-antigravity-primary-google-family-harness.md)
- [028 — Claude Command Explicit Invocation via Generated Frontmatter](028-claude-command-explicit-invocation-via-generated-frontmatter.md) — superseded
- [029 — Projection Conflicts at Naming Time](029-projection-conflicts-at-naming-time.md)
- [030 — Skill + Invocation Policy replace the canonical Command](030-skill-plus-invocation-policy.md)
- [031 — Adopt a canonical portable Agent capability](031-adopt-a-canonical-portable-agent-capability.md)
- [032 — Restore marketplace.json as the Marketplace Manifest](032-restore-marketplace-manifest.md)
- [033 — Adopt a canonical portable Hook capability](033-adopt-portable-hook-capability.md)
- [034 — Adopt GitHub Releases as the official Linux distribution channel](034-adopt-github-releases-linux-distribution.md)
- [036 — Qualify Store plugins by marketplace](036-qualify-store-plugins-by-marketplace.md)
- [037 — Adopt bounded environment maintenance](037-adopt-bounded-environment-maintenance.md)
