## Context

UZE has global machine state (`~/.uze/store`, `marketplaces.json`, `attachments.json`, harness provisioning) and project-scoped context (`AGENTS.md`, `.agents/`, harness bridges). Projects lack a reproducible declaration of which plugins compose their agent environment.

The marketplace registry (`marketplaces.json`) and plugin install (`plugin install name@marketplace`) already provide the acquisition pipeline. The missing piece is a project-scoped lock that persists desired state for reproducible install on a fresh machine.

See proposal.md for motivation and North Star experience.

## Goals / Non-Goals

**Goals:**
- `agents.lock` as project-scoped desired agent environment (vendor-neutral, reproducible, Git-versionable)
- `uze <plugin>@<marketplace>` project shorthand (requires `@`, writes lock)
- `uze install` consumer of lock (fresh-machine repro, no silent re-resolution)
- `uze remove <plugin>` disambiguated (project lock vs global)
- Project root resolution (deterministic walk: `agents.lock` > `AGENTS.md` > `.git`)
- Application API: `project_environment()`, `plan_project_environment()`, `add_project_plugin()`, `remove_project_plugin()`, `install_project_environment()`
- Preserve vendor neutrality: Store/Engine/Integration remain lock-neutral

**Non-Goals:**
- `uze sync` (use `install` for now)
- Transitive dependency graph (plugins are independent)
- Semantic version solver (commit is identity)
- `agents.toml` manifest vs `agents.lock` lock split (deferred)
- Automatic lock update (explicit `update` future)
- Remote marketplace search / federation
- Cryptographic signature of marketplace
- Automatic garbage collection of Store
- `integrity: sha256:...` implementation (reserved, commit is identity today)

## Decisions

### 1. Three project-scoped artifacts with distinct responsibilities

- **`AGENTS.md`** — portable instructions baseline (existing, unchanged)
- **`.agents/`** — portable agent resources (existing, unchanged)
- **`agents.lock`** — resolved external agent dependencies (NEW)

**Rationale:** Separation of concerns. `AGENTS.md` is instructions, `.agents/` is resources, `agents.lock` is dependency resolution. No overlap, no confusion.

### 2. Global vs Project state separation

- **Global (machine):** `~/.uze/store`, `marketplaces.json`, `attachments.json`, harness provisioning
- **Project (portable):** `AGENTS.md`, `.agents/`, `agents.lock`

**Invariant:** `uze marketplace add` and `uze plugin install` (global admin) NEVER write `agents.lock`. Only `uze <plugin>@<marketplace>` (project shorthand) writes lock.

**Rationale:** Installing something globally must not implicitly modify the current project. Explicit is better than implicit.

### 3. `agents.lock` YAML schema v1

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
- `version: 1` (not `lockfileVersion`) — short, explicit
- `marketplaces` top-level — reproducible identity, not just alias
- `source.type: git | path | embedded` — mirrors `PackageSource` variants
- `resolved.revision` — Git commit SHA, `embedded` literal, or empty for `path`
- `integrity: sha256:...` — reserved but not implemented (commit is identity today)
- `BTreeMap` ordering — deterministic serialization
- `write_atomic` — nonce temp + rename + sync (same as other UZE state)

**Rationale:** Small, human-readable, deterministic, vendor-neutral. `path` type explicitly non-reproducible (warning, not silent).

### 4. Project root resolution

**Rule:** Walk upward from `cwd` looking for `agents.lock` (priority), then `AGENTS.md`, then `.git`. First found is root. Fallback: `cwd` itself.

**Rationale:** One predictable rule, no git assumption. Preserves existing behavior (`AGENTS.md` as project signal) while adding `agents.lock` as stronger anchor. Monorepo: each subproject with its own lock wins over parent.

### 5. Reproducibility semantics

- `install` uses `resolved.revision` (frozen), never re-resolves `source.reference`
- Lock revision X wins over global marketplace pointing to Y
- Offline + Store hit → success; offline + Store miss → `Unavailable`
- `desired ≠ actual` is valid state (diagnosticable via `plan`/`status`)

**Rationale:** Lock is source of truth for project. `install` respects lock, not global state. `update` (future) would be explicit operation.

### 6. Trust boundary

- `agents.lock` NEVER grants trust
- `authorize` is always called if `crosses_trust_boundary` + `executable_capabilities`
- Fresh machine with locked MCP server → `TrustRequired` error, not silent execution

**Rationale:** Lock is dependency declaration, not consent. Trust is per-installation, not per-project.

### 7. Atomicity order

```
resolve → authorize → acquire → validate → ingest → republish → attach → persist lock
```

