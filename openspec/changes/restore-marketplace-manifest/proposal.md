## Why

`agents.json` conflates a distribution registry with project instruction and lock-file vocabulary. UZE is pre-production and single-operator, so it can restore the clearer, harness-aligned `marketplace.json` name without compatibility debt.

## What Changes

- **BREAKING (pre-production):** replace `agents.json` with `marketplace.json` as the sole marketplace-root manifest.
- Rename the embedded marketplace, acquisition/workspace/overview inputs, fixtures, conformance seeds, docs, and diagnostics.
- Reject roots containing only `agents.json`; no alias, automatic rename, or warning period.
- Add an ADR superseding ADR-023 and restoring ADR-012/ADR-015's root-manifest clauses.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `overview`: workspace marketplace detection recognizes `marketplace.json` rather than `agents.json`.

## Impact

Core acquisition/workspace detection, application bootstrap and overview, CLI/TUI wording, testkit, fixtures, conformance, repository root manifest, architecture docs, and ADR index. Vendor catalogues and persisted marketplace state are unchanged.
