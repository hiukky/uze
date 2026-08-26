## Context

See [proposal.md](proposal.md). ADR-023 renamed the root discovery manifest to `agents.json`; the active marketplace spec still names `marketplace.json`.

## Goals / Non-Goals

**Goals:** restore one `marketplace.json` root contract everywhere, preserve the schema/domain state, and leave vendor catalogues untouched.

**Non-Goals:** accept or migrate `agents.json`; change `AGENTS.md`, `agents.lock`, package manifests, state files, dependencies, or the LikeC4 model.

## Decisions

### One clean replacement

Rename every UZE-owned root input and embedded asset to `marketplace.json`. A root containing only `agents.json` is not a marketplace. No compatibility branch is justified before production use.

### Preserve ownership boundaries

Vendor-owned `marketplace.json` paths remain unchanged. `~/.uze/state/marketplaces.json` remains state; only the root discovery manifest is renamed.

### Supersede ADR-023

Create ADR-032 to supersede ADR-023 and restore the filename clauses of ADR-012 and ADR-015. See that ADR for the durable decision and implementation plan.

## Risks / Trade-offs

- [Stale UZE-owned reference] → repository-wide search and negative tests for `agents.json`.
- [Vendor catalogue renamed accidentally] → scope replacement to the root contract and retain integration paths.
- [Detection disagreement] → cover Core, application overview, CLI registration/install, bootstrap, fixtures, and conformance.

## Migration Plan

Rename manifests and fixtures; update all consumers atomically; update docs/ADRs; validate deterministic tests, all conformance verticals, strict OpenSpec, and a final reference scan. Rollback is a source revert only.