Lock is persisted after `ingest` succeeds (avoids orphan lock pointing to non-ingested package). If `attach` fails, lock persists but `doctor/plan` reports `delivery missing/drifted`.

**Rationale:** Lock represents desired state. `desired ≠ actual` is valid and diagnosticable. No destructive rollback.

### 8. CLI grammar

```bash
uze <plugin>@<marketplace>        # project shorthand (requires @)
uze install                       # consumer of lock
uze remove <plugin>               # disambiguated: project lock vs global

# Global admin (unchanged, NEVER touch lock):
uze marketplace add <source>
uze marketplace remove <name>
uze plugin install <plugin>@<marketplace>
uze plugin remove <plugin>
uze add <source> / uze remove <plugin>  # when no lock present → delegate global
```

**Rationale:** `uze flow@ai` is the North Star. `@` required to avoid ambiguity. `remove` disambiguated by context (lock present + plugin in lock → project; else → global).

### 9. Application API

```rust
// Read model
pub struct ProjectEnvironment {
    pub root: PathBuf,
    pub canonical: PathBuf,
    pub lock: Option<ProjectLock>,
    pub diagnostics: Vec<String>,
}

pub struct ProjectEnvironmentPlan {
    pub dependencies: Vec<LockedPlugin>,
    pub installed: Vec<StoredPackage>,
    pub missing: Vec<LockedPlugin>,
    pub trust_required: Vec<TrustRequest>,
    pub delivery_changes: Vec<PublicationOutcome>,
    pub conflicts: Vec<String>,
    pub offline_unavailable: Vec<String>,
    pub has_changes: bool,
}

impl UzeApplication {
    pub fn project_environment(&self, root: &Path) -> Result<ProjectEnvironment>
    pub fn plan_project_environment(&self, root: &Path) -> Result<ProjectEnvironmentPlan>
    pub fn add_project_plugin(&self, plugin: &str, marketplace: &str, root: &Path, authority: &dyn TrustAuthority) -> Result<AddPluginReport>
    pub fn remove_project_plugin(&self, plugin: &str, root: &Path) -> Result<RemoveProjectPluginReport>
    pub fn install_project_environment(&self, root: &Path, authority: &dyn TrustAuthority) -> Result<InstallReport>
}
```

**Rationale:** Reuses existing `inspect/plan/reconcile` pattern. `plan_*` is read-only, `install` is apply. Shares `authorize→acquire→ingest→republish→attach` with existing lifecycle.

### 10. Architecture boundaries

- **`uze-core`** — vendor-neutral domain: `project_lock` (parser/serializer), `project_root` (resolution), error variants
- **`uze-application`** — use cases: `project_environment`, `plan/add/remove/install`
- **`uze-integrations`** — harness adapters (unchanged, lock-neutral)
- **Store/Engine** — bytes + composition (unchanged, lock-neutral)
- **CLI/TUI** — framework/UI: CLI shorthand + `install`/`remove`; TUI reads `project_environment()` API

**Rationale:** Parser/serializer in Core (vendor-neutral). Use cases in Application. Store/Engine/Integration remain lock-neutral (vendor neutrality preserved).

## Risks / Trade-offs

- **[Deprecated `serde_yaml`]** → Mitigation: use `noyalib::compat::serde_yaml` (maintained fork, zero unsafe, MSRV 1.86 ≤ our 1.97)
- **[Non-reproducible `path` type]** → Mitigation: `resolved: {}` empty, `plan` warns `NonReproducibleMarketplace`
- **[Lock bypasses trust]** → Mitigation: `authorize` always called, lock never grants consent
- **[Project root ambiguity in monorepo]** → Mitigation: deterministic walk rule, `agents.lock` wins over `AGENTS.md` wins over `.git`
- **[Drift `attachments.json` vs lock]** → Mitigation: ADR-009 `Matched/Missing/Drifted/Blocked`; only `Matched` detach
- **[Store `PackageConflict` on `install`]** → Mitigation: `store.ingest` rejects `PackageConflict` `store.rs:130`; report, not overwrite

## LikeC4

This change adds a new component (`agents.lock` as project-scoped artifact) and a new relationship (project → global Store via `install`). The LikeC4 model under `docs/architecture/likec4/` should be updated to reflect the project-agent-environment layer. However, since the current LikeC4 model focuses on the global machine state and harness integrations, and `agents.lock` is a project-scoped file (not a runtime component), the LikeC4 update is deferred to a follow-up when the project-scoped layer is more fully modeled.

## ADRs

This change produces two ADRs:
- `docs/adr/016-project-agent-environment.md` — Project Agent Environment (global vs project separation)
- `docs/adr/016-project-agent-environment.md` — the `agents.lock` schema, reproducibility, and trust
