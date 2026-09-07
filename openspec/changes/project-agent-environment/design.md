## Context

UZE has global machine state (`~/.uze/store`, `marketplaces.json`, `attachments.json`, harness provisioning) and project-scoped context (`AGENTS.md`, `.agents/`, harness bridges). Projects lack a reproducible declaration of which plugins compose their agent environment.

The marketplace registry (`marketplaces.json`) and plugin install (`plugin install name@marketplace`) already provide the acquisition pipeline. The missing piece is a project-scoped lock that persists desired state for reproducible install on a fresh machine.

See proposal.md for motivation and North Star experience.

## Goals / Non-Goals

**Goals:**
- `agents.yaml` as the project-scoped authored declaration, and `agents.lock` as what resolving it produced (vendor-neutral, reproducible, Git-versionable)
- `uze <plugin>@<marketplace>` project shorthand (requires `@`, writes lock)
- `uze install` consumer of lock (fresh-machine repro, no silent re-resolution)
- `uze remove <plugin>` disambiguated (project lock vs global)
- Project root resolution (deterministic walk: `agents.yaml` > `AGENTS.md` > `.git`)
- Application API: `project_environment()`, `plan_project_environment()`, `add_project_plugin()`, `remove_project_plugin()`, `install_project_environment()`
- Preserve vendor neutrality: Store/Engine/Integration remain lock-neutral

**Non-Goals:**
- `uze sync` (use `install` for now)
- Transitive dependency graph (plugins are independent)
- Semantic version solver (commit is identity)
- Automatic lock update (explicit `update` future)
- Migrating `plugin.json`, `marketplace.json`, `hooks.json` or `mcp.json` to YAML (ADR-017 §10)
- Remote marketplace search / federation
- Cryptographic signature of marketplace
- Automatic garbage collection of Store

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

### 11. The manifest is the top of the chain (added 2026-09-07)

Decisions 1-2 gave the project four states — declared (`agents.yaml`),
locked (`agents.lock`), installed (the Store), delivered (the projected
region and the harness bridges) — but only one command ever reads the
first of them, and it only ever reads it forwards. Three consequences,
all reported by a person who edited the manifest and watched nothing
happen:

- `project_lock::stale_against` iterates `manifest.declared_plugins()`,
  so it finds an addition and a re-pointing and can, by construction,
  never find a removal. A plugin deleted from the manifest stays locked,
  stays in the Store and stays attached.
- `Project::plan` starts from the lock (`has_changes = !missing`), which
  makes it blind to exactly the edit a person just made — and nothing
  calls it, so the blindness was never felt.
- `Health::status` compares lock to Store, never manifest to lock.

**Decision: drift is computed over the whole chain, and the manifest is
its head.** `plan` is re-founded on `agents.yaml`; `status` reports what
it finds; `install` converges both directions.

**Removal needs no confirmation, because it removes nothing from the
machine.** This was planned the other way round — as the destructive half,
gated behind an explicit answer — and the journey proved the premise
wrong: `remove_project_plugin` edits the manifest and the lock, and the
Store keeps the package while every harness keeps reading it. Other
projects share both, so taking it off this machine is `uze plugin remove`,
in machine scope, by ADR-019. What is left here is a derived file being
made to agree with the authored one it derives from, and asking permission
for that would teach people to click through a prompt that never
protected anything.

**Detect and offer; never apply.** The client shows the drift and the
action beside it. `install`'s own comment already states the rule this
follows — creating the manifest belongs to "an explicit act of setting
this project up… unlike opening the client, which must write nothing
into a repository somebody is only looking at". A drift signal that
applied itself would be that write, arriving through a different door.
The signal is affordable on the render path for the same reason it is
safe: two file reads and a set difference against the Store index, no
acquisition and no remote read, inside the `Budgeted` cost `status`
already holds.

**Install reconciles the context it just changed.** Installing a package
changes what the project's packages contribute to `AGENTS.md`; leaving
that to a second command is why a policy change could sit unprojected
while agents read the previous instruction. `uze i` is the alias, on the
same argument ADR-019 used for `market`: this is CLI vocabulary, not a
second entry point.

**Alternatives considered.** *Watch the manifest and re-install on
change* — rejected: it is the write-on-open rule with a file watcher
attached, and it would acquire packages behind a person who was editing
a line. *Report drift only in `status`* — rejected: the person who
edits `agents.yaml` is usually inside the client, and a report they have
to leave to read is a report they do not read.

## Candidate ADRs

- **Editing the authored manifest without a document-model dependency** — the
  manifest must survive a write with its comments intact, which a serde
  round-trip cannot do (it emits from the struct; trivia has nowhere to
  live). The ready-made answer, `yamlpath`/`yamlpatch`, pulls a
  tree-sitter C grammar into a five-dependency domain crate and onto a
  release matrix that already needed a workaround for one C dependency.
  Chosen instead: a surgical editor over the three paths UZE writes,
  where every edit is verified by re-parsing and discarded unless it
  produced exactly the intended structure. Hard to reverse once fixtures
  and refusal messages are written against it, and it is the load-bearing
  reason the manifest can promise comment preservation at all.

- **Keeping `noyalib`, isolated** — it was adopted in passing (it entered
  with the implementation commit; ADR-017 records only "replacing
  deprecated `serde_yaml`"), and its outward signals are bad: `0.0.x`
  across 33 releases, one author, renamed once from `serde_yml`, scope
  sprawl. Reading 0.0.28 contradicts the signals on quality — 43.6k lines
  of source to 65.5k of tests, 351 vendored `yaml-test-suite` cases,
  `#![forbid(unsafe_code)]` with zero `unsafe` — and it carries a `cst`
  module whose `Document` already does lossless round-trip, path-targeted
  `set`, comment read/write, and the re-parse-and-reject guard the
  manifest needs. Good code does not remove the risk that a `0.0.x`
  single-author crate breaks or goes away, so the decision is to keep it
  behind a single module (`manifest::edit`), the way `uze-git` isolates
  Git. Hard to reverse only if the crate leaks into call sites, which is
  exactly what the isolation prevents.

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
- `docs/adr/017-reproducible-agent-dependency-lock.md` — the `agents.yaml` / `agents.lock` split, their schemas, reproducibility, integrity, and trust
