# Reproducible Agent Dependency Lock

Status: Accepted

## Context

Following ADR-016 (Project Agent Environment), UZE needs a file format for `agents.lock` that is:
- Small and human-readable
- Deterministic (repeated writes produce identical bytes)
- Vendor-neutral (no harness-specific state)
- Reproducible (contains enough info for fresh-machine install)
- Versionable in Git
- Independent of harnesses installed on the machine

The lock must persist reproducible identity for marketplaces and plugins. A marketplace alias like `ai` is UX; the identity is `source.url + resolved.revision`. On a fresh machine, `uze install` must reconstruct the environment using only the lock, without prior `uze marketplace add`.

## Decision

`agents.lock` is a YAML file (via `noyalib::compat::serde_yaml`, replacing deprecated `serde_yaml`) with schema version `version: 1`.

**Schema:**
```yaml
version: 1

marketplaces:
  ai:
    source:
      type: git
      url: https://github.com/hiukky/ai.git
      # reference: main  # optional
      # subdirectory: marketplace  # optional
    resolved:
      revision: 9f3a1c2d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0

  local-dev:
    source:
      type: path
      path: ../local-marketplace
    resolved: {}  # empty = non-reproducible

plugins:
  flow:
    source:
      type: marketplace
      marketplace: ai
      plugin: flow
    resolved:
      revision: 9f3a1c2d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0
      version: 0.3.1  # informational
      # integrity: sha256:...  # reserved for future

  uze:
    source:
      type: marketplace
      marketplace: uze-official
      plugin: uze
    resolved:
      revision: embedded
      version: 0.0.0-alpha.8
```

**Key decisions:**
1. **`version: 1`** (not `lockfileVersion`) — short, explicit
2. **`marketplaces` top-level** — reproducible identity, not just alias
3. **`source.type: git | path | embedded`** — mirrors `PackageSource` variants
4. **`resolved.revision`** — Git commit SHA, `embedded` literal, or empty for `path` (non-reproducible)
5. **`integrity: sha256:...`** — reserved but not implemented (commit is identity today)
6. **`BTreeMap` ordering** — deterministic serialization
7. **`write_atomic`** — nonce temp + rename + sync (same as other UZE state)
8. **`path` type** — explicitly non-reproducible; `plan` warns `NonReproducibleMarketplace`
9. **`embedded` type** — `revision: embedded` literal (bytes in binary)

**Reproducibility semantics:**
- `install` uses `resolved.revision` (frozen), never re-resolves `source.reference`
- Lock revision X wins over global marketplace pointing to Y
- Offline + Store hit → success; offline + Store miss → `Unavailable`
- `desired ≠ actual` is valid state (diagnosticable via `plan`/`status`)

**Trust boundary:**
- `agents.lock` NEVER grants trust
- `authorize` is always called if `crosses_trust_boundary` + `executable_capabilities`
- Fresh machine with locked MCP server → `TrustRequired` error, not silent execution

**Lifecycle:**
- Lock absent → create `version:1` + entries
- Lock exists → merge `BTreeMap` (preserve order, other entries intact)
- Same resolution → no-op (idempotent via `BTreeMap` + `to_string`)
- Malformed YAML → `Err(MalformedLock)`, no overwrite
- Unknown version → `Err(UnsupportedLockVersion)`
- Marketplace source conflict → `Err(MarketplaceSourceConflict)`, not silent

**Atomicity order:**
```
resolve → authorize → acquire → validate → ingest → republish → attach → persist lock
```
Lock is persisted after `ingest` succeeds (avoids orphan lock pointing to non-ingested package). If `attach` fails, lock persists but `doctor/plan` reports `delivery missing/drifted`.

## Consequences

**Positive:**
- Fresh-machine repro: `git clone + uze install` works without prior setup
- Deterministic: repeated `uze flow@ai` produces identical bytes
- Non-reproducible sources explicit: `path` type with empty `resolved`
- Trust boundary preserved: lock never bypasses consent
- Atomic: lock persisted only after successful ingest

**Negative:**
- New dependency: `noyalib` (maintained fork of deprecated `serde_yaml`)
- New error variants: `UnsupportedLockVersion`, `MalformedLock`, `MarketplaceSourceConflict`, `MarketplaceMismatch`
- `path` type is non-reproducible (explicit warning, not silent)

**Neutral:**
- `integrity` field reserved but not implemented (commit is identity)
- `agents.toml` manifest vs `agents.lock` lock split deferred (non-goal)
- `uze sync` deferred (use `install` for now)

Source change: openspec/changes/project-agent-environment/
