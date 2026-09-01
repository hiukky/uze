# Project Agent Environment: AGENTS.md, .agents/, and agents.lock

Status: Accepted
Consolidates: ADR-017 (reproducible agent dependency lock — the `agents.lock`
schema) — see the "Consolidated records" section of `README.md`.
Amended by [ADR-019](019-explicit-project-machine-boundary-in-cli-command-grammar.md),
narrowly: `uze remove` is now strictly project-scoped, with no fallback to
global removal. Every other decision here remains in effect, reaffirmed by
ADR-019.

## Context

UZE has global machine state (`~/.uze/store`, `marketplaces.json`,
`attachments.json`, harness provisioning) and project-scoped context
(`AGENTS.md`, `.agents/`, harness bridges). Projects had no reproducible
declaration of which plugins compose their agent environment: a contributor
cloning a repo could not reconstruct the author's environment without
manually re-running marketplace and plugin commands for every dependency.
Global state is not versionable; project state must be.

The target experience:

```bash
# author
uze flow@ai
git add agents.lock && git commit

# contributor
git clone <repo> && cd <repo>
uze install
```

Same project, same agent environment, any supported harness.

## Decision

Three project-scoped artifacts with distinct responsibilities:

- **`AGENTS.md`** — portable instructions baseline
- **`.agents/`** — portable agent resources
- **`agents.lock`** — resolved external agent dependencies

This separates **Machine Actual State** (global, `~/.uze/*`) from **Project
Desired State** (portable, `agents.lock`).

### Invariants

1. Machine-level admin (`market add`, `plugin install`) NEVER writes
   `agents.lock`.
2. The project shorthand `uze <plugin>@<marketplace>` writes `agents.lock`.
3. `uze install` consumes `agents.lock` without silently re-resolving.
4. `agents.lock` carries reproducible source identity — a Git commit, never
   a branch alias.
5. `agents.lock` NEVER grants trust; `authorize` is always called.
6. Store, Engine, and integrations remain lock-neutral, preserving vendor
   neutrality.

### Project root resolution

Walk upward from `cwd` looking for `agents.lock` (priority), then
`AGENTS.md`, then `.git`. First found is root; fallback is `cwd` itself. One
predictable rule, no git assumption.

### `agents.lock` schema

YAML, `version: 1`, serialized deterministically (`BTreeMap` ordering) and
persisted with the same nonce-temp + rename + fsync `write_atomic` used for
all other UZE state — and only after a successful ingest.

```yaml
version: 1

marketplaces:
  ai:
    source:
      type: git
      url: https://github.com/hiukky/ai.git
      # reference: main       # optional
      # subdirectory: market  # optional
    resolved:
      revision: 9f3a1c2d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0

  local-dev:
    source:
      type: path
      path: ../local-marketplace
    resolved: {}   # empty = non-reproducible

plugins:
  flow:
    source:
      type: marketplace
      marketplace: ai
      plugin: flow
    resolved:
      revision: 9f3a1c2d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0
      version: 0.3.1          # informational
      # integrity: sha256:... # reserved, not implemented
```

- `version: 1`, not `lockfileVersion` — short and explicit.
- `marketplaces` is top-level: reproducible identity, not just an alias.
- `source.type` is `git | path | embedded`, mirroring `PackageSource`.
- `resolved.revision` is a Git commit SHA, the literal `embedded`, or empty
  for `path`.
- `path` is explicitly non-reproducible; `plan` warns
  `NonReproducibleMarketplace` rather than failing silently.
- `integrity: sha256:...` is reserved but unimplemented — the commit is
  identity today.

## Consequences

Easier: `git clone && uze install` reconstructs the environment from the
lock alone, on a machine that has never seen the project. Admin commands
never implicitly modify project state. The lock contains no harness-specific
path or config, so it stays portable across all four harnesses. Repeated
`uze <plugin>@<market>` produces identical bytes.

Harder: one more file to manage, a YAML dependency (`noyalib`, a maintained
fork of the deprecated `serde_yaml`), and new error variants to surface
honestly — `UnsupportedLockVersion`, `MalformedLock`,
`MarketplaceSourceConflict`, `MarketplaceMismatch`.

Neutral: `agents.lock` is *desired* state — `desired ≠ actual` is a valid,
diagnosable condition, not an error. A separate `agents.toml` manifest and a
`uze sync` verb were both considered and deferred as non-goals.
