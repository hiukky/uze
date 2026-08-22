# Project Agent Environment — Specification

## Requirements

### REQ-PAE-001: Project-scoped desired state

**MUST** provide a project-scoped file (`agents.lock`) declaring the desired agent environment (marketplaces + plugins with resolved sources).

**Scenario:** A project declares dependency on `flow@ai`.
**WHEN** the author runs `uze flow@ai`
**THEN** `agents.lock` is created/updated with `flow` plugin entry referencing `ai` marketplace

---

### REQ-PAE-002: Global vs project separation

**MUST** separate global machine state (`~/.uze/*`) from project desired state (`agents.lock`).

**Scenario:** Global admin command does not touch project lock.
**WHEN** the user runs `uze marketplace add <source>` or `uze plugin install <plugin>@<marketplace>`
**THEN** `agents.lock` is NOT modified (if present)

---

### REQ-PAE-003: Project shorthand requires `@`

**MUST** require `@marketplace` in project shorthand `uze <plugin>@<marketplace>`.

**Scenario:** Shorthand without `@` is rejected.
**WHEN** the user runs `uze flow` (no `@`)
**THEN** the command fails with error indicating `@marketplace` is required

---

### REQ-PAE-004: Fresh-machine reproducibility

**MUST** allow `uze install` to reconstruct the environment on a fresh machine using only `agents.lock` (no prior `uze marketplace add`).

**Scenario:** Fresh machine with only `agents.lock`.
**WHEN** the user runs `uze install` in a project with `agents.lock`
**THEN** the environment is reconstructed (marketplaces resolved, packages ingested, attachments created) without requiring prior global setup

---

### REQ-PAE-005: Lock revision wins over global

**MUST** respect `agents.lock` resolved revision over global marketplace state.

**Scenario:** Lock revision X, global points to Y.
**WHEN** the user runs `uze install`
**THEN** revision X (from lock) is used, not Y (from global)

---

### REQ-PAE-006: Trust boundary preserved

**MUST** never grant trust via `agents.lock`. `authorize` is always called if `crosses_trust_boundary` + `executable_capabilities`.

**Scenario:** Locked plugin declares MCP server with `command`.
**WHEN** the user runs `uze install` on a fresh machine
**THEN** the command fails with `TrustRequired` error (not silent execution)

---

### REQ-PAE-007: Vendor neutrality

**MUST** preserve vendor neutrality: Store/Engine/Integration remain lock-neutral.

**Scenario:** Lock parser/serializer is in Core, not Integration.
**WHEN** `uze-core` is compiled
**THEN** it does NOT import any integration-specific code (Claude/Codex/Gemini/OpenCode)

---

### REQ-PAE-008: Project root resolution

**MUST** deterministically resolve project root by walking upward from `cwd` looking for `agents.lock` (priority), then `AGENTS.md`, then `.git`. Fallback: `cwd` itself.

**Scenario:** Project root detection from subdirectory.
**WHEN** the user runs `uze install` from `cwd/project/subdir`
**THEN** the project root is resolved as `cwd/project` (where `agents.lock` or `AGENTS.md` exists)

---

### REQ-PAE-009: `desired ≠ actual` is valid state

**MUST** allow `desired ≠ actual` as a valid, diagnosticable state (not an error).

**Scenario:** Lock declares plugin, Store does not contain it.
**WHEN** the user runs `uze status` or `uze plan`
**THEN** the state is reported as `Installed ✗ Used ✓` (not collapsed into "unhealthy")

---

### REQ-PAE-010: Remove disambiguation

**MUST** disambiguate `uze remove <plugin>`: if lock present + plugin in lock → remove from project; else → delegate to global `remove_plugin`.

**Scenario:** Remove from project lock.
**WHEN** the user runs `uze remove flow` in a project with `agents.lock` containing `flow`
**THEN** `flow` is removed from `agents.lock` (Store bytes remain if referenced elsewhere)

**Scenario:** Remove from global Store.
**WHEN** the user runs `uze remove flow` in a project without `agents.lock` or with `flow` not in lock
**THEN** `flow` is removed from global Store (existing behavior)

---

### REQ-PAE-011: Application API

**MUST** provide Application API: `project_environment()`, `plan_project_environment()`, `add_project_plugin()`, `remove_project_plugin()`, `install_project_environment()`.

**Scenario:** Read-only plan.
**WHEN** the user calls `plan_project_environment(root)`
**THEN** the plan is computed without writing anything (zero `MutationLock`, zero `write_atomic`)

**Scenario:** Apply install.
**WHEN** the user calls `install_project_environment(root, authority)`
**THEN** the environment is reconciled (acquire, ingest, attach, persist lock)

---

### REQ-PAE-012: Atomicity order

**MUST** persist `agents.lock` after `ingest` succeeds (avoids orphan lock pointing to non-ingested package).

**Scenario:** Ingest fails.
**WHEN** `store.ingest()` fails (e.g., `PackageConflict`)
**THEN** `agents.lock` is NOT modified

**Scenario:** Attach fails.
**WHEN** `attach_package()` fails after successful `ingest`
**THEN** `agents.lock` IS persisted (desired state), but `doctor/plan` reports `delivery missing/drifted`

---

## Non-Goals

- `uze sync` (use `install` for now)
- Transitive dependency graph (plugins are independent)
- Semantic version solver (commit is identity)
- `agents.toml` manifest vs `agents.lock` lock split (deferred)
- Automatic lock update (explicit `update` future)
- Remote marketplace search / federation
- Cryptographic signature of marketplace
- Automatic garbage collection of Store
- `integrity: sha256:...` implementation (reserved, commit is identity today)
