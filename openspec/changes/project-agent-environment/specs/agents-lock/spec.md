# Agents Lock — Specification

## Requirements

### REQ-LOCK-001: Schema version

**MUST** use `version: 1` (not `lockfileVersion`).

**Scenario:** Lock with version 1.
**WHEN** the lock is parsed
**THEN** `version: 1` is accepted

**Scenario:** Lock with unsupported version.
**WHEN** the lock has `version: 99`
**THEN** parsing fails with `UnsupportedLockVersion { found: 99, expected: 1 }`

---

### REQ-LOCK-002: YAML format

**MUST** use YAML format via `noyalib::compat::serde_yaml` (maintained fork, replacing deprecated `serde_yaml`).

**Scenario:** Lock serialization.
**WHEN** the lock is written to disk
**THEN** the format is valid YAML (parseable by `serde_yaml::from_str`)

---

### REQ-LOCK-003: Deterministic serialization

**MUST** produce identical bytes for repeated writes of the same lock (via `BTreeMap` ordering).

**Scenario:** Repeated writes.
**WHEN** the user runs `uze flow@ai` twice with no changes
**THEN** `agents.lock` bytes are identical (idempotent)

---

### REQ-LOCK-004: Marketplace source types

**MUST** support `source.type: git | path | embedded` mirroring `PackageSource` variants.

**Scenario:** Git marketplace.
**WHEN** the lock declares `type: git, url: https://..., reference: main`
**THEN** the marketplace is resolved via Git acquisition

**Scenario:** Path marketplace.
**WHEN** the lock declares `type: path, path: ../local`
**THEN** the marketplace is resolved via local path (non-reproducible)

**Scenario:** Embedded marketplace.
**WHEN** the lock declares `type: embedded, id: uze-official`
**THEN** the marketplace is resolved via embedded snapshot

---

### REQ-LOCK-005: Resolved revision

**MUST** persist `resolved.revision` as Git commit SHA, `embedded` literal, or empty for `path`.

**Scenario:** Git revision.
**WHEN** the marketplace is Git-based
**THEN** `resolved.revision` contains the commit SHA (e.g., `9f3a1c2d...`)

**Scenario:** Embedded revision.
**WHEN** the marketplace is embedded
**THEN** `resolved.revision` contains the literal `embedded`

**Scenario:** Path revision.
**WHEN** the marketplace is path-based
**THEN** `resolved.revision` is empty (`{}`)

---

### REQ-LOCK-006: Plugin source types

**MUST** support `source.type: marketplace | git` for plugins.

**Scenario:** Marketplace plugin.
**WHEN** the lock declares `type: marketplace, marketplace: ai, plugin: flow`
**THEN** the plugin is resolved via marketplace `ai`

**Scenario:** Git plugin (future).
**WHEN** the lock declares `type: git, url: https://...`
**THEN** the plugin is resolved via direct Git acquisition

---

### REQ-LOCK-007: Integrity field (reserved)

**MAY** include `integrity: sha256:...` field (reserved for future, not implemented today).

**Scenario:** Integrity field present.
**WHEN** the lock declares `integrity: sha256:abc123...`
**THEN** the field is parsed but not validated (commit is identity today)

---

### REQ-LOCK-008: Non-reproducible warning

**MUST** warn when `path` type is used (explicitly non-reproducible).

**Scenario:** Path marketplace in lock.
**WHEN** `plan_project_environment()` is called
**THEN** the plan includes a warning `NonReproducibleMarketplace` for `path` sources

---

### REQ-LOCK-009: Malformed lock handling

**MUST** reject malformed YAML without overwriting the existing lock.

**Scenario:** Malformed YAML.
**WHEN** the lock contains invalid YAML (e.g., unclosed bracket)
**THEN** parsing fails with `MalformedLock { path, reason }` and the file is NOT overwritten

---

### REQ-LOCK-010: Atomic write

**MUST** use `write_atomic` (nonce temp + rename + sync) for lock persistence.

**Scenario:** Atomic write.
**WHEN** the lock is written to disk
**THEN** the write is atomic (temp file + rename, same as other UZE state)

---

### REQ-LOCK-011: Marketplace source conflict

**MUST** reject lock with marketplace source conflicting with global registry.

**Scenario:** Lock has `ai → url-A`, global has `ai → url-B`.
**WHEN** `add_project_plugin()` is called
**THEN** the operation fails with `MarketplaceSourceConflict { marketplace: "ai", lock_source: "url-A", global_source: "url-B" }`

---

### REQ-LOCK-012: Plugin marketplace mismatch

**MUST** reject lock with plugin referencing different marketplace than expected.

**Scenario:** Lock has `flow → ai`, but user tries to add `flow → other`.
**WHEN** `add_project_plugin("flow", "other")` is called
**THEN** the operation fails with `MarketplaceMismatch { plugin: "flow", expected: "ai", found: "other" }`

---

### REQ-LOCK-013: UTF-8 encoding

**MUST** require valid UTF-8 encoding for lock file.

**Scenario:** Non-UTF-8 lock.
**WHEN** the lock contains invalid UTF-8 bytes
**THEN** parsing fails with `MalformedLock { path, reason: "agents.lock is not valid UTF-8" }`

---

### REQ-LOCK-014: Empty lock

**MAY** allow empty lock (no marketplaces, no plugins) as valid state.

**Scenario:** Empty lock.
**WHEN** the lock contains only `version: 1`
**THEN** the lock is valid (no dependencies declared)

---

### REQ-LOCK-015: BTreeMap ordering

**MUST** use `BTreeMap` for `marketplaces` and `plugins` to ensure deterministic key ordering.

**Scenario:** Multiple entries.
**WHEN** the lock contains `marketplaces: {ai, local}` and `plugins: {flow, uze}`
**THEN** serialization orders keys alphabetically (`ai` before `local`, `flow` before `uze`)

---

## Non-Goals

- `integrity: sha256:...` validation (reserved, commit is identity today)
- Cryptographic signature of marketplace
- Transitive dependency graph
- Semantic version solver
- `agents.toml` manifest vs `agents.lock` lock split
